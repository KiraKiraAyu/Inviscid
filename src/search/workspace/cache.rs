use parking_lot::Mutex;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::AtomicBool;

use super::discovery::discover_files;

/// Shared listing of the searchable files under a workspace root.
///
/// It is the only owner of the listing's validity. The entry is keyed by root, so a listing of a
/// previous workspace is never served for the current one. Every invalidation starts a new
/// generation, so a walk that was already running when the files changed cannot store its stale
/// result afterwards. A cancelled walk stores nothing.
#[derive(Clone, Default)]
pub struct WorkspaceFileCache {
    state: Arc<Mutex<CacheState>>,
}

#[derive(Default)]
struct CacheState {
    generation: u64,
    entry: Option<CacheEntry>,
}

struct CacheEntry {
    root: PathBuf,
    collocated_mode: bool,
    files: Arc<[PathBuf]>,
}

/// Returned on a cache miss; storing the walk result requires it to still be current.
struct Ticket {
    generation: u64,
}

impl WorkspaceFileCache {
    /// Returns the cached listing for `(root, collocated_mode)`, walking the tree on a miss.
    ///
    /// Returns `None` only when the walk was cancelled.
    pub fn get_or_discover(
        &self,
        root: &Path,
        collocated_mode: bool,
        cancel: &AtomicBool,
    ) -> Option<Arc<[PathBuf]>> {
        let ticket = match self.lookup(root, collocated_mode) {
            Ok(files) => return Some(files),
            Err(ticket) => ticket,
        };
        let files: Arc<[PathBuf]> = discover_files(root, collocated_mode, cancel)?.into();
        self.store(ticket, root, collocated_mode, files.clone());
        Some(files)
    }

    /// Drops the listing; the next lookup walks the tree again.
    pub fn invalidate(&self) {
        let mut state = self.state.lock();
        state.generation += 1;
        state.entry = None;
    }

    fn lookup(&self, root: &Path, collocated_mode: bool) -> Result<Arc<[PathBuf]>, Ticket> {
        let state = self.state.lock();
        match &state.entry {
            Some(entry) if entry.root == root && entry.collocated_mode == collocated_mode => {
                Ok(entry.files.clone())
            }
            _ => Err(Ticket {
                generation: state.generation,
            }),
        }
    }

    fn store(&self, ticket: Ticket, root: &Path, collocated_mode: bool, files: Arc<[PathBuf]>) {
        let mut state = self.state.lock();
        if state.generation == ticket.generation {
            state.entry = Some(CacheEntry {
                root: root.to_path_buf(),
                collocated_mode,
                files,
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

    #[test]
    fn serves_cached_listing_until_invalidated() {
        let temp_dir = tempfile::tempdir().unwrap();
        let root = temp_dir.path();
        fs::write(root.join("a.txt"), "x").unwrap();
        let cache = WorkspaceFileCache::default();

        assert_eq!(
            cache
                .get_or_discover(root, false, &not_cancelled())
                .unwrap()
                .len(),
            1
        );

        fs::write(root.join("b.txt"), "x").unwrap();
        assert_eq!(
            cache
                .get_or_discover(root, false, &not_cancelled())
                .unwrap()
                .len(),
            1
        );

        cache.invalidate();
        assert_eq!(
            cache
                .get_or_discover(root, false, &not_cancelled())
                .unwrap()
                .len(),
            2
        );
    }

    #[test]
    fn walk_started_before_invalidation_is_not_stored() {
        let root = Path::new("/workspace");
        let cache = WorkspaceFileCache::default();

        let ticket = cache.lookup(root, false).unwrap_err();
        cache.invalidate();
        cache.store(
            ticket,
            root,
            false,
            Arc::from(vec![PathBuf::from("/workspace/stale.txt")]),
        );

        assert!(cache.lookup(root, false).is_err());
    }

    #[test]
    fn listing_of_another_root_is_a_miss() {
        let cache = WorkspaceFileCache::default();
        let ticket = cache.lookup(Path::new("/old"), false).unwrap_err();
        cache.store(ticket, Path::new("/old"), false, Arc::from(Vec::new()));

        assert!(cache.lookup(Path::new("/old"), false).is_ok());
        assert!(cache.lookup(Path::new("/new"), false).is_err());
    }

    #[test]
    fn cancelled_walk_leaves_the_cache_empty() {
        let temp_dir = tempfile::tempdir().unwrap();
        let root = temp_dir.path();
        fs::write(root.join("a.txt"), "x").unwrap();
        let cache = WorkspaceFileCache::default();

        assert!(
            cache
                .get_or_discover(root, false, &AtomicBool::new(true))
                .is_none()
        );
        assert!(cache.lookup(root, false).is_err());
    }

    #[test]
    fn switching_mode_does_not_reuse_other_mode_cache_entry() {
        let temp_dir = tempfile::tempdir().unwrap();
        let root = temp_dir.path();
        fs::write(root.join("loose.md"), "loose").unwrap();
        fs::create_dir_all(root.join("doc")).unwrap();
        fs::write(root.join("doc").join("index.md"), "bundle").unwrap();

        let cache = WorkspaceFileCache::default();
        assert_eq!(
            cache
                .get_or_discover(root, false, &not_cancelled())
                .unwrap()
                .len(),
            2
        );
        assert_eq!(
            cache
                .get_or_discover(root, true, &not_cancelled())
                .unwrap()
                .as_ref(),
            &[root.join("doc").join("index.md")]
        );
    }
}
