use gpui::{App, ClipboardItem};
use std::path::{Path, PathBuf};

/// Creates a new empty file in the target parent directory.
pub fn create_file(parent: &Path, name: &str) -> std::io::Result<PathBuf> {
    std::fs::create_dir_all(parent)?;
    let target = parent.join(name);
    std::fs::File::create(&target)?;
    Ok(target)
}

/// Creates a new folder in the target parent directory.
pub fn create_folder(parent: &Path, name: &str) -> std::io::Result<PathBuf> {
    let target = parent.join(name);
    std::fs::create_dir_all(&target)?;
    Ok(target)
}

/// Renames a file or folder within its existing parent directory.
pub fn rename_entry(path: &Path, new_name: &str) -> std::io::Result<PathBuf> {
    let parent = path.parent().ok_or_else(|| {
        std::io::Error::new(std::io::ErrorKind::InvalidInput, "Root cannot be renamed")
    })?;
    let target = parent.join(new_name);
    if target == path {
        return Ok(target);
    }
    std::fs::rename(path, &target)?;
    Ok(target)
}

/// Duplicates a file or directory, generating a standard "copy" suffix next to the original.
pub fn duplicate_entry(path: &Path) -> std::io::Result<PathBuf> {
    let parent = path.parent().ok_or_else(|| {
        std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "Root cannot be duplicated",
        )
    })?;
    let (stem, ext) = name_parts(path)
        .ok_or_else(|| std::io::Error::new(std::io::ErrorKind::InvalidInput, "Invalid filename"))?;

    let target = parent.join(free_copy_name(parent, &stem, ext.as_deref()));
    copy_entry(path, &target)?;
    Ok(target)
}

/// Deletes a file or directory either to system Trash or permanently.
pub fn delete_entry(path: &Path, to_trash: bool) -> std::io::Result<()> {
    if to_trash {
        crate::platform::move_to_trash(path)
    } else {
        remove_entry(path)
    }
}

/// Executes paste operation (either move for Cut or copy for Copy) into the target directory.
pub fn perform_paste(src: &Path, is_cut: bool, dest_dir: &Path) -> std::io::Result<PathBuf> {
    if !dest_dir.is_dir() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "Destination must be a directory",
        ));
    }
    let file_name = src.file_name().ok_or_else(|| {
        std::io::Error::new(std::io::ErrorKind::InvalidInput, "Invalid source filename")
    })?;

    // A copy that would land on a taken name gets a "copy" suffix *inside the destination*.
    // Deriving that name from the source's parent instead silently drops the paste at the
    // destination and leaves a stray duplicate next to the original.
    let target = if !is_cut && dest_dir.join(file_name).exists() {
        let (stem, ext) = name_parts(src).ok_or_else(|| {
            std::io::Error::new(std::io::ErrorKind::InvalidInput, "Invalid source filename")
        })?;
        dest_dir.join(free_copy_name(dest_dir, &stem, ext.as_deref()))
    } else {
        dest_dir.join(file_name)
    };

    if is_cut {
        // `std::fs::rename` refuses to overwrite on Windows, so a name clash (or a cross-device
        // move) falls through to copy-then-delete and replaces the destination.
        if std::fs::rename(src, &target).is_err() {
            copy_entry(src, &target)?;
            remove_entry(src)?;
        }
    } else {
        copy_entry(src, &target)?;
    }

    Ok(target)
}

/// Reports a failed filesystem operation.
///
/// The panel has no error surface yet, so this is the single place that decides how a failure
/// leaves the module.
pub fn report_error(operation: &str, path: &Path, error: &std::io::Error) {
    eprintln!("{} failed for {}: {}", operation, path.display(), error);
}

/// Splits a path's file name into `(stem, extension)`.
///
/// Directories and extension-less files report no extension, so a duplicate of `notes` is
/// `notes copy` rather than `notes copy.notes`.
fn name_parts(path: &Path) -> Option<(String, Option<String>)> {
    let file_name = path.file_name()?.to_str()?;
    if path.is_dir() {
        return Some((file_name.to_string(), None));
    }
    match path.extension().and_then(|e| e.to_str()) {
        Some(ext) => {
            let stem = path
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or(file_name);
            Some((stem.to_string(), Some(ext.to_string())))
        }
        None => Some((file_name.to_string(), None)),
    }
}

/// The first free `"{stem} copy"`, `"{stem} copy 2"`, ... name inside `dir`.
fn free_copy_name(dir: &Path, stem: &str, ext: Option<&str>) -> String {
    let candidate = |counter: Option<usize>| match (ext, counter) {
        (Some(ext), None) => format!("{stem} copy.{ext}"),
        (Some(ext), Some(n)) => format!("{stem} copy {n}.{ext}"),
        (None, None) => format!("{stem} copy"),
        (None, Some(n)) => format!("{stem} copy {n}"),
    };

    let mut name = candidate(None);
    let mut counter = 2;
    while dir.join(&name).exists() {
        name = candidate(Some(counter));
        counter += 1;
    }
    name
}

/// Copies `src` — a file, or a whole directory tree — onto `dst`.
fn copy_entry(src: &Path, dst: &Path) -> std::io::Result<()> {
    if src.is_dir() {
        copy_dir_recursive(src, dst)
    } else {
        std::fs::copy(src, dst).map(|_| ())
    }
}

/// Removes a file or a whole directory tree.
fn remove_entry(path: &Path) -> std::io::Result<()> {
    if path.is_dir() {
        std::fs::remove_dir_all(path)
    } else {
        std::fs::remove_file(path)
    }
}

/// Copies the full absolute path of the target to the system clipboard.
pub fn copy_path_to_clipboard(path: &Path, cx: &mut App) {
    cx.write_to_clipboard(ClipboardItem::new_string(
        path.to_string_lossy().to_string(),
    ));
}

/// Copies the relative path of the target from the workspace root to the system clipboard.
pub fn copy_relative_path_to_clipboard(path: &Path, root: Option<&Path>, cx: &mut App) {
    let rel = match root {
        Some(r) => path.strip_prefix(r).unwrap_or(path),
        None => path,
    };
    cx.write_to_clipboard(ClipboardItem::new_string(rel.to_string_lossy().to_string()));
}

/// Recursively copies a directory tree from `src` to `dst`.
fn copy_dir_recursive(src: &Path, dst: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(dst)?;
    for entry in std::fs::read_dir(src)?.flatten() {
        let entry_path = entry.path();
        let target_path = dst.join(entry.file_name());
        if entry_path.is_dir() {
            copy_dir_recursive(&entry_path, &target_path)?;
        } else {
            std::fs::copy(&entry_path, &target_path)?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A scratch directory that cleans itself up.
    struct Scratch(PathBuf);

    impl Scratch {
        fn new(prefix: &str) -> Self {
            let dir = std::env::temp_dir().join(format!(
                "inviscid_fs_ops_{}_{}",
                prefix,
                std::process::id()
            ));
            let _ = std::fs::remove_dir_all(&dir);
            std::fs::create_dir_all(&dir).unwrap();
            Self(dir)
        }

        fn dir(&self, rel: &str) -> PathBuf {
            let path = self.0.join(rel);
            std::fs::create_dir_all(&path).unwrap();
            path
        }

        fn file(&self, rel: &str, contents: &str) -> PathBuf {
            let path = self.0.join(rel);
            if let Some(parent) = path.parent() {
                std::fs::create_dir_all(parent).unwrap();
            }
            std::fs::write(&path, contents).unwrap();
            path
        }

        fn read(&self, rel: &str) -> String {
            std::fs::read_to_string(self.0.join(rel)).unwrap()
        }
    }

    impl Drop for Scratch {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn copy_paste_onto_a_taken_name_stays_in_the_destination() {
        // Regression: the copy used to be named after the *source's* parent, so pasting into a
        // directory that already held the same name did nothing there and left a stray
        // "note copy.md" beside the original.
        let scratch = Scratch::new("paste_name_clash");
        let source_dir = scratch.dir("a");
        let dest_dir = scratch.dir("b");
        let src = scratch.file("a/note.md", "FROM_A");
        scratch.file("b/note.md", "ORIGINAL_IN_B");

        let pasted = perform_paste(&src, false, &dest_dir).unwrap();

        assert_eq!(pasted, dest_dir.join("note copy.md"));
        assert_eq!(
            scratch.read("b/note.md"),
            "ORIGINAL_IN_B",
            "existing file must survive"
        );
        assert_eq!(scratch.read("b/note copy.md"), "FROM_A");
        assert_eq!(
            std::fs::read_dir(&source_dir).unwrap().count(),
            1,
            "the source directory must not collect a stray duplicate"
        );
    }

    #[test]
    fn copy_paste_keeps_the_plain_name_when_it_is_free() {
        let scratch = Scratch::new("paste_free_name");
        let dest_dir = scratch.dir("b");
        let src = scratch.file("a/note.md", "FROM_A");

        let pasted = perform_paste(&src, false, &dest_dir).unwrap();

        assert_eq!(pasted, dest_dir.join("note.md"));
        assert_eq!(scratch.read("b/note.md"), "FROM_A");
    }

    #[test]
    fn cut_paste_moves_the_entry() {
        let scratch = Scratch::new("paste_cut");
        let dest_dir = scratch.dir("b");
        let src = scratch.file("a/note.md", "BODY");

        let pasted = perform_paste(&src, true, &dest_dir).unwrap();

        assert_eq!(pasted, dest_dir.join("note.md"));
        assert_eq!(scratch.read("b/note.md"), "BODY");
        assert!(!src.exists(), "cut must remove the original");
    }

    #[test]
    fn duplicate_suffixes_keep_counting_up() {
        let scratch = Scratch::new("duplicate_counter");
        let src = scratch.file("note.md", "BODY");
        scratch.file("note copy.md", "first");
        scratch.file("note copy 2.md", "second");

        let duplicated = duplicate_entry(&src).unwrap();

        assert_eq!(duplicated, scratch.0.join("note copy 3.md"));
        assert_eq!(scratch.read("note copy 3.md"), "BODY");
    }

    #[test]
    fn duplicate_of_an_extensionless_file_gets_no_dot() {
        let scratch = Scratch::new("duplicate_no_ext");
        let src = scratch.file("LICENSE", "BODY");

        let duplicated = duplicate_entry(&src).unwrap();

        assert_eq!(duplicated, scratch.0.join("LICENSE copy"));
    }

    #[test]
    fn duplicate_of_a_directory_copies_the_tree() {
        let scratch = Scratch::new("duplicate_dir");
        scratch.file("docs/intro.md", "INTRO");
        let src = scratch.dir("docs");

        let duplicated = duplicate_entry(&src).unwrap();

        assert_eq!(duplicated, scratch.0.join("docs copy"));
        assert_eq!(scratch.read("docs copy/intro.md"), "INTRO");
    }
}
