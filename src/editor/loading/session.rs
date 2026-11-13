use crate::buffer::{
    LineSource, SourceLine, SourceSnapshot, StreamingBuffer, StructureIndex, TextBuffer,
};
use crate::markdown::{BlockKind, InlineSpan, MarkdownScanner, ParsedLine};
use crate::sync::{Mutex, VersionedNotifier};
use futures::StreamExt;
use std::{
    collections::VecDeque,
    io::{self, Read},
    ops::Range,
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
};

const READ_BYTES: usize = 64 * 1024;
const WINDOW_ROWS: usize = 256;
const MAX_WINDOWS: usize = 8;

#[derive(Clone, Debug)]
pub(crate) struct PreparedWindow {
    pub range: Range<usize>,
    pub rows: Arc<Vec<ParsedLine>>,
}

pub(crate) struct Publication {
    pub buffer: Option<TextBuffer>,
    pub revision: u64,
    pub windows: VecDeque<PreparedWindow>,
    pub complete: bool,
    pub disk_hash: u64,
}

#[derive(Clone, Debug)]
struct CachedWindow {
    range: Range<usize>,
    rows: Arc<Vec<ParsedLine>>,
    revision: u64,
}

#[derive(Clone, Debug)]
struct WorkTarget {
    source: SourceSnapshot,
    index: StructureIndex,
    revision: u64,
    from_disk: bool,
    complete: bool,
    disk_hash: u64,
    immutable_before: usize,
}

#[derive(Debug)]
struct SessionState {
    latest: Option<io::Result<Publication>>,
    requested: Range<usize>,
    target: Option<WorkTarget>,
    windows: VecDeque<CachedWindow>,
}

/// Decoupled reader mailbox, viewport request target, and background window cache.
#[derive(Debug)]
pub(crate) struct DocumentSession {
    state: Mutex<SessionState>,
    work: VersionedNotifier,
    pub updates: VersionedNotifier,
    cancelled: AtomicBool,
}

impl std::fmt::Debug for Publication {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Publication")
            .field("revision", &self.revision)
            .field("has_buffer", &self.buffer.is_some())
            .field("complete", &self.complete)
            .finish()
    }
}

#[derive(Clone, Debug)]
pub(crate) struct PreparedSnapshot {
    pub revision: u64,
    pub session: Arc<DocumentSession>,
    pub windows: VecDeque<PreparedWindow>,
}

fn clamp_request(requested: &Range<usize>, line_count: usize) -> Range<usize> {
    requested.start.min(line_count)..requested.end.min(line_count)
}

impl DocumentSession {
    pub fn new() -> Arc<Self> {
        Arc::new(Self {
            state: Mutex::new(SessionState {
                latest: None,
                requested: 0..WINDOW_ROWS * 2,
                target: None,
                windows: VecDeque::new(),
            }),
            work: VersionedNotifier::new(),
            updates: VersionedNotifier::new(),
            cancelled: AtomicBool::new(false),
        })
    }

    pub fn cancel(&self) {
        self.cancelled.store(true, Ordering::Release);
        self.work.notify();
        self.updates.close();
    }

    pub fn request(&self, range: Range<usize>) {
        let start = range.start / WINDOW_ROWS * WINDOW_ROWS;
        let end = range.end.div_ceil(WINDOW_ROWS) * WINDOW_ROWS;
        let mut state = self.state.lock();
        if state.requested != (start..end) {
            state.requested = start..end;
            drop(state);
            self.work.notify();
        }
    }

    pub fn submit_target(&self, buffer: &TextBuffer) {
        let mut state = self.state.lock();
        if state
            .target
            .as_ref()
            .is_some_and(|target| !target.from_disk && target.revision == buffer.revision())
        {
            return;
        }
        state.windows.clear();
        let disk_hash = state.target.as_ref().map_or(0, |t| t.disk_hash);
        state.target = Some(WorkTarget {
            source: buffer.source_snapshot(),
            index: buffer.line_index().structure_snapshot(),
            revision: buffer.revision(),
            from_disk: false,
            complete: true,
            disk_hash,
            immutable_before: 0,
        });
        drop(state);
        self.work.notify();
    }

    pub fn take(&self) -> Option<io::Result<Publication>> {
        self.state.lock().latest.take()
    }

    fn publish(&self, publication: io::Result<Publication>) {
        if !self.cancelled.load(Ordering::Acquire) {
            self.state.lock().latest = Some(publication);
            self.updates.notify();
        }
    }

    fn publish_stream(
        &self,
        buffer: TextBuffer,
        complete: bool,
        disk_hash: u64,
        previous_tail: usize,
    ) {
        if self.cancelled.load(Ordering::Acquire) {
            return;
        }
        let source = buffer.source_snapshot();
        let index = buffer.line_index().structure_snapshot();
        let revision = buffer.revision();
        let line_count = buffer.line_count();
        let mut state = self.state.lock();
        state
            .windows
            .retain(|window| window.range.end < previous_tail);
        state.target = Some(WorkTarget {
            source: source.clone(),
            index,
            revision,
            from_disk: true,
            complete,
            disk_hash,
            immutable_before: previous_tail,
        });
        let range = clamp_request(&state.requested, line_count);
        let mut windows: VecDeque<PreparedWindow> = state
            .windows
            .iter()
            .map(|window| PreparedWindow {
                range: window.range.clone(),
                rows: window.rows.clone(),
            })
            .collect();
        if !windows
            .iter()
            .any(|window| window.range.start <= range.start && window.range.end >= range.end)
        {
            windows.push_back(source_window(&source, range));
        }
        state.latest = Some(Ok(Publication {
            buffer: Some(buffer),
            revision,
            windows,
            complete,
            disk_hash,
        }));
        drop(state);
        self.updates.notify();
        self.work.notify();
    }

    fn prepare_pending(&self, path: &Path, force_refresh: bool) {
        if self.cancelled.load(Ordering::Acquire) {
            return;
        }
        let target = {
            let state = self.state.lock();
            let Some(target) = state.target.clone() else {
                return;
            };
            let range = clamp_request(&state.requested, target.source.line_count());
            let already_covered = !force_refresh
                && state.windows.iter().any(|window| {
                    window.range.start <= range.start
                        && window.range.end >= range.end
                        && (window.revision == target.revision
                            || (target.from_disk && window.range.end < target.immutable_before))
                });
            if already_covered {
                return;
            }
            (target, range)
        };
        let (target, range) = target;
        let (scanned_range, rows) =
            MarkdownScanner::scan_snapshot_window(&target.source, &target.index, range);
        prepare_resources(&rows, path);
        if self.cancelled.load(Ordering::Acquire) {
            return;
        }
        let rows = Arc::new(rows);
        let mut state = self.state.lock();
        let Some(current) = state.target.clone() else {
            return;
        };
        let still_valid = current.revision == target.revision
            || (current.from_disk && scanned_range.end < current.immutable_before);
        if !still_valid {
            return;
        }
        state.windows.retain(|window| window.range != scanned_range);
        state.windows.push_back(CachedWindow {
            range: scanned_range,
            rows,
            revision: current.revision,
        });
        while state.windows.len() > MAX_WINDOWS {
            state.windows.pop_front();
        }
        let windows: VecDeque<PreparedWindow> = state
            .windows
            .iter()
            .map(|window| PreparedWindow {
                range: window.range.clone(),
                rows: window.rows.clone(),
            })
            .collect();
        let (prev_buffer, prev_complete, prev_hash) = match state.latest.take() {
            Some(Ok(prev)) => (prev.buffer, prev.complete, prev.disk_hash),
            Some(Err(err)) => {
                state.latest = Some(Err(err));
                return;
            }
            None => (None, false, 0),
        };
        state.latest = Some(Ok(Publication {
            buffer: prev_buffer,
            revision: current.revision,
            windows,
            complete: current.complete || prev_complete,
            disk_hash: current.disk_hash.max(prev_hash),
        }));
        drop(state);
        self.updates.notify();
    }

    fn stream_reader(self: &Arc<Self>, mut reader: impl Read, path: &Path) -> io::Result<()> {
        let mut stream = StreamingBuffer::new(Some(path.to_path_buf()));
        let mut bytes = vec![0; READ_BYTES];
        let mut first = true;
        loop {
            if self.cancelled.load(Ordering::Acquire) {
                return Ok(());
            }
            // The first read is deliberately small so a slow file can publish immediately.
            let capacity = if first { 4096 } else { READ_BYTES };
            let count = match reader.read(&mut bytes[..capacity]) {
                Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
                result => result?,
            };
            let complete = count == 0;
            let previous_tail = stream.buffer.line_count().saturating_sub(1);
            stream
                .push(&bytes[..count], complete)
                .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
            if complete {
                stream.buffer.set_last_saved_mtime(
                    std::fs::metadata(path).ok().and_then(|m| m.modified().ok()),
                );
            }
            self.publish_stream(
                stream.buffer.clone(),
                complete,
                stream.disk_hash(),
                previous_tail,
            );
            first = false;
            if complete {
                return Ok(());
            }
        }
    }

    pub fn run_reader(self: &Arc<Self>, path: PathBuf) {
        let result = if !crate::editor::is_whitelisted_extension(&path) {
            self.stream_reader(io::empty(), &path)
        } else {
            match std::fs::File::open(&path) {
                Ok(file) => self.stream_reader(file, &path),
                Err(error) => Err(error),
            }
        };
        if let Err(error) = result {
            self.publish(Err(error));
            self.cancel();
        }
    }

    pub async fn run_worker(self: Arc<Self>, path: PathBuf) {
        let work_events = self.work.subscribe();
        let resources = futures::stream::select(
            crate::http::subscribe_image_updates().map(|_| true),
            crate::syntax::subscribe_syntax_updates().map(|_| true),
        );
        let mut wakeups = futures::stream::select(work_events.map(|_| false), resources);
        let mut force_refresh = false;
        loop {
            if self.cancelled.load(Ordering::Acquire) {
                break;
            }
            self.prepare_pending(&path, force_refresh);
            if self.cancelled.load(Ordering::Acquire) {
                break;
            }
            match wakeups.next().await {
                Some(refresh) => force_refresh = refresh,
                None => break,
            }
        }
        self.updates.close();
    }

    #[cfg(test)]
    async fn read_and_prepare(
        self: &Arc<Self>,
        reader: impl Read + Send + 'static,
        path: PathBuf,
    ) -> io::Result<()> {
        use futures::{FutureExt, channel::oneshot, select_biased};
        let work_events = self.work.subscribe();
        let resources = futures::stream::select(
            crate::http::subscribe_image_updates().map(|_| true),
            crate::syntax::subscribe_syntax_updates().map(|_| true),
        );
        let mut wakeups = futures::stream::select(work_events.map(|_| false), resources);

        let (reader_tx, reader_rx) = oneshot::channel::<io::Result<()>>();
        let reader_session = self.clone();
        let reader_path = path.clone();
        let reader_thread = std::thread::spawn(move || {
            let result = reader_session.stream_reader(reader, &reader_path);
            let _ = reader_tx.send(result);
        });

        let mut reader_rx = reader_rx.fuse();
        let mut reader_handle = Some(reader_thread);
        loop {
            if self.cancelled.load(Ordering::Acquire) {
                break;
            }
            let force_refresh = select_biased! {
                res = reader_rx => {
                    if let Some(handle) = reader_handle.take() {
                        let _ = handle.join();
                    }
                    match res {
                        Ok(Ok(())) | Err(_) => false,
                        Ok(Err(error)) => return Err(error),
                    }
                }
                event = wakeups.next().fuse() => {
                    match event {
                        Some(force_refresh) => force_refresh,
                        None => break,
                    }
                }
            };
            if self.cancelled.load(Ordering::Acquire) {
                break;
            }
            self.prepare_pending(&path, force_refresh);
        }
        if let Some(handle) = reader_handle.take() {
            let _ = handle.join();
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::mpsc;

    #[gpui::test]
    fn consecutive_publications_before_layout_use_the_latest_snapshot(
        cx: &mut gpui::TestAppContext,
    ) {
        cx.update(|cx| {
            let config = crate::config::AppConfig::default();
            cx.set_global(crate::theme::ThemeManager::from_config(&config));
            cx.set_global(config);
        });
        let (editor, cx) = cx.add_window_view(|_, cx| {
            crate::editor::Editor::new_loading("publication-order.md".into(), cx)
        });
        editor.update(cx, |editor, cx| {
            let session = DocumentSession::new();
            let mut stream = StreamingBuffer::new(Some("publication-order.md".into()));
            for scenario in 0..3 {
                stream.push(b"# heading\nbody\n", false).unwrap();
                let source = source_window(
                    &stream.buffer.source_snapshot(),
                    0..stream.buffer.line_count(),
                );
                editor.install_publication(
                    &session,
                    Ok(Publication {
                        buffer: Some(stream.buffer.clone()),
                        revision: stream.buffer.revision(),
                        windows: VecDeque::from([source]),
                        complete: false,
                        disk_hash: stream.disk_hash(),
                    }),
                    cx,
                );
                if scenario == 2 {
                    editor.layout_snapshot();
                    editor.invalidate_layout_cache();
                }
                // No frame has caught up with the new buffer (or its layout was reset).
                let stale = editor.layout_cache.borrow().clone();
                let stale_len = stale.metrics.len();
                let (range, rows) =
                    MarkdownScanner::scan_window(&stream.buffer, 0..stream.buffer.line_count());
                editor.install_publication(
                    &session,
                    Ok(Publication {
                        buffer: Some(stream.buffer.clone()),
                        revision: stream.buffer.revision(),
                        windows: VecDeque::from([PreparedWindow {
                            range,
                            rows: Arc::new(rows),
                        }]),
                        complete: false,
                        disk_hash: stream.disk_hash(),
                    }),
                    cx,
                );
                assert_eq!(stale.metrics.len(), stale_len);
                let layout = editor.layout_snapshot();
                assert_eq!(layout.metrics.len(), stream.buffer.line_count());
                assert_eq!(layout.buffer_revision, stream.buffer.revision());
                assert!(matches!(
                    layout.parsed_lines.row(0).unwrap().kind,
                    BlockKind::Heading { level: 1 }
                ));
            }
            session.cancel();
        });
    }

    #[gpui::test]
    fn partial_snapshot_is_visible_read_only_and_stale_completion_cannot_replace_edits(
        cx: &mut gpui::TestAppContext,
    ) {
        cx.update(|cx| {
            let config = crate::config::AppConfig::default();
            cx.set_global(crate::theme::ThemeManager::from_config(&config));
            cx.set_global(config);
        });
        let path = PathBuf::from("progressive.md");
        let session = DocumentSession::new();
        let mut stream = StreamingBuffer::new(Some(path.clone()));
        stream.push(b"```rust\n# code\n", false).unwrap();
        let window = source_window(
            &stream.buffer.source_snapshot(),
            0..stream.buffer.line_count(),
        );
        let (editor, cx) = cx.add_window_view(|_, cx| crate::editor::Editor::new_loading(path, cx));
        editor.update(cx, |editor, cx| {
            editor.install_publication(
                &session,
                Ok(Publication {
                    buffer: Some(stream.buffer.clone()),
                    revision: stream.buffer.revision(),
                    windows: VecDeque::from([window]),
                    complete: false,
                    disk_hash: stream.disk_hash(),
                }),
                cx,
            );
            assert!(editor.is_loading());
            assert!(!editor.can_edit());
            assert!(editor.save_file_async(cx).is_err());
            assert_eq!(
                editor.layout_snapshot().parsed_lines.row(1).unwrap().kind,
                BlockKind::CodeBlockContent
            );
        });
        cx.run_until_parked();
        editor.update(cx, |editor, cx| {
            editor.install_publication(
                &session,
                Ok(Publication {
                    buffer: Some(stream.buffer.clone()),
                    revision: stream.buffer.revision(),
                    windows: VecDeque::new(),
                    complete: true,
                    disk_hash: stream.disk_hash(),
                }),
                cx,
            );
            assert!(editor.can_edit());
            editor.buffer.insert_text("edited");
            let content = editor.buffer.to_string_content();
            editor.install_publication(
                &session,
                Ok(Publication {
                    buffer: Some(stream.buffer.clone()),
                    revision: stream.buffer.revision(),
                    windows: VecDeque::new(),
                    complete: true,
                    disk_hash: stream.disk_hash(),
                }),
                cx,
            );
            assert_eq!(editor.buffer.to_string_content(), content);
            assert!(session.cancelled.load(Ordering::Acquire));
        });
    }

    struct PausedReader {
        second_read: mpsc::Sender<()>,
        resume: mpsc::Receiver<()>,
        reads: usize,
    }

    #[gpui::test]
    fn ui_renders_first_publication_while_next_read_is_blocked(cx: &mut gpui::TestAppContext) {
        cx.update(|cx| {
            let config = crate::config::AppConfig::default();
            cx.set_global(crate::theme::ThemeManager::from_config(&config));
            cx.set_global(config);
        });
        let session = DocumentSession::new();
        let (entered, entered_rx) = mpsc::channel();
        let (resume, resume_rx) = mpsc::channel();
        let worker_session = session.clone();
        let worker = std::thread::spawn(move || {
            futures::executor::block_on(worker_session.read_and_prepare(
                PausedReader {
                    second_read: entered,
                    resume: resume_rx,
                    reads: 0,
                },
                "blocked.md".into(),
            ))
        });
        entered_rx
            .recv_timeout(std::time::Duration::from_secs(5))
            .unwrap();
        let (editor, cx) =
            cx.add_window_view(|_, cx| crate::editor::Editor::new_loading("blocked.md".into(), cx));
        editor.update(cx, |editor, cx| {
            editor.install_publication(&session, session.take().unwrap(), cx)
        });
        cx.run_until_parked();
        editor.update(cx, |editor, cx| {
            assert!(editor.is_loading());
            assert!(!editor.can_edit());
            assert_eq!(
                editor
                    .layout_snapshot()
                    .parsed_lines
                    .row(1)
                    .unwrap()
                    .raw_text,
                "# code, not a heading"
            );
            assert_eq!(editor.layout_stats.get().lines_scanned, 0);
            editor.on_blur(cx);
        });
        session.cancel();
        resume.send(()).unwrap();
        worker.join().unwrap().unwrap();
    }

    impl Read for PausedReader {
        fn read(&mut self, bytes: &mut [u8]) -> io::Result<usize> {
            self.reads += 1;
            if self.reads == 1 {
                let text = b"```rust\n# code, not a heading\n";
                bytes[..text.len()].copy_from_slice(text);
                return Ok(text.len());
            }
            self.second_read.send(()).unwrap();
            self.resume.recv().unwrap();
            Ok(0)
        }
    }

    #[test]
    fn publishes_before_second_read_finishes_and_cancels_without_eof() {
        let session = DocumentSession::new();
        let (entered, entered_rx) = mpsc::channel();
        let (resume, resume_rx) = mpsc::channel();
        let worker_session = session.clone();
        let worker = std::thread::spawn(move || {
            futures::executor::block_on(worker_session.read_and_prepare(
                PausedReader {
                    second_read: entered,
                    resume: resume_rx,
                    reads: 0,
                },
                PathBuf::from("stream.md"),
            ))
        });
        entered_rx
            .recv_timeout(std::time::Duration::from_secs(5))
            .unwrap();
        let publication = session.take().unwrap().unwrap();
        assert!(!publication.complete);
        assert_eq!(publication.buffer.as_ref().unwrap().line_count(), 3);
        assert!(matches!(
            publication.windows[0].rows[1].kind,
            BlockKind::CodeBlockContent
        ));
        session.cancel();
        resume.send(()).unwrap();
        worker.join().unwrap().unwrap();
        assert!(session.take().is_none());
    }

    #[test]
    fn publication_mailbox_retains_only_latest_snapshot() {
        let session = DocumentSession::new();
        let mut stream = StreamingBuffer::new(None);
        for _ in 0..100 {
            stream.push(b"line\n", false).unwrap();
            session.publish(Ok(Publication {
                buffer: Some(stream.buffer.clone()),
                revision: stream.buffer.revision(),
                windows: VecDeque::new(),
                complete: false,
                disk_hash: stream.disk_hash(),
            }));
        }
        assert_eq!(
            session
                .take()
                .unwrap()
                .unwrap()
                .buffer
                .as_ref()
                .unwrap()
                .line_count(),
            101
        );
        assert!(session.take().is_none());
    }

    #[test]
    fn completed_session_prepares_requested_windows_and_bounds_retention() {
        let session = DocumentSession::new();
        let worker_session = session.clone();
        let worker = std::thread::spawn(move || {
            futures::executor::block_on(worker_session.read_and_prepare(
                io::Cursor::new("paragraph\n".repeat(8192)),
                "windows.md".into(),
            ))
        });
        let wait = |range: Range<usize>| {
            let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
            loop {
                if let Some(publication) = session.take() {
                    let publication = publication.unwrap();
                    if publication.complete
                        && publication.windows.iter().any(|window| {
                            window.range.start <= range.start && window.range.end >= range.end
                        })
                    {
                        assert!(publication.windows.len() <= MAX_WINDOWS);
                        return;
                    }
                }
                assert!(std::time::Instant::now() < deadline);
                std::thread::sleep(std::time::Duration::from_millis(1));
            }
        };
        wait(0..256);
        for start in (1024..8192).step_by(512) {
            session.request(start..start + 64);
            wait(start..start + 64);
        }
        assert!(session.take().is_none());
        session.cancel();
        worker.join().unwrap().unwrap();
    }

    #[test]
    fn invalid_final_utf8_is_reported_after_partial_publication() {
        let session = DocumentSession::new();
        let bytes = b"hello\n\xf0\x9f";
        let error = futures::executor::block_on(
            session.read_and_prepare(io::Cursor::new(bytes), "invalid.md".into()),
        )
        .unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::InvalidData);
        assert_eq!(
            session
                .take()
                .unwrap()
                .unwrap()
                .buffer
                .as_ref()
                .unwrap()
                .line_text(SourceLine::new(0))
                .as_deref(),
            Some("hello")
        );
    }

    #[test]
    fn streaming_append_past_cached_window_does_not_rescan_prefix() {
        struct ChunkedReader {
            first_done: mpsc::Sender<()>,
            resume: mpsc::Receiver<()>,
            step: usize,
        }
        impl Read for ChunkedReader {
            fn read(&mut self, bytes: &mut [u8]) -> io::Result<usize> {
                self.step += 1;
                let chunk = match self.step {
                    1 => {
                        let mut data = String::from("# Title\n");
                        data.push_str(&"paragraph\n".repeat(300));
                        data
                    }
                    2 => {
                        self.first_done.send(()).unwrap();
                        self.resume.recv().unwrap();
                        "paragraph\n".repeat(300)
                    }
                    _ => return Ok(0),
                };
                let len = chunk.len().min(bytes.len());
                bytes[..len].copy_from_slice(&chunk.as_bytes()[..len]);
                Ok(len)
            }
        }

        let session = DocumentSession::new();
        session.request(0..256);
        let (first_done, first_done_rx) = mpsc::channel();
        let (resume, resume_rx) = mpsc::channel();
        let worker_session = session.clone();
        let worker = std::thread::spawn(move || {
            futures::executor::block_on(worker_session.read_and_prepare(
                ChunkedReader {
                    first_done,
                    resume: resume_rx,
                    step: 0,
                },
                "chunked.md".into(),
            ))
        });
        first_done_rx
            .recv_timeout(std::time::Duration::from_secs(5))
            .unwrap();
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        let initial_rows = loop {
            if let Some(Ok(pub_item)) = session.take()
                && let Some(win) = pub_item.windows.iter().find(|w| {
                    w.range == (0..256) && matches!(w.rows[0].kind, BlockKind::Heading { level: 1 })
                })
            {
                break win.rows.clone();
            }
            assert!(std::time::Instant::now() < deadline);
            std::thread::sleep(std::time::Duration::from_millis(1));
        };
        resume.send(()).unwrap();
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        let complete_rows = loop {
            if let Some(Ok(pub_item)) = session.take()
                && pub_item.complete
                && let Some(win) = pub_item.windows.iter().find(|w| w.range == (0..256))
            {
                break win.rows.clone();
            }
            assert!(std::time::Instant::now() < deadline);
            std::thread::sleep(std::time::Duration::from_millis(1));
        };
        assert!(Arc::ptr_eq(&initial_rows, &complete_rows));
        session.cancel();
        worker.join().unwrap().unwrap();
    }

    #[test]
    fn background_worker_remains_active_for_edits_after_loading() {
        let session = DocumentSession::new();
        let worker_session = session.clone();
        let worker = std::thread::spawn(move || {
            futures::executor::block_on(
                worker_session
                    .read_and_prepare(io::Cursor::new("# Title\nBody"), "live-edit.md".into()),
            )
        });
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        let mut buffer = loop {
            if let Some(Ok(pub_item)) = session.take()
                && pub_item.complete
                && let Some(buffer) = pub_item.buffer
            {
                break buffer;
            }
            assert!(std::time::Instant::now() < deadline);
            std::thread::sleep(std::time::Duration::from_millis(1));
        };
        buffer.record_single_line_edit(SourceLine::new(0), |line, _| *line = "# Edited".into());
        session.submit_target(&buffer);
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        let edited_pub = loop {
            if let Some(Ok(pub_item)) = session.take()
                && pub_item.revision == buffer.revision()
            {
                break pub_item;
            }
            assert!(std::time::Instant::now() < deadline);
            std::thread::sleep(std::time::Duration::from_millis(1));
        };
        assert!(edited_pub.buffer.is_none());
        assert_eq!(edited_pub.windows[0].rows[0].raw_text, "# Edited");
        assert!(matches!(
            edited_pub.windows[0].rows[0].kind,
            BlockKind::Heading { level: 1 }
        ));
        session.cancel();
        worker.join().unwrap().unwrap();
    }
}

pub(crate) fn source_window(
    source: &crate::buffer::SourceSnapshot,
    range: Range<usize>,
) -> PreparedWindow {
    let index = source.structure_index();
    let rows = range
        .clone()
        .map(|line| {
            let raw_text = source.source_line(SourceLine::new(line)).into_owned();
            let kind = match index.fence_range(SourceLine::new(line)) {
                Some((start, _)) if line == start.index() => BlockKind::CodeBlock {
                    is_fence_start: true,
                },
                Some((_, Some(end))) if line == end.index() => BlockKind::CodeBlock {
                    is_fence_start: false,
                },
                Some(_) => BlockKind::CodeBlockContent,
                None => BlockKind::Paragraph,
            };
            let spans = vec![InlineSpan::plain(
                raw_text.clone(),
                (0, raw_text.chars().count()),
            )];
            ParsedLine {
                raw_text,
                kind,
                spans,
                quote: None,
            }
        })
        .collect();
    PreparedWindow {
        range,
        rows: Arc::new(rows),
    }
}

fn prepare_resources(rows: &[ParsedLine], path: &std::path::Path) {
    for row in rows {
        if let BlockKind::Image { url, .. } = &row.kind {
            crate::http::get_image_size(url, Some(path));
        }
    }
}

pub(crate) struct CancelSession(pub Arc<DocumentSession>);
impl Drop for CancelSession {
    fn drop(&mut self) {
        self.0.cancel();
    }
}
