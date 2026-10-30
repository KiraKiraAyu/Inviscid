use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

use super::results::{FileSearchResult, WorkspaceMatch};
use crate::search::SearchQuery;

/// Files with a NUL byte in this prefix are treated as binary and skipped.
const BINARY_SNIFF_LEN: usize = 4096;
const READ_BUFFER_CAPACITY: usize = 64 * 1024;

#[derive(Clone, Copy, Debug)]
pub struct ScanLimits {
    pub max_total_matches: usize,
    pub max_matches_per_file: usize,
    pub max_file_size: u64,
}

impl Default for ScanLimits {
    fn default() -> Self {
        Self {
            max_total_matches: 5000,
            max_matches_per_file: 500,
            max_file_size: 10 * 1024 * 1024,
        }
    }
}

/// One scan over a fixed file list, shared by any number of workers.
///
/// Workers pull files from a shared cursor, so a few large files cannot leave the other workers
/// idle. The match budget is reserved atomically, so the total never exceeds the limit no matter
/// how many workers race for it. The type is executor-agnostic: the caller decides where the
/// workers run.
pub struct WorkspaceScan<'a> {
    files: &'a [PathBuf],
    root: &'a Path,
    query: &'a SearchQuery,
    cancel: &'a AtomicBool,
    limits: ScanLimits,
    next_file: AtomicUsize,
    total_matches: AtomicUsize,
    is_capped: AtomicBool,
    is_stopped: AtomicBool,
}

impl<'a> WorkspaceScan<'a> {
    pub fn new(
        files: &'a [PathBuf],
        root: &'a Path,
        query: &'a SearchQuery,
        cancel: &'a AtomicBool,
        limits: ScanLimits,
    ) -> Self {
        Self {
            files,
            root,
            query,
            cancel,
            limits,
            next_file: AtomicUsize::new(0),
            total_matches: AtomicUsize::new(0),
            is_capped: AtomicBool::new(false),
            is_stopped: AtomicBool::new(false),
        }
    }

    /// Runs one worker until the files run out, the scan is cancelled, the match budget is
    /// spent, or `on_match` returns `false` (which stops every worker).
    pub fn run_worker(&self, mut on_match: impl FnMut(FileSearchResult) -> bool) {
        let mut buffer = Vec::with_capacity(READ_BUFFER_CAPACITY);
        while !self.should_stop() {
            let idx = self.next_file.fetch_add(1, Ordering::Relaxed);
            let Some(path) = self.files.get(idx) else {
                return;
            };
            let Some(content) = read_text_file(path, self.limits.max_file_size, &mut buffer) else {
                continue;
            };

            let remaining = self
                .limits
                .max_total_matches
                .saturating_sub(self.total_matches.load(Ordering::Relaxed));
            let limit = remaining.min(self.limits.max_matches_per_file);
            let (mut matches, is_truncated) = find_line_matches(content, self.query, limit);
            if is_truncated {
                self.is_capped.store(true, Ordering::Relaxed);
            }
            if matches.is_empty() {
                continue;
            }

            matches.truncate(self.reserve(matches.len()));
            if matches.is_empty() {
                continue;
            }
            let result = FileSearchResult {
                path: path.clone(),
                relative_path: relative_path(path, self.root),
                matches,
            };
            if !on_match(result) {
                self.is_stopped.store(true, Ordering::Relaxed);
            }
        }
    }

    /// Whether some matches were left out because a limit was hit.
    pub fn is_capped(&self) -> bool {
        self.is_capped.load(Ordering::Relaxed)
    }

    fn should_stop(&self) -> bool {
        self.cancel.load(Ordering::Relaxed) || self.is_stopped.load(Ordering::Relaxed)
    }

    /// Claims up to `wanted` matches from the budget and returns how many were granted.
    fn reserve(&self, wanted: usize) -> usize {
        let max = self.limits.max_total_matches;
        let previous = self
            .total_matches
            .try_update(Ordering::Relaxed, Ordering::Relaxed, |total| {
                Some(total + wanted.min(max.saturating_sub(total)))
            })
            .unwrap_or_else(|total| total);
        let granted = wanted.min(max.saturating_sub(previous));
        if previous + granted >= max {
            self.is_capped.store(true, Ordering::Relaxed);
            self.is_stopped.store(true, Ordering::Relaxed);
        }
        granted
    }
}

/// Reads `path` as UTF-8 text into `buffer`.
///
/// Oversized, binary, and non-UTF-8 files are not searchable documents and are skipped quietly;
/// I/O failures are reported.
fn read_text_file<'b>(path: &Path, max_size: u64, buffer: &'b mut Vec<u8>) -> Option<&'b str> {
    let report = |e: std::io::Error| eprintln!("[search] Failed to read {}: {e}", path.display());
    let mut file = std::fs::File::open(path).map_err(report).ok()?;
    let size = file.metadata().map_err(report).ok()?.len();
    if size > max_size {
        return None;
    }
    buffer.clear();
    file.read_to_end(buffer).map_err(report).ok()?;

    let sniff_len = buffer.len().min(BINARY_SNIFF_LEN);
    if memchr::memchr(0, &buffer[..sniff_len]).is_some() {
        return None;
    }
    std::str::from_utf8(buffer).ok()
}

/// Collects up to `limit` non-empty matches, line by line. The flag reports whether more
/// matches were left out.
fn find_line_matches(
    content: &str,
    query: &SearchQuery,
    limit: usize,
) -> (Vec<WorkspaceMatch>, bool) {
    let regex = query.regex();
    let mut matches = Vec::new();
    if !regex.is_match(content) {
        return (matches, false);
    }

    for (line_idx, line) in content.lines().enumerate() {
        let mut shared_line: Option<Arc<str>> = None;
        for m in regex.find_iter(line).filter(|m| !m.is_empty()) {
            if matches.len() == limit {
                return (matches, true);
            }
            let char_start = line[..m.start()].chars().count();
            let char_end = char_start + m.as_str().chars().count();
            let line_content = shared_line.get_or_insert_with(|| Arc::from(line)).clone();
            matches.push(WorkspaceMatch {
                line_number: line_idx + 1,
                line_content,
                byte_range: (m.start(), m.end()),
                char_range: (char_start, char_end),
            });
        }
    }
    (matches, false)
}

fn relative_path(path: &Path, root: &Path) -> String {
    path.strip_prefix(root)
        .unwrap_or(path)
        .to_string_lossy()
        .replace('\\', "/")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::search::SearchOptions;
    use std::fs;

    fn literal(text: &str) -> SearchQuery {
        SearchQuery::new(text, SearchOptions::default()).unwrap()
    }

    /// Runs `workers` workers on real threads and returns what they streamed.
    fn run_scan(
        files: &[PathBuf],
        root: &Path,
        query: &SearchQuery,
        limits: ScanLimits,
        workers: usize,
    ) -> (Vec<FileSearchResult>, bool) {
        let cancel = AtomicBool::new(false);
        let scan = WorkspaceScan::new(files, root, query, &cancel, limits);
        let streamed = parking_lot::Mutex::new(Vec::new());
        std::thread::scope(|s| {
            for _ in 0..workers {
                s.spawn(|| {
                    scan.run_worker(|result| {
                        streamed.lock().push(result);
                        true
                    })
                });
            }
        });
        let mut streamed = streamed.into_inner();
        streamed.sort_by(|a, b| a.relative_path.cmp(&b.relative_path));
        (streamed, scan.is_capped())
    }

    fn write_files(root: &Path, files: &[(&str, &str)]) -> Vec<PathBuf> {
        files
            .iter()
            .map(|(name, content)| {
                let path = root.join(name);
                fs::create_dir_all(path.parent().unwrap()).unwrap();
                fs::write(&path, content).unwrap();
                path
            })
            .collect()
    }

    fn match_count(results: &[FileSearchResult]) -> usize {
        results.iter().map(|f| f.matches.len()).sum()
    }

    #[test]
    fn reports_line_numbers_char_ranges_and_relative_paths() {
        let temp_dir = tempfile::tempdir().unwrap();
        let root = temp_dir.path();
        let files = write_files(
            root,
            &[
                ("doc1.txt", "hello world\nsecond line\nanother hello\n"),
                ("sub/doc2.txt", "foo bar\r\nhéllo hello there\r\n"),
            ],
        );

        let (results, is_capped) =
            run_scan(&files, root, &literal("hello"), ScanLimits::default(), 2);

        assert!(!is_capped);
        assert_eq!(results.len(), 2);
        let f1 = &results[0];
        assert_eq!(f1.relative_path, "doc1.txt");
        assert_eq!(f1.matches[0].line_number, 1);
        assert_eq!(f1.matches[0].char_range, (0, 5));
        assert_eq!(f1.matches[1].line_number, 3);
        assert_eq!(f1.matches[1].char_range, (8, 13));
        let f2 = &results[1];
        assert_eq!(f2.relative_path, "sub/doc2.txt");
        assert_eq!(f2.matches.len(), 1);
        assert_eq!(f2.matches[0].line_number, 2);
        // "héllo " is 6 chars but 7 bytes.
        assert_eq!(f2.matches[0].char_range, (6, 11));
        assert_eq!(f2.matches[0].byte_range, (7, 12));
        assert_eq!(&*f2.matches[0].line_content, "héllo hello there");
    }

    #[test]
    fn skips_binary_non_utf8_and_oversized_files() {
        let temp_dir = tempfile::tempdir().unwrap();
        let root = temp_dir.path();
        let mut files = write_files(
            root,
            &[("text.txt", "needle"), ("big.txt", "needle needle needle")],
        );
        let binary = root.join("binary.txt");
        fs::write(&binary, b"needle\0rest").unwrap();
        let latin1 = root.join("latin1.txt");
        fs::write(&latin1, b"needle \xE9").unwrap();
        files.extend([binary, latin1]);
        // Above every skipped file except `big.txt`, so each one is rejected for its own reason.
        let limits = ScanLimits {
            max_file_size: 16,
            ..ScanLimits::default()
        };

        let (results, _) = run_scan(&files, root, &literal("needle"), limits, 1);

        assert_eq!(results.len(), 1);
        assert_eq!(results[0].relative_path, "text.txt");
    }

    #[test]
    fn total_limit_is_exact_across_workers() {
        let temp_dir = tempfile::tempdir().unwrap();
        let root = temp_dir.path();
        let names: Vec<String> = (0..8).map(|i| format!("f{i}.txt")).collect();
        let specs: Vec<(&str, &str)> = names.iter().map(|n| (n.as_str(), "m\nm\nm\n")).collect();
        let files = write_files(root, &specs);
        let limits = ScanLimits {
            max_total_matches: 10,
            ..ScanLimits::default()
        };

        let (results, is_capped) = run_scan(&files, root, &literal("m"), limits, 4);

        assert_eq!(match_count(&results), 10);
        assert!(is_capped);
    }

    #[test]
    fn per_file_limit_truncates_without_stopping_the_scan() {
        let temp_dir = tempfile::tempdir().unwrap();
        let root = temp_dir.path();
        let files = write_files(root, &[("a.txt", &"m\n".repeat(10)), ("b.txt", "m\n")]);
        let limits = ScanLimits {
            max_matches_per_file: 3,
            ..ScanLimits::default()
        };

        let (results, is_capped) = run_scan(&files, root, &literal("m"), limits, 1);

        assert_eq!(results.len(), 2);
        assert_eq!(results[0].matches.len(), 3);
        assert_eq!(results[1].matches.len(), 1);
        assert!(is_capped);
    }

    #[test]
    fn regex_query_matches_per_line() {
        let temp_dir = tempfile::tempdir().unwrap();
        let root = temp_dir.path();
        let files = write_files(
            root,
            &[(
                "code.md",
                "fn test_foo() {}\nfn test_bar() {}\nfn ignore_me() {}\n",
            )],
        );
        let options = SearchOptions {
            use_regex: true,
            ..SearchOptions::default()
        };
        let query = SearchQuery::new(r"^fn (test_\w+)", options).unwrap();

        let (results, _) = run_scan(&files, root, &query, ScanLimits::default(), 1);

        let lines: Vec<_> = results[0].matches.iter().map(|m| m.line_number).collect();
        assert_eq!(lines, [1, 2]);
    }

    #[test]
    fn cancelled_scan_reports_nothing() {
        let temp_dir = tempfile::tempdir().unwrap();
        let root = temp_dir.path();
        let files = write_files(root, &[("a.txt", "needle")]);
        let query = literal("needle");
        let cancel = AtomicBool::new(true);
        let scan = WorkspaceScan::new(&files, root, &query, &cancel, ScanLimits::default());

        let mut streamed = 0;
        scan.run_worker(|_| {
            streamed += 1;
            true
        });

        assert_eq!(streamed, 0);
    }

    #[test]
    fn consumer_can_stop_every_worker() {
        let temp_dir = tempfile::tempdir().unwrap();
        let root = temp_dir.path();
        let files = write_files(root, &[("a.txt", "m"), ("b.txt", "m"), ("c.txt", "m")]);
        let query = literal("m");
        let cancel = AtomicBool::new(false);
        let scan = WorkspaceScan::new(&files, root, &query, &cancel, ScanLimits::default());

        let mut streamed = 0;
        scan.run_worker(|_| {
            streamed += 1;
            false
        });
        scan.run_worker(|_| {
            streamed += 1;
            true
        });

        assert_eq!(streamed, 1);
    }
}
