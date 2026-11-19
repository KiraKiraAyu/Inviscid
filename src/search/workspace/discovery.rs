use ignore::{WalkBuilder, WalkState};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};

/// Workspace search only covers the documents this editor opens.
const ALLOWED_EXTENSIONS: &[&str] = &["txt", "md", "markdown"];

/// Build-output and dependency directories that are skipped even without a `.gitignore`.
/// Hidden directories (`.git`, `.venv`, ...) are already excluded by the walker.
const PRUNED_DIRECTORIES: &[&str] = &["target", "node_modules", "dist", "build", "vendor"];

const MAX_WALK_THREADS: usize = 4;

fn is_allowed_extension(path: &Path) -> bool {
    path.extension()
        .and_then(|ext| ext.to_str())
        .is_some_and(|ext| {
            ALLOWED_EXTENSIONS
                .iter()
                .any(|allowed| ext.eq_ignore_ascii_case(allowed))
        })
}

fn is_pruned_directory(entry: &ignore::DirEntry) -> bool {
    entry.depth() > 0
        && entry.file_type().is_some_and(|ft| ft.is_dir())
        && entry
            .file_name()
            .to_str()
            .is_some_and(|name| PRUNED_DIRECTORIES.contains(&name))
}

/// Lists the searchable files under `root`, sorted, honoring `.gitignore` rules.
///
/// In collocated mode, only `index.md` bundle entry files (not loose files, not `<root>/index.md`,
/// and not nested subdirectories inside collocated bundles) are included.
///
/// Returns `None` when cancelled: a partial listing must never be mistaken for the full one.
pub(super) fn discover_files(
    root: &Path,
    collocated_mode: bool,
    cancel: &AtomicBool,
) -> Option<Vec<PathBuf>> {
    let threads = std::thread::available_parallelism()
        .map_or(MAX_WALK_THREADS, |n| n.get().min(MAX_WALK_THREADS));
    let root_buf = root.to_path_buf();
    let filter_root = root_buf.clone();
    let walker = WalkBuilder::new(root)
        .hidden(true)
        .git_ignore(true)
        .git_global(true)
        .git_exclude(true)
        .parents(false)
        .require_git(false)
        .threads(threads)
        .filter_entry(move |entry| {
            if is_pruned_directory(entry) {
                return false;
            }
            if collocated_mode
                && entry.depth() > 1
                && entry.file_type().is_some_and(|ft| ft.is_dir())
                && entry.path().parent().is_some_and(|parent| {
                    parent != filter_root.as_path() && crate::fs::has_collocated_index(parent)
                })
            {
                return false;
            }
            true
        })
        .build_parallel();

    let (tx, rx) = std::sync::mpsc::channel();
    walker.run(|| {
        let tx = tx.clone();
        let root_for_thread = root_buf.clone();
        Box::new(move |result| {
            if cancel.load(Ordering::Relaxed) {
                return WalkState::Quit;
            }
            match result {
                Ok(entry) => {
                    if entry.file_type().is_some_and(|ft| ft.is_file())
                        && is_allowed_extension(entry.path())
                        && (!collocated_mode
                            || crate::fs::collocated_bundle_dir_within(
                                entry.path(),
                                Some(root_for_thread.as_path()),
                            )
                            .is_some())
                    {
                        // The receiver outlives the walk, so the send cannot fail.
                        let _ = tx.send(entry.into_path());
                    }
                }
                Err(e) => eprintln!(
                    "[search] Skipped entry under {}: {e}",
                    root_for_thread.display()
                ),
            }
            WalkState::Continue
        })
    });
    drop(tx);

    if cancel.load(Ordering::Relaxed) {
        return None;
    }
    let mut files: Vec<PathBuf> = rx.into_iter().collect();
    files.sort();
    Some(files)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn discover(root: &Path) -> Vec<PathBuf> {
        discover_files(root, false, &AtomicBool::new(false)).expect("not cancelled")
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
        assert_eq!(discover(root), expected);
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

        assert_eq!(discover(root), vec![normal]);
    }

    #[test]
    fn prunes_build_directories_without_gitignore() {
        let temp_dir = tempfile::tempdir().unwrap();
        let root = temp_dir.path();
        for dir in ["target", "node_modules", "src"] {
            fs::create_dir_all(root.join(dir)).unwrap();
            fs::write(root.join(dir).join("note.md"), "x").unwrap();
        }

        assert_eq!(discover(root), vec![root.join("src").join("note.md")]);
    }

    #[test]
    fn cancelled_walk_returns_none() {
        let temp_dir = tempfile::tempdir().unwrap();
        fs::write(temp_dir.path().join("a.txt"), "x").unwrap();

        assert_eq!(
            discover_files(temp_dir.path(), false, &AtomicBool::new(true)),
            None
        );
    }

    #[test]
    fn collocated_discovery_only_includes_bundle_index_files() {
        let temp_dir = tempfile::tempdir().unwrap();
        let root = temp_dir.path();

        // Loose files and a root-level index.md must be excluded in collocated mode
        fs::write(root.join("index.md"), "root index").unwrap();
        fs::write(root.join("loose.md"), "loose markdown").unwrap();
        fs::write(root.join("loose.txt"), "loose text").unwrap();

        // Top-level collocated bundle with co-located assets / nested md
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

        // Category folder with nested collocated bundle and loose file
        let category = root.join("notes");
        let nested_bundle = category.join("chapter");
        fs::create_dir_all(&nested_bundle).unwrap();
        fs::write(category.join("stray.markdown"), "stray").unwrap();
        let nested_index = nested_bundle.join("INDEX.MD");
        fs::write(&nested_index, "nested bundle").unwrap();

        let collocated_files = discover_files(root, true, &AtomicBool::new(false)).unwrap();
        assert_eq!(collocated_files, vec![top_index, nested_index]);

        // Standard mode still sees everything including .markdown
        let standard_files = discover(root);
        assert_eq!(standard_files.len(), 8);
    }
}
