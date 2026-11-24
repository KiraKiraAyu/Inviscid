use super::window_cache::{EditWindowPatch, MAX_WINDOWS, WINDOW_ROWS, WindowEntry, WindowQuality};
use crate::markdown::{BlockKind, ParsedLine, ScanOptions};
use std::{
    cell::RefCell,
    collections::VecDeque,
    ops::{Deref, Range},
    path::PathBuf,
    sync::Arc,
};

pub(in crate::editor) mod resources;
use resources::Resource;

/// Parsed source rows shared by immutable layout snapshots.
#[derive(Clone, Debug, Default)]
pub struct ParsedLineStore {
    #[cfg(test)]
    eager: Arc<Vec<ParsedLine>>,
    windows: Option<Arc<WindowStore>>,
    resources: Arc<RefCell<Vec<Resource>>>,
}

#[derive(Clone, Debug)]
struct WindowStore {
    session: Option<Arc<super::loading::DocumentSession>>,
    streaming_optimistic: bool,
    doc_path: Option<PathBuf>,
    is_dark_theme: bool,
    source: crate::buffer::SourceSnapshot,
    index: crate::buffer::StructureIndex,
    windows: RefCell<VecDeque<WindowEntry>>,
    scanned: std::cell::Cell<usize>,
}

fn window_contains_mermaid(rows: &[ParsedLine]) -> bool {
    rows.iter().any(|row| {
        matches!(
            row.kind,
            BlockKind::MermaidBlock { .. } | BlockKind::MermaidBlockContent
        ) || (matches!(
            row.kind,
            BlockKind::CodeBlock {
                is_fence_start: true
            }
        ) && crate::markdown::is_mermaid_fence(crate::markdown::strip_quote_prefix(
            &row.raw_text,
        )))
    })
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
                session: None,
                streaming_optimistic: false,
                doc_path: buffer.file_path_buf(),
                is_dark_theme: crate::theme::DEFAULT_THEME.is_dark(),
                source: buffer.source_snapshot(),
                index: buffer.line_index().structure_snapshot(),
                windows: RefCell::default(),
                scanned: std::cell::Cell::new(0),
            })),
            resources: Arc::default(),
        }
    }

    pub(crate) fn with_theme(mut self, is_dark_theme: bool) -> Self {
        if let Some(store) = &mut self.windows {
            Arc::make_mut(store).is_dark_theme = is_dark_theme;
        }
        self
    }

    pub(crate) fn with_session(
        mut self,
        session: Option<Arc<super::loading::DocumentSession>>,
    ) -> Self {
        if let Some(store) = &mut self.windows {
            Arc::make_mut(store).session = session;
        }
        self
    }

    pub(crate) fn update_theme(&self, is_dark_theme: bool) -> Self {
        let Some(prev_store) = &self.windows else {
            return self.clone();
        };
        if prev_store.is_dark_theme == is_dark_theme {
            return self.clone();
        }
        let mut windows = VecDeque::with_capacity(prev_store.windows.borrow().len());
        let mut invalidated_ranges = Vec::new();
        for window in prev_store.windows.borrow().iter() {
            if window_contains_mermaid(&window.rows) {
                invalidated_ranges.push(window.range.clone());
            } else {
                windows.push_back(window.clone());
            }
        }
        let resources: Vec<Resource> = self
            .resources
            .borrow()
            .iter()
            .filter(|res| {
                !invalidated_ranges
                    .iter()
                    .any(|inv| res.range.start < inv.end && inv.start < res.range.end)
            })
            .cloned()
            .collect();
        Self {
            #[cfg(test)]
            eager: Arc::default(),
            windows: Some(Arc::new(WindowStore {
                session: prev_store.session.clone(),
                streaming_optimistic: prev_store.streaming_optimistic,
                doc_path: prev_store.doc_path.clone(),
                is_dark_theme,
                source: prev_store.source.clone(),
                index: prev_store.index.clone(),
                windows: RefCell::new(windows),
                scanned: std::cell::Cell::new(0),
            })),
            resources: Arc::new(RefCell::new(resources)),
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

    pub(crate) fn with_prepared(
        mut self,
        prepared: Option<Arc<super::loading::PreparedSnapshot>>,
    ) -> Self {
        if let Some(prepared) = prepared
            && let Some(store) = &mut self.windows
        {
            let store = Arc::make_mut(store);
            store.session = Some(prepared.session.clone());
            store.streaming_optimistic = prepared.optimistic;
            *store.windows.borrow_mut() = prepared.windows.clone();
            let mut resources = self.resources.borrow_mut();
            for window in &prepared.windows {
                if window.quality == WindowQuality::SourceStub {
                    continue;
                }
                for resource in Resource::collect(
                    window.range.start,
                    &window.rows,
                    store.doc_path.as_deref(),
                    Some((&store.source, &store.index)),
                ) {
                    if !resources.iter().any(|old| old.range == resource.range) {
                        resources.push(resource);
                    }
                }
            }
        }
        self
    }

    pub(crate) fn snapshot_windows(&self) -> VecDeque<WindowEntry> {
        let Some(store) = &self.windows else {
            return VecDeque::new();
        };
        store.windows.borrow().clone()
    }

    pub(crate) fn apply_edit(
        &self,
        buffer: &crate::buffer::TextBuffer,
        patch: &EditWindowPatch,
        is_dark_theme: bool,
    ) -> Self {
        let Some(prev_store) = &self.windows else {
            return Self::windowed(buffer).with_theme(is_dark_theme);
        };
        if prev_store.is_dark_theme != is_dark_theme {
            return self
                .update_theme(is_dark_theme)
                .apply_edit(buffer, patch, is_dark_theme);
        }
        let mut windows = prev_store.windows.borrow().clone();
        use crate::buffer::LineSource;
        let source = buffer.source_snapshot();
        let index = buffer.line_index().structure_snapshot();
        let (scanned, rescanned_ranges) =
            patch.apply_edit(&mut windows, &source, &index, is_dark_theme);

        let mut resources: Vec<Resource> = if patch.structural {
            let cut = patch.local_new_range.start;
            self.resources
                .borrow()
                .iter()
                .filter(|res| res.range.end <= cut)
                .cloned()
                .collect()
        } else {
            let line_delta = patch.line_delta();
            let dirty_old = patch.dirty_old_range();
            self.resources
                .borrow()
                .iter()
                .filter_map(|res| {
                    if res.range.end <= dirty_old.start {
                        Some(res.clone())
                    } else if res.range.start >= dirty_old.end {
                        let mut shifted = res.clone();
                        shifted.range = super::window_cache::shift_range(&res.range, line_delta);
                        Some(shifted)
                    } else {
                        None
                    }
                })
                .collect()
        };
        for range in rescanned_ranges {
            if let Some(window) = windows
                .iter()
                .find(|win| win.range.start <= range.start && range.end <= win.range.end)
            {
                let start = range.start - window.range.start;
                let end = range.end - window.range.start;
                for resource in Resource::collect(
                    range.start,
                    &window.rows[start..end],
                    prev_store.doc_path.as_deref(),
                    Some((&source, &index)),
                ) {
                    if !resources.iter().any(|old| old.range == resource.range) {
                        resources.push(resource);
                    }
                }
            }
        }

        Self {
            #[cfg(test)]
            eager: Arc::default(),
            windows: Some(Arc::new(WindowStore {
                session: prev_store.session.clone(),
                streaming_optimistic: false,
                doc_path: buffer.file_path_buf(),
                is_dark_theme,
                source,
                index,
                windows: RefCell::new(windows),
                scanned: std::cell::Cell::new(scanned),
            })),
            resources: Arc::new(RefCell::new(resources)),
        }
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

    fn load_window(&self, store: &WindowStore, requested: Range<usize>) -> WindowEntry {
        let mut windows = store.windows.borrow_mut();
        let window = if let Some(position) = windows.iter().position(|window| {
            window.range.start <= requested.start && window.range.end >= requested.end
        }) {
            let window = windows.remove(position).unwrap();
            if window.quality != WindowQuality::Full
                && let Some(session) = &store.session
            {
                session.request(requested);
            }
            window
        } else if store.streaming_optimistic {
            if let Some(session) = &store.session {
                session.request(requested.clone());
            }
            let index = store.index.optimistic();
            super::loading::source_window(&store.source, &index, requested)
        } else {
            let (range, rows, has_deferred) =
                crate::markdown::MarkdownScanner::scan_snapshot_window_with_options(
                    &store.source,
                    &store.index,
                    requested.clone(),
                    ScanOptions::ui_deferred(store.is_dark_theme),
                );
            let quality = if has_deferred {
                if let Some(session) = &store.session {
                    session.request(requested);
                }
                WindowQuality::UiDeferred
            } else {
                WindowQuality::Full
            };
            store.scanned.set(store.scanned.get() + rows.len());
            let mut resources = self.resources.borrow_mut();
            for resource in Resource::collect(
                range.start,
                &rows,
                store.doc_path.as_deref(),
                Some((&store.source, &store.index)),
            ) {
                if !resources.iter().any(|old| old.range == resource.range) {
                    resources.push(resource);
                }
            }
            WindowEntry {
                range,
                rows: Arc::new(rows),
                quality,
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
            .is_some_and(|store| store.streaming_optimistic)
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
                let (range, rows, has_deferred) =
                    crate::markdown::MarkdownScanner::scan_snapshot_window_with_options(
                        &store.source,
                        &store.index,
                        range,
                        ScanOptions::ui_deferred(store.is_dark_theme),
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
                    if has_deferred {
                        window.quality = window.quality.min(WindowQuality::UiDeferred);
                    }
                }
            }
            self.windows = Some(Arc::new(WindowStore {
                session: store.session.clone(),
                streaming_optimistic: store.streaming_optimistic,
                doc_path: store.doc_path.clone(),
                is_dark_theme: store.is_dark_theme,
                source: store.source.clone(),
                index: store.index.clone(),
                windows: RefCell::new(windows),
                scanned: std::cell::Cell::new(scanned),
            }));
        }
        ranges
    }

    pub(crate) fn image_line_indices(&self) -> Vec<usize> {
        self.resources
            .borrow()
            .iter()
            .filter(|res| !res.is_syntax())
            .map(|res| res.range.start)
            .collect()
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

    /// Reads an already-resident parsed row without triggering window scans.
    pub(crate) fn cached_row_if_loaded(&self, line: usize) -> Option<ParsedRow> {
        let store = self.windows.as_ref()?;
        let guard = store.windows.borrow();
        let window = guard.iter().find(|w| w.range.contains(&line))?;
        Some(ParsedRow {
            rows: window.rows.clone(),
            index: line - window.range.start,
        })
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
        let resources = Arc::new(RefCell::new(Resource::collect(0, &lines, None, None)));
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
    use crate::editor::window_cache::{WINDOW_ROWS, WindowEntry, WindowQuality};
    use crate::markdown::{BlockKind, MarkdownScanner};
    use std::collections::VecDeque;
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

    #[test]
    fn relative_local_image_size_key_refreshes_matching_row() {
        let dir = tempfile::tempdir().expect("tempdir");
        let doc_path = dir.path().join("notes.md");
        let buffer = crate::buffer::TextBuffer::from_str(
            "paragraph\n![diagram](./assets/pic.png)\nend",
            Some(doc_path.clone()),
        );
        let mut store = ParsedLineStore::windowed(&buffer);
        let _ = store.read_range(0..buffer.line_count());
        assert!(store.refresh_resources(true, false).is_empty());

        let resolved_key = crate::http::image_size_key("./assets/pic.png", Some(&doc_path));
        assert_ne!(resolved_key, "./assets/pic.png");
        crate::http::record_image_size(&resolved_key, 640, 320);

        assert_eq!(store.refresh_resources(true, false), vec![1..2]);
        assert!(store.refresh_resources(true, false).is_empty());
    }

    #[test]
    fn post_load_scroll_to_uncached_window_uses_ui_deferred_instead_of_source_stub() {
        let mut text = String::new();
        for i in 0..400 {
            if i == 300 {
                text.push_str("# Deferred Heading\n");
            } else {
                text.push_str("paragraph\n");
            }
        }
        let buffer = crate::buffer::TextBuffer::from_str(&text, None);
        let session = crate::editor::loading::DocumentSession::new();
        let (range, rows) = MarkdownScanner::scan_window(&buffer, 0..WINDOW_ROWS);
        let snapshot = Arc::new(crate::editor::loading::PreparedSnapshot {
            revision: buffer.revision(),
            session,
            windows: VecDeque::from([WindowEntry {
                range,
                rows: Arc::new(rows),
                quality: WindowQuality::Full,
            }]),
            optimistic: false,
            is_dark_theme: crate::theme::DEFAULT_THEME.is_dark(),
        });

        let store = ParsedLineStore::windowed(&buffer).with_prepared(Some(snapshot));
        let row300 = store.row(300).unwrap();
        assert_eq!(row300.kind, BlockKind::Heading { level: 1 });
    }
}
