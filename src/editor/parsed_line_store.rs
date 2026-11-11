use crate::markdown::ParsedLine;
use std::{
    cell::RefCell,
    collections::VecDeque,
    ops::{Deref, Range},
    sync::Arc,
};

mod resources;
use resources::Resource;

/// Parsed source rows shared by immutable layout snapshots.
#[derive(Clone, Debug, Default)]
pub struct ParsedLineStore {
    #[cfg(test)]
    eager: Arc<Vec<ParsedLine>>,
    windows: Option<Arc<WindowStore>>,
    resources: Arc<RefCell<Vec<Resource>>>,
}

const WINDOW_ROWS: usize = 256;
const MAX_WINDOWS: usize = 8;

#[derive(Clone, Debug)]
struct WindowStore {
    prepared: Option<Arc<super::loading::PreparedSnapshot>>,
    source: crate::buffer::SourceSnapshot,
    index: crate::buffer::StructureIndex,
    windows: RefCell<VecDeque<ParsedWindow>>,
    scanned: std::cell::Cell<usize>,
    preedit: Option<(usize, Range<usize>)>,
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

#[cfg(test)]
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
            #[cfg(test)]
            eager: Arc::default(),
            windows: Some(Arc::new(WindowStore {
                prepared: None,
                source: buffer.source_snapshot(),
                index: buffer.line_index().structure_snapshot(),
                windows: RefCell::default(),
                scanned: std::cell::Cell::new(0),
                preedit: None,
            })),
            resources: Arc::default(),
        }
    }

    pub fn row(&self, line: usize) -> Option<ParsedRow> {
        if line >= self.len() {
            return None;
        }
        let Some(store) = &self.windows else {
            #[cfg(test)]
            return Some(ParsedRow {
                rows: self.eager.clone(),
                index: line,
            });
            #[cfg(not(test))]
            return None;
        };
        let start = line / WINDOW_ROWS * WINDOW_ROWS;
        let window = self.load_window(store, start..(start + WINDOW_ROWS).min(self.len()));
        Some(ParsedRow {
            rows: window.rows,
            index: line - window.range.start,
        })
    }

    pub(crate) fn with_preedit(mut self, preedit: Option<(usize, Range<usize>)>) -> Self {
        if let Some(store) = &mut self.windows {
            Arc::make_mut(store).preedit = preedit;
        }
        self
    }

    pub(crate) fn with_prepared(
        mut self,
        prepared: Option<Arc<super::loading::PreparedSnapshot>>,
    ) -> Self {
        if let Some(store) = &mut self.windows {
            Arc::make_mut(store).prepared = prepared;
        }
        self
    }

    pub fn read_range(&self, range: Range<usize>) -> ParsedSlice {
        assert!(range.start <= range.end && range.end <= self.len());
        let Some(store) = &self.windows else {
            return ParsedSlice {
                #[cfg(test)]
                rows: self.eager.clone(),
                #[cfg(not(test))]
                rows: Arc::default(),
                range,
            };
        };
        let start = range.start / WINDOW_ROWS * WINDOW_ROWS;
        let end = range.end.div_ceil(WINDOW_ROWS) * WINDOW_ROWS;
        let window = self.load_window(store, start..end.min(self.len()));
        ParsedSlice {
            rows: window.rows,
            range: range.start - window.range.start..range.end - window.range.start,
        }
    }

    fn load_window(&self, store: &WindowStore, requested: Range<usize>) -> ParsedWindow {
        if let Some(prepared) = &store.prepared {
            if let Some(window) = prepared.windows.iter().rev().find(|window| {
                window.range.start <= requested.start && window.range.end >= requested.end
            }) {
                return ParsedWindow {
                    range: window.range.clone(),
                    rows: window.rows.clone(),
                };
            }
            prepared.session.request(requested.clone());
            let mut windows = store.windows.borrow_mut();
            if let Some(window) = windows.iter().find(|window| window.range == requested) {
                return window.clone();
            }
            let window = super::loading::source_window(&store.source, requested);
            let window = ParsedWindow {
                range: window.range,
                rows: window.rows,
            };
            if windows.len() == MAX_WINDOWS {
                windows.pop_front();
            }
            windows.push_back(window.clone());
            return window;
        }
        let mut windows = store.windows.borrow_mut();
        let window = if let Some(position) = windows.iter().position(|window| {
            window.range.start <= requested.start && window.range.end >= requested.end
        }) {
            windows.remove(position).unwrap()
        } else {
            let (range, mut rows) = crate::markdown::MarkdownScanner::scan_snapshot_window(
                &store.source,
                &store.index,
                requested,
            );
            if let Some((line, columns)) = &store.preedit
                && range.contains(line)
            {
                let row = &mut rows[line - range.start];
                row.spans =
                    crate::markdown::mark_ime_preedit(&row.spans, (columns.start, columns.end));
                if let crate::markdown::BlockKind::Table { cells, .. } = &mut row.kind {
                    for cell in cells {
                        cell.spans = crate::markdown::mark_ime_preedit(
                            &cell.spans,
                            (columns.start, columns.end),
                        );
                    }
                }
            }
            store.scanned.set(store.scanned.get() + rows.len());
            let mut resources = self.resources.borrow_mut();
            for resource in Resource::collect(range.start, &rows) {
                if !resources.iter().any(|old| old.range == resource.range) {
                    resources.push(resource);
                }
            }
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
        self.windows.as_ref().map_or(self.len(), |store| {
            store
                .windows
                .borrow()
                .iter()
                .map(|window| window.rows.len())
                .sum()
        })
    }

    pub(crate) fn refresh_resources(&mut self, images: bool, syntax: bool) -> Vec<Range<usize>> {
        if self
            .windows
            .as_ref()
            .is_some_and(|store| store.prepared.is_some())
        {
            return Vec::new();
        }
        if !images && !syntax {
            return Vec::new();
        }
        let mut resources = self.resources.borrow().clone();
        let mut syntax_ranges = Vec::new();
        let ranges: Vec<_> = resources
            .iter_mut()
            .filter_map(|resource| {
                if !resource.changed(images, syntax) {
                    return None;
                }
                if resource.is_syntax() {
                    syntax_ranges.push(resource.range.clone());
                }
                Some(resource.range.clone())
            })
            .collect();
        if ranges.is_empty() {
            return ranges;
        }
        self.resources = Arc::new(RefCell::new(resources));
        if let Some(store) = &self.windows {
            let mut windows = store.windows.borrow().clone();
            let mut scanned = store.scanned.get();
            for range in syntax_ranges {
                if !windows
                    .iter()
                    .any(|window| range.start < window.range.end && window.range.start < range.end)
                {
                    continue;
                }
                let (range, rows) = crate::markdown::MarkdownScanner::scan_snapshot_window(
                    &store.source,
                    &store.index,
                    range,
                );
                scanned += rows.len();
                for window in windows.iter_mut() {
                    let start = range.start.max(window.range.start);
                    let end = range.end.min(window.range.end);
                    if start >= end {
                        continue;
                    }
                    Arc::make_mut(&mut window.rows)
                        [start - window.range.start..end - window.range.start]
                        .clone_from_slice(&rows[start - range.start..end - range.start]);
                }
            }
            self.windows = Some(Arc::new(WindowStore {
                prepared: store.prepared.clone(),
                source: store.source.clone(),
                index: store.index.clone(),
                windows: RefCell::new(windows),
                scanned: std::cell::Cell::new(scanned),
                preedit: store.preedit.clone(),
            }));
        }
        ranges
    }

    pub(crate) fn take_scanned(&self) -> usize {
        self.windows
            .as_ref()
            .map_or(0, |store| store.scanned.replace(0))
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
    pub fn is_windowed(&self) -> bool {
        self.windows.is_some()
    }

    pub fn shares_storage_with(&self, other: &Self) -> bool {
        match (&self.windows, &other.windows) {
            (Some(left), Some(right)) => Arc::ptr_eq(left, right),
            #[cfg(test)]
            (None, None) => Arc::ptr_eq(&self.eager, &other.eager),
            _ => false,
        }
    }

    pub fn get(&self, line: usize) -> Option<ParsedRow> {
        self.row(line)
    }

    pub fn range(&self, range: Range<usize>) -> ParsedSlice {
        self.read_range(range)
    }

    pub fn len(&self) -> usize {
        use crate::buffer::LineSource;
        self.windows.as_ref().map_or_else(
            || {
                #[cfg(test)]
                {
                    self.eager.len()
                }
                #[cfg(not(test))]
                {
                    0
                }
            },
            |store| store.source.line_count(),
        )
    }

    #[cfg(test)]
    pub(crate) fn make_mut(&mut self) -> &mut Vec<ParsedLine> {
        assert!(self.windows.is_none());
        Arc::make_mut(&mut self.eager)
    }
}

#[cfg(test)]
impl From<Arc<Vec<ParsedLine>>> for ParsedLineStore {
    fn from(lines: Arc<Vec<ParsedLine>>) -> Self {
        let resources = Arc::new(RefCell::new(Resource::collect(0, &lines)));
        Self {
            eager: lines,
            windows: None,
            resources,
        }
    }
}

#[cfg(test)]
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
    fn resource_changes_only_invalidate_matching_rows_and_keep_old_snapshots() {
        let url = "https://example.invalid/phase-g-resource.png";
        let buffer = crate::buffer::TextBuffer::from_str(
            &format!("paragraph\n![test]({url})\n```unknown_phase_g\ncode\n```\nend"),
            None,
        );
        let mut store = ParsedLineStore::windowed(&buffer);
        let mut old = store.clone();
        store.read_range(0..buffer.line_count());
        assert!(store.refresh_resources(false, true).is_empty());
        crate::http::record_image_size(url, 731, 349);
        assert_eq!(store.refresh_resources(true, false), vec![1..2]);
        assert!(!store.shares_storage_with(&old));
        assert!(store.refresh_resources(true, false).is_empty());
        assert_eq!(old.refresh_resources(true, false), vec![1..2]);
    }

    #[test]
    fn lazy_windows_are_bounded_and_snapshot_rows_survive_eviction() {
        let mut buffer = crate::buffer::TextBuffer::from_str(&"paragraph\n".repeat(20_000), None);
        let store = ParsedLineStore::windowed(&buffer);
        assert_eq!(store.resident_rows(), 0);
        assert!(store.get(100).is_some());
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
        assert_eq!(store.get(1).as_deref(), original.get(1));
        assert_eq!(&*store.range(0..1), &original[..1]);
        store.make_mut().remove(0);
        assert_eq!(snapshot.as_slice(), original);
        assert_eq!(store.get(0).as_deref(), original.get(1));
    }
}
