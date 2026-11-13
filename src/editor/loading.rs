use super::{Editor, EditorEvent, ExternalChangeState, Position};
use gpui::Context;
use std::{io, path::PathBuf, sync::Arc};
mod session;
pub(super) use session::source_window;
pub(crate) use session::{CancelSession, DocumentSession, PreparedSnapshot};

impl Editor {
    pub fn is_loading(&self) -> bool {
        self.is_loading
    }

    pub(crate) fn new_loading(path: PathBuf, cx: &mut Context<Self>) -> Self {
        let mut editor = Self::new(cx);
        editor.begin_loading(path);
        editor
    }

    pub(crate) fn begin_loading(&mut self, path: PathBuf) {
        self.display_map = super::DisplayMap::new();
        if let Some(snapshot) = self.prepared_document.take() {
            snapshot.session.cancel();
        }
        self.load_task = None;
        self.auto_save_task = None;
        self.file_watch_task = None;
        self.fingerprint_task = None;
        self.reset_find_search();
        self.external_change = ExternalChangeState::None;
        self.buffer.set_file_path(Some(path));
        self.load_error = None;
        self.is_loading = true;
        self.pending_open_position = None;
        self.invalidate_layout_cache();
    }

    pub(crate) fn open_at_position(&mut self, position: Position, cx: &mut Context<Self>) {
        if self.is_loading {
            self.pending_open_position = Some(position);
        } else {
            self.buffer.set_cursor(position);
            self.scroll_to_cursor();
            self.start_scroll_animation_if_needed(cx);
            self.reset_cursor_blink(cx);
        }
    }

    pub(crate) fn install_publication(
        &mut self,
        session: &Arc<DocumentSession>,
        result: io::Result<session::Publication>,
        cx: &mut Context<Self>,
    ) {
        let mut emit_document_events = self.is_loading;
        match result {
            Ok(file) => {
                if !self.is_loading && file.buffer.is_some() {
                    if self.buffer.revision() != file.revision {
                        session.cancel();
                    }
                    return;
                }
                if !self.is_loading && file.revision != self.buffer.revision() {
                    return;
                }
                let has_disk_buffer = file.buffer.is_some();
                let finishing = self.is_loading && file.complete;
                if self.is_loading
                    && let Some(buffer) = file.buffer
                {
                    let cursor = self.buffer.cursor_pos();
                    self.buffer = buffer;
                    self.buffer.set_cursor(cursor);
                }
                let previous_windows = self
                    .prepared_document
                    .as_ref()
                    .filter(|prev| prev.revision == self.buffer.revision())
                    .map(|prev| prev.windows.clone())
                    .unwrap_or_default();
                let changed_ranges: Vec<_> = file
                    .windows
                    .iter()
                    .filter(|new_win| {
                        !previous_windows.iter().any(|old_win| {
                            old_win.range == new_win.range
                                && Arc::ptr_eq(&old_win.rows, &new_win.rows)
                        })
                    })
                    .map(|win| win.range.clone())
                    .collect();
                self.prepared_document = Some(Arc::new(PreparedSnapshot {
                    revision: self.buffer.revision(),
                    session: session.clone(),
                    windows: file.windows,
                }));
                {
                    let mut cache = self.layout_cache.borrow_mut();
                    // Multiple publications can arrive before a frame catches layout up.
                    // Only invalidate geometry that belongs to this source snapshot.
                    if cache.buffer_revision == self.buffer.revision()
                        && cache.lines_len == self.buffer.line_count()
                        && cache.metrics.len() == self.buffer.line_count()
                        && cache.parsed_lines.is_windowed()
                    {
                        let cache = Arc::make_mut(&mut cache);
                        cache.parsed_lines =
                            super::parsed_line_store::ParsedLineStore::windowed(&self.buffer)
                                .with_prepared(self.prepared_document.clone());
                        for range in &changed_ranges {
                            cache.metrics.invalidate_measured(range.clone());
                        }
                    }
                }
                if finishing {
                    self.is_loading = false;
                    self.external_disk_hash = Some(file.disk_hash);
                    self.external_change = ExternalChangeState::None;
                    self.schedule_fingerprints(cx);
                    self.start_file_watcher(cx);
                    self.refresh_find_matches(cx);
                    self.sync_find_bar(cx);
                    self.last_cursor_action = std::time::Instant::now();
                    if let Some(position) = self.pending_open_position.take() {
                        self.open_at_position(position, cx);
                    }
                }
                if !emit_document_events && !has_disk_buffer {
                    if !changed_ranges.is_empty() {
                        cx.notify();
                    }
                    return;
                }
            }
            Err(error) => {
                emit_document_events = true;
                eprintln!("Failed to load file {:?}: {error}", self.buffer.file_path());
                self.is_loading = false;
                self.load_error = Some(format!("Could not open file: {error}"));
                session.cancel();
            }
        }
        if emit_document_events {
            cx.emit(EditorEvent::TitleChanged);
            cx.emit(EditorEvent::DirtyChanged(self.is_dirty()));
            cx.emit(EditorEvent::ExternalChangeChanged(
                ExternalChangeState::None,
            ));
            cx.emit(EditorEvent::CursorMoved);
        }
        cx.notify();
    }
}

pub(crate) fn spawn_session(
    session: &Arc<DocumentSession>,
    path: PathBuf,
    executor: &gpui::BackgroundExecutor,
) -> (gpui::Task<()>, gpui::Task<()>, CancelSession) {
    let reader_session = session.clone();
    let reader_path = path.clone();
    let read_work = executor.spawn(async move { reader_session.run_reader(reader_path) });
    let worker_session = session.clone();
    let prepare_work = executor.spawn(async move { worker_session.run_worker(path).await });
    (read_work, prepare_work, CancelSession(session.clone()))
}

pub(crate) fn spawn_editor_load(path: PathBuf, cx: &mut Context<Editor>) -> gpui::Task<()> {
    let requested_path = path.clone();
    let session = DocumentSession::new();
    let mut updates = session.updates.subscribe();
    let session_work = spawn_session(&session, path, cx.background_executor());
    cx.spawn(async move |this, cx| {
        use futures::StreamExt;
        let _session_work = session_work;
        while updates.next().await.is_some() {
            let Some(result) = session.take() else {
                continue;
            };
            let keep_running = this
                .update(cx, |editor, cx| {
                    if editor.file_path() != Some(requested_path.as_path()) {
                        return false;
                    }
                    editor.install_publication(&session, result, cx);
                    true
                })
                .unwrap_or(false);
            if !keep_running {
                break;
            }
        }
    })
}
