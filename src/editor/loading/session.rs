use crate::buffer::{LineSource, SourceLine, StreamingBuffer, TextBuffer};
use crate::markdown::{BlockKind, InlineSpan, MarkdownScanner, ParsedLine};
use crate::sync::{Mutex, VersionedNotifier};
use futures::StreamExt;
use std::{
    collections::VecDeque,
    io::{self, Read},
    ops::Range,
    path::PathBuf,
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
    pub buffer: TextBuffer,
    pub windows: VecDeque<PreparedWindow>,
    pub complete: bool,
    pub disk_hash: u64,
}

/// One replaceable publication and one replaceable viewport request per document.
#[derive(Debug)]
pub(crate) struct DocumentSession {
    latest: Mutex<Option<io::Result<Publication>>>,
    requested: Mutex<Range<usize>>,
    requests: VersionedNotifier,
    pub updates: VersionedNotifier,
    cancelled: AtomicBool,
}

impl std::fmt::Debug for Publication {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Publication")
            .field("revision", &self.buffer.revision())
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

impl DocumentSession {
    pub fn new() -> Arc<Self> {
        Arc::new(Self {
            latest: Mutex::new(None),
            requested: Mutex::new(0..WINDOW_ROWS * 2),
            requests: VersionedNotifier::new(),
            updates: VersionedNotifier::new(),
            cancelled: AtomicBool::new(false),
        })
    }

    pub fn cancel(&self) {
        self.cancelled.store(true, Ordering::Release);
        self.requests.notify();
    }

    pub fn request(&self, range: Range<usize>) {
        let start = range.start / WINDOW_ROWS * WINDOW_ROWS;
        let end = range.end.div_ceil(WINDOW_ROWS) * WINDOW_ROWS;
        let mut requested = self.requested.lock();
        if *requested != (start..end) {
            *requested = start..end;
            drop(requested);
            self.requests.notify();
        }
    }

    pub fn take(&self) -> Option<io::Result<Publication>> {
        self.latest.lock().take()
    }

    fn publish(&self, publication: io::Result<Publication>) {
        if !self.cancelled.load(Ordering::Acquire) {
            *self.latest.lock() = Some(publication);
            self.updates.notify();
        }
    }

    pub async fn run(self: Arc<Self>, path: PathBuf) {
        if !crate::editor::is_whitelisted_extension(&path) {
            if let Err(error) = self.read_and_prepare(io::empty(), path).await {
                self.publish(Err(error));
            }
            return;
        }
        let result = match std::fs::File::open(&path) {
            Ok(file) => self.read_and_prepare(file, path).await,
            Err(error) => Err(error),
        };
        if let Err(error) = result {
            self.publish(Err(error));
        }
        self.updates.close();
    }

    async fn read_and_prepare(&self, mut reader: impl Read, path: PathBuf) -> io::Result<()> {
        let mut stream = StreamingBuffer::new(Some(path.clone()));
        let mut bytes = vec![0; READ_BYTES];
        let mut windows: VecDeque<PreparedWindow> = VecDeque::new();
        let mut first = true;
        let mut requests = self.requests.subscribe();
        let resources = futures::stream::select(
            crate::http::subscribe_image_updates(),
            crate::syntax::subscribe_syntax_updates(),
        );
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
            let range = self.clamped_request(&stream.buffer);
            windows.retain(|window| {
                window.range.end < previous_tail
                    && window.range.start <= range.start
                    && window.range.end >= range.end
            });
            if windows.is_empty() {
                windows.push_back(source_window(
                    &stream.buffer.source_snapshot(),
                    range.clone(),
                ));
            }
            self.publish(Ok(Publication {
                buffer: stream.buffer.clone(),
                windows: windows.clone(),
                complete: false,
                disk_hash: stream.disk_hash(),
            }));
            // Rich preparation belongs to this session, never to UI cache misses.
            let (range, rows) = MarkdownScanner::scan_window(&stream.buffer, range);
            prepare_resources(&rows, &path);
            windows.clear();
            windows.push_back(PreparedWindow {
                range,
                rows: Arc::new(rows),
            });
            if complete {
                stream.buffer.set_last_saved_mtime(
                    std::fs::metadata(&path)
                        .ok()
                        .and_then(|m| m.modified().ok()),
                );
            }
            self.publish(Ok(Publication {
                buffer: stream.buffer.clone(),
                windows: windows.clone(),
                complete,
                disk_hash: stream.disk_hash(),
            }));
            first = false;
            if complete {
                break;
            }
        }
        let mut wakeups = futures::stream::select(&mut requests, resources);
        while wakeups.next().await.is_some() {
            if self.cancelled.load(Ordering::Acquire) {
                break;
            }
            let range = self.clamped_request(&stream.buffer);
            let (range, rows) = MarkdownScanner::scan_window(&stream.buffer, range);
            prepare_resources(&rows, &path);
            windows.retain(|window| window.range != range);
            windows.push_back(PreparedWindow {
                range,
                rows: Arc::new(rows),
            });
            while windows.len() > MAX_WINDOWS {
                windows.pop_front();
            }
            self.publish(Ok(Publication {
                buffer: stream.buffer.clone(),
                windows: windows.clone(),
                complete: true,
                disk_hash: stream.disk_hash(),
            }));
        }
        Ok(())
    }

    fn clamped_request(&self, buffer: &TextBuffer) -> Range<usize> {
        let range = self.requested.lock().clone();
        range.start.min(buffer.line_count())..range.end.min(buffer.line_count())
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
                        buffer: stream.buffer.clone(),
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
                        buffer: stream.buffer.clone(),
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
                    buffer: stream.buffer.clone(),
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
                    buffer: stream.buffer.clone(),
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
                    buffer: stream.buffer.clone(),
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
        assert_eq!(publication.buffer.line_count(), 3);
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
                buffer: stream.buffer.clone(),
                windows: VecDeque::new(),
                complete: false,
                disk_hash: stream.disk_hash(),
            }));
        }
        assert_eq!(session.take().unwrap().unwrap().buffer.line_count(), 101);
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
                .line_text(SourceLine::new(0))
                .as_deref(),
            Some("hello")
        );
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
