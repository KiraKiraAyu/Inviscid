use crate::markdown::ParsedLine;
use std::{
    cell::RefCell,
    collections::VecDeque,
    ops::{Deref, Range},
    sync::Arc,
};

/// Parsed source rows shared by immutable layout snapshots.
#[derive(Clone, Debug, Default)]
pub struct ParsedLineStore {
    eager: Arc<Vec<ParsedLine>>,
    windows: Option<Arc<WindowStore>>,
}

const WINDOW_ROWS: usize = 256;
const MAX_WINDOWS: usize = 8;

#[derive(Debug)]
struct WindowStore {
    source: crate::buffer::SourceSnapshot,
    index: crate::buffer::StructureIndex,
    windows: RefCell<VecDeque<ParsedWindow>>,
}

#[derive(Clone, Debug)]
struct ParsedWindow {
    range: Range<usize>,
    rows: Arc<Vec<ParsedLine>>,
}

/// Owns its window so eviction cannot invalidate an in-flight row reader.
pub struct ParsedRow {
    rows: Arc<Vec<ParsedLine>>,
    index: usize,
}

pub struct ParsedSlice {
    rows: Arc<Vec<ParsedLine>>,
    range: Range<usize>,
}

impl Deref for ParsedSlice {
    type Target = [ParsedLine];
    fn deref(&self) -> &Self::Target {
        &self.rows[self.range.clone()]
    }
}

impl Deref for ParsedRow {
    type Target = ParsedLine;
    fn deref(&self) -> &Self::Target {
        &self.rows[self.index]
    }
}

impl PartialEq for ParsedLineStore {
    fn eq(&self, other: &Self) -> bool {
        self.len() == other.len()
            && (0..self.len()).all(|row| *self.row(row).unwrap() == *other.row(row).unwrap())
    }
}

impl ParsedLineStore {
    pub(crate) fn windowed(buffer: &crate::buffer::TextBuffer) -> Self {
        use crate::buffer::LineSource;
        Self {
            eager: Arc::default(),
            windows: Some(Arc::new(WindowStore {
                source: buffer.source_snapshot(),
                index: buffer.line_index().structure_snapshot(),
                windows: RefCell::default(),
            })),
        }
    }

    pub fn row(&self, line: usize) -> Option<ParsedRow> {
        if line >= self.len() {
            return None;
        }
        let Some(store) = &self.windows else {
            return Some(ParsedRow {
                rows: self.eager.clone(),
                index: line,
            });
        };
        let start = line / WINDOW_ROWS * WINDOW_ROWS;
        let window = self.load_window(store, start..(start + WINDOW_ROWS).min(self.len()));
        Some(ParsedRow {
            rows: window.rows,
            index: line - window.range.start,
        })
    }

    pub fn read_range(&self, range: Range<usize>) -> ParsedSlice {
        assert!(range.start <= range.end && range.end <= self.len());
        let Some(store) = &self.windows else {
            return ParsedSlice {
                rows: self.eager.clone(),
                range,
            };
        };
        let window = self.load_window(store, range.clone());
        ParsedSlice {
            rows: window.rows,
            range: range.start - window.range.start..range.end - window.range.start,
        }
    }

    fn load_window(&self, store: &WindowStore, requested: Range<usize>) -> ParsedWindow {
        let mut windows = store.windows.borrow_mut();
        let window = if let Some(position) = windows.iter().position(|window| {
            window.range.start <= requested.start && window.range.end >= requested.end
        }) {
            windows.remove(position).unwrap()
        } else {
            let (range, rows) = crate::markdown::MarkdownScanner::scan_snapshot_window(
                &store.source,
                &store.index,
                requested,
            );
            ParsedWindow {
                range,
                rows: Arc::new(rows),
            }
        };
        if windows.len() == MAX_WINDOWS {
            windows.pop_front();
        }
        windows.push_back(window.clone());
        window
    }

    pub fn resident_rows(&self) -> usize {
        self.windows.as_ref().map_or(self.eager.len(), |store| {
            store
                .windows
                .borrow()
                .iter()
                .map(|window| window.rows.len())
                .sum()
        })
    }

    pub fn get(&self, line: usize) -> Option<&ParsedLine> {
        self.eager.get(line)
    }

    pub fn range(&self, range: Range<usize>) -> &[ParsedLine] {
        &self.eager[range]
    }

    pub fn len(&self) -> usize {
        use crate::buffer::LineSource;
        self.windows
            .as_ref()
            .map_or(self.eager.len(), |store| store.source.line_count())
    }

    pub(crate) fn make_mut(&mut self) -> &mut Vec<ParsedLine> {
        assert!(self.windows.is_none());
        Arc::make_mut(&mut self.eager)
    }
}

impl From<Arc<Vec<ParsedLine>>> for ParsedLineStore {
    fn from(lines: Arc<Vec<ParsedLine>>) -> Self {
        Self {
            eager: lines,
            windows: None,
        }
    }
}

// Transitional slice access while consumers migrate to range queries.
impl Deref for ParsedLineStore {
    type Target = Arc<Vec<ParsedLine>>;

    fn deref(&self) -> &Self::Target {
        assert!(
            self.windows.is_none(),
            "windowed rows require explicit row/range access"
        );
        &self.eager
    }
}

#[cfg(test)]
mod tests {
    use super::ParsedLineStore;
    use crate::markdown::MarkdownScanner;
    use std::sync::Arc;

    #[test]
    fn lazy_windows_are_bounded_and_snapshot_rows_survive_eviction() {
        let mut buffer = crate::buffer::TextBuffer::from_str(&"paragraph\n".repeat(20_000), None);
        let store = ParsedLineStore::windowed(&buffer);
        assert_eq!(store.resident_rows(), 0);
        let retained = store.row(0).unwrap();
        for row in (0..buffer.line_count()).step_by(127) {
            assert_eq!(store.row(row).unwrap().raw_text, "paragraph");
            assert!(store.resident_rows() <= 2048);
        }
        buffer.record_single_line_edit(crate::buffer::SourceLine::new(0), |line, _| {
            *line = "changed".into()
        });
        let edited = ParsedLineStore::windowed(&buffer);
        assert_eq!(edited.row(0).unwrap().raw_text, "changed");
        assert_eq!(retained.raw_text, "paragraph");
        assert_eq!(store.row(0).unwrap().raw_text, "paragraph");
    }

    #[test]
    fn ranges_and_edits_preserve_retained_snapshots() {
        let original = MarkdownScanner::scan_document(&["one".into(), "two".into()]);
        let mut store = ParsedLineStore::from(Arc::new(original.clone()));
        let snapshot = store.clone();
        assert_eq!(store.len(), 2);
        assert_eq!(store.get(1), original.get(1));
        assert_eq!(store.range(0..1), &original[..1]);
        store.make_mut().remove(0);
        assert_eq!(snapshot.as_slice(), original);
        assert_eq!(store.get(0), original.get(1));
    }
}
