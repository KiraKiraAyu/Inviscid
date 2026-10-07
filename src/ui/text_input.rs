use gpui::{
    App, Bounds, ClipboardItem, ContentMask, Context, DispatchPhase, ElementId,
    ElementInputHandler, Entity, EntityInputHandler, EventEmitter, FocusHandle, Focusable, Hsla,
    InteractiveElement, IntoElement, KeyDownEvent, MouseButton, MouseDownEvent, MouseMoveEvent,
    MouseUpEvent, ParentElement, Pixels, Point, Render, ScrollDelta, ScrollWheelEvent,
    SharedString, Styled, Task, TextRun, UTF16Selection, UnderlineStyle, Window, WrappedLine,
    canvas, div, fill, point, px, size,
};
use std::cell::Cell;
use std::ops::Range;
use std::rc::Rc;

use crate::buffer::{Position, Selection, TextBuffer, char_col_to_byte_offset};
use crate::editor::actions::{
    Backspace, CopyAction, CutAction, Delete, DeleteToNextWord, DeleteToPreviousWord, MoveLeft,
    MoveRight, MoveToBeginningOfLine, MoveToEndOfLine, MoveToNextWord, MoveToPreviousWord,
    PasteAction, RedoAction, SelectAll, SelectLeft, SelectRight, SelectToBeginningOfLine,
    SelectToEndOfLine, SelectToNextWord, SelectToPreviousWord, UndoAction,
};
use crate::theme::{DEFAULT_THEME, Theme, ThemeManager};
use crate::ui::tokens::{ControlHeight, FontSize, LineHeight, Radius, Spacing};
use crate::ui::{calculate_caret_size, start_cursor_animation};

/// Events emitted by [`TextInput`] upon user confirmation or cancellation.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TextInputEvent {
    Commit(String),
    Cancel,
}

/// Action determined by processing a keystroke in [`TextInput`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum KeyAction {
    Handled,
    Commit,
    Cancel,
    Ignored,
}

#[inline]
fn measurement_run(len: usize, font: gpui::Font) -> TextRun {
    TextRun {
        len,
        font,
        color: gpui::transparent_black(),
        background_color: None,
        underline: None,
        strikethrough: None,
    }
}

fn default_single_line_filter(c: char) -> bool {
    c != '\n' && c != '\r'
}

/// Computes the updated horizontal scroll offset so that the active cursor / selection head
/// remains comfortably within the visible viewport.
pub fn calculate_scroll_offset(
    current_scroll: Pixels,
    cursor_x: Pixels,
    caret_width: Pixels,
    viewport_width: Pixels,
    text_width: Pixels,
    margin: Pixels,
) -> Pixels {
    let total_width = text_width + caret_width;
    if total_width <= viewport_width || viewport_width <= px(0.0) {
        return px(0.0);
    }

    let max_scroll = (total_width - viewport_width).max(px(0.0));
    let effective_margin = margin.min(viewport_width / 4.0).max(px(0.0));
    let mut new_scroll = current_scroll;

    // If cursor is to the left of the visible viewport (with margin):
    let left_bound = cursor_x - effective_margin;
    if left_bound < new_scroll {
        new_scroll = left_bound.max(px(0.0));
    }

    // If cursor + caret is to the right of the visible viewport (with margin):
    let right_bound = cursor_x + caret_width + effective_margin;
    if right_bound > new_scroll + viewport_width {
        new_scroll = right_bound - viewport_width;
    }

    new_scroll.clamp(px(0.0), max_scroll)
}

/// Single-line text input component providing text editing, clipboard actions,
/// IME support, subpixel text shaping, drag selection, and cursor animation.
pub struct TextInput {
    pub(crate) id: ElementId,
    pub(crate) buffer: TextBuffer,
    pub(crate) focus_handle: FocusHandle,
    pub(crate) placeholder: Option<SharedString>,
    pub(crate) char_filter: fn(char) -> bool,
    pub(crate) key_context: &'static str,
    pub(crate) height: Pixels,
    pub(crate) radius: Pixels,
    pub(crate) padding_x: Pixels,
    pub(crate) marked_range: Option<Range<usize>>,
    pub(crate) cursor_opacity: f32,
    pub(crate) blink_epoch: usize,
    pub(crate) blink_task: Option<Task<()>>,
    pub(crate) is_dragging: bool,
    pub(crate) drag_anchor: Option<usize>,
    pub(crate) scroll_offset: Rc<Cell<Pixels>>,
    pub(crate) last_bounds: Rc<Cell<Bounds<Pixels>>>,
}

impl Focusable for TextInput {
    fn focus_handle(&self, _cx: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl EventEmitter<TextInputEvent> for TextInput {}

impl TextInput {
    /// Creates an unstarted text input without spawning cursor blink tasks.
    pub fn new_unstarted(initial_text: &str, focus_handle: FocusHandle) -> Self {
        let mut buffer = TextBuffer::from_str(initial_text, None);
        let char_count = initial_text.chars().count();
        buffer.set_selection(Selection::cursor(Position::new(0, char_count)));

        Self {
            id: ElementId::Name("text_input".into()),
            buffer,
            focus_handle,
            placeholder: None,
            char_filter: default_single_line_filter,
            key_context: crate::ui::key_context::TEXT_INPUT,
            height: ControlHeight::XS,
            radius: Radius::XS,
            padding_x: Spacing::XS,
            marked_range: None,
            cursor_opacity: 1.0,
            blink_epoch: 0,
            blink_task: None,
            is_dragging: false,
            drag_anchor: None,
            scroll_offset: Rc::new(Cell::new(px(0.0))),
            last_bounds: Rc::new(Cell::new(Bounds::default())),
        }
    }

    /// Creates a new single-line text input with the cursor placed at the end of `initial_text`.
    pub fn new(initial_text: &str, cx: &mut Context<Self>) -> Self {
        let mut input = Self::new_unstarted(initial_text, cx.focus_handle());
        input.reset_cursor_blink(cx);
        input
    }

    pub fn id(mut self, id: impl Into<ElementId>) -> Self {
        self.id = id.into();
        self
    }

    pub fn placeholder(mut self, placeholder: impl Into<SharedString>) -> Self {
        self.placeholder = Some(placeholder.into());
        self
    }

    pub fn char_filter(mut self, filter: fn(char) -> bool) -> Self {
        self.char_filter = filter;
        self
    }

    pub fn key_context(mut self, key_context: &'static str) -> Self {
        self.key_context = key_context;
        self
    }

    pub fn selection(mut self, range: Range<usize>) -> Self {
        self.buffer.set_selection(Selection {
            anchor: Position::new(0, range.start),
            head: Position::new(0, range.end),
        });
        self
    }

    pub fn height(mut self, height: Pixels) -> Self {
        self.height = height;
        self
    }

    pub fn radius(mut self, radius: Pixels) -> Self {
        self.radius = radius;
        self
    }

    pub fn padding_x(mut self, padding_x: Pixels) -> Self {
        self.padding_x = padding_x;
        self
    }

    #[inline]
    pub fn text(&self) -> String {
        self.buffer.line(0).unwrap_or("").to_string()
    }

    #[inline]
    pub fn buffer(&self) -> &TextBuffer {
        &self.buffer
    }

    #[inline]
    pub fn buffer_mut(&mut self) -> &mut TextBuffer {
        &mut self.buffer
    }

    #[inline]
    pub fn is_dragging(&self) -> bool {
        self.is_dragging
    }

    #[inline]
    pub fn drag_anchor(&self) -> Option<usize> {
        self.drag_anchor
    }

    #[inline]
    pub fn cursor_opacity(&self) -> f32 {
        self.cursor_opacity
    }

    #[inline]
    pub fn has_blink_task(&self) -> bool {
        self.blink_task.is_some()
    }

    #[inline]
    pub fn scroll_offset(&self) -> Pixels {
        self.scroll_offset.get()
    }

    #[inline]
    pub fn set_scroll_offset(&self, offset: Pixels) {
        self.scroll_offset.set(offset);
    }

    pub fn sanitize_input(&self, text: &str) -> String {
        let filter = self.char_filter;
        text.chars().filter(|&c| filter(c)).collect()
    }

    /// Clears all text in the input buffer without triggering context notifications.
    pub fn clear_buffer(&mut self) -> bool {
        if !self.text().is_empty() {
            self.buffer.select_all();
            self.buffer.insert_text("");
            self.marked_range = None;
            self.scroll_offset.set(px(0.0));
            true
        } else {
            false
        }
    }

    /// Clears all text in the input as an undoable buffer edit and notifies context.
    pub fn clear(&mut self, cx: &mut Context<Self>) {
        if self.clear_buffer() {
            self.on_mutation(cx);
        }
    }

    pub fn on_mutation(&mut self, cx: &mut Context<Self>) {
        self.reset_cursor_blink(cx);
    }

    pub fn commit(&mut self, cx: &mut Context<Self>) {
        let text = self.text();
        cx.emit(TextInputEvent::Commit(text));
    }

    pub fn cancel(&mut self, cx: &mut Context<Self>) {
        cx.emit(TextInputEvent::Cancel);
    }

    /// Processes commit (Enter) and cancel (Escape) keystrokes.
    /// Text editing, cursor navigation, and word jumps are dispatched via standard GPUI actions.
    pub fn handle_key(&mut self, event: &KeyDownEvent) -> KeyAction {
        let key = &event.keystroke.key;
        if key.eq_ignore_ascii_case("enter") {
            KeyAction::Commit
        } else if key.eq_ignore_ascii_case("escape") {
            KeyAction::Cancel
        } else {
            KeyAction::Ignored
        }
    }

    /// Handles a key down event when [`TextInput`] is focused directly.
    pub fn handle_key_down(&mut self, event: &KeyDownEvent, cx: &mut Context<Self>) -> bool {
        self.handle_host_key(event, cx)
    }

    pub fn undo(&mut self) {
        self.buffer.undo();
    }

    pub fn redo(&mut self) {
        self.buffer.redo();
    }

    pub fn select_all(&mut self) {
        self.buffer.select_all();
    }

    pub fn copy_selection(&self, cx: &mut App) -> bool {
        if let Some(text) = self.buffer.selected_text() {
            cx.write_to_clipboard(ClipboardItem::new_string(text));
            true
        } else {
            false
        }
    }

    pub fn cut_selection(&mut self, cx: &mut App) -> bool {
        if let Some(text) = self.buffer.selected_text() {
            cx.write_to_clipboard(ClipboardItem::new_string(text));
            self.buffer.insert_text("");
            true
        } else {
            false
        }
    }

    pub fn paste_from_clipboard(&mut self, cx: &mut App) -> bool {
        if let Some(item) = cx.read_from_clipboard()
            && let Some(text) = item.text()
        {
            let clean = self.sanitize_input(&text);
            self.buffer.insert_text(&clean);
            true
        } else {
            false
        }
    }

    /// Restarts cursor animation for a host entity with unified epoch and breathing curve.
    pub(crate) fn reset_blink_host<H: TextInputHost>(&mut self, cx: &mut Context<H>) {
        self.cursor_opacity = 1.0;
        self.blink_epoch = self.blink_epoch.wrapping_add(1);
        let epoch = self.blink_epoch;
        cx.notify();
        let (blink_enabled, breathing_enabled) = cx
            .try_global::<crate::config::AppConfig>()
            .map(|c| (c.cursor_blink, c.cursor_breathing))
            .unwrap_or((true, true));
        if blink_enabled {
            self.blink_task = Some(start_cursor_animation(
                cx,
                breathing_enabled,
                epoch,
                |host| host.text_input().blink_epoch,
                |host, opacity, cx| {
                    host.text_input_mut().cursor_opacity = opacity;
                    cx.notify();
                },
            ));
        } else {
            self.blink_task = None;
        }
    }

    /// Suspends cursor blinking and sets opacity to 0.0 without notifying context.
    pub fn pause_cursor_blink(&mut self) {
        self.cursor_opacity = 0.0;
        self.blink_epoch = self.blink_epoch.wrapping_add(1);
        self.blink_task = None;
    }

    pub fn reset_cursor_blink(&mut self, cx: &mut Context<Self>) {
        self.reset_blink_host(cx);
    }

    pub fn on_blur(&mut self, cx: &mut Context<Self>) {
        self.pause_cursor_blink();
        cx.notify();
    }

    pub fn on_focus(&mut self, cx: &mut Context<Self>) {
        self.reset_cursor_blink(cx);
    }

    pub fn start_drag(&mut self, col: usize) {
        self.buffer
            .set_selection(Selection::cursor(Position::new(0, col)));
        self.is_dragging = true;
        self.drag_anchor = Some(col);
    }

    pub fn handle_drag_move(&mut self, point: Point<Pixels>, window: &mut Window) {
        if !self.is_dragging {
            return;
        }
        let col = self.col_for_point(self.rel_x_for_point(point), window);
        if let Some(anchor) = self.drag_anchor {
            self.buffer.set_selection(Selection {
                anchor: Position::new(0, anchor),
                head: Position::new(0, col),
            });
        }
    }

    pub fn stop_drag(&mut self) {
        if self.is_dragging {
            self.is_dragging = false;
            self.drag_anchor = None;
        }
    }

    #[inline]
    fn rel_x_for_point(&self, point: Point<Pixels>) -> Pixels {
        (point.x - self.last_bounds.get().origin.x - self.padding_x + self.scroll_offset.get())
            .max(px(0.0))
    }

    /// Resolves character column from a relative X pixel coordinate using text shaping.
    pub fn col_for_point(&self, rel_x: Pixels, window: &mut Window) -> usize {
        let line_str = self.buffer.line(0).unwrap_or("");
        if line_str.is_empty() {
            return 0;
        }
        let font_size = FontSize::SM.to_pixels(window.rem_size());
        let line_height = LineHeight::SM.to_pixels(window.rem_size());
        let run = measurement_run(line_str.len(), window.text_style().font());
        if let Ok(lines) = window.text_system().shape_text(
            line_str.to_string().into(),
            font_size,
            &[run],
            None,
            None,
        ) && let Some(line) = lines.first()
        {
            let rel_point = point(rel_x, px(0.0));
            let byte_idx = line
                .closest_index_for_position(rel_point, line_height)
                .unwrap_or_else(|idx| idx);
            let safe_byte = line_str.floor_char_boundary(byte_idx);
            return line_str[..safe_byte].chars().count();
        }
        0
    }

    pub fn text_for_utf16_range(
        &self,
        range_utf16: Range<usize>,
        adjusted_range: &mut Option<Range<usize>>,
    ) -> Option<String> {
        let (text, adjusted) = self.buffer.text_for_utf16_range(range_utf16);
        *adjusted_range = Some(adjusted);
        Some(text)
    }

    pub fn selected_utf16_range(&self) -> Option<UTF16Selection> {
        let (range, reversed) = self.buffer.selected_utf16_range();
        Some(UTF16Selection { range, reversed })
    }

    pub fn marked_text_range(&self) -> Option<Range<usize>> {
        self.marked_range.clone()
    }

    pub fn unmark_text(&mut self) {
        self.marked_range = None;
    }

    pub fn replace_text_in_utf16_range(&mut self, range_utf16: Option<Range<usize>>, text: &str) {
        let fallback_marked = self.marked_range.take();
        self.buffer.select_utf16_range(range_utf16, fallback_marked);

        let clean = self.sanitize_input(text);
        self.buffer.insert_text(&clean);
    }

    pub fn replace_and_mark_utf16_range(
        &mut self,
        range_utf16: Option<Range<usize>>,
        new_text: &str,
        new_selected_range: Option<Range<usize>>,
    ) {
        let clean = self.sanitize_input(new_text);
        let fallback_marked = self.marked_range.clone();
        self.marked_range = self.buffer.replace_and_mark_utf16(
            range_utf16,
            fallback_marked,
            &clean,
            new_selected_range,
        );
    }

    pub fn bounds_for_utf16_range(
        &self,
        range_utf16: Range<usize>,
        element_bounds: Bounds<Pixels>,
        window: &mut Window,
    ) -> Option<Bounds<Pixels>> {
        let cursor_col = if range_utf16.is_empty() {
            self.buffer.cursor_pos().col
        } else {
            self.buffer.utf16_offset_to_pos(range_utf16.start).col
        };
        let line_str = self.buffer.line(0).unwrap_or("");
        let font_size = FontSize::SM.to_pixels(window.rem_size());
        let line_height = LineHeight::SM.to_pixels(window.rem_size());
        let (caret_w, _) = calculate_caret_size(font_size, line_height);
        let byte_offset = char_col_to_byte_offset(line_str, cursor_col);

        let run = measurement_run(line_str.len(), window.text_style().font());
        let x = window
            .text_system()
            .shape_text(line_str.to_string().into(), font_size, &[run], None, None)
            .ok()
            .as_ref()
            .and_then(|lines| lines.first())
            .and_then(|l| l.position_for_index(byte_offset, line_height))
            .map(|p| p.x)
            .unwrap_or(px(0.0));

        Some(Bounds {
            origin: point(
                element_bounds.origin.x + x + self.padding_x - self.scroll_offset.get(),
                element_bounds.origin.y,
            ),
            size: size(caret_w, element_bounds.size.height),
        })
    }

    pub fn character_index_for_pixel_point(
        &self,
        point: Point<Pixels>,
        window: &mut Window,
    ) -> Option<usize> {
        let col = self.col_for_point(self.rel_x_for_point(point), window);
        Some(self.buffer.pos_to_utf16_offset(Position::new(0, col)))
    }
}

/// Host contract implemented by views containing an embedded [`TextInput`].
pub(crate) trait TextInputHost: EntityInputHandler + 'static {
    fn text_input(&self) -> &TextInput;
    fn text_input_mut(&mut self) -> &mut TextInput;
    fn on_commit(&mut self, cx: &mut Context<Self>);
    fn on_cancel(&mut self, cx: &mut Context<Self>);
    fn on_mutation(&mut self, cx: &mut Context<Self>) {
        self.text_input_mut().reset_blink_host(cx);
    }

    fn handle_host_key(&mut self, event: &KeyDownEvent, cx: &mut Context<Self>) -> bool {
        match self.text_input_mut().handle_key(event) {
            KeyAction::Commit => {
                cx.stop_propagation();
                self.on_commit(cx);
                true
            }
            KeyAction::Cancel => {
                cx.stop_propagation();
                self.on_cancel(cx);
                true
            }
            KeyAction::Handled => {
                cx.stop_propagation();
                self.on_mutation(cx);
                true
            }
            KeyAction::Ignored => false,
        }
    }
}

impl TextInputHost for TextInput {
    fn text_input(&self) -> &TextInput {
        self
    }

    fn text_input_mut(&mut self) -> &mut TextInput {
        self
    }

    fn on_commit(&mut self, cx: &mut Context<Self>) {
        self.commit(cx);
    }

    fn on_cancel(&mut self, cx: &mut Context<Self>) {
        self.cancel(cx);
    }
}

impl EntityInputHandler for TextInput {
    fn text_for_range(
        &mut self,
        range_utf16: Range<usize>,
        adjusted_range: &mut Option<Range<usize>>,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<String> {
        self.text_for_utf16_range(range_utf16, adjusted_range)
    }

    fn selected_text_range(
        &mut self,
        _ignore_disabled_input: bool,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<UTF16Selection> {
        self.selected_utf16_range()
    }

    fn marked_text_range(
        &self,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<Range<usize>> {
        self.marked_text_range()
    }

    fn unmark_text(&mut self, _window: &mut Window, cx: &mut Context<Self>) {
        self.unmark_text();
        cx.notify();
    }

    fn replace_text_in_range(
        &mut self,
        range_utf16: Option<Range<usize>>,
        text: &str,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.replace_text_in_utf16_range(range_utf16, text);
        self.on_mutation(cx);
    }

    fn replace_and_mark_text_in_range(
        &mut self,
        range_utf16: Option<Range<usize>>,
        new_text: &str,
        new_selected_range: Option<Range<usize>>,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.replace_and_mark_utf16_range(range_utf16, new_text, new_selected_range);
        self.on_mutation(cx);
    }

    fn bounds_for_range(
        &mut self,
        range_utf16: Range<usize>,
        element_bounds: Bounds<Pixels>,
        window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<Bounds<Pixels>> {
        self.bounds_for_utf16_range(range_utf16, element_bounds, window)
    }

    fn character_index_for_point(
        &mut self,
        point: Point<Pixels>,
        window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<usize> {
        self.character_index_for_pixel_point(point, window)
    }
}

struct TextInputPrepaint {
    line: Option<Box<WrappedLine>>,
    selection_quad: Option<Bounds<Pixels>>,
    selection_color: Hsla,
    caret_bounds: Option<Bounds<Pixels>>,
    cursor_color: Hsla,
    scroll_offset: Pixels,
}

#[derive(Clone)]
struct TextInputSnapshot {
    line_str: String,
    selection: Selection,
    cursor_col: usize,
    cursor_opacity: f32,
    placeholder: Option<SharedString>,
    marked_range: Option<Range<usize>>,
    padding_x: Pixels,
    is_dragging: bool,
    scroll_offset: Pixels,
}

fn shape_prepaint(
    snapshot: &TextInputSnapshot,
    bounds: Bounds<Pixels>,
    is_focused: bool,
    theme: &Theme,
    window: &mut Window,
) -> TextInputPrepaint {
    let font_size = FontSize::SM.to_pixels(window.rem_size());
    let line_height = LineHeight::SM.to_pixels(window.rem_size());
    let base_origin_x = bounds.origin.x + snapshot.padding_x;
    let content_origin_y = (bounds.origin.y + (bounds.size.height - line_height) / 2.0).round();
    let (caret_w, caret_h) = calculate_caret_size(font_size, line_height);
    let caret_y = (content_origin_y + (line_height - caret_h) / 2.0).round();

    let cursor_color = if is_focused && snapshot.cursor_opacity > 0.01 {
        theme.cursor.opacity(snapshot.cursor_opacity)
    } else {
        gpui::transparent_black()
    };
    let selection_color = theme.selection;

    let line_str = &snapshot.line_str;
    let selection = snapshot.selection;

    if line_str.is_empty() && snapshot.marked_range.is_none() {
        let placeholder_line = snapshot
            .placeholder
            .as_ref()
            .filter(|p| !p.is_empty())
            .and_then(|p| {
                let run = TextRun {
                    len: p.len(),
                    font: window.text_style().font(),
                    color: theme.text_muted,
                    background_color: None,
                    underline: None,
                    strikethrough: None,
                };
                window
                    .text_system()
                    .shape_text(p.clone(), font_size, &[run], None, None)
                    .ok()
                    .and_then(|lines| lines.into_iter().next())
                    .map(Box::new)
            });

        let caret_bounds = if is_focused
            && selection.is_empty()
            && !snapshot.is_dragging
            && snapshot.cursor_opacity > 0.01
        {
            Some(Bounds::new(
                point(base_origin_x.round(), caret_y),
                size(caret_w, caret_h.round()),
            ))
        } else {
            None
        };

        return TextInputPrepaint {
            line: placeholder_line,
            selection_quad: None,
            selection_color,
            caret_bounds,
            cursor_color,
            scroll_offset: px(0.0),
        };
    }

    let text_run = |len: usize, underline: Option<UnderlineStyle>| TextRun {
        len,
        font: window.text_style().font(),
        color: theme.text_primary,
        background_color: None,
        underline,
        strikethrough: None,
    };

    let mut runs = Vec::new();
    if let Some(marked) = &snapshot.marked_range {
        let start_char = marked.start.min(line_str.chars().count());
        let end_char = marked.end.min(line_str.chars().count());
        let byte_start = char_col_to_byte_offset(line_str, start_char);
        let byte_end = char_col_to_byte_offset(line_str, end_char);

        if byte_start > 0 {
            runs.push(text_run(byte_start, None));
        }
        if byte_end > byte_start {
            runs.push(text_run(
                byte_end - byte_start,
                Some(UnderlineStyle {
                    thickness: px(1.0),
                    color: Some(theme.text_accent),
                    wavy: false,
                }),
            ));
        }
        if line_str.len() > byte_end {
            runs.push(text_run(line_str.len() - byte_end, None));
        }
    } else {
        runs.push(text_run(line_str.len(), None));
    }

    let Some(wrapped_line) = window
        .text_system()
        .shape_text(line_str.clone().into(), font_size, &runs, None, None)
        .ok()
        .and_then(|lines| lines.into_iter().next())
    else {
        return TextInputPrepaint {
            line: None,
            selection_quad: None,
            selection_color,
            caret_bounds: None,
            cursor_color,
            scroll_offset: px(0.0),
        };
    };

    let viewport_width = (bounds.size.width - snapshot.padding_x * 2.0).max(px(0.0));
    let text_width = wrapped_line.unwrapped_layout.width;
    let cursor_byte = char_col_to_byte_offset(line_str, snapshot.cursor_col);
    let cursor_x = wrapped_line
        .position_for_index(cursor_byte, line_height)
        .map(|p| p.x)
        .unwrap_or(px(0.0));

    let scroll_offset = calculate_scroll_offset(
        snapshot.scroll_offset,
        cursor_x,
        caret_w,
        viewport_width,
        text_width,
        px(8.0),
    );
    let content_origin_x = base_origin_x - scroll_offset;

    let selection_quad = if !selection.is_empty() {
        let start_byte = char_col_to_byte_offset(line_str, selection.start().col);
        let end_byte = char_col_to_byte_offset(line_str, selection.end().col);
        let x1 = wrapped_line
            .position_for_index(start_byte, line_height)
            .map(|p| p.x)
            .unwrap_or(px(0.0));
        let x2 = wrapped_line
            .position_for_index(end_byte, line_height)
            .map(|p| p.x)
            .unwrap_or(wrapped_line.unwrapped_layout.width);
        let min_x = x1.min(x2);
        let max_x = x1.max(x2);
        Some(Bounds::from_corners(
            point(content_origin_x + min_x, content_origin_y),
            point(content_origin_x + max_x, content_origin_y + line_height),
        ))
    } else {
        None
    };

    let caret_bounds = if is_focused
        && selection.is_empty()
        && !snapshot.is_dragging
        && snapshot.cursor_opacity > 0.01
    {
        Some(Bounds::new(
            point((content_origin_x + cursor_x).round(), caret_y),
            size(caret_w, caret_h.round()),
        ))
    } else {
        None
    };

    TextInputPrepaint {
        line: Some(Box::new(wrapped_line)),
        selection_quad,
        selection_color,
        caret_bounds,
        cursor_color,
        scroll_offset,
    }
}

fn paint_content(
    prepaint: TextInputPrepaint,
    bounds: Bounds<Pixels>,
    padding_x: Pixels,
    window: &mut Window,
    cx: &mut App,
) {
    let content_origin_x = bounds.origin.x + padding_x - prepaint.scroll_offset;
    let line_height = LineHeight::SM.to_pixels(window.rem_size());
    let content_origin_y = (bounds.origin.y + (bounds.size.height - line_height) / 2.0).round();

    let content_bounds = Bounds::new(
        point(bounds.origin.x + padding_x, bounds.origin.y),
        size(
            (bounds.size.width - padding_x * 2.0).max(px(0.0)),
            bounds.size.height,
        ),
    );

    window.with_content_mask(
        Some(ContentMask {
            bounds: content_bounds,
        }),
        |window| {
            if let Some(sel) = prepaint.selection_quad {
                window.paint_quad(fill(sel, prepaint.selection_color));
            }
            if let Some(line) = prepaint.line {
                let _ = line.paint(
                    point(content_origin_x, content_origin_y),
                    line_height,
                    gpui::TextAlign::Left,
                    None,
                    window,
                    cx,
                );
            }
            if let Some(caret) = prepaint.caret_bounds {
                window.paint_quad(fill(caret, prepaint.cursor_color));
            }
        },
    );
}

fn bind_drag_mouse_events<H: TextInputHost>(entity: Entity<H>, window: &mut Window) {
    let entity_move = entity.clone();
    window.on_mouse_event(
        move |event: &MouseMoveEvent, phase: DispatchPhase, window: &mut Window, cx: &mut App| {
            if phase != DispatchPhase::Capture {
                return;
            }
            let handled = entity_move.update(cx, |this, cx| {
                let input = this.text_input_mut();
                if !input.is_dragging {
                    return false;
                }
                if event.pressed_button != Some(MouseButton::Left) {
                    input.stop_drag();
                    this.on_mutation(cx);
                    return true;
                }
                input.handle_drag_move(event.position, window);
                this.on_mutation(cx);
                true
            });
            if handled {
                cx.stop_propagation();
            }
        },
    );

    let entity_up = entity;
    window.on_mouse_event(
        move |event: &MouseUpEvent, phase: DispatchPhase, _window: &mut Window, cx: &mut App| {
            if phase != DispatchPhase::Capture {
                return;
            }
            if event.button == MouseButton::Left {
                let stopped = entity_up.update(cx, |this, cx| {
                    let input = this.text_input_mut();
                    if input.is_dragging {
                        input.stop_drag();
                        this.on_mutation(cx);
                        true
                    } else {
                        false
                    }
                });
                if stopped {
                    cx.stop_propagation();
                }
            }
        },
    );
}

impl TextInput {
    /// Renders the text input element hosted inside view `H`.
    pub(crate) fn render_hosted<H: TextInputHost>(
        &mut self,
        entity: Entity<H>,
        window: &mut Window,
        cx: &mut Context<H>,
    ) -> impl IntoElement {
        let theme = cx
            .try_global::<ThemeManager>()
            .map(|m| m.theme())
            .unwrap_or(&DEFAULT_THEME);

        let focus_handle = self.focus_handle.clone();
        let prepaint_focus = focus_handle.clone();
        let bounds_cell = self.last_bounds.clone();
        let scroll_cell = self.scroll_offset.clone();
        let is_dragging = self.is_dragging;
        let padding_x = self.padding_x;

        let is_focused = self.focus_handle.is_focused(window);
        if !is_focused && self.blink_task.is_some() {
            self.cursor_opacity = 0.0;
            self.blink_task = None;
        }

        let snapshot = TextInputSnapshot {
            line_str: self.text(),
            selection: self.buffer.selection(),
            cursor_col: self.buffer.cursor_pos().col,
            cursor_opacity: self.cursor_opacity,
            placeholder: self.placeholder.clone(),
            marked_range: self.marked_range.clone(),
            padding_x: self.padding_x,
            is_dragging: self.is_dragging,
            scroll_offset: self.scroll_offset.get(),
        };

        div()
            .id(self.id.clone())
            .key_context(self.key_context)
            .track_focus(&self.focus_handle)
            .on_scroll_wheel(cx.listener(|this, event: &ScrollWheelEvent, _, cx| {
                let delta_x = match event.delta {
                    ScrollDelta::Pixels(p) => {
                        if p.x.abs() > px(0.0) {
                            p.x
                        } else {
                            p.y
                        }
                    }
                    ScrollDelta::Lines(l) => {
                        let dy = if l.x.abs() > 0.0 { l.x } else { l.y };
                        px(dy * 20.0)
                    }
                };
                if delta_x != px(0.0) {
                    let input = this.text_input_mut();
                    let cur = input.scroll_offset.get();
                    let new_offset = (cur - delta_x).max(px(0.0));
                    input.scroll_offset.set(new_offset);
                    this.on_mutation(cx);
                }
            }))
            .on_key_down(cx.listener(|this, event: &KeyDownEvent, _, cx| {
                this.handle_host_key(event, cx);
            }))
            .on_action(cx.listener(|this, _: &UndoAction, _, cx| {
                this.text_input_mut().undo();
                this.on_mutation(cx);
            }))
            .on_action(cx.listener(|this, _: &RedoAction, _, cx| {
                this.text_input_mut().redo();
                this.on_mutation(cx);
            }))
            .on_action(cx.listener(|this, _: &SelectAll, _, cx| {
                this.text_input_mut().select_all();
                this.on_mutation(cx);
            }))
            .on_action(cx.listener(|this, _: &CopyAction, _, cx| {
                this.text_input().copy_selection(cx);
            }))
            .on_action(cx.listener(|this, _: &CutAction, _, cx| {
                if this.text_input_mut().cut_selection(cx) {
                    this.on_mutation(cx);
                }
            }))
            .on_action(cx.listener(|this, _: &PasteAction, _, cx| {
                if this.text_input_mut().paste_from_clipboard(cx) {
                    this.on_mutation(cx);
                }
            }))
            .on_action(cx.listener(|this, _: &MoveLeft, _, cx| {
                this.text_input_mut().buffer.move_left_mode(false, true);
                this.on_mutation(cx);
            }))
            .on_action(cx.listener(|this, _: &SelectLeft, _, cx| {
                this.text_input_mut().buffer.move_left_mode(true, true);
                this.on_mutation(cx);
            }))
            .on_action(cx.listener(|this, _: &MoveRight, _, cx| {
                this.text_input_mut().buffer.move_right_mode(false, true);
                this.on_mutation(cx);
            }))
            .on_action(cx.listener(|this, _: &SelectRight, _, cx| {
                this.text_input_mut().buffer.move_right_mode(true, true);
                this.on_mutation(cx);
            }))
            .on_action(cx.listener(|this, _: &MoveToPreviousWord, _, cx| {
                this.text_input_mut().buffer.move_to_previous_word(false);
                this.on_mutation(cx);
            }))
            .on_action(cx.listener(|this, _: &SelectToPreviousWord, _, cx| {
                this.text_input_mut().buffer.move_to_previous_word(true);
                this.on_mutation(cx);
            }))
            .on_action(cx.listener(|this, _: &MoveToNextWord, _, cx| {
                this.text_input_mut().buffer.move_to_next_word(false);
                this.on_mutation(cx);
            }))
            .on_action(cx.listener(|this, _: &SelectToNextWord, _, cx| {
                this.text_input_mut().buffer.move_to_next_word(true);
                this.on_mutation(cx);
            }))
            .on_action(cx.listener(|this, _: &MoveToBeginningOfLine, _, cx| {
                this.text_input_mut()
                    .buffer
                    .move_to_line_start_mode(false, true);
                this.on_mutation(cx);
            }))
            .on_action(cx.listener(|this, _: &SelectToBeginningOfLine, _, cx| {
                this.text_input_mut()
                    .buffer
                    .move_to_line_start_mode(true, true);
                this.on_mutation(cx);
            }))
            .on_action(cx.listener(|this, _: &MoveToEndOfLine, _, cx| {
                this.text_input_mut().buffer.move_to_line_end(false);
                this.on_mutation(cx);
            }))
            .on_action(cx.listener(|this, _: &SelectToEndOfLine, _, cx| {
                this.text_input_mut().buffer.move_to_line_end(true);
                this.on_mutation(cx);
            }))
            .on_action(cx.listener(|this, _: &Backspace, _, cx| {
                this.text_input_mut().buffer.backspace_mode(true);
                this.on_mutation(cx);
            }))
            .on_action(cx.listener(|this, _: &Delete, _, cx| {
                this.text_input_mut().buffer.delete_forward_mode(true);
                this.on_mutation(cx);
            }))
            .on_action(cx.listener(|this, _: &DeleteToPreviousWord, _, cx| {
                this.text_input_mut().buffer.delete_to_previous_word();
                this.on_mutation(cx);
            }))
            .on_action(cx.listener(|this, _: &DeleteToNextWord, _, cx| {
                this.text_input_mut().buffer.delete_to_next_word();
                this.on_mutation(cx);
            }))
            .h(self.height)
            .flex_1()
            .overflow_hidden()
            .bg(theme.bg_editor)
            .border_1()
            .border_color(theme.text_accent)
            .rounded(self.radius)
            .cursor_text()
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, event: &MouseDownEvent, window, cx| {
                    cx.stop_propagation();
                    window.focus(&this.text_input().focus_handle);
                    let col = this
                        .text_input()
                        .col_for_point(this.text_input().rel_x_for_point(event.position), window);
                    let input = this.text_input_mut();
                    if event.click_count == 2 {
                        input.buffer.select_word_at(Position::new(0, col));
                        input.is_dragging = false;
                        input.drag_anchor = None;
                    } else if event.click_count == 3 {
                        input.buffer.select_all();
                        input.is_dragging = false;
                        input.drag_anchor = None;
                    } else {
                        input.start_drag(col);
                    }
                    this.on_mutation(cx);
                }),
            )
            .child(
                canvas(
                    move |bounds, window, cx| {
                        bounds_cell.set(bounds);
                        let theme = cx
                            .try_global::<ThemeManager>()
                            .map(|m| m.theme())
                            .unwrap_or(&DEFAULT_THEME);

                        let focused = prepaint_focus.is_focused(window);
                        let prepaint = shape_prepaint(&snapshot, bounds, focused, theme, window);
                        scroll_cell.set(prepaint.scroll_offset);
                        prepaint
                    },
                    move |bounds, prepaint, window, cx| {
                        window.handle_input(
                            &focus_handle,
                            ElementInputHandler::new(bounds, entity.clone()),
                            cx,
                        );

                        if is_dragging {
                            bind_drag_mouse_events(entity, window);
                        }

                        paint_content(prepaint, bounds, padding_x, window, cx);
                    },
                )
                .size_full(),
            )
    }
}

impl Render for TextInput {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let entity = cx.entity().clone();
        self.render_hosted(entity, window, cx)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[gpui::test]
    fn test_text_input_commit_and_cancel_keys(cx: &mut gpui::TestAppContext) {
        let (input, cx) = cx.add_window_view(|_window, cx| TextInput::new("hello world", cx));

        cx.update(|_window, cx| {
            input.update(cx, |inp, _cx| {
                let enter_event = KeyDownEvent {
                    keystroke: gpui::Keystroke::parse("enter").unwrap(),
                    is_held: false,
                };
                assert_eq!(inp.handle_key(&enter_event), KeyAction::Commit);

                let esc_event = KeyDownEvent {
                    keystroke: gpui::Keystroke::parse("escape").unwrap(),
                    is_held: false,
                };
                assert_eq!(inp.handle_key(&esc_event), KeyAction::Cancel);

                let other_event = KeyDownEvent {
                    keystroke: gpui::Keystroke::parse("left").unwrap(),
                    is_held: false,
                };
                assert_eq!(inp.handle_key(&other_event), KeyAction::Ignored);
            });
        });
    }

    #[gpui::test]
    fn test_text_input_sanitization_and_filtering(cx: &mut gpui::TestAppContext) {
        let (input, cx) = cx.add_window_view(|_window, cx| TextInput::new("", cx));

        cx.update(|_window, cx| {
            input.update(cx, |inp, _cx| {
                // Default single-line filter strips \r and \n
                inp.replace_text_in_utf16_range(None, "foo\r\nbar\nbaz");
                assert_eq!(inp.text(), "foobarbaz");

                // Custom filter: alphanumeric only
                inp.clear_buffer();
                inp.char_filter = |c| c.is_alphanumeric();
                inp.replace_text_in_utf16_range(None, "path/to-file_123.rs");
                assert_eq!(inp.text(), "pathtofile123rs");
            });
        });
    }

    #[gpui::test]
    fn test_text_input_clipboard_and_undo_stack(cx: &mut gpui::TestAppContext) {
        let (input, cx) = cx.add_window_view(|_window, cx| TextInput::new("initial", cx));

        cx.update(|_window, cx| {
            input.update(cx, |inp, _cx| {
                inp.replace_text_in_utf16_range(None, " replaced");
                assert_eq!(inp.text(), "initial replaced");

                inp.undo();
                assert_eq!(inp.text(), "initial");

                inp.redo();
                assert_eq!(inp.text(), "initial replaced");

                inp.select_all();
                assert_eq!(
                    inp.buffer().selected_text(),
                    Some("initial replaced".to_string())
                );
            });
        });
    }

    #[gpui::test]
    fn test_text_input_ime_and_utf16_ranges(cx: &mut gpui::TestAppContext) {
        let (input, cx) = cx.add_window_view(|_window, cx| TextInput::new("你好世界", cx));

        cx.update(|_window, cx| {
            input.update(cx, |inp, _cx| {
                let mut adjusted = None;
                assert_eq!(
                    inp.text_for_utf16_range(0..2, &mut adjusted),
                    Some("你好".to_string())
                );

                // Composition marked range
                inp.replace_and_mark_utf16_range(Some(2..4), "测试", Some(0..2));
                assert_eq!(inp.text(), "你好测试");
                assert_eq!(inp.marked_text_range(), Some(2..4));

                inp.unmark_text();
                assert_eq!(inp.marked_text_range(), None);
            });
        });
    }

    #[gpui::test]
    fn test_text_input_lifecycle_and_events(cx: &mut gpui::TestAppContext) {
        let (input, cx) = cx.add_window_view(|_window, cx| {
            TextInput::new("event_test", cx)
                .placeholder("enter text...")
                .id("lifecycle_input")
        });

        let committed = std::rc::Rc::new(std::cell::RefCell::new(None));
        let committed_sub = committed.clone();
        let cancelled = std::rc::Rc::new(std::cell::Cell::new(false));
        let cancelled_sub = cancelled.clone();

        let _sub = cx.update(|_window, cx| {
            cx.subscribe(
                &input,
                move |_emitter, event: &TextInputEvent, _cx| match event {
                    TextInputEvent::Commit(text) => {
                        *committed_sub.borrow_mut() = Some(text.clone())
                    }
                    TextInputEvent::Cancel => cancelled_sub.set(true),
                },
            )
        });

        cx.update(|_window, cx| {
            input.update(cx, |inp, cx| {
                inp.commit(cx);
            });
        });
        cx.run_until_parked();
        assert_eq!(*committed.borrow(), Some("event_test".to_string()));

        cx.update(|_window, cx| {
            input.update(cx, |inp, cx| {
                inp.cancel(cx);
            });
        });
        cx.run_until_parked();
        assert!(cancelled.get());
    }

    #[test]
    fn test_calculate_scroll_offset() {
        // Fits entirely within viewport -> 0.0
        assert_eq!(
            calculate_scroll_offset(px(0.0), px(20.0), px(2.0), px(100.0), px(60.0), px(8.0)),
            px(0.0)
        );

        // Long text, cursor at origin (x=0) -> 0.0
        assert_eq!(
            calculate_scroll_offset(px(0.0), px(0.0), px(2.0), px(100.0), px(300.0), px(8.0)),
            px(0.0)
        );

        // Cursor moves past right bound:
        // text_width = 300, viewport = 100, margin = 8, caret = 2
        // cursor_x = 120. right_bound = 120 + 2 + 8 = 130.
        // new_scroll = 130 - 100 = 30.
        assert_eq!(
            calculate_scroll_offset(px(0.0), px(120.0), px(2.0), px(100.0), px(300.0), px(8.0)),
            px(30.0)
        );

        // Cursor reaches end of text:
        // cursor_x = 300. total_width = 302. max_scroll = 302 - 100 = 202.
        assert_eq!(
            calculate_scroll_offset(px(50.0), px(300.0), px(2.0), px(100.0), px(300.0), px(8.0)),
            px(202.0)
        );

        // Cursor moves backward towards left:
        // current_scroll = 202. cursor_x = 50.
        // left_bound = 50 - 8 = 42 < 202.
        // new_scroll = 42.
        assert_eq!(
            calculate_scroll_offset(px(202.0), px(50.0), px(2.0), px(100.0), px(300.0), px(8.0)),
            px(42.0)
        );

        // Cursor back to start:
        assert_eq!(
            calculate_scroll_offset(px(42.0), px(0.0), px(2.0), px(100.0), px(300.0), px(8.0)),
            px(0.0)
        );

        // Text shrinks / deletion: current_scroll was 150, but text_width became 120.
        // total_width = 122, max_scroll = 22.
        // clamped to max_scroll: 22.
        assert_eq!(
            calculate_scroll_offset(px(150.0), px(120.0), px(2.0), px(100.0), px(120.0), px(8.0)),
            px(22.0)
        );
    }

    #[gpui::test]
    fn test_text_input_horizontal_scrolling(cx: &mut gpui::TestAppContext) {
        let (input, cx) = cx.add_window_view(|_window, cx| {
            TextInput::new(
                "this_is_a_very_long_file_name_that_should_definitely_exceed_normal_width_limit.rs",
                cx,
            )
        });

        cx.update(|_window, cx| {
            input.update(cx, |inp, _cx| {
                // Initial state
                assert_eq!(inp.scroll_offset(), px(0.0));

                // Manually setting scroll offset
                inp.set_scroll_offset(px(45.0));
                assert_eq!(inp.scroll_offset(), px(45.0));

                // Relative X calculation with scroll offset
                // bounds origin = 0, padding_x = 4 (Spacing::XS), scroll_offset = 45
                inp.last_bounds.set(Bounds::new(
                    point(px(0.0), px(0.0)),
                    size(px(100.0), px(20.0)),
                ));
                // Screen X = 4 -> rel_x = 4 - 0 - 4 + 45 = 45
                let rel_x = inp.rel_x_for_point(point(px(4.0), px(10.0)));
                assert_eq!(rel_x, px(45.0));

                // clear_buffer resets scroll offset
                inp.clear_buffer();
                assert_eq!(inp.scroll_offset(), px(0.0));
            });
        });
    }

    #[gpui::test]
    fn test_text_input_action_dispatch(cx: &mut gpui::TestAppContext) {
        cx.update(|cx| {
            crate::app::actions::bind_default_bindings(cx);
        });
        let (input, cx) = cx.add_window_view(|_window, cx| TextInput::new("hello world", cx));

        cx.update(|window, cx| {
            window.focus(&input.read(cx).focus_handle);
            assert!(input.read(cx).focus_handle(cx).is_focused(window));
        });

        // Dispatch MoveLeft
        cx.dispatch_action(MoveLeft);
        cx.run_until_parked();
        cx.update(|_window, cx| {
            assert_eq!(input.read(cx).buffer().cursor_pos().col, 10);
        });

        // Dispatch MoveToBeginningOfLine
        cx.dispatch_action(MoveToBeginningOfLine);
        cx.run_until_parked();
        cx.update(|_window, cx| {
            assert_eq!(input.read(cx).buffer().cursor_pos().col, 0);
        });

        // Dispatch MoveToEndOfLine
        cx.dispatch_action(MoveToEndOfLine);
        cx.run_until_parked();
        cx.update(|_window, cx| {
            assert_eq!(input.read(cx).buffer().cursor_pos().col, 11);
        });

        // Dispatch DeleteToPreviousWord
        cx.dispatch_action(DeleteToPreviousWord);
        cx.run_until_parked();
        cx.update(|_window, cx| {
            assert_eq!(input.read(cx).text(), "hello ");
        });
    }
}
