use ignore::{WalkBuilder, WalkState};
use parking_lot::Mutex;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

/// Workspace search only covers the documents this editor opens.
const SEARCHABLE_EXTENSIONS: &[&str] = &["txt", "md", "markdown"];

/// Build-output and dependency directories that are skipped even without a `.gitignore`.
/// Hidden directories (`.git`, `.venv`, ...) are already excluded by the walker.
const PRUNED_DIRECTORIES: &[&str] = &["target", "node_modules", "dist", "build", "vendor"];

const MAX_WALK_THREADS: usize = 4;

/// The type and expansion state of an entry in the file tree.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FileTreeEntryKind {
    File,
    CollocatedDocument,
    Directory { is_expanded: bool },
}

/// A single entry (file or folder) in the flattened visible file tree.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FileTreeEntry {
    pub path: PathBuf,
    pub name: String,
    pub depth: usize,
    pub kind: FileTreeEntryKind,
}

impl FileTreeEntry {
    /// Creates a file tree entry for a regular file.
    pub fn new_file(path: PathBuf, name: String, depth: usize) -> Self {
        Self {
            path,
            name,
            depth,
            kind: FileTreeEntryKind::File,
        }
    }

    /// Creates a file tree entry for a collocated document bundle (`<dir>/index.md`).
    pub fn new_collocated_doc(path: PathBuf, name: String, depth: usize) -> Self {
        Self {
            path,
            name,
            depth,
            kind: FileTreeEntryKind::CollocatedDocument,
        }
    }

    /// Creates a file tree entry for a directory.
    pub fn new_dir(path: PathBuf, name: String, depth: usize, is_expanded: bool) -> Self {
        Self {
            path,
            name,
            depth,
            kind: FileTreeEntryKind::Directory { is_expanded },
        }
    }

    /// Returns `true` if this entry represents an expandable directory.
    #[inline]
    pub fn is_dir(&self) -> bool {
        matches!(self.kind, FileTreeEntryKind::Directory { .. })
    }

    /// Returns `true` if this entry represents a collocated document bundle.
    #[inline]
    pub fn is_collocated_doc(&self) -> bool {
        matches!(self.kind, FileTreeEntryKind::CollocatedDocument)
    }

    /// Returns `true` if this entry is an expanded directory. Files always return `false`.
    #[inline]
    pub fn is_expanded(&self) -> bool {
        matches!(
            self.kind,
            FileTreeEntryKind::Directory { is_expanded: true }
        )
    }

    /// Returns `true` when `active_file` corresponds to this entry.
    pub fn matches_active_file(&self, active_file: Option<&Path>) -> bool {
        let Some(active) = active_file else {
            return false;
        };
        match self.kind {
            FileTreeEntryKind::File => active == self.path.as_path(),
            FileTreeEntryKind::CollocatedDocument => {
                active == self.path.as_path()
                    || super::collocated_bundle_dir(active) == Some(self.path.as_path())
            }
            FileTreeEntryKind::Directory { .. } => false,
        }
    }
}

/// Shared snapshot of a workspace's non-ignored tree entries and searchable document paths.
#[derive(Debug)]
pub struct WorkspaceSnapshot {
    entries: Arc<[FileTreeEntry]>,
    searchable_files: Arc<[PathBuf]>,
}

impl WorkspaceSnapshot {
    /// Flattened tree entries ordered depth-first with directories before sibling files.
    pub fn tree_entries(&self) -> &[FileTreeEntry] {
        &self.entries
    }

    /// Sorted document paths eligible for workspace full-text search.
    pub fn searchable_files(&self) -> &[PathBuf] {
        &self.searchable_files
    }
}

fn is_searchable_extension(path: &Path) -> bool {
    path.extension()
        .and_then(|ext| ext.to_str())
        .is_some_and(|ext| {
            SEARCHABLE_EXTENSIONS
                .iter()
                .any(|allowed| ext.eq_ignore_ascii_case(allowed))
        })
}

fn is_pruned_dir_name(name: &str) -> bool {
    PRUNED_DIRECTORIES.contains(&name)
}

fn workspace_walk_builder(root: &Path) -> WalkBuilder {
    let mut builder = WalkBuilder::new(root);
    builder
        .hidden(true)
        .git_ignore(true)
        .git_global(true)
        .git_exclude(true)
        .parents(false)
        .require_git(false)
        .follow_links(false);
    builder
}

fn compare_tree_entries(a: &FileTreeEntry, b: &FileTreeEntry) -> std::cmp::Ordering {
    match (a.is_dir(), b.is_dir()) {
        (true, false) => std::cmp::Ordering::Less,
        (false, true) => std::cmp::Ordering::Greater,
        _ => a
            .name
            .to_lowercase()
            .cmp(&b.name.to_lowercase())
            .then_with(|| a.name.cmp(&b.name)),
    }
}

/// Shallowly scans `root` and only descends into directories present in `expanded_dirs`,
/// honoring `.gitignore`, hidden filtering, and `PRUNED_DIRECTORIES`.
///
/// Returns `None` if `cancel` is set before completion.
pub fn scan_expanded_tree(
    root: &Path,
    expanded_dirs: &HashSet<PathBuf>,
    collocated_mode: bool,
    cancel: &AtomicBool,
) -> Option<Vec<FileTreeEntry>> {
    if !root.is_dir() || cancel.load(Ordering::Relaxed) {
        return (!cancel.load(Ordering::Relaxed)).then(Vec::new);
    }
    let mut matcher = workspace_walk_builder(root)
        .build_matchers()
        .into_iter()
        .next()?;
    let mut out = Vec::new();
    scan_expanded_dir(
        root,
        root,
        0,
        expanded_dirs,
        collocated_mode,
        &mut matcher,
        &mut out,
        cancel,
    )
    .then_some(out)
}

struct EntryKindInfo {
    is_real_dir: bool,
    raw_is_dir: bool,
}

impl EntryKindInfo {
    fn classify(file_type: Option<&std::fs::FileType>, is_symlink: bool, path: &Path) -> Self {
        let is_real_dir = file_type.is_some_and(|ft| ft.is_dir());
        let raw_is_dir = is_real_dir || (is_symlink && path.is_dir());
        Self {
            is_real_dir,
            raw_is_dir,
        }
    }
}

fn build_tree_entry(
    path: PathBuf,
    name: String,
    depth: usize,
    kind_info: EntryKindInfo,
    collocated_mode: bool,
    is_collocated_bundle: impl FnOnce(&Path) -> bool,
    is_expanded: impl FnOnce(&Path) -> bool,
) -> Option<(FileTreeEntry, bool)> {
    if collocated_mode {
        if !kind_info.raw_is_dir {
            return None;
        }
        if is_collocated_bundle(&path) {
            Some((FileTreeEntry::new_collocated_doc(path, name, depth), false))
        } else {
            let expanded = kind_info.is_real_dir && is_expanded(&path);
            Some((
                FileTreeEntry::new_dir(path, name, depth, expanded),
                expanded,
            ))
        }
    } else if kind_info.raw_is_dir {
        let expanded = kind_info.is_real_dir && is_expanded(&path);
        Some((
            FileTreeEntry::new_dir(path, name, depth, expanded),
            expanded,
        ))
    } else {
        Some((FileTreeEntry::new_file(path, name, depth), false))
    }
}

#[allow(clippy::too_many_arguments)]
fn scan_expanded_dir(
    root: &Path,
    dir: &Path,
    depth: usize,
    expanded_dirs: &HashSet<PathBuf>,
    collocated_mode: bool,
    matcher: &mut ignore::IncrementalIgnore,
    out: &mut Vec<FileTreeEntry>,
    cancel: &AtomicBool,
) -> bool {
    let Ok(read_dir) = std::fs::read_dir(dir) else {
        return true;
    };
    let mut level = Vec::new();
    for entry in read_dir.flatten() {
        if cancel.load(Ordering::Relaxed) {
            return false;
        }
        let path = entry.path();
        let Ok(rel) = path.strip_prefix(root) else {
            continue;
        };
        let name = entry.file_name().to_string_lossy().into_owned();
        let file_type = entry.file_type().ok();
        let is_symlink = file_type.as_ref().is_some_and(|ft| ft.is_symlink());
        let kind_info = EntryKindInfo::classify(file_type.as_ref(), is_symlink, &path);
        if kind_info.raw_is_dir && is_pruned_dir_name(&name) {
            continue;
        }
        if matcher.matched(rel, kind_info.raw_is_dir).is_ignore() {
            continue;
        }

        if let Some(item) = build_tree_entry(
            path,
            name,
            depth,
            kind_info,
            collocated_mode,
            super::has_collocated_index,
            |p| expanded_dirs.contains(p),
        ) {
            level.push(item);
        }
    }

    level.sort_by(|(a, _), (b, _)| compare_tree_entries(a, b));
    for (entry, should_descend) in level {
        if cancel.load(Ordering::Relaxed) {
            return false;
        }
        let path = should_descend.then(|| entry.path.clone());
        out.push(entry);
        if let Some(sub_dir) = path
            && !scan_expanded_dir(
                root,
                &sub_dir,
                depth + 1,
                expanded_dirs,
                collocated_mode,
                matcher,
                out,
                cancel,
            )
        {
            return false;
        }
    }
    true
}

/// Walks `root` once in parallel using `ignore::WalkBuilder`, producing both the full tree listing
/// (sorted depth-first with directories before files) and the sorted searchable document paths.
///
/// Returns `None` when cancelled: a partial walk must never populate the shared cache.
fn walk_workspace(
    root: &Path,
    collocated_mode: bool,
    cancel: &AtomicBool,
) -> Option<WorkspaceSnapshot> {
    if cancel.load(Ordering::Relaxed) {
        return None;
    }

    let threads = std::thread::available_parallelism()
        .map_or(MAX_WALK_THREADS, |n| n.get().min(MAX_WALK_THREADS));
    let root_buf = root.to_path_buf();
    let filter_root = root_buf.clone();
    let mut builder = workspace_walk_builder(root);
    builder.threads(threads).filter_entry(move |entry| {
        if entry.depth() == 0 {
            return true;
        }
        let kind_info = EntryKindInfo::classify(
            entry.file_type().as_ref(),
            entry.path_is_symlink(),
            entry.path(),
        );
        if kind_info.raw_is_dir && entry.file_name().to_str().is_some_and(is_pruned_dir_name) {
            return false;
        }
        if collocated_mode {
            if !kind_info.raw_is_dir {
                return entry.depth() > 1 && super::is_collocated_index_path(entry.path());
            }
            if entry.depth() > 1
                && entry.path().parent().is_some_and(|parent| {
                    parent != filter_root.as_path() && super::has_collocated_index(parent)
                })
            {
                return false;
            }
        }
        true
    });
    let walker = builder.build_parallel();

    let (entry_tx, entry_rx) = std::sync::mpsc::channel();
    let (search_tx, search_rx) = std::sync::mpsc::channel();
    walker.run(|| {
        let entry_tx = entry_tx.clone();
        let search_tx = search_tx.clone();
        let root_for_thread = root_buf.clone();
        Box::new(move |result| {
            if cancel.load(Ordering::Relaxed) {
                return WalkState::Quit;
            }
            match result {
                Ok(entry) => {
                    if entry.depth() == 0 {
                        return WalkState::Continue;
                    }
                    let depth = entry.depth() - 1;
                    let name = entry.file_name().to_string_lossy().into_owned();
                    let ft = entry.file_type();
                    let is_symlink = entry.path_is_symlink();
                    let path = entry.into_path();
                    let kind_info = EntryKindInfo::classify(ft.as_ref(), is_symlink, &path);

                    if collocated_mode {
                        if kind_info.raw_is_dir {
                            let _ = entry_tx.send((path, name, depth, kind_info));
                        } else if ft.is_some_and(|ft| ft.is_file())
                            && super::is_collocated_index_path(&path)
                            && path.parent() != Some(root_for_thread.as_path())
                        {
                            let _ = search_tx.send(path);
                        }
                    } else {
                        if !kind_info.raw_is_dir
                            && ft.is_some_and(|ft| ft.is_file())
                            && is_searchable_extension(&path)
                        {
                            let _ = search_tx.send(path.clone());
                        }
                        let _ = entry_tx.send((path, name, depth, kind_info));
                    }
                }
                Err(e) => eprintln!(
                    "[workspace-index] Skipped entry under {}: {e}",
                    root_for_thread.display()
                ),
            }
            WalkState::Continue
        })
    });
    drop(entry_tx);
    drop(search_tx);

    if cancel.load(Ordering::Relaxed) {
        return None;
    }

    let mut searchable_files: Vec<PathBuf> = search_rx.into_iter().collect();
    searchable_files.sort();

    let bundle_dirs: HashSet<&Path> = if collocated_mode {
        searchable_files.iter().filter_map(|p| p.parent()).collect()
    } else {
        HashSet::new()
    };
    let raw_entries: Vec<FileTreeEntry> = entry_rx
        .into_iter()
        .filter_map(|(path, name, depth, kind_info)| {
            build_tree_entry(
                path,
                name,
                depth,
                kind_info,
                collocated_mode,
                |p| bundle_dirs.contains(p),
                |_| true,
            )
            .map(|(entry, _)| entry)
        })
        .collect();
    let entries = sort_entries_in_tree_order(root, raw_entries);

    Some(WorkspaceSnapshot {
        entries: entries.into(),
        searchable_files: searchable_files.into(),
    })
}

fn sort_entries_in_tree_order(root: &Path, raw: Vec<FileTreeEntry>) -> Vec<FileTreeEntry> {
    if raw.is_empty() {
        return raw;
    }

    let mut children_by_parent: HashMap<&Path, Vec<usize>> = HashMap::new();
    for (idx, entry) in raw.iter().enumerate() {
        let parent = entry.path.parent().unwrap_or(root);
        children_by_parent.entry(parent).or_default().push(idx);
    }

    for children in children_by_parent.values_mut() {
        children.sort_by(|&a, &b| compare_tree_entries(&raw[a], &raw[b]));
    }

    let mut ordered_indices = Vec::with_capacity(raw.len());

    fn push_dfs<'a>(
        dir: &'a Path,
        raw: &'a [FileTreeEntry],
        children_by_parent: &HashMap<&'a Path, Vec<usize>>,
        out: &mut Vec<usize>,
    ) {
        let Some(children) = children_by_parent.get(dir) else {
            return;
        };
        for &idx in children {
            out.push(idx);
            if raw[idx].is_dir() {
                push_dfs(&raw[idx].path, raw, children_by_parent, out);
            }
        }
    }

    push_dfs(root, &raw, &children_by_parent, &mut ordered_indices);

    let mut slots: Vec<Option<FileTreeEntry>> = raw.into_iter().map(Some).collect();
    ordered_indices
        .into_iter()
        .filter_map(|idx| slots[idx].take())
        .collect()
}

/// Shared workspace-wide index serving both `ProjectPanel` filtering and `SearchPanel` discovery
/// from a single `.gitignore`-aware parallel traversal.
#[derive(Clone, Default)]
pub struct WorkspaceFileIndex {
    state: Arc<Mutex<IndexState>>,
}

#[derive(Default)]
struct IndexState {
    generation: u64,
    entry: Option<IndexEntry>,
    expected: Option<(PathBuf, bool)>,
    #[cfg(test)]
    walk_count: u64,
}

struct IndexEntry {
    root: PathBuf,
    collocated_mode: bool,
    snapshot: Arc<WorkspaceSnapshot>,
}

/// Returned on a cache miss; storing the walk result requires the generation to still match.
struct Ticket {
    generation: u64,
}

impl WorkspaceFileIndex {
    /// Returns the cached snapshot if already populated for `(root, collocated_mode)`.
    pub fn get_cached(&self, root: &Path, collocated_mode: bool) -> Option<Arc<WorkspaceSnapshot>> {
        self.lookup(root, collocated_mode).ok()
    }

    /// Returns the cached snapshot for `(root, collocated_mode)`, walking the workspace on a miss.
    ///
    /// Returns `None` only when the walk was cancelled.
    pub fn get_or_scan(
        &self,
        root: &Path,
        collocated_mode: bool,
        cancel: &AtomicBool,
    ) -> Option<Arc<WorkspaceSnapshot>> {
        let ticket = match self.begin_scan(root, collocated_mode) {
            Ok(snapshot) => return Some(snapshot),
            Err(ticket) => ticket,
        };
        let snapshot = Arc::new(walk_workspace(root, collocated_mode, cancel)?);
        #[cfg(test)]
        {
            self.state.lock().walk_count += 1;
        }
        self.store(ticket, root, collocated_mode, snapshot.clone());
        Some(snapshot)
    }

    /// Drops the cached snapshot; the next lookup walks the workspace again.
    pub fn invalidate(&self) {
        let mut state = self.state.lock();
        state.generation += 1;
        state.entry = None;
        state.expected = None;
    }

    /// Invalidates cached data unless it already matches `(root, collocated_mode)`.
    ///
    /// When `entry` is already `None` and `expected` already matches, avoids advancing
    /// `generation` so a sibling panel's concurrent walk for the same target can commit cleanly.
    pub fn invalidate_unless(&self, root: Option<&Path>, collocated_mode: bool) {
        let Some(root) = root else {
            self.invalidate();
            return;
        };
        let mut state = self.state.lock();
        let entry_mismatch = state
            .entry
            .as_ref()
            .is_some_and(|e| e.root != root || e.collocated_mode != collocated_mode);
        let expected_mismatch = state
            .expected
            .as_ref()
            .is_some_and(|(r, m)| r.as_path() != root || *m != collocated_mode);
        if entry_mismatch || expected_mismatch {
            state.generation += 1;
            state.entry = None;
            state.expected = Some((root.to_path_buf(), collocated_mode));
        }
    }

    #[cfg(test)]
    pub(crate) fn is_populated(&self) -> bool {
        self.state.lock().entry.is_some()
    }

    #[cfg(test)]
    pub(crate) fn walk_count(&self) -> u64 {
        self.state.lock().walk_count
    }

    fn begin_scan(
        &self,
        root: &Path,
        collocated_mode: bool,
    ) -> Result<Arc<WorkspaceSnapshot>, Ticket> {
        let mut state = self.state.lock();
        match &state.entry {
            Some(entry) if entry.root == root && entry.collocated_mode == collocated_mode => {
                Ok(entry.snapshot.clone())
            }
            _ => {
                state.expected = Some((root.to_path_buf(), collocated_mode));
                Err(Ticket {
                    generation: state.generation,
                })
            }
        }
    }

    fn lookup(&self, root: &Path, collocated_mode: bool) -> Result<Arc<WorkspaceSnapshot>, Ticket> {
        let state = self.state.lock();
        match &state.entry {
            Some(entry) if entry.root == root && entry.collocated_mode == collocated_mode => {
                Ok(entry.snapshot.clone())
            }
            _ => Err(Ticket {
                generation: state.generation,
            }),
        }
    }

    fn store(
        &self,
        ticket: Ticket,
        root: &Path,
        collocated_mode: bool,
        snapshot: Arc<WorkspaceSnapshot>,
    ) {
        let mut state = self.state.lock();
        if state.generation == ticket.generation {
            state.entry = Some(IndexEntry {
                root: root.to_path_buf(),
                collocated_mode,
                snapshot,
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn not_cancelled() -> AtomicBool {
        AtomicBool::new(false)
    }

    fn discover_searchable(root: &Path) -> Vec<PathBuf> {
        walk_workspace(root, false, &not_cancelled())
            .expect("not cancelled")
            .searchable_files()
            .to_vec()
    }

    #[test]
    fn lists_only_allowed_extensions_outside_hidden_directories() {
        let temp_dir = tempfile::tempdir().unwrap();
        let root = temp_dir.path();
        let txt = root.join("file1.txt");
        let md = root.join("file2.MD");
        fs::write(&txt, "txt").unwrap();
        fs::write(&md, "md").unwrap();
        fs::write(root.join("file3.rs"), "rs").unwrap();
        fs::write(root.join("doc.pdf"), "pdf").unwrap();
        fs::create_dir_all(root.join(".git")).unwrap();
        fs::write(root.join(".git").join("head.txt"), "git").unwrap();

        let mut expected = vec![txt, md];
        expected.sort();
        assert_eq!(discover_searchable(root), expected);
    }

    #[test]
    fn respects_gitignore() {
        let temp_dir = tempfile::tempdir().unwrap();
        let root = temp_dir.path();
        fs::create_dir_all(root.join(".git")).unwrap();
        fs::write(root.join(".gitignore"), "ignored.txt\nsecret/\n").unwrap();
        let normal = root.join("normal.txt");
        fs::write(&normal, "x").unwrap();
        fs::write(root.join("ignored.txt"), "x").unwrap();
        fs::create_dir_all(root.join("secret")).unwrap();
        fs::write(root.join("secret").join("secret.txt"), "x").unwrap();

        assert_eq!(discover_searchable(root), vec![normal]);
    }

    #[test]
    fn prunes_build_directories_without_gitignore() {
        let temp_dir = tempfile::tempdir().unwrap();
        let root = temp_dir.path();
        for dir in ["target", "node_modules", "dist", "build", "vendor", "src"] {
            fs::create_dir_all(root.join(dir)).unwrap();
            fs::write(root.join(dir).join("note.md"), "x").unwrap();
        }

        assert_eq!(
            discover_searchable(root),
            vec![root.join("src").join("note.md")]
        );
    }

    #[test]
    fn cancelled_walk_returns_none() {
        let temp_dir = tempfile::tempdir().unwrap();
        fs::write(temp_dir.path().join("a.txt"), "x").unwrap();

        assert!(walk_workspace(temp_dir.path(), false, &AtomicBool::new(true)).is_none());
    }

    #[test]
    fn collocated_discovery_only_includes_bundle_index_files() {
        let temp_dir = tempfile::tempdir().unwrap();
        let root = temp_dir.path();

        fs::write(root.join("index.md"), "root index").unwrap();
        fs::write(root.join("loose.md"), "loose markdown").unwrap();
        fs::write(root.join("loose.txt"), "loose text").unwrap();

        let top_bundle = root.join("guide");
        fs::create_dir_all(top_bundle.join("assets").join("sub_bundle")).unwrap();
        let top_index = top_bundle.join("index.md");
        fs::write(&top_index, "top bundle").unwrap();
        fs::write(top_bundle.join("notes.md"), "extra md inside bundle").unwrap();
        fs::write(
            top_bundle
                .join("assets")
                .join("sub_bundle")
                .join("index.md"),
            "pruned nested",
        )
        .unwrap();

        let category = root.join("notes");
        let nested_bundle = category.join("chapter");
        fs::create_dir_all(&nested_bundle).unwrap();
        fs::write(category.join("stray.markdown"), "stray").unwrap();
        let nested_index = nested_bundle.join("INDEX.MD");
        fs::write(&nested_index, "nested bundle").unwrap();

        let collocated = walk_workspace(root, true, &not_cancelled()).unwrap();
        assert_eq!(collocated.searchable_files(), &[top_index, nested_index]);

        let tree_names: Vec<&str> = collocated
            .tree_entries()
            .iter()
            .map(|e| e.name.as_str())
            .collect();
        assert_eq!(tree_names, vec!["notes", "chapter", "guide"]);

        assert_eq!(discover_searchable(root).len(), 8);
    }

    #[test]
    fn serves_cached_listing_until_invalidated() {
        let temp_dir = tempfile::tempdir().unwrap();
        let root = temp_dir.path();
        fs::write(root.join("a.txt"), "x").unwrap();
        let index = WorkspaceFileIndex::default();

        assert_eq!(
            index
                .get_or_scan(root, false, &not_cancelled())
                .unwrap()
                .searchable_files()
                .len(),
            1
        );

        fs::write(root.join("b.txt"), "x").unwrap();
        assert_eq!(
            index
                .get_or_scan(root, false, &not_cancelled())
                .unwrap()
                .searchable_files()
                .len(),
            1
        );

        index.invalidate();
        assert_eq!(
            index
                .get_or_scan(root, false, &not_cancelled())
                .unwrap()
                .searchable_files()
                .len(),
            2
        );
    }

    #[test]
    fn walk_started_before_invalidation_is_not_stored() {
        let root = Path::new("/workspace");
        let index = WorkspaceFileIndex::default();

        let ticket = index.lookup(root, false).unwrap_err();
        index.invalidate();
        index.store(
            ticket,
            root,
            false,
            Arc::new(WorkspaceSnapshot {
                entries: Arc::from([]),
                searchable_files: Arc::from(vec![PathBuf::from("/workspace/stale.txt")]),
            }),
        );

        assert!(index.lookup(root, false).is_err());
    }

    #[test]
    fn listing_of_another_root_is_a_miss() {
        let index = WorkspaceFileIndex::default();
        let ticket = index.lookup(Path::new("/old"), false).unwrap_err();
        index.store(
            ticket,
            Path::new("/old"),
            false,
            Arc::new(WorkspaceSnapshot {
                entries: Arc::from([]),
                searchable_files: Arc::from([]),
            }),
        );

        assert!(index.lookup(Path::new("/old"), false).is_ok());
        assert!(index.lookup(Path::new("/new"), false).is_err());
    }

    #[test]
    fn cancelled_walk_leaves_the_cache_empty() {
        let temp_dir = tempfile::tempdir().unwrap();
        let root = temp_dir.path();
        fs::write(root.join("a.txt"), "x").unwrap();
        let index = WorkspaceFileIndex::default();

        assert!(
            index
                .get_or_scan(root, false, &AtomicBool::new(true))
                .is_none()
        );
        assert!(index.lookup(root, false).is_err());
    }

    #[test]
    fn switching_mode_does_not_reuse_other_mode_cache_entry() {
        let temp_dir = tempfile::tempdir().unwrap();
        let root = temp_dir.path();
        fs::write(root.join("loose.md"), "loose").unwrap();
        fs::create_dir_all(root.join("doc")).unwrap();
        fs::write(root.join("doc").join("index.md"), "bundle").unwrap();

        let index = WorkspaceFileIndex::default();
        assert_eq!(
            index
                .get_or_scan(root, false, &not_cancelled())
                .unwrap()
                .searchable_files()
                .len(),
            2
        );
        assert_eq!(
            index
                .get_or_scan(root, true, &not_cancelled())
                .unwrap()
                .searchable_files(),
            &[root.join("doc").join("index.md")]
        );
    }

    #[test]
    fn scan_expanded_tree_respects_gitignore_and_pruned_dirs() {
        let temp_dir = tempfile::tempdir().unwrap();
        let root = temp_dir.path();
        fs::create_dir_all(root.join(".git")).unwrap();
        fs::write(root.join(".gitignore"), "ignored.md\nignored_dir/\n").unwrap();
        fs::create_dir_all(root.join("sub/nested_ignored")).unwrap();
        fs::write(root.join("sub/.gitignore"), "nested_ignored/\n*.tmp\n").unwrap();

        fs::create_dir_all(root.join("collapsed")).unwrap();
        fs::write(root.join("collapsed/hidden_child.md"), "x").unwrap();
        fs::create_dir_all(root.join("ignored_dir")).unwrap();
        fs::write(root.join("ignored_dir/a.md"), "x").unwrap();
        for pruned in ["target", "node_modules", "dist", "build", "vendor"] {
            fs::create_dir_all(root.join(pruned)).unwrap();
        }
        fs::write(root.join(".hidden.md"), "x").unwrap();
        fs::write(root.join("ignored.md"), "x").unwrap();
        fs::write(root.join("visible.md"), "x").unwrap();
        fs::write(root.join("sub/keep.md"), "x").unwrap();
        fs::write(root.join("sub/skip.tmp"), "x").unwrap();
        fs::write(root.join("sub/nested_ignored/nope.md"), "x").unwrap();

        let mut expanded = HashSet::new();
        expanded.insert(root.join("sub"));
        let entries = scan_expanded_tree(root, &expanded, false, &not_cancelled()).unwrap();
        let names: Vec<(&str, usize)> =
            entries.iter().map(|e| (e.name.as_str(), e.depth)).collect();
        assert_eq!(
            names,
            vec![
                ("collapsed", 0),
                ("sub", 0),
                ("keep.md", 1),
                ("visible.md", 0),
            ]
        );
        assert!(!entries[0].is_expanded());
        assert!(entries[1].is_expanded());
    }

    #[test]
    fn walk_workspace_tree_order_places_dirs_before_files_in_nested_subtrees() {
        let temp_dir = tempfile::tempdir().unwrap();
        let root = temp_dir.path();
        fs::create_dir_all(root.join("a_dir/z_sub")).unwrap();
        fs::write(root.join("a_dir/a_file.md"), "x").unwrap();
        fs::write(root.join("a_dir/z_sub/leaf.png"), "x").unwrap();
        fs::write(root.join("b_file.txt"), "x").unwrap();

        let snap = walk_workspace(root, false, &not_cancelled()).unwrap();
        let tree: Vec<(&str, usize, bool)> = snap
            .tree_entries()
            .iter()
            .map(|e| (e.name.as_str(), e.depth, e.is_dir()))
            .collect();
        assert_eq!(
            tree,
            vec![
                ("a_dir", 0, true),
                ("z_sub", 1, true),
                ("leaf.png", 2, false),
                ("a_file.md", 1, false),
                ("b_file.txt", 0, false),
            ]
        );
    }

    #[test]
    fn walk_workspace_does_not_follow_directory_symlinks() {
        let temp_dir = tempfile::tempdir().unwrap();
        let root = temp_dir.path();
        let target = root.join("real_dir");
        fs::create_dir(&target).unwrap();
        fs::write(target.join("needle.rs"), "").unwrap();
        let link = root.join("linked_dir");

        #[cfg(unix)]
        let link_result = std::os::unix::fs::symlink(&target, &link);
        #[cfg(windows)]
        let link_result = std::os::windows::fs::symlink_dir(&target, &link);
        #[cfg(not(any(unix, windows)))]
        return;

        if let Err(error) = link_result {
            if cfg!(windows) && error.kind() == std::io::ErrorKind::PermissionDenied {
                return;
            }
            panic!("failed to create directory symlink for test: {error}");
        }

        let snap = walk_workspace(root, false, &not_cancelled()).unwrap();
        assert_eq!(
            snap.tree_entries()
                .iter()
                .filter(|entry| entry.name == "needle.rs")
                .count(),
            1
        );
    }

    #[test]
    fn workspace_file_index_serves_both_tree_and_search_from_single_walk() {
        let temp_dir = tempfile::tempdir().unwrap();
        let root = temp_dir.path();
        fs::create_dir_all(root.join("notes")).unwrap();
        fs::write(root.join("notes/readme.md"), "hello").unwrap();
        fs::write(root.join("notes/cover.png"), "png").unwrap();

        let index = WorkspaceFileIndex::default();
        assert_eq!(index.walk_count(), 0);

        let snap1 = index.get_or_scan(root, false, &not_cancelled()).unwrap();
        assert_eq!(index.walk_count(), 1);
        assert_eq!(snap1.searchable_files(), &[root.join("notes/readme.md")]);

        let snap2 = index.get_cached(root, false).unwrap();
        assert!(Arc::ptr_eq(&snap1, &snap2));
        assert_eq!(snap2.tree_entries().len(), 3);
        assert_eq!(index.walk_count(), 1);
    }

    #[test]
    fn collocated_walk_respects_gitignore_on_index_file() {
        let temp_dir = tempfile::tempdir().unwrap();
        let root = temp_dir.path();
        fs::create_dir_all(root.join(".git")).unwrap();
        fs::write(root.join(".gitignore"), "ignored_bundle/index.md\n").unwrap();

        let visible_bundle = root.join("visible_bundle");
        fs::create_dir_all(&visible_bundle).unwrap();
        fs::write(visible_bundle.join("index.md"), "visible").unwrap();

        let ignored_bundle = root.join("ignored_bundle");
        fs::create_dir_all(&ignored_bundle).unwrap();
        fs::write(ignored_bundle.join("index.md"), "ignored").unwrap();

        let snap = walk_workspace(root, true, &not_cancelled()).unwrap();
        assert_eq!(snap.searchable_files(), &[visible_bundle.join("index.md")]);

        let visible_entry = snap
            .tree_entries()
            .iter()
            .find(|e| e.path == visible_bundle)
            .unwrap();
        assert!(visible_entry.is_collocated_doc());

        let ignored_entry = snap
            .tree_entries()
            .iter()
            .find(|e| e.path == ignored_bundle)
            .unwrap();
        assert!(ignored_entry.is_dir());
        assert!(!ignored_entry.is_collocated_doc());
    }

    #[test]
    fn invalidate_unless_blocks_stale_commit_when_cache_is_empty() {
        let root_a = Path::new("/workspace_a");
        let root_b = Path::new("/workspace_b");
        let index = WorkspaceFileIndex::default();

        // Matching invalidate_unless on empty cache must not block the in-flight scan.
        let ticket_a = index.begin_scan(root_a, false).unwrap_err();
        index.invalidate_unless(Some(root_a), false);
        index.store(
            ticket_a,
            root_a,
            false,
            Arc::new(WorkspaceSnapshot {
                entries: Arc::from([]),
                searchable_files: Arc::from(vec![root_a.join("ok.md")]),
            }),
        );
        assert!(index.get_cached(root_a, false).is_some());

        // Clear cache and start a new scan for root_a; switching to root_b while entry is None
        // must bump generation and block the stale root_a commit.
        index.invalidate();
        let stale_ticket = index.begin_scan(root_a, false).unwrap_err();
        index.invalidate_unless(Some(root_b), false);
        index.store(
            stale_ticket,
            root_a,
            false,
            Arc::new(WorkspaceSnapshot {
                entries: Arc::from([]),
                searchable_files: Arc::from(vec![root_a.join("stale.md")]),
            }),
        );
        assert!(index.get_cached(root_a, false).is_none());
        assert!(!index.is_populated());
    }
}
