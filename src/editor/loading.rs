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
        if let Some(snapshot) = self.prepared_document.take() {
            snapshot.session.cancel();
        }
        self.auto_save_task = None;
        self.file_watch_task = None;
        self.fingerprint_task = None;
        self.buffer.set_file_path(Some(path));
        self.load_error = None;
        self.is_loading = true;
        self.pending_open_position = None;
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
        match result {
            Ok(file) => {
                if !self.is_loading && self.buffer.revision() != file.buffer.revision() {
                    session.cancel();
                    return;
                }
                let finishing = self.is_loading && file.complete;
                if self.is_loading {
                    let cursor = self.buffer.cursor_pos();
                    self.buffer = file.buffer;
                    self.buffer.set_cursor(cursor);
                }
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
                        if let Some(snapshot) = &self.prepared_document {
                            for window in &snapshot.windows {
                                cache.metrics.invalidate_measured(window.range.clone());
                            }
                        }
                    }
                }
                if finishing {
                    self.is_loading = false;
                    self.external_disk_hash = Some(file.disk_hash);
                    self.schedule_fingerprints(cx);
                    self.start_file_watcher(cx);
                    if let Some(position) = self.pending_open_position.take() {
                        self.open_at_position(position, cx);
                    }
                }
            }
            Err(error) => {
                eprintln!("Failed to load file {:?}: {error}", self.buffer.file_path());
                self.is_loading = false;
                self.load_error = Some(format!("Could not open file: {error}"));
                session.cancel();
            }
        }
        cx.emit(EditorEvent::TitleChanged);
        cx.emit(EditorEvent::DirtyChanged(self.is_dirty()));
        cx.emit(EditorEvent::ExternalChangeChanged(
            ExternalChangeState::None,
        ));
        cx.notify();
    }
}
