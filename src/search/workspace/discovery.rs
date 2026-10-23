use ignore::{WalkBuilder, WalkState};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};

/// Workspace search only covers the documents this editor opens.
const ALLOWED_EXTENSIONS: &[&str] = &["txt", "md"];

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
/// Returns `None` when cancelled: a partial listing must never be mistaken for the full one.
pub(super) fn discover_files(root: &Path, cancel: &AtomicBool) -> Option<Vec<PathBuf>> {
    let threads = std::thread::available_parallelism()
        .map_or(MAX_WALK_THREADS, |n| n.get().min(MAX_WALK_THREADS));
    let walker = WalkBuilder::new(root)
        .hidden(true)
        .git_ignore(true)
        .git_global(true)
        .git_exclude(true)
        .parents(false)
        .require_git(false)
        .threads(threads)
        .filter_entry(|entry| !is_pruned_directory(entry))
        .build_parallel();

    let (tx, rx) = std::sync::mpsc::channel();
    walker.run(|| {
        let tx = tx.clone();
        Box::new(move |result| {
            if cancel.load(Ordering::Relaxed) {
                return WalkState::Quit;
            }
            match result {
                Ok(entry) => {
                    if entry.file_type().is_some_and(|ft| ft.is_file())
                        && is_allowed_extension(entry.path())
                    {
                        // The receiver outlives the walk, so the send cannot fail.
                        let _ = tx.send(entry.into_path());
                    }
                }
                Err(e) => eprintln!("[search] Skipped entry under {}: {e}", root.display()),
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
        discover_files(root, &AtomicBool::new(false)).expect("not cancelled")
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
            discover_files(temp_dir.path(), &AtomicBool::new(true)),
            None
        );
    }
}
