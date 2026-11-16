use gpui::*;
use serde::{Deserialize, Serialize};
use std::cell::RefCell;

#[cfg(test)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct LayoutStats {
    pub full_rebuilds: usize,
    pub lines_scanned: usize,
    pub lines_measured: usize,
    pub cache_deep_clones: usize,
}
use std::ops::Range;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use gpui::prelude::FluentBuilder;

use crate::app::actions::{
    CloseFindBar, FindInDocument, FindNext, FindPrevious, ReplaceInDocument,
};
use crate::buffer::{Position, Selection, TextBuffer};
use crate::markdown::{normalize_buffer_live_cursor, toggle_task_checkbox};
use crate::search::FindState;
use crate::theme::{ActiveTheme, Theme};
use crate::ui::Spacing;

pub mod actions;
pub mod display_map;
pub mod element;
pub mod find;
pub mod find_bar;
pub mod input_handler;
pub mod keymap;
pub mod layout;
pub mod line_metrics;
pub(crate) mod loading;
pub mod mouse;
pub mod navigation;
pub mod parsed_line_store;
pub mod render;
mod scroll_anchor;
pub mod shaping;
#[cfg(test)]
mod tests;
mod window_cache;
mod window_layout;

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
    // When present, physics offsets are relative to this source row's top.
    pub(crate) anchor_line: Option<usize>,
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
            anchor_line: None,
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
    pub line: crate::buffer::SourceLine,
    pub col_range: (usize, usize),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EditorStatusMeta {
    pub is_loading: bool,
    pub load_error: Option<String>,
    pub is_dirty: bool,
    pub external_change: ExternalChangeState,
    pub cursor_pos: Position,
    pub word_count: usize,
    pub char_count: usize,
    pub render_mode: RenderMode,
}

/// Describes a change on disk that has not been incorporated into the editor buffer.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum ExternalChangeState {
    #[default]
    None,
    Pending,
    Deleted,
}

pub struct Editor {
    background_session: Option<Arc<loading::DocumentSession>>,
    prepared_document: Option<Arc<loading::PreparedSnapshot>>,
    is_loading: bool,
    pending_open_position: Option<Position>,
    load_error: Option<String>,
    pub(in crate::editor) buffer: TextBuffer,
    pub(crate) focus_handle: FocusHandle,
    pub(crate) render_mode: RenderMode,
    pub(crate) is_dragging: bool,
    pub(crate) drag_code_block_bounds: Option<(Position, Position)>,
    pub(crate) marked_range: Option<Range<usize>>,
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
    pub(crate) load_task: Option<Task<()>>,

    pub(crate) layout_cache: RefCell<Arc<DocumentLayoutCache>>,
    layout_scroll_adjustment: std::cell::Cell<Pixels>,
    #[cfg(test)]
    pub(crate) layout_stats: std::cell::Cell<LayoutStats>,
    #[cfg(test)]
    layout_validation_scans: std::cell::Cell<usize>,
    pub(crate) display_map: DisplayMap,
    pub(crate) is_read_only: bool,

    pub(crate) auto_save_task: Option<Task<()>>,
    fingerprint_task: Option<Task<()>>,
    pub(crate) auto_save_delay: std::time::Duration,
    pub(crate) file_watch_task: Option<Task<()>>,
    pub(crate) external_check_task: Option<Task<()>>,
    pub(crate) external_change: ExternalChangeState,
    pub(crate) external_disk_hash: Option<u64>,
    pub(crate) external_baseline_overridden: bool,
    pub(crate) baseline_generation: u64,

    pub(crate) has_explicit_render_mode: bool,
    pub(crate) font_size: f32,
    pub(crate) line_height: f32,
    pub(crate) soft_wrap: bool,
    pub(crate) cursor_blink: bool,
    pub(crate) cursor_breathing: bool,

    /// Window text system handle, captured on first render (`None` before then and in headless tests).
    pub(crate) text_system: Option<Arc<WindowTextSystem>>,

    /// Memoized width of one space character in the UI font, keyed by the font
    /// size it was measured at. Single measurement feeds visual indent layout.
    pub(crate) space_width_memo: std::cell::Cell<Option<(f32, Pixels)>>,

    pub(crate) find_state: FindState,
    pub(crate) find_search: find::FindSearch,
    /// `Some` while the find bar is open.
    pub(crate) find_bar: Option<find::FindBarHandle>,
    pub(crate) needs_focus_after_find: bool,
}

impl Focusable for Editor {
    fn focus_handle(&self, _cx: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum EditorEvent {
    DirtyChanged(bool),
    ExternalChangeChanged(ExternalChangeState),
    TitleChanged,
    CursorMoved,
    ScrollChanged,
}

struct SaveSnapshot {
    rope: ropey::Rope,
    line_ending: crate::buffer::LineEnding,
    is_empty_doc: bool,
    revision: u64,
    line_count: usize,
    char_count: usize,
    ready_hash: Option<u64>,
}

impl SaveSnapshot {
    fn write_to(
        self,
        path: &Path,
    ) -> std::io::Result<(std::time::SystemTime, u64, crate::buffer::SavedBaseline)> {
        let (saved_mtime, written_hash) =
            crate::fs::save_rope_snapshot(path, &self.rope, self.line_ending, self.is_empty_doc)?;
        let content_hash = self
            .ready_hash
            .unwrap_or_else(|| TextBuffer::compute_rope_content_hash(&self.rope));
        Ok((
            saved_mtime,
            written_hash,
            crate::buffer::SavedBaseline {
                content_hash,
                revision: self.revision,
                line_count: self.line_count,
                char_count: self.char_count,
                is_empty_doc: self.is_empty_doc,
            },
        ))
    }
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
        if let Some(session) = self.background_session.take() {
            session.cancel();
        }
        if let Some(snapshot) = self.prepared_document.take() {
            snapshot.session.cancel();
            self.invalidate_layout_cache();
        }
        self.buffer.set_file_path(Some(path.clone()));
        if self.is_loading {
            cx.emit(EditorEvent::TitleChanged);
            cx.notify();
            return;
        }
        let (session, task) = loading::spawn_editor_worker(Some(path), cx);
        self.background_session = Some(session);
        self.load_task = Some(task);
        self.external_baseline_overridden = false;
        self.set_external_change_state(ExternalChangeState::None, cx);
        self.refresh_external_baseline_async(true, true, cx);
        self.start_file_watcher(cx);
        cx.emit(EditorEvent::TitleChanged);
        cx.notify();
    }

    pub fn new(cx: &mut Context<Self>) -> Self {
        Self::new_with_buffer(TextBuffer::new(), cx)
    }

    pub fn new_with_buffer(buffer: TextBuffer, cx: &mut Context<Self>) -> Self {
        let mut editor = Self::new_with_buffer_and_disk_hash(buffer, None, cx);
        if editor.buffer.file_path().is_some() {
            editor.refresh_external_baseline_async(true, false, cx);
        }
        editor
    }

    fn new_with_buffer_and_disk_hash(
        buffer: TextBuffer,
        external_disk_hash: Option<u64>,
        cx: &mut Context<Self>,
    ) -> Self {
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

        let doc_path = buffer.file_path_buf();
        let mut editor = Self {
            background_session: None,
            prepared_document: None,
            is_loading: false,
            pending_open_position: None,
            load_error: None,
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
            drag_code_block_bounds: None,
            marked_range: None,
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
            load_task: None,
            layout_cache: RefCell::new(Arc::new(DocumentLayoutCache::default())),
            layout_scroll_adjustment: std::cell::Cell::new(px(0.0)),
            #[cfg(test)]
            layout_stats: std::cell::Cell::new(LayoutStats::default()),
            #[cfg(test)]
            layout_validation_scans: std::cell::Cell::new(0),
            display_map: DisplayMap::new(),
            is_read_only: false,
            auto_save_task: None,
            fingerprint_task: None,
            auto_save_delay: std::time::Duration::from_millis(1000),
            file_watch_task: None,
            external_check_task: None,
            external_change: ExternalChangeState::None,
            external_disk_hash,
            external_baseline_overridden: false,
            baseline_generation: 0,
            text_system: None,
            space_width_memo: std::cell::Cell::new(None),
            find_state: FindState::new(),
            find_search: find::FindSearch::default(),
            find_bar: None,
            needs_focus_after_find: false,
        };
        let (session, worker_task) = loading::spawn_editor_worker(doc_path, cx);
        editor.background_session = Some(session);
        editor.load_task = Some(worker_task);
        editor.reset_cursor_blink(cx);
        editor.schedule_fingerprints(cx);
        editor.start_file_watcher(cx);
        editor
    }

    fn schedule_fingerprints(&mut self, cx: &mut Context<Self>) {
        if self.fingerprint_task.is_some() {
            return;
        }
        let Some(job) = self.buffer.fingerprint_job() else {
            return;
        };
        let revision = self.buffer.revision();
        let work = cx.background_executor().spawn(async move { job.run() });
        self.fingerprint_task = Some(cx.spawn(async move |this, cx| {
            let result = work.await;
            let _ = this.update(cx, |editor, cx| {
                editor.fingerprint_task = None;
                let dirty = editor.is_dirty();
                if !editor.buffer.install_fingerprints(result, revision) {
                    editor.schedule_fingerprints(cx);
                }
                if editor.is_dirty() != dirty {
                    cx.emit(EditorEvent::DirtyChanged(editor.is_dirty()));
                    cx.notify();
                }
            });
        }));
    }

    pub fn new_with_path(path: PathBuf, cx: &mut Context<Self>) -> Self {
        let mut load_error = None;
        let (buffer, disk_hash) = if path.exists() && is_whitelisted_extension(&path) {
            let disk_hash = crate::fs::read_file_hash(&path).ok();
            match std::fs::read_to_string(&path) {
                Ok(content) => (
                    TextBuffer::from_bytes(content.as_bytes(), Some(path))
                        .expect("validated UTF-8"),
                    disk_hash,
                ),
                Err(e) => {
                    eprintln!("Failed to read file {:?}: {}", path, e);
                    load_error = Some(format!("Could not open file: {e}"));
                    (TextBuffer::from_str("", Some(path)), None)
                }
            }
        } else {
            (TextBuffer::from_str("", Some(path)), None)
        };
        let mut editor = Self::new_with_buffer_and_disk_hash(buffer, disk_hash, cx);
        editor.load_error = load_error;
        let mtime = editor
            .buffer
            .file_path()
            .and_then(|path| std::fs::metadata(path).ok())
            .and_then(|metadata| metadata.modified().ok());
        editor.buffer.set_last_saved_mtime(mtime);
        editor
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
        self.is_read_only || self.load_error.is_some()
    }

    #[inline]
    pub fn can_edit(&self) -> bool {
        self.is_editable() && !self.is_read_only() && !self.is_loading
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
        self.display_map.synchronize(&self.buffer);
        &self.display_map
    }

    /// Folds `start_line..=end_line`, moving the cursor to `start_line` if it was inside the folded region.
    pub fn fold_internal(
        &mut self,
        start_line: crate::buffer::SourceLine,
        end_line: crate::buffer::SourceLine,
    ) {
        self.display_map.synchronize(&self.buffer);
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

    pub fn fold(
        &mut self,
        start_line: crate::buffer::SourceLine,
        end_line: crate::buffer::SourceLine,
        cx: &mut Context<Self>,
    ) {
        self.fold_internal(start_line, end_line);
        cx.notify();
    }

    pub fn unfold_internal(&mut self, start_line: crate::buffer::SourceLine) -> bool {
        self.display_map.synchronize(&self.buffer);
        let changed = self.display_map.fold_map_mut().unfold(start_line);
        if changed {
            self.ensure_layout_cache();
        }
        changed
    }

    pub fn unfold(
        &mut self,
        start_line: crate::buffer::SourceLine,
        cx: &mut Context<Self>,
    ) -> bool {
        let changed = self.unfold_internal(start_line);
        if changed {
            cx.notify();
        }
        changed
    }

    pub fn unfold_all_internal(&mut self) {
        self.display_map.synchronize(&self.buffer);
        self.display_map.fold_map_mut().unfold_all();
        self.ensure_layout_cache();
    }

    pub fn unfold_all(&mut self, cx: &mut Context<Self>) {
        self.unfold_all_internal();
        cx.notify();
    }

    #[inline]
    pub fn is_line_folded(&self, line: crate::buffer::SourceLine) -> bool {
        self.display_map().fold_map().is_line_folded(line)
    }

    #[inline]
    pub fn is_fold_header(&self, line: crate::buffer::SourceLine) -> bool {
        self.display_map().fold_map().is_fold_header(line)
    }

    pub fn status_meta(&self) -> Option<EditorStatusMeta> {
        if !self.is_editable() {
            return None;
        }
        Some(EditorStatusMeta {
            is_loading: self.is_loading,
            load_error: self.load_error.clone(),
            is_dirty: self.buffer.is_dirty(),
            external_change: self.external_change,
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

        let target_pos = normalize_buffer_live_cursor(&self.buffer, self.buffer.cursor_pos());
        self.buffer.set_cursor(target_pos);
        self.pixel_column_goal = None;
    }

    pub(crate) fn start_scroll_animation(&mut self, cx: &mut Context<Self>) {
        crate::ui::start_scroll_animation_with_event(
            self,
            cx,
            |editor| &mut editor.scroll_task,
            |editor, cx| {
                let previous = editor.scroll.current_scroll_top;
                let still_animating = editor.scroll.step_interpolation();
                if editor.scroll.current_scroll_top != previous {
                    cx.emit(EditorEvent::ScrollChanged);
                }
                still_animating
            },
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
        if self.find_state.is_stale(self.buffer.revision()) {
            self.clear_pending_find_actions();
            self.refresh_find_matches(cx);
            self.sync_find_bar(cx);
        }
        if self.buffer.is_dirty() != prev_dirty {
            cx.emit(EditorEvent::DirtyChanged(self.buffer.is_dirty()));
        }
        if self.buffer.cursor_pos() != prev_head || self.buffer.char_count() != prev_chars {
            cx.emit(EditorEvent::CursorMoved);
        }
        if self.buffer.version() != prev_version {
            self.schedule_fingerprints(cx);
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
                    let prefix_len = crate::markdown::get_block_prefix_len(&line);
                    let char_count = line.chars().count();
                    if start.col <= prefix_len && end.col == char_count {
                        let end_col = end.col.min(char_count);
                        return Some(line.chars().take(end_col).collect());
                    }
                }
            } else {
                let mut res = Vec::with_capacity(end.line - start.line + 1);
                for line_idx in start.line.index()..=end.line.index() {
                    if let Some(line) = buffer.line(crate::buffer::SourceLine::new(line_idx)) {
                        if line_idx == start.line.index() {
                            let prefix_len = crate::markdown::get_block_prefix_len(&line);
                            let actual_start_col = if start.col <= prefix_len {
                                0
                            } else {
                                start.col
                            };
                            res.push(line.chars().skip(actual_start_col).collect::<String>());
                        } else if line_idx == end.line.index() {
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

    pub fn toggle_task_checkbox(
        &mut self,
        source_line: crate::buffer::SourceLine,
        cx: &mut Context<Self>,
    ) {
        if !self.can_edit() || source_line.index() >= self.buffer.line_count() {
            return;
        }
        let Some(line) = self.buffer.line(source_line) else {
            return;
        };
        if let Some(new_line) = toggle_task_checkbox(&line) {
            self.track_buffer_change(cx, |this| {
                this.buffer.replace_line_range(
                    source_line,
                    1,
                    vec![new_line],
                    this.buffer.selection(),
                );
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
        self.begin_loading(path.clone());
        cx.emit(EditorEvent::TitleChanged);
        cx.emit(EditorEvent::ExternalChangeChanged(self.external_change));
        cx.notify();
        let (session, task) = loading::spawn_editor_load(path, cx);
        self.background_session = Some(session);
        self.load_task = Some(task);
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
        if self.external_change_pending() {
            return Err(std::io::Error::other(
                "File changed externally; reload it or keep the local version before saving",
            ));
        }
        let Some(path) = self.buffer.file_path_buf() else {
            return Err(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                "No file path specified for saving",
            ));
        };

        enum SaveOutcome {
            Conflict(ExternalChangeState),
            Saved {
                saved_mtime: std::time::SystemTime,
                written_hash: u64,
                baseline: crate::buffer::SavedBaseline,
            },
        }

        let snapshot = self.capture_save_snapshot();
        let expected_disk_hash = self.external_disk_hash;
        let allow_overwrite = std::mem::take(&mut self.external_baseline_overridden);

        let bg_path = path.clone();
        let write_work = cx.background_executor().spawn(async move {
            if !allow_overwrite {
                let conflict = match crate::fs::read_file_hash(&bg_path) {
                    Ok(current_hash) if expected_disk_hash != Some(current_hash) => {
                        Some(ExternalChangeState::Pending)
                    }
                    Err(error)
                        if error.kind() == std::io::ErrorKind::NotFound
                            && expected_disk_hash.is_some() =>
                    {
                        Some(ExternalChangeState::Deleted)
                    }
                    _ => None,
                };
                if let Some(state) = conflict {
                    return Ok::<_, std::io::Error>(SaveOutcome::Conflict(state));
                }
            }
            let (saved_mtime, written_hash, baseline) = snapshot.write_to(&bg_path)?;
            Ok(SaveOutcome::Saved {
                saved_mtime,
                written_hash,
                baseline,
            })
        });

        Ok(cx.spawn(async move |this, cx| {
            let outcome = write_work.await?;
            this.update(cx, |editor, cx| match outcome {
                SaveOutcome::Conflict(state) => {
                    editor.external_change = state;
                    cx.emit(EditorEvent::ExternalChangeChanged(state));
                    cx.notify();
                    Err(std::io::Error::other(
                        "File changed externally; reload it or keep the local version before saving",
                    ))
                }
                SaveOutcome::Saved {
                    saved_mtime,
                    written_hash,
                    baseline,
                } => {
                    if editor.file_path() == Some(path.as_path()) {
                        editor.apply_saved_state(
                            saved_mtime,
                            written_hash,
                            baseline,
                            false,
                            cx,
                        );
                    }
                    Ok(())
                }
            })
            .unwrap_or_else(|e| Err(std::io::Error::other(e.to_string())))
        }))
    }

    pub fn save_file_as_async(
        &mut self,
        path: PathBuf,
        cx: &mut Context<Self>,
    ) -> Task<Result<(), std::io::Error>> {
        self.auto_save_task = None;
        if !self.can_edit() {
            return cx
                .spawn(async move |_this, _cx| Err(std::io::Error::other("File is not editable")));
        }

        let snapshot = self.capture_save_snapshot();
        let bg_path = path.clone();
        let write_work = cx
            .background_executor()
            .spawn(async move { snapshot.write_to(&bg_path) });

        cx.spawn(async move |this, cx| {
            let (saved_mtime, written_hash, baseline) = write_work.await?;
            this.update(cx, |editor, cx| {
                editor.buffer.set_file_path(Some(path));
                editor.apply_saved_state(saved_mtime, written_hash, baseline, true, cx);
            })
            .map_err(|e| std::io::Error::other(e.to_string()))
        })
    }

    fn capture_save_snapshot(&self) -> SaveSnapshot {
        SaveSnapshot {
            rope: self.buffer.text_snapshot(),
            line_ending: self.buffer.line_ending(),
            is_empty_doc: self.buffer.is_empty(),
            revision: self.buffer.revision(),
            line_count: self.buffer.line_count(),
            char_count: self.buffer.char_count(),
            ready_hash: self.buffer.prepared_content_hash(),
        }
    }

    fn apply_saved_state(
        &mut self,
        saved_mtime: std::time::SystemTime,
        written_hash: u64,
        baseline: crate::buffer::SavedBaseline,
        restart_watcher: bool,
        cx: &mut Context<Self>,
    ) {
        self.buffer.mark_saved_as(baseline);
        self.buffer.set_last_saved_mtime(Some(saved_mtime));
        self.external_disk_hash = Some(written_hash);
        self.external_baseline_overridden = false;
        self.external_change = ExternalChangeState::None;
        self.baseline_generation = self.baseline_generation.wrapping_add(1);
        if restart_watcher {
            self.external_check_task = None;
            self.start_file_watcher(cx);
        }
        let dirty = self.buffer.is_dirty();
        cx.emit(EditorEvent::TitleChanged);
        cx.emit(EditorEvent::DirtyChanged(dirty));
        cx.emit(EditorEvent::ExternalChangeChanged(
            ExternalChangeState::None,
        ));
        cx.notify();
    }

    /// Returns true if the file on disk has been modified externally since it was opened or last saved.
    pub fn is_externally_modified(&self) -> bool {
        if let Some(path) = self.buffer.file_path() {
            return match crate::fs::read_file_hash(path) {
                Ok(current_hash) => self.external_disk_hash != Some(current_hash),
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                    self.external_disk_hash.is_some()
                }
                Err(error) => {
                    eprintln!(
                        "Failed to inspect file {:?} for external changes: {}",
                        path, error
                    );
                    false
                }
            };
        }
        false
    }

    #[inline]
    pub fn external_change_pending(&self) -> bool {
        self.external_change != ExternalChangeState::None
    }

    #[inline]
    pub fn external_change_state(&self) -> ExternalChangeState {
        self.external_change
    }

    fn refresh_external_baseline_async(
        &mut self,
        clear_not_found: bool,
        clear_overridden: bool,
        cx: &mut Context<Self>,
    ) {
        self.external_check_task = None;
        self.baseline_generation = self.baseline_generation.wrapping_add(1);
        let generation = self.baseline_generation;
        let Some(path) = self.buffer.file_path_buf() else {
            return;
        };
        let bg_path = path.clone();
        let check = cx.background_executor().spawn(async move {
            let hash = crate::fs::read_file_hash(&bg_path);
            let mtime = std::fs::metadata(&bg_path)
                .ok()
                .and_then(|metadata| metadata.modified().ok());
            (hash, mtime)
        });
        self.external_check_task = Some(cx.spawn(async move |this, cx| {
            let (hash, mtime) = check.await;
            let _ = this.update(cx, |editor, _cx| {
                if editor.baseline_generation != generation
                    || editor.file_path() != Some(path.as_path())
                {
                    return;
                }
                match hash {
                    Ok(h) => editor.external_disk_hash = Some(h),
                    Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                        if clear_not_found {
                            editor.external_disk_hash = None;
                        }
                    }
                    Err(error) => {
                        eprintln!(
                            "Failed to update external file baseline for {:?}: {}",
                            path, error
                        );
                    }
                }
                editor.buffer.set_last_saved_mtime(mtime);
                if clear_overridden {
                    editor.external_baseline_overridden = false;
                }
            });
        }));
    }

    /// Clears the conflict marker while retaining the current buffer. A subsequent save will
    /// intentionally replace the current on-disk version.
    pub fn keep_local_changes(&mut self, cx: &mut Context<Self>) {
        self.external_change = ExternalChangeState::None;
        self.external_baseline_overridden = true;
        self.refresh_external_baseline_async(true, true, cx);
        cx.emit(EditorEvent::ExternalChangeChanged(self.external_change));
        cx.notify();
    }

    /// Handles a notification from the file watcher. Clean buffers reload immediately; dirty
    /// buffers keep the user's edits and expose an explicit reload decision to the UI.
    pub fn handle_external_file_change(&mut self, cx: &mut Context<Self>) {
        let Some(path) = self.buffer.file_path_buf() else {
            return;
        };
        let generation = self.baseline_generation;
        let bg_path = path.clone();
        let check = cx
            .background_executor()
            .spawn(async move { crate::fs::read_file_hash(&bg_path) });
        self.external_check_task = Some(cx.spawn(async move |this, cx| {
            let result = check.await;
            let _ = this.update(cx, |editor, cx| {
                if editor.baseline_generation != generation
                    || editor.file_path() != Some(path.as_path())
                {
                    return;
                }
                let disk_hash = match result {
                    Ok(hash) => Some(hash),
                    Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                        editor.set_external_change_state(ExternalChangeState::Deleted, cx);
                        return;
                    }
                    Err(error) => {
                        eprintln!(
                            "Failed to inspect file {:?} after a watcher event: {}",
                            path, error
                        );
                        return;
                    }
                };
                if editor.external_disk_hash == disk_hash {
                    editor.external_baseline_overridden = false;
                    editor.set_external_change_state(ExternalChangeState::None, cx);
                    return;
                }
                if editor.is_dirty() {
                    editor.set_external_change_state(ExternalChangeState::Pending, cx);
                } else if let Err(error) = editor.reload_file(cx) {
                    eprintln!(
                        "Failed to reload externally modified file {:?}: {}",
                        path, error
                    );
                }
            });
        }));
    }

    fn set_external_change_state(&mut self, state: ExternalChangeState, cx: &mut Context<Self>) {
        if self.external_change != state {
            self.external_change = state;
            cx.emit(EditorEvent::ExternalChangeChanged(state));
            cx.notify();
        }
    }

    /// Reloads the current file from disk, replacing the in-memory buffer and clearing the
    /// external-change marker. Callers should use this only after an explicit user choice when
    /// the buffer contains local edits.
    pub fn reload_file(&mut self, cx: &mut Context<Self>) -> Result<(), std::io::Error> {
        let Some(path) = self.buffer.file_path_buf() else {
            return Err(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                "No file path specified for reloading",
            ));
        };
        self.load_file(path, cx)
    }

    fn start_file_watcher(&mut self, cx: &mut Context<Self>) {
        self.file_watch_task = None;
        let Some(path) = self.buffer.file_path_buf() else {
            return;
        };
        let parent = path
            .parent()
            .filter(|parent| !parent.as_os_str().is_empty())
            .unwrap_or_else(|| Path::new("."))
            .to_path_buf();

        let target_name = path.file_name().map(|name| name.to_os_string());
        let (tx, mut rx) = futures::channel::mpsc::unbounded::<()>();
        let watch_path = path.clone();
        let setup = cx.background_executor().spawn(async move {
            let mut watcher =
                match notify::recommended_watcher(move |result: notify::Result<notify::Event>| {
                    let event = match result {
                        Ok(event) => event,
                        Err(error) => {
                            eprintln!("File watcher error for {:?}: {}", watch_path, error);
                            return;
                        }
                    };
                    if !(event.kind.is_modify() || event.kind.is_create() || event.kind.is_remove())
                    {
                        return;
                    }
                    let Some(target_name) = target_name.as_ref() else {
                        return;
                    };
                    if event
                        .paths
                        .iter()
                        .any(|event_path| event_path.file_name() == Some(target_name.as_os_str()))
                    {
                        let _ = tx.unbounded_send(());
                    }
                }) {
                    Ok(watcher) => watcher,
                    Err(error) => {
                        eprintln!("Failed to watch file {:?}: {}", path, error);
                        return None;
                    }
                };

            if let Err(error) =
                notify::Watcher::watch(&mut watcher, &parent, notify::RecursiveMode::NonRecursive)
            {
                eprintln!("Failed to watch file {:?}: {}", path, error);
                return None;
            }
            Some(watcher)
        });

        self.file_watch_task = Some(cx.spawn(async move |this, cx| {
            use futures::StreamExt;
            let Some(watcher) = setup.await else {
                return;
            };
            while rx.next().await.is_some() {
                if this
                    .update(cx, |editor, cx| editor.handle_external_file_change(cx))
                    .is_err()
                {
                    break;
                }
            }
            drop(watcher);
        }));
    }

    fn render_unsupported_file_prompt(&self, theme: &Theme) -> impl IntoElement {
        div()
            .id("unsupported_file_view")
            .key_context(crate::ui::key_context::EDITOR)
            .track_focus(&self.focus_handle)
            .w_full()
            .h_full()
            .flex()
            .items_center()
            .justify_center()
            .text_size(px(14.0))
            .text_color(theme.text_muted)
            .child("The file type is not supported")
    }
}

impl Render for Editor {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if self.text_system.is_none() {
            // Layout measurement must shape text exactly like painting does, which requires the
            // window's text system; drop anything measured with the headless fallback.
            self.text_system = Some(window.text_system().clone());
            self.space_width_memo.set(None);
            self.invalidate_layout_cache();
        }

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

        if self.is_loading && self.buffer.is_empty() {
            return div()
                .size_full()
                .track_focus(&self.focus_handle)
                .flex()
                .items_center()
                .justify_center()
                .text_color(theme.text_muted)
                .child("Loading...")
                .into_any_element();
        }

        if !self.is_editable() {
            return self
                .render_unsupported_file_prompt(theme)
                .into_any_element();
        }

        let is_focused = self.focus_handle.is_focused(window);
        if !is_focused && self.blink_task.is_some() {
            self.cursor_opacity = 0.0;
            self.blink_epoch = self.blink_epoch.wrapping_add(1);
            self.blink_task = None;
        }

        let cursor_style = if self.hovered_link.is_some() {
            CursorStyle::PointingHand
        } else {
            CursorStyle::IBeam
        };

        let element = EditorElement::new(cx.entity().clone())
            .key_context(crate::ui::key_context::EDITOR)
            .id("editor_view")
            .track_focus(&self.focus_handle)
            .cursor(cursor_style);

        let element = Self::register_actions(element, cx);
        if self.needs_focus_after_find {
            self.needs_focus_after_find = false;
            window.focus(&self.focus_handle);
        }

        let element = Self::register_mouse_listeners(element, cx);

        div()
            .key_context(self.pane_key_context())
            .size_full()
            .flex()
            .flex_col()
            .on_action(
                cx.listener(|this, _: &FindInDocument, window, cx| this.open_find_bar(window, cx)),
            )
            .on_action(
                cx.listener(|this, _: &ReplaceInDocument, window, cx| {
                    this.open_replace(window, cx)
                }),
            )
            .on_action(cx.listener(|this, _: &FindNext, _, cx| this.find_next(cx)))
            .on_action(cx.listener(|this, _: &FindPrevious, _, cx| this.find_previous(cx)))
            .on_action(cx.listener(|this, _: &CloseFindBar, window, cx| {
                this.close_find_bar(Some(window), cx)
            }))
            .child(
                div()
                    .relative()
                    .flex_1()
                    .size_full()
                    .overflow_hidden()
                    .child(element)
                    .when_some(self.find_bar(), |this, bar| {
                        this.child(
                            div()
                                .absolute()
                                .top(Spacing::MD)
                                .right(Spacing::LG)
                                .w(px(500.0))
                                .max_w(relative(0.92))
                                .child(bar.clone()),
                        )
                    }),
            )
            .into_any_element()
    }
}

#[derive(Clone, Copy)]
pub struct LineRenderContext<'a> {
    pub is_active_line: bool,
    pub is_math_block_editing: bool,
    pub(crate) math_preview: Option<&'a crate::math::MathSvg>,
    pub is_mermaid_block_editing: bool,
    pub(crate) mermaid_preview: Option<&'a crate::mermaid::MermaidSvg>,
    pub is_disclosure_line: bool,
    pub is_selection_disclosure: bool,
    pub is_focused: bool,
    pub cursor_col: usize,
    /// Column used for markdown syntax-marker disclosure.
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
    /// Per-line visual indent in pixels (Live Preview only).
    pub indent_px: Pixels,
    /// Indentation inside an enclosing blockquote.
    pub quote_content_indent_px: Pixels,
    /// Whether an inactive paragraph line is a syntactic separator.
    pub is_syntactic_separator: bool,
    pub text_system: Option<&'a Arc<WindowTextSystem>>,
    pub highlights: &'a [LineHighlight],
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HighlightKind {
    SearchMatch,
    ActiveSearchMatch,
}

impl HighlightKind {
    pub fn color(self, theme: &Theme) -> Hsla {
        match self {
            HighlightKind::SearchMatch => theme.search_match,
            HighlightKind::ActiveSearchMatch => theme.search_match_active,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LineHighlight {
    pub cols: Range<usize>,
    pub continues: bool,
    pub kind: HighlightKind,
}
