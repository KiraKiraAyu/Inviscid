use gpui::prelude::*;
use gpui::*;
use std::cell::Cell;
use std::ops::Range;
use std::path::PathBuf;
use std::rc::Rc;

use crate::buffer::{Position, Selection, TextBuffer};
use crate::editor::actions::{
    CopyAction, CutAction, PasteAction, RedoAction, SelectAll, UndoAction,
};
use crate::theme::{DEFAULT_THEME, ThemeManager};
use crate::ui::{calculate_caret_size, start_cursor_animation};

/// Target type and parameters for inline text input (New File, New Folder, Rename).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum InlineInputKind {
    NewFile { parent: PathBuf },
    NewFolder { parent: PathBuf },
    Rename { target: PathBuf, is_dir: bool },
}

/// Events emitted by InlineInput on user completion or cancellation.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum InlineInputEvent {
    Commit(String),
    Cancel,
}

/// Rich inline text input entity for ProjectPanel file and folder naming.
///
/// Reuses the editor's text engine (`TextBuffer`), cursor navigation, subpixel
/// mouse hit-testing, smooth cosine breathing animation, and native GPUI `EntityInputHandler` for seamless IME typing.
pub struct InlineInput {
    pub(crate) buffer: TextBuffer,
    pub(crate) focus_handle: FocusHandle,
    pub(crate) kind: InlineInputKind,
    pub(crate) depth: usize,
    pub(crate) marked_range: Option<Range<usize>>,
    pub(crate) cursor_opacity: f32,
    pub(crate) blink_epoch: usize,
    pub(crate) blink_task: Option<Task<()>>,
    pub(crate) is_dragging: bool,
    pub(crate) drag_anchor: Option<usize>,
    pub(crate) last_bounds: Rc<Cell<Bounds<Pixels>>>,
}

impl Focusable for InlineInput {
    fn focus_handle(&self, _cx: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl EventEmitter<InlineInputEvent> for InlineInput {}

impl InlineInput {
    pub fn new(
        kind: InlineInputKind,
        initial_text: &str,
        depth: usize,
        cx: &mut Context<Self>,
    ) -> Self {
        let focus_handle = cx.focus_handle();
        let mut buffer = TextBuffer::from_str(initial_text, None);

        // For Rename: pre-select the filename stem (excluding extension if file)
        match &kind {
            InlineInputKind::Rename { is_dir, .. } => {
                let char_count = initial_text.chars().count();
                let stem_len = if *is_dir {
                    char_count
                } else if let Some(dot_idx) = initial_text.rfind('.') {
                    if dot_idx > 0 {
                        initial_text[..dot_idx].chars().count()
                    } else {
                        char_count
                    }
                } else {
                    char_count
                };
                buffer.set_selection(Selection {
                    anchor: Position::new(0, 0),
                    head: Position::new(0, stem_len),
                });
            }
            _ => {
                let char_count = initial_text.chars().count();
                buffer.set_selection(Selection {
                    anchor: Position::new(0, char_count),
                    head: Position::new(0, char_count),
                });
            }
        }

        let mut input = Self {
            buffer,
            focus_handle,
            kind,
            depth,
            marked_range: None,
            cursor_opacity: 1.0,
            blink_epoch: 0,
            blink_task: None,
            is_dragging: false,
            drag_anchor: None,
            last_bounds: Rc::new(Cell::new(Bounds::default())),
        };
        input.reset_cursor_blink(cx);
        input
    }

    #[inline]
    pub fn text(&self) -> String {
        self.buffer.line(0).unwrap_or("").to_string()
    }

    #[inline]
    pub fn kind(&self) -> &InlineInputKind {
        &self.kind
    }

    #[inline]
    pub fn depth(&self) -> usize {
        self.depth
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

    /// Resets cursor to solid visible (1.0) and starts the unified cursor animation.
    /// Guarded by `blink_epoch` to avoid race conditions.
    pub fn reset_cursor_blink(&mut self, cx: &mut Context<Self>) {
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
                |input| input.blink_epoch,
                |input, opacity, cx| {
                    input.cursor_opacity = opacity;
                    cx.notify();
                },
            ));
        } else {
            self.blink_task = None;
        }
    }

    /// Called when the inline input loses focus. Immediately stops the animation
    /// and resets opacity to 0.0 to eliminate background GPU/CPU wakeups.
    pub fn on_blur(&mut self, cx: &mut Context<Self>) {
        self.cursor_opacity = 0.0;
        self.blink_epoch = self.blink_epoch.wrapping_add(1);
        self.blink_task = None;
        cx.notify();
    }

    /// Called when the inline input gains focus. Restores solid visible cursor and starts
    /// the breathing animation if enabled in settings.
    pub fn on_focus(&mut self, cx: &mut Context<Self>) {
        self.reset_cursor_blink(cx);
    }

    /// Begins mouse drag selection at the specified column.
    pub fn start_drag(&mut self, col: usize, cx: &mut Context<Self>) {
        self.buffer
            .set_selection(Selection::cursor(Position::new(0, col)));
        self.is_dragging = true;
        self.drag_anchor = Some(col);
        self.on_mutation(cx);
    }

    /// Updates drag selection as the mouse moves across or outside the input box.
    pub fn handle_drag_move(
        &mut self,
        point: Point<Pixels>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.is_dragging {
            return;
        }
        let bounds = self.last_bounds.get();
        let rel_x = (point.x - bounds.origin.x - px(4.0)).max(px(0.0));
        let col = self.col_for_point(rel_x, window);
        if let Some(anchor) = self.drag_anchor {
            self.buffer.set_selection(Selection {
                anchor: Position::new(0, anchor),
                head: Position::new(0, col),
            });
            self.on_mutation(cx);
        }
    }

    /// Ends active mouse drag selection.
    pub fn stop_drag(&mut self, cx: &mut Context<Self>) {
        if self.is_dragging {
            self.is_dragging = false;
            self.drag_anchor = None;
            cx.notify();
        }
    }

    pub fn on_mutation(&mut self, cx: &mut Context<Self>) {
        self.reset_cursor_blink(cx);
        cx.notify();
    }

    pub fn commit(&mut self, cx: &mut Context<Self>) {
        let text = self.buffer.line(0).unwrap_or("").trim().to_string();
        cx.emit(InlineInputEvent::Commit(text));
    }

    pub fn cancel(&mut self, cx: &mut Context<Self>) {
        cx.emit(InlineInputEvent::Cancel);
    }

    /// Subpixel hit-testing calculating character column from relative X offset
    pub fn col_for_point(&self, rel_x: Pixels, window: &mut Window) -> usize {
        let line_str = self.buffer.line(0).unwrap_or("").to_string();
        if line_str.is_empty() {
            return 0;
        }
        let font_size = px(12.0);
        let line_height = px(16.0);
        let run = TextRun {
            len: line_str.len(),
            font: window.text_style().font(),
            color: gpui::transparent_black(),
            background_color: None,
            underline: None,
            strikethrough: None,
        };
        if let Ok(lines) = window.text_system().shape_text(
            SharedString::from(line_str.clone()),
            font_size,
            &[run],
            None,
            None,
        ) {
            if let Some(line) = lines.first() {
                let rel_point = point(rel_x, px(0.0));
                let byte_idx = match line.closest_index_for_position(rel_point, line_height) {
                    Ok(idx) => idx,
                    Err(idx) => idx,
                };
                let mut safe_byte = byte_idx.min(line_str.len());
                while safe_byte > 0 && !line_str.is_char_boundary(safe_byte) {
                    safe_byte -= 1;
                }
                let char_col = line_str[..safe_byte].chars().count();
                return char_col;
            }
        }
        0
    }

    /// Handles the keys this input owns itself.
    ///
    /// Only navigation, deletion and commit/cancel are handled here. Undo/Redo/Cut/Copy/Paste/
    /// SelectAll are deliberately absent: the keymap binds them with the `Editor || InlineInput`
    /// context, and a matched binding is consumed before element `on_key_down` listeners run
    /// (`gpui-0.2.2/src/window.rs:3834`), so a copy of that logic here would never execute. The
    /// element's `on_action` handlers below are the live implementation.
    pub fn handle_key_down(&mut self, event: &KeyDownEvent, cx: &mut Context<Self>) {
        let key = &event.keystroke.key;
        if key.eq_ignore_ascii_case("enter") {
            cx.stop_propagation();
            self.commit(cx);
        } else if key.eq_ignore_ascii_case("escape") {
            cx.stop_propagation();
            self.cancel(cx);
        } else if key.eq_ignore_ascii_case("backspace") {
            cx.stop_propagation();
            let is_word_del = if cfg!(target_os = "macos") {
                event.keystroke.modifiers.alt
            } else {
                event.keystroke.modifiers.control || event.keystroke.modifiers.secondary()
            };
            if is_word_del {
                self.buffer.delete_to_previous_word();
            } else {
                self.buffer.backspace_mode(true);
            }
            self.on_mutation(cx);
        } else if key.eq_ignore_ascii_case("delete") {
            cx.stop_propagation();
            let is_word_del = if cfg!(target_os = "macos") {
                event.keystroke.modifiers.alt
            } else {
                event.keystroke.modifiers.control || event.keystroke.modifiers.secondary()
            };
            if is_word_del {
                self.buffer.delete_to_next_word();
            } else {
                self.buffer.delete_forward_mode(true);
            }
            self.on_mutation(cx);
        } else if key.eq_ignore_ascii_case("left") {
            cx.stop_propagation();
            let shift = event.keystroke.modifiers.shift;
            if cfg!(target_os = "macos") {
                if event.keystroke.modifiers.platform {
                    self.buffer.move_to_line_start_mode(shift, false);
                } else if event.keystroke.modifiers.alt {
                    self.buffer.move_to_previous_word(shift);
                } else {
                    self.buffer.move_left_mode(shift, false);
                }
            } else {
                let word = event.keystroke.modifiers.control
                    || event.keystroke.modifiers.secondary()
                    || event.keystroke.modifiers.alt;
                if word {
                    self.buffer.move_to_previous_word(shift);
                } else {
                    self.buffer.move_left_mode(shift, false);
                }
            }
            self.on_mutation(cx);
        } else if key.eq_ignore_ascii_case("right") {
            cx.stop_propagation();
            let shift = event.keystroke.modifiers.shift;
            if cfg!(target_os = "macos") {
                if event.keystroke.modifiers.platform {
                    self.buffer.move_to_line_end(shift);
                } else if event.keystroke.modifiers.alt {
                    self.buffer.move_to_next_word(shift);
                } else {
                    self.buffer.move_right_mode(shift, false);
                }
            } else {
                let word = event.keystroke.modifiers.control
                    || event.keystroke.modifiers.secondary()
                    || event.keystroke.modifiers.alt;
                if word {
                    self.buffer.move_to_next_word(shift);
                } else {
                    self.buffer.move_right_mode(shift, false);
                }
            }
            self.on_mutation(cx);
        } else if key.eq_ignore_ascii_case("home") {
            cx.stop_propagation();
            let shift = event.keystroke.modifiers.shift;
            self.buffer.move_to_line_start_mode(shift, false);
            self.on_mutation(cx);
        } else if key.eq_ignore_ascii_case("end") {
            cx.stop_propagation();
            let shift = event.keystroke.modifiers.shift;
            self.buffer.move_to_line_end(shift);
            self.on_mutation(cx);
        }
    }
    /// Strips invalid characters for file/folder names (newlines and path separators).
    fn sanitize_filename_input(text: &str) -> String {
        text.chars()
            .filter(|c| *c != '\n' && *c != '\r' && *c != '/' && *c != '\\')
            .collect()
    }
}

impl EntityInputHandler for InlineInput {
    fn text_for_range(
        &mut self,
        range_utf16: Range<usize>,
        adjusted_range: &mut Option<Range<usize>>,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<String> {
        let (text, adjusted) = self.buffer.text_for_utf16_range(range_utf16);
        *adjusted_range = Some(adjusted);
        Some(text)
    }

    fn selected_text_range(
        &mut self,
        _ignore_disabled_input: bool,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<UTF16Selection> {
        let (range, reversed) = self.buffer.selected_utf16_range();
        Some(UTF16Selection { range, reversed })
    }

    fn marked_text_range(
        &self,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<Range<usize>> {
        self.marked_range.clone()
    }

    fn unmark_text(&mut self, _window: &mut Window, cx: &mut Context<Self>) {
        self.marked_range = None;
        cx.notify();
    }

    fn replace_text_in_range(
        &mut self,
        range_utf16: Option<Range<usize>>,
        text: &str,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let fallback_marked = self.marked_range.take();
        self.buffer.select_utf16_range(range_utf16, fallback_marked);

        let clean = Self::sanitize_filename_input(text);
        self.buffer.insert_text(&clean);

        self.marked_range = None;
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
        let clean = Self::sanitize_filename_input(new_text);
        let fallback_marked = self.marked_range.clone();
        self.marked_range = self.buffer.replace_and_mark_utf16(
            range_utf16,
            fallback_marked,
            &clean,
            new_selected_range,
        );
        self.on_mutation(cx);
    }

    fn bounds_for_range(
        &mut self,
        range_utf16: Range<usize>,
        element_bounds: Bounds<Pixels>,
        window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<Bounds<Pixels>> {
        let cursor_col = if range_utf16.is_empty() {
            self.buffer.cursor_pos().col
        } else {
            self.buffer.utf16_offset_to_pos(range_utf16.start).col
        };
        let line_str = self.buffer.line(0).unwrap_or("").to_string();
        let font_size = px(12.0);
        let line_height = px(16.0);
        let byte_offset = line_str
            .chars()
            .take(cursor_col)
            .map(|c| c.len_utf8())
            .sum::<usize>();

        let run = TextRun {
            len: line_str.len(),
            font: window.text_style().font(),
            color: gpui::transparent_black(),
            background_color: None,
            underline: None,
            strikethrough: None,
        };
        let x = if let Ok(lines) = window.text_system().shape_text(
            SharedString::from(line_str),
            font_size,
            &[run],
            None,
            None,
        ) {
            lines
                .first()
                .and_then(|l| l.position_for_index(byte_offset, line_height))
                .map(|p| p.x)
                .unwrap_or(px(0.0))
        } else {
            px(0.0)
        };

        Some(Bounds {
            origin: point(
                element_bounds.origin.x + x + px(4.0),
                element_bounds.origin.y,
            ),
            size: size(px(2.0), element_bounds.size.height),
        })
    }

    fn character_index_for_point(
        &mut self,
        point: Point<Pixels>,
        window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<usize> {
        let rel_x = (point.x - self.last_bounds.get().origin.x - px(4.0)).max(px(0.0));
        let col = self.col_for_point(rel_x, window);
        Some(self.buffer.pos_to_utf16_offset(Position::new(0, col)))
    }
}

enum InlinePrepaintState {
    Empty {
        caret_bounds: Option<Bounds<Pixels>>,
        cursor_color: Hsla,
    },
    Shaped {
        wrapped_line: Box<WrappedLine>,
        selection_quad: Option<Bounds<Pixels>>,
        selection_color: Hsla,
        caret_bounds: Option<Bounds<Pixels>>,
        cursor_color: Hsla,
    },
}

impl Render for InlineInput {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx
            .try_global::<ThemeManager>()
            .map(|m| m.theme())
            .unwrap_or(&DEFAULT_THEME);

        let entity = cx.entity().clone();
        let focus_handle = self.focus_handle.clone();
        let prepaint_focus = focus_handle.clone();
        let bounds_cell = self.last_bounds.clone();

        let line_str = self.buffer.line(0).unwrap_or("").to_string();
        let cursor_col = self.buffer.cursor_pos().col;
        let selection = self.buffer.selection();
        let is_dragging = self.is_dragging;

        let is_focused = self.focus_handle.is_focused(_window);
        if !is_focused && self.blink_task.is_some() {
            self.cursor_opacity = 0.0;
            self.blink_task = None;
        }

        let cursor_opacity = self.cursor_opacity;
        let marked_range = self.marked_range.clone();

        div()
            .id("inline_input_box")
            .key_context(crate::ui::key_context::INLINE_INPUT)
            .track_focus(&self.focus_handle)
            .on_key_down(cx.listener(|this, event: &KeyDownEvent, _, cx| {
                this.handle_key_down(event, cx);
            }))
            // These 6 actions are bound with context `Editor || InlineInput`. Without handlers
            // here they bubble up to InviscidApp and mutate the *editor* buffer instead.
            .on_action(cx.listener(|this, _: &UndoAction, _, cx| {
                this.buffer.undo();
                this.on_mutation(cx);
            }))
            .on_action(cx.listener(|this, _: &RedoAction, _, cx| {
                this.buffer.redo();
                this.on_mutation(cx);
            }))
            .on_action(cx.listener(|this, _: &SelectAll, _, cx| {
                this.buffer.select_all();
                this.on_mutation(cx);
            }))
            .on_action(cx.listener(|this, _: &CopyAction, _, cx| {
                if let Some(text) = this.buffer.selected_text() {
                    cx.write_to_clipboard(ClipboardItem::new_string(text));
                }
            }))
            .on_action(cx.listener(|this, _: &CutAction, _, cx| {
                if let Some(text) = this.buffer.selected_text() {
                    cx.write_to_clipboard(ClipboardItem::new_string(text));
                    this.buffer.insert_text("");
                    this.on_mutation(cx);
                }
            }))
            .on_action(cx.listener(|this, _: &PasteAction, _, cx| {
                if let Some(item) = cx.read_from_clipboard()
                    && let Some(text) = item.text()
                {
                    // The input names one file or directory: newlines would corrupt the path.
                    this.buffer.insert_text(&text.replace(['\r', '\n'], " "));
                    this.on_mutation(cx);
                }
            }))
            .h(px(20.0))
            .flex_1()
            .overflow_hidden()
            .bg(theme.bg_editor)
            .border_1()
            .border_color(theme.text_accent)
            .rounded(px(2.0))
            .cursor_text()
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, event: &MouseDownEvent, window, cx| {
                    cx.stop_propagation();
                    window.focus(&this.focus_handle);
                    let rel_x =
                        (event.position.x - this.last_bounds.get().origin.x - px(4.0)).max(px(0.0));
                    let col = this.col_for_point(rel_x, window);
                    if event.click_count == 2 {
                        this.buffer.select_word_at(Position::new(0, col));
                        this.is_dragging = false;
                        this.drag_anchor = None;
                    } else if event.click_count == 3 {
                        this.buffer.select_all();
                        this.is_dragging = false;
                        this.drag_anchor = None;
                    } else {
                        this.start_drag(col, cx);
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

                        let is_focused = prepaint_focus.is_focused(window);
                        let font_size = px(12.0);
                        let line_height = px(16.0);
                        let content_origin_x = bounds.origin.x + px(4.0);
                        let content_origin_y =
                            (bounds.origin.y + (bounds.size.height - line_height) / 2.0).round();
                        let (caret_w, caret_h) = calculate_caret_size(font_size, line_height);
                        let caret_y = (content_origin_y + (line_height - caret_h) / 2.0).round();

                        let cursor_color = if is_focused && cursor_opacity > 0.01 {
                            theme.cursor.opacity(cursor_opacity)
                        } else {
                            gpui::transparent_black()
                        };
                        let selection_color = theme.selection;

                        if line_str.is_empty() && marked_range.is_none() {
                            let caret_bounds = if is_focused
                                && selection.is_empty()
                                && !is_dragging
                                && cursor_opacity > 0.01
                            {
                                Some(Bounds::new(
                                    point(content_origin_x.round(), caret_y),
                                    size(caret_w, caret_h.round()),
                                ))
                            } else {
                                None
                            };
                            return InlinePrepaintState::Empty {
                                caret_bounds,
                                cursor_color,
                            };
                        }

                        // Build text runs, adding underline for marked IME text if active
                        let mut runs = Vec::new();
                        if let Some(marked) = &marked_range {
                            let start_char = marked.start.min(line_str.chars().count());
                            let end_char = marked.end.min(line_str.chars().count());
                            let byte_start = line_str
                                .chars()
                                .take(start_char)
                                .map(|c| c.len_utf8())
                                .sum();
                            let byte_end = line_str
                                .chars()
                                .take(end_char)
                                .map(|c| c.len_utf8())
                                .sum::<usize>();

                            if byte_start > 0 {
                                runs.push(TextRun {
                                    len: byte_start,
                                    font: window.text_style().font(),
                                    color: theme.text_primary,
                                    background_color: None,
                                    underline: None,
                                    strikethrough: None,
                                });
                            }
                            if byte_end > byte_start {
                                runs.push(TextRun {
                                    len: byte_end - byte_start,
                                    font: window.text_style().font(),
                                    color: theme.text_primary,
                                    background_color: None,
                                    underline: Some(UnderlineStyle {
                                        thickness: px(1.0),
                                        color: Some(theme.text_accent),
                                        wavy: false,
                                    }),
                                    strikethrough: None,
                                });
                            }
                            if line_str.len() > byte_end {
                                runs.push(TextRun {
                                    len: line_str.len() - byte_end,
                                    font: window.text_style().font(),
                                    color: theme.text_primary,
                                    background_color: None,
                                    underline: None,
                                    strikethrough: None,
                                });
                            }
                        } else {
                            runs.push(TextRun {
                                len: line_str.len(),
                                font: window.text_style().font(),
                                color: theme.text_primary,
                                background_color: None,
                                underline: None,
                                strikethrough: None,
                            });
                        }

                        let shaped_lines = window.text_system().shape_text(
                            SharedString::from(line_str.clone()),
                            font_size,
                            &runs,
                            None,
                            None,
                        );

                        let Ok(lines) = shaped_lines else {
                            return InlinePrepaintState::Empty {
                                caret_bounds: None,
                                cursor_color,
                            };
                        };

                        let Some(wrapped_line) = lines.into_iter().next() else {
                            return InlinePrepaintState::Empty {
                                caret_bounds: None,
                                cursor_color,
                            };
                        };

                        // Compute selection quad
                        let selection_quad = if !selection.is_empty() {
                            let (s_start, s_end) = (selection.start().col, selection.end().col);
                            let mut start_byte = 0;
                            let mut end_byte = 0;
                            for (i, c) in line_str.chars().enumerate() {
                                if i < s_start {
                                    start_byte += c.len_utf8();
                                }
                                if i < s_end {
                                    end_byte += c.len_utf8();
                                }
                            }
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

                        // Compute caret quad
                        let caret_bounds = if is_focused
                            && selection.is_empty()
                            && !is_dragging
                            && cursor_opacity > 0.01
                        {
                            let mut cursor_byte: usize = 0;
                            for (i, c) in line_str.chars().enumerate() {
                                if i == cursor_col {
                                    break;
                                }
                                cursor_byte += c.len_utf8();
                            }
                            let pos = wrapped_line
                                .position_for_index(cursor_byte, line_height)
                                .unwrap_or_default();
                            Some(Bounds::new(
                                point((content_origin_x + pos.x).round(), caret_y),
                                size(caret_w, caret_h.round()),
                            ))
                        } else {
                            None
                        };

                        InlinePrepaintState::Shaped {
                            wrapped_line: Box::new(wrapped_line),
                            selection_quad,
                            selection_color,
                            caret_bounds,
                            cursor_color,
                        }
                    },
                    move |bounds, prepaint, window, cx| {
                        // Bind native IME input handler during paint
                        window.handle_input(
                            &focus_handle,
                            ElementInputHandler::new(bounds, entity.clone()),
                            cx,
                        );

                        // Window-level mouse capture while dragging
                        if is_dragging {
                            let entity_move = entity.clone();
                            window.on_mouse_event(
                                move |event: &MouseMoveEvent,
                                      phase: DispatchPhase,
                                      window: &mut Window,
                                      cx: &mut App| {
                                    if phase != DispatchPhase::Capture {
                                        return;
                                    }
                                    let handled = entity_move.update(cx, |this, cx| {
                                        if !this.is_dragging {
                                            return false;
                                        }
                                        if event.pressed_button != Some(MouseButton::Left) {
                                            this.stop_drag(cx);
                                            return true;
                                        }
                                        this.handle_drag_move(event.position, window, cx);
                                        true
                                    });
                                    if handled {
                                        cx.stop_propagation();
                                    }
                                },
                            );

                            let entity_up = entity.clone();
                            window.on_mouse_event(
                                move |event: &MouseUpEvent,
                                      phase: DispatchPhase,
                                      _window: &mut Window,
                                      cx: &mut App| {
                                    if phase != DispatchPhase::Capture {
                                        return;
                                    }
                                    if event.button == MouseButton::Left {
                                        let stopped = entity_up.update(cx, |this, cx| {
                                            if this.is_dragging {
                                                this.stop_drag(cx);
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

                        let content_origin_x = bounds.origin.x + px(4.0);
                        let line_height = px(16.0);
                        let content_origin_y =
                            (bounds.origin.y + (bounds.size.height - line_height) / 2.0).round();

                        match prepaint {
                            InlinePrepaintState::Empty {
                                caret_bounds,
                                cursor_color,
                            } => {
                                if let Some(caret) = caret_bounds {
                                    window.paint_quad(fill(caret, cursor_color));
                                }
                            }
                            InlinePrepaintState::Shaped {
                                wrapped_line,
                                selection_quad,
                                selection_color,
                                caret_bounds,
                                cursor_color,
                            } => {
                                if let Some(sel) = selection_quad {
                                    window.paint_quad(fill(sel, selection_color));
                                }
                                let _ = wrapped_line.paint(
                                    point(content_origin_x, content_origin_y),
                                    line_height,
                                    gpui::TextAlign::Left,
                                    None,
                                    window,
                                    cx,
                                );
                                if let Some(caret) = caret_bounds {
                                    window.paint_quad(fill(caret, cursor_color));
                                }
                            }
                        }
                    },
                )
                .size_full(),
            )
    }
}
