use gpui::*;
use serde::{Deserialize, Serialize};
use std::cell::RefCell;
use std::ops::Range;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use crate::buffer::{Position, Selection, TextBuffer};
use crate::markdown::{VisualDocument, get_block_prefix_len, toggle_task_checkbox};
use crate::theme::{ActiveTheme, Theme};

pub mod actions;
pub mod display_map;
pub mod element;
pub mod input_handler;
pub mod keymap;
pub mod layout;
pub mod mouse;
pub mod navigation;
pub mod render;
pub mod shaping;
#[cfg(test)]
mod tests;

pub use actions::*;
pub use display_map::*;
pub use element::EditorElement;
pub use layout::*;
pub use render::*;

/// Allowed file extensions for editing in Inviscid.
pub const WHITELISTED_EXTENSIONS: &[&str] = &["md", "markdown", "txt"];

/// Returns true if the file path has an extension in the editable whitelist.
#[inline]
pub fn is_whitelisted_extension(path: &Path) -> bool {
    let ext = match path.extension().and_then(|e| e.to_str()) {
        Some(e) => e,
        None => return false,
    };
    WHITELISTED_EXTENSIONS
        .iter()
        .any(|&allowed| ext.eq_ignore_ascii_case(allowed))
}

#[derive(Clone, Debug)]
pub struct ScrollState {
    pub physics: crate::ui::SmoothScrollPhysics,
    pub is_scrollbar_dragging: bool,
    pub scrollbar_drag_start_y: Option<Pixels>,
    pub scrollbar_drag_start_scroll: Option<Pixels>,
    pub last_scroll_action: std::time::Instant,
}

impl std::ops::Deref for ScrollState {
    type Target = crate::ui::SmoothScrollPhysics;

    #[inline]
    fn deref(&self) -> &Self::Target {
        &self.physics
    }
}

impl std::ops::DerefMut for ScrollState {
    #[inline]
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.physics
    }
}

impl Default for ScrollState {
    fn default() -> Self {
        Self::new()
    }
}

impl ScrollState {
    pub fn new() -> Self {
        Self {
            physics: crate::ui::SmoothScrollPhysics::new(),
            is_scrollbar_dragging: false,
            scrollbar_drag_start_y: None,
            scrollbar_drag_start_scroll: None,
            last_scroll_action: std::time::Instant::now(),
        }
    }

    /// Direct 1:1 displacement for precision touchpads and scrollbar thumb dragging.
    pub fn scroll_direct(&mut self, delta_y: Pixels, max_scroll: Pixels) {
        self.physics.scroll_direct(delta_y, max_scroll);
        self.last_scroll_action = std::time::Instant::now();
    }

    /// Mouse wheel impulse displacement with cubic ease-out interpolation.
    pub fn scroll_by_wheel(&mut self, delta_y: Pixels, max_scroll: Pixels) {
        self.physics.scroll_by_wheel(delta_y, max_scroll);
        self.last_scroll_action = std::time::Instant::now();
    }

    pub fn set_target_scroll_top(&mut self, target: Pixels, max_scroll: Pixels) {
        self.physics.set_target_scroll_top(target, max_scroll);
        self.last_scroll_action = std::time::Instant::now();
    }
}

/// Editing mode persisted in `config.toml`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum RenderMode {
    #[default]
    #[serde(rename = "Live Preview")]
    LivePreview,
    #[serde(rename = "Source")]
    Source,
}

impl RenderMode {
    pub fn label(self) -> &'static str {
        match self {
            RenderMode::LivePreview => "Live Preview",
            RenderMode::Source => "Source",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HoveredLink {
    pub url: String,
    pub line_idx: usize,
    pub col_range: (usize, usize),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EditorStatusMeta {
    pub is_dirty: bool,
    pub cursor_pos: Position,
    pub word_count: usize,
    pub char_count: usize,
    pub render_mode: RenderMode,
}

pub struct Editor {
    pub(in crate::editor) buffer: TextBuffer,
    pub(crate) focus_handle: FocusHandle,
    pub(crate) render_mode: RenderMode,
    pub(crate) is_dragging: bool,
    pub(crate) marked_range: Option<Range<usize>>,
    /// Timestamp when the IME last cleared an active composition to empty text,
    /// used to ignore the trailing `WM_KEYDOWN` for the key the IME already handled.
    pub(crate) ime_consumed_edit_key_at: Option<std::time::Instant>,
    pub(crate) viewport_bounds: Bounds<Pixels>,
    pub(crate) last_cursor_action: std::time::Instant,
    pub(crate) pixel_column_goal: Option<Pixels>,
    pub(crate) last_mouse_pos: Option<Point<Pixels>>,
    pub(crate) hovered_link: Option<HoveredLink>,

    pub(crate) cursor_opacity: f32,
    pub(crate) blink_epoch: usize,
    pub(crate) blink_task: Option<Task<()>>,

    pub(crate) scroll: ScrollState,
    pub(crate) scroll_task: Option<Task<()>>,
    pub(crate) drag_scroll_task: Option<Task<()>>,
    pub(crate) _async_update_task: Option<Task<()>>,

    pub(crate) layout_cache: RefCell<Arc<DocumentLayoutCache>>,
    pub(crate) display_map: DisplayMap,
    pub(crate) is_read_only: bool,

    pub(crate) auto_save_task: Option<Task<()>>,
    pub(crate) auto_save_delay: std::time::Duration,

    pub(crate) has_explicit_render_mode: bool,
    pub(crate) font_size: f32,
    pub(crate) line_height: f32,
    pub(crate) soft_wrap: bool,
    pub(crate) cursor_blink: bool,
    pub(crate) cursor_breathing: bool,

    /// Native text system handle (`None` in headless unit tests).
    pub(crate) text_system: Option<Arc<TextSystem>>,
}

impl Focusable for Editor {
    fn focus_handle(&self, _cx: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum EditorEvent {
    DirtyChanged(bool),
    TitleChanged,
    CursorMoved,
}

impl EventEmitter<EditorEvent> for Editor {}

impl Editor {
    #[inline]
    pub fn viewport_bounds_val(&self) -> Bounds<Pixels> {
        self.viewport_bounds
    }

    #[inline]
    pub fn file_path(&self) -> Option<&Path> {
        self.buffer.file_path()
    }

    #[inline]
    pub fn file_path_buf(&self) -> Option<PathBuf> {
        self.buffer.file_path_buf()
    }

    #[inline]
    pub fn is_dirty(&self) -> bool {
        self.buffer.is_dirty()
    }

    #[inline]
    pub fn buffer_version(&self) -> usize {
        self.buffer.version()
    }

    #[inline]
    pub fn cursor_pos(&self) -> Position {
        self.buffer.cursor_pos()
    }

    #[inline]
    pub fn is_empty(&self) -> bool {
        self.buffer.is_empty()
    }

    #[inline]
    pub fn cursor_opacity(&self) -> f32 {
        self.cursor_opacity
    }

    #[inline]
    pub fn has_blink_task(&self) -> bool {
        self.blink_task.is_some()
    }

    pub fn on_blur(&mut self, cx: &mut Context<Self>) {
        self.cursor_opacity = 0.0;
        self.blink_epoch = self.blink_epoch.wrapping_add(1);
        self.blink_task = None;
        cx.notify();
    }

    pub fn on_focus(&mut self, cx: &mut Context<Self>) {
        self.reset_cursor_blink(cx);
    }

    #[inline]
    pub fn has_scroll_task(&self) -> bool {
        self.scroll_task.is_some()
    }

    #[inline]
    pub fn has_auto_save_task(&self) -> bool {
        self.auto_save_task.is_some()
    }

    #[inline]
    pub fn auto_save_delay(&self) -> std::time::Duration {
        self.auto_save_delay
    }

    pub fn set_auto_save_delay(&mut self, delay: std::time::Duration) {
        self.auto_save_delay = delay;
    }

    pub(crate) fn schedule_auto_save_if_needed(&mut self, cx: &mut Context<Self>) {
        if !self.can_edit() || self.buffer.file_path().is_none() || !self.buffer.is_dirty() {
            self.auto_save_task = None;
            return;
        }
        let is_auto_save_enabled = cx
            .try_global::<crate::config::AppConfig>()
            .map(|c| c.auto_save)
            .unwrap_or(false);
        if !is_auto_save_enabled {
            self.auto_save_task = None;
            return;
        }

        let delay = self.auto_save_delay;
        self.auto_save_task = Some(cx.spawn(async move |this, cx| {
            cx.background_executor().timer(delay).await;

            let _ = this.update(cx, |editor, cx| {
                let is_auto_save_enabled = cx
                    .try_global::<crate::config::AppConfig>()
                    .map(|c| c.auto_save)
                    .unwrap_or(false);
                if is_auto_save_enabled
                    && editor.can_edit()
                    && editor.is_dirty()
                    && editor.buffer.file_path().is_some()
                {
                    if let Ok(task) = editor.save_file_async(cx) {
                        cx.spawn(async move |_this, _cx| {
                            if let Err(e) = task.await {
                                eprintln!("Auto save failed: {}", e);
                            }
                        })
                        .detach();
                    }
                }
            });
        }));
    }

    #[inline]
    pub fn scroll_state(&self) -> &ScrollState {
        &self.scroll
    }

    #[inline]
    pub fn scroll_state_mut(&mut self) -> &mut ScrollState {
        &mut self.scroll
    }

    #[inline]
    pub fn buffer(&self) -> &TextBuffer {
        &self.buffer
    }

    #[inline]
    pub fn buffer_mut(&mut self) -> &mut TextBuffer {
        &mut self.buffer
    }

    pub fn title(&self) -> String {
        self.buffer
            .file_path()
            .and_then(|p| p.file_name())
            .map(|f| f.to_string_lossy().to_string())
            .unwrap_or_else(|| "Untitled.md".to_string())
    }

    pub fn dir_hint(&self) -> Option<String> {
        self.buffer.file_path().and_then(|p| {
            p.parent()
                .and_then(|d| d.file_name())
                .map(|n| n.to_string_lossy().to_string())
        })
    }

    pub fn update_file_path(&mut self, path: PathBuf, cx: &mut Context<Self>) {
        self.buffer.set_file_path(Some(path));
        cx.emit(EditorEvent::TitleChanged);
        cx.notify();
    }

    pub fn new(cx: &mut Context<Self>) -> Self {
        Self::new_with_buffer(TextBuffer::new(), cx)
    }

    pub fn new_with_buffer(buffer: TextBuffer, cx: &mut Context<Self>) -> Self {
        let focus_handle = cx.focus_handle();

        let image_rx = crate::http::subscribe_image_updates();
        let syntax_rx = crate::syntax::subscribe_syntax_updates();
        let mut async_update_rx = futures::stream::select(image_rx, syntax_rx);
        let async_update_task = cx.spawn(async move |this, cx| {
            use futures::StreamExt;
            while let Some(_version) = async_update_rx.next().await {
                let res = this.update(cx, |_editor, cx| {
                    cx.notify();
                });
                if res.is_err() {
                    break;
                }
            }
        });

        let (default_mode, font_size, line_height, soft_wrap, cursor_blink, cursor_breathing) = cx
            .try_global::<crate::config::AppConfig>()
            .map(|c| {
                (
                    c.default_render_mode,
                    c.editor_font_size,
                    c.line_height,
                    c.soft_wrap,
                    c.cursor_blink,
                    c.cursor_breathing,
                )
            })
            .unwrap_or((RenderMode::LivePreview, 15.0, 1.6, true, true, true));

        let mut editor = Self {
            buffer,
            focus_handle,
            render_mode: default_mode,
            has_explicit_render_mode: false,
            font_size,
            line_height,
            soft_wrap,
            cursor_blink,
            cursor_breathing,
            is_dragging: false,
            marked_range: None,
            ime_consumed_edit_key_at: None,
            viewport_bounds: Bounds::default(),
            last_cursor_action: std::time::Instant::now(),
            pixel_column_goal: None,
            last_mouse_pos: None,
            hovered_link: None,
            cursor_opacity: 1.0,
            blink_epoch: 0,
            blink_task: None,
            scroll: ScrollState::new(),
            scroll_task: None,
            drag_scroll_task: None,
            _async_update_task: Some(async_update_task),
            layout_cache: RefCell::new(Arc::new(DocumentLayoutCache::default())),
            display_map: DisplayMap::new(),
            is_read_only: false,
            auto_save_task: None,
            auto_save_delay: std::time::Duration::from_millis(1000),
            text_system: Some(cx.text_system().clone()),
        };
        editor.reset_cursor_blink(cx);
        editor
    }

    pub fn new_with_path(path: PathBuf, cx: &mut Context<Self>) -> Self {
        let buffer = if path.exists() && is_whitelisted_extension(&path) {
            match std::fs::read_to_string(&path) {
                Ok(content) => TextBuffer::from_str(&content, Some(path)),
                Err(e) => {
                    eprintln!("Failed to read file {:?}: {}", path, e);
                    TextBuffer::from_str("", Some(path))
                }
            }
        } else {
            TextBuffer::from_str("", Some(path))
        };
        Self::new_with_buffer(buffer, cx)
    }

    /// In-memory buffers (no path) are always editable; file-backed buffers require a whitelisted extension.
    #[inline]
    pub fn is_editable(&self) -> bool {
        match self.buffer.file_path() {
            Some(path) => is_whitelisted_extension(path),
            None => true,
        }
    }

    #[inline]
    pub fn is_read_only(&self) -> bool {
        self.is_read_only
    }

    #[inline]
    pub fn can_edit(&self) -> bool {
        self.is_editable() && !self.is_read_only
    }

    pub fn set_read_only(&mut self, is_read_only: bool) {
        self.is_read_only = is_read_only;
        if self.is_read_only {
            self.auto_save_task = None;
        }
    }

    pub fn toggle_read_only(&mut self) {
        self.is_read_only = !self.is_read_only;
        if self.is_read_only {
            self.auto_save_task = None;
        }
    }

    #[inline]
    pub fn display_map(&self) -> &DisplayMap {
        &self.display_map
    }

    /// Folds `start_line..=end_line`, moving the cursor to `start_line` if it was inside the folded region.
    pub fn fold_internal(&mut self, start_line: usize, end_line: usize) {
        self.display_map.fold_map_mut().fold(start_line, end_line);
        let cursor = self.buffer.cursor_pos();
        if cursor.line > start_line && cursor.line <= end_line {
            self.buffer.update_cursor(
                false,
                Position::new(start_line, self.buffer.line_len(start_line)),
            );
        }
        self.ensure_layout_cache();
    }

    pub fn fold(&mut self, start_line: usize, end_line: usize, cx: &mut Context<Self>) {
        self.fold_internal(start_line, end_line);
        cx.notify();
    }

    pub fn unfold_internal(&mut self, start_line: usize) -> bool {
        let changed = self.display_map.fold_map_mut().unfold(start_line);
        if changed {
            self.ensure_layout_cache();
        }
        changed
    }

    pub fn unfold(&mut self, start_line: usize, cx: &mut Context<Self>) -> bool {
        let changed = self.unfold_internal(start_line);
        if changed {
            cx.notify();
        }
        changed
    }

    pub fn unfold_all_internal(&mut self) {
        self.display_map.fold_map_mut().unfold_all();
        self.ensure_layout_cache();
    }

    pub fn unfold_all(&mut self, cx: &mut Context<Self>) {
        self.unfold_all_internal();
        cx.notify();
    }

    #[inline]
    pub fn is_line_folded(&self, line: usize) -> bool {
        self.display_map.fold_map().is_line_folded(line)
    }

    #[inline]
    pub fn is_fold_header(&self, line: usize) -> bool {
        self.display_map.fold_map().is_fold_header(line)
    }

    pub fn status_meta(&self) -> Option<EditorStatusMeta> {
        if !self.is_editable() {
            return None;
        }
        Some(EditorStatusMeta {
            is_dirty: self.buffer.is_dirty(),
            cursor_pos: self.buffer.cursor_pos(),
            word_count: self.buffer.word_count(),
            char_count: self.buffer.char_count(),
            render_mode: self.render_mode,
        })
    }

    /// Clamps cursor out of hidden markdown prefixes when switching into Live Preview.
    pub fn normalize_cursor_for_mode(&mut self, new_mode: RenderMode) {
        if new_mode != RenderMode::LivePreview {
            return;
        }

        let layout = self.layout_snapshot();
        let pos = self.buffer.cursor_pos();
        let vdoc = VisualDocument::from_parsed(self.buffer.lines(), &layout.parsed_lines);
        let vpos = vdoc.source_to_visual(pos);
        let mut target_pos = vdoc.visual_to_source(vpos);

        let current_line_str = &self.buffer.lines()[target_pos.line];
        let prefix_len = get_block_prefix_len(current_line_str);
        if prefix_len > 0 && target_pos.col < prefix_len {
            target_pos.col = prefix_len.min(self.buffer.line_len(target_pos.line));
        }

        self.buffer.set_cursor(target_pos);
        self.pixel_column_goal = None;
    }

    pub(crate) fn start_scroll_animation(&mut self, cx: &mut Context<Self>) {
        crate::ui::start_scroll_animation(
            self,
            cx,
            |editor| &mut editor.scroll_task,
            |editor| editor.scroll.step_interpolation(),
        );
    }

    #[inline]
    pub(crate) fn start_scroll_animation_if_needed(&mut self, cx: &mut Context<Self>) {
        if self.scroll.is_animating()
            || (self.scroll.target_scroll_top - self.scroll.current_scroll_top).abs() > px(0.5)
        {
            self.start_scroll_animation(cx);
        }
    }

    pub(crate) fn reset_cursor_blink(&mut self, cx: &mut Context<Self>) {
        self.cursor_opacity = 1.0;
        self.last_cursor_action = std::time::Instant::now();
        self.blink_epoch = self.blink_epoch.wrapping_add(1);
        let epoch = self.blink_epoch;
        cx.notify();
        let (blink_enabled, breathing_enabled) = cx
            .try_global::<crate::config::AppConfig>()
            .map(|c| (c.cursor_blink, c.cursor_breathing))
            .unwrap_or((self.cursor_blink, self.cursor_breathing));
        if blink_enabled {
            self.blink_task = Some(crate::ui::start_cursor_animation(
                cx,
                breathing_enabled,
                epoch,
                |ed| ed.blink_epoch,
                |ed, opacity, cx| {
                    ed.cursor_opacity = opacity;
                    cx.notify();
                },
            ));
        } else {
            self.blink_task = None;
        }
    }

    pub(crate) fn track_buffer_change<F>(&mut self, cx: &mut Context<Self>, f: F)
    where
        F: FnOnce(&mut Self),
    {
        let prev_dirty = self.buffer.is_dirty();
        let prev_head = self.buffer.cursor_pos();
        let prev_chars = self.buffer.char_count();
        let prev_version = self.buffer.version();
        f(self);
        if self.buffer.is_dirty() != prev_dirty {
            cx.emit(EditorEvent::DirtyChanged(self.buffer.is_dirty()));
        }
        if self.buffer.cursor_pos() != prev_head || self.buffer.char_count() != prev_chars {
            cx.emit(EditorEvent::CursorMoved);
        }
        if self.buffer.version() != prev_version {
            self.schedule_auto_save_if_needed(cx);
        }
    }

    pub fn render_mode(&self) -> RenderMode {
        self.render_mode
    }

    pub fn set_render_mode(&mut self, mode: RenderMode, cx: &mut Context<Self>) {
        self.has_explicit_render_mode = true;
        if self.render_mode != mode {
            self.normalize_cursor_for_mode(mode);
            self.render_mode = mode;
            self.last_cursor_action = std::time::Instant::now();
            self.scroll_to_cursor();
            self.start_scroll_animation_if_needed(cx);
            self.reset_cursor_blink(cx);
            cx.notify();
        }
    }

    pub fn undo(&mut self, cx: &mut Context<Self>) {
        self.perform_edit(cx, |this| this.buffer.undo());
    }

    pub fn redo(&mut self, cx: &mut Context<Self>) {
        self.perform_edit(cx, |this| this.buffer.redo());
    }

    pub fn select_all(&mut self, cx: &mut Context<Self>) {
        self.pixel_column_goal = None;
        let prev_head = self.buffer.cursor_pos();
        self.buffer.select_all();
        if self.buffer.cursor_pos() != prev_head {
            cx.emit(EditorEvent::CursorMoved);
        }
        self.scroll_to_cursor();
        self.start_scroll_animation_if_needed(cx);
        self.reset_cursor_blink(cx);
        cx.notify();
    }

    pub fn cut(&mut self, cx: &mut Context<Self>) {
        if !self.can_edit() {
            self.copy(cx);
            return;
        }
        if let Some(selected_text) = self.selected_text_for_clipboard() {
            cx.write_to_clipboard(ClipboardItem::new_string(selected_text));
            self.perform_edit(cx, |this| this.buffer.insert_text(""));
        }
    }

    pub fn copy(&mut self, cx: &mut Context<Self>) {
        if let Some(selected_text) = self.selected_text_for_clipboard() {
            cx.write_to_clipboard(ClipboardItem::new_string(selected_text));
        }
    }

    /// Returns selected text for clipboard, including leading block prefixes (`#`, `-`, `>`) when a full line is selected in Live Preview.
    pub fn selected_text_for_clipboard(&self) -> Option<String> {
        Self::selected_text_for_clipboard_internal(&self.buffer, self.render_mode)
    }

    pub(crate) fn selected_text_for_clipboard_internal(
        buffer: &crate::buffer::TextBuffer,
        render_mode: RenderMode,
    ) -> Option<String> {
        if buffer.selection().is_empty() {
            return None;
        }

        if render_mode == RenderMode::LivePreview {
            let (start, end) = buffer.selection().range();
            if start.line == end.line {
                if let Some(line) = buffer.line(start.line) {
                    let prefix_len = crate::markdown::get_block_prefix_len(line);
                    let char_count = line.chars().count();
                    if start.col <= prefix_len && end.col == char_count {
                        let end_col = end.col.min(char_count);
                        return Some(line.chars().take(end_col).collect());
                    }
                }
            } else {
                let mut res = Vec::with_capacity(end.line - start.line + 1);
                for line_idx in start.line..=end.line {
                    if let Some(line) = buffer.line(line_idx) {
                        if line_idx == start.line {
                            let prefix_len = crate::markdown::get_block_prefix_len(line);
                            let actual_start_col = if start.col <= prefix_len {
                                0
                            } else {
                                start.col
                            };
                            res.push(line.chars().skip(actual_start_col).collect::<String>());
                        } else if line_idx == end.line {
                            res.push(line.chars().take(end.col).collect::<String>());
                        } else {
                            res.push(line.to_string());
                        }
                    }
                }
                return Some(res.join("\n"));
            }
        }

        buffer.selected_text()
    }

    pub fn paste(&mut self, cx: &mut Context<Self>) {
        if !self.can_edit() {
            return;
        }
        if let Some(item) = cx.read_from_clipboard()
            && let Some(text) = item.text()
        {
            self.perform_edit(cx, |this| this.buffer.insert_text(&text));
        }
    }

    pub fn toggle_task_checkbox(&mut self, line_idx: usize, cx: &mut Context<Self>) {
        if !self.can_edit() || line_idx >= self.buffer.line_count() {
            return;
        }
        let line = &self.buffer.lines()[line_idx];
        if let Some(new_line) = toggle_task_checkbox(line) {
            self.track_buffer_change(cx, |this| {
                this.buffer.record_edit(|lines, _| {
                    lines[line_idx] = new_line;
                });
            });
            self.last_cursor_action = std::time::Instant::now();
            self.reset_cursor_blink(cx);
            cx.notify();
        }
    }

    pub fn load_file(
        &mut self,
        path: PathBuf,
        cx: &mut Context<Self>,
    ) -> Result<(), std::io::Error> {
        self.auto_save_task = None;
        let content = if is_whitelisted_extension(&path) {
            std::fs::read_to_string(&path)?
        } else {
            String::new()
        };
        let mtime = std::fs::metadata(&path)
            .ok()
            .and_then(|m| m.modified().ok());
        self.buffer = TextBuffer::from_str(&content, Some(path));
        self.buffer.set_last_saved_mtime(mtime);
        self.last_cursor_action = std::time::Instant::now();
        cx.emit(EditorEvent::TitleChanged);
        cx.emit(EditorEvent::DirtyChanged(false));
        cx.emit(EditorEvent::CursorMoved);
        cx.notify();
        Ok(())
    }

    pub fn save_file_async(
        &mut self,
        cx: &mut Context<Self>,
    ) -> Result<Task<Result<(), std::io::Error>>, std::io::Error> {
        self.auto_save_task = None;
        if !self.can_edit() {
            return Err(std::io::Error::other("File is not editable"));
        }
        if let Some(path) = self.buffer.file_path_buf() {
            let saved_mtime =
                crate::fs::save_document_chunks(&path, self.buffer.chunks_with_line_ending())?;
            let baseline = self.buffer.saved_baseline_snapshot();
            self.buffer.mark_saved_as(baseline);
            self.buffer.set_last_saved_mtime(Some(saved_mtime));
            let dirty = self.buffer.is_dirty();
            cx.emit(EditorEvent::TitleChanged);
            cx.emit(EditorEvent::DirtyChanged(dirty));
            cx.notify();
            Ok(cx.spawn(async move |_this, _cx| Ok(())))
        } else {
            Err(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                "No file path specified for saving",
            ))
        }
    }

    pub fn save_file_as_async(
        &mut self,
        path: PathBuf,
        cx: &mut Context<Self>,
    ) -> Task<Result<(), std::io::Error>> {
        self.auto_save_task = None;
        match crate::fs::save_document_chunks(&path, self.buffer.chunks_with_line_ending()) {
            Ok(saved_mtime) => {
                let baseline = self.buffer.saved_baseline_snapshot();
                self.buffer.set_file_path(Some(path));
                self.buffer.mark_saved_as(baseline);
                self.buffer.set_last_saved_mtime(Some(saved_mtime));
                let dirty = self.buffer.is_dirty();
                cx.emit(EditorEvent::TitleChanged);
                cx.emit(EditorEvent::DirtyChanged(dirty));
                cx.notify();
                cx.spawn(async move |_this, _cx| Ok(()))
            }
            Err(e) => cx.spawn(async move |_this, _cx| Err(e)),
        }
    }

    /// Returns true if the file on disk has been modified externally since it was opened or last saved.
    pub fn is_externally_modified(&self) -> bool {
        if let Some(path) = self.buffer.file_path() {
            if let Some(last_mtime) = self.buffer.last_saved_mtime() {
                if let Ok(metadata) = std::fs::metadata(path) {
                    if let Ok(current_mtime) = metadata.modified() {
                        return current_mtime > last_mtime;
                    }
                }
            }
        }
        false
    }

    fn render_unsupported_file_prompt(&self, theme: &Theme) -> impl IntoElement {
        div()
            .id("unsupported_file_view")
            .key_context(crate::ui::key_context::EDITOR)
            .track_focus(&self.focus_handle)
            .w_full()
            .h_full()
            .bg(theme.bg_editor)
            .flex()
            .items_center()
            .justify_center()
            .text_size(px(14.0))
            .text_color(theme.text_muted)
            .child("The file type is not supported")
    }
}

impl Render for Editor {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if let Some(config) = cx.try_global::<crate::config::AppConfig>() {
            if !self.has_explicit_render_mode {
                let target_mode = config.default_render_mode;
                if self.render_mode != target_mode {
                    self.normalize_cursor_for_mode(target_mode);
                    self.render_mode = target_mode;
                }
            }

            let mut layout_changed = false;
            if (self.font_size - config.editor_font_size).abs() > f32::EPSILON {
                self.font_size = config.editor_font_size;
                layout_changed = true;
            }
            if (self.line_height - config.line_height).abs() > f32::EPSILON {
                self.line_height = config.line_height;
                layout_changed = true;
            }
            if self.soft_wrap != config.soft_wrap {
                self.soft_wrap = config.soft_wrap;
                layout_changed = true;
            }
            if layout_changed {
                self.ensure_layout_cache();
            }

            if self.cursor_blink != config.cursor_blink
                || self.cursor_breathing != config.cursor_breathing
            {
                self.cursor_blink = config.cursor_blink;
                self.cursor_breathing = config.cursor_breathing;
                if self.cursor_blink {
                    self.reset_cursor_blink(cx);
                } else {
                    self.blink_epoch = self.blink_epoch.wrapping_add(1);
                    self.blink_task = None;
                    self.cursor_opacity = 1.0;
                }
            }
        }

        let theme = cx.theme();

        if !self.is_editable() {
            return self
                .render_unsupported_file_prompt(theme)
                .into_any_element();
        }

        let is_focused = self.focus_handle.is_focused(_window);
        if !is_focused && self.blink_task.is_some() {
            self.cursor_opacity = 0.0;
            self.blink_epoch = self.blink_epoch.wrapping_add(1);
            self.blink_task = None;
        }

        let bg_color = theme.bg_editor;

        let cursor_style = if self.hovered_link.is_some() {
            CursorStyle::PointingHand
        } else {
            CursorStyle::IBeam
        };

        let element = EditorElement::new(cx.entity().clone())
            .key_context(crate::ui::key_context::EDITOR)
            .id("editor_view")
            .track_focus(&self.focus_handle)
            .cursor(cursor_style)
            .bg(bg_color);

        let element = Self::register_actions(element, cx);
        Self::register_mouse_listeners(element, cx).into_any_element()
    }
}

#[derive(Clone, Copy)]
pub struct LineRenderContext<'a> {
    pub is_active_line: bool,
    pub is_focused: bool,
    pub cursor_col: usize,
    /// Column used for markdown syntax-marker disclosure (pinned to the IME composition start when composing).
    pub disclosure_col: usize,
    pub selection: Selection,
    pub is_dragging: bool,
    pub theme: &'a Theme,
    pub mode: RenderMode,
    pub cursor_opacity: f32,
    pub hovered_link: Option<&'a HoveredLink>,
    pub content_width: Pixels,
    pub doc_path: Option<&'a std::path::Path>,
    pub font_size: f32,
    pub line_height: f32,
    pub soft_wrap: bool,
    pub text_system: Option<&'a Arc<TextSystem>>,
}
