//! File system utilities.

pub(crate) mod workspace_index;
pub use workspace_index::{
    FileTreeEntry, WorkspaceFileIndex, WorkspaceSnapshot, scan_expanded_tree,
};

use crate::buffer::LineEnding;
use std::collections::hash_map::DefaultHasher;
use std::fs;
use std::hash::Hasher;
use std::io::{BufReader, BufWriter, Read, Write};
use std::path::Path;
use std::time::SystemTime;

const IO_BUF_BYTES: usize = 64 * 1024;

/// Computes the exact disk content hash (matching `DefaultHasher` on `str`) by
/// streaming file bytes without reading the entire document into a `String`.
pub fn read_file_hash(path: &Path) -> std::io::Result<u64> {
    let file = fs::File::open(path)?;
    let mut reader = BufReader::with_capacity(IO_BUF_BYTES, file);
    let mut hasher = DefaultHasher::new();
    let mut buf = [0u8; 8192];
    loop {
        let count = match reader.read(&mut buf) {
            Err(error) if error.kind() == std::io::ErrorKind::Interrupted => continue,
            result => result?,
        };
        if count == 0 {
            break;
        }
        hasher.write(&buf[..count]);
    }
    // Match `Hash for str` / `StreamingBuffer::disk_hash` trailing delimiter.
    hasher.write_u8(0xff);
    Ok(hasher.finish())
}

/// Streams a `ropey::Rope` snapshot directly to disk with the target line ending,
/// returning `(mtime, disk_hash)` in a single pass with zero full-document allocation.
pub fn save_rope_snapshot(
    path: &Path,
    rope: &ropey::Rope,
    line_ending: LineEnding,
    is_empty_doc: bool,
) -> std::io::Result<(SystemTime, u64)> {
    if let Some(parent) = path.parent()
        && !parent.as_os_str().is_empty()
    {
        let _ = fs::create_dir_all(parent);
    }

    let file = fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .open(path)?;

    let mut writer = BufWriter::with_capacity(IO_BUF_BYTES, file);
    let mut hasher = DefaultHasher::new();
    if !is_empty_doc {
        let ending = line_ending.as_str().as_bytes();
        for chunk in rope.chunks() {
            if line_ending == LineEnding::Lf {
                let bytes = chunk.as_bytes();
                writer.write_all(bytes)?;
                hasher.write(bytes);
            } else {
                let mut first = true;
                for segment in chunk.split('\n') {
                    if !first {
                        writer.write_all(ending)?;
                        hasher.write(ending);
                    }
                    first = false;
                    if !segment.is_empty() {
                        let bytes = segment.as_bytes();
                        writer.write_all(bytes)?;
                        hasher.write(bytes);
                    }
                }
            }
        }
    }
    writer.flush()?;
    hasher.write_u8(0xff);

    let mtime = writer.get_ref().metadata()?.modified()?;
    Ok((mtime, hasher.finish()))
}

/// Saves a document directly to disk by streaming string chunks into a buffered writer.
///
/// Overwrites the file in place to preserve file identity, hard links, and file watchers.
pub fn save_document_chunks<I, C>(path: &Path, chunks: I) -> std::io::Result<SystemTime>
where
    I: IntoIterator<Item = C>,
    C: AsRef<[u8]>,
{
    if let Some(parent) = path.parent() {
        if !parent.as_os_str().is_empty() {
            let _ = fs::create_dir_all(parent);
        }
    }

    let file = fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .open(path)?;

    let mut writer = std::io::BufWriter::with_capacity(64 * 1024, file);
    for chunk in chunks {
        writer.write_all(chunk.as_ref())?;
    }
    writer.flush()?;

    let mtime = writer.get_ref().metadata()?.modified()?;
    Ok(mtime)
}

/// Atomically writes content to `target_path` using a temporary file and rename.
pub fn atomic_write(target_path: &Path, content: impl AsRef<[u8]>) -> std::io::Result<()> {
    if fs::metadata(target_path)
        .map(|m| m.is_dir())
        .unwrap_or(false)
    {
        return Err(std::io::Error::from(std::io::ErrorKind::IsADirectory));
    }

    let parent = match target_path.parent() {
        Some(p) if !p.as_os_str().is_empty() => p,
        _ => Path::new("."),
    };

    if parent != Path::new(".") {
        let _ = fs::create_dir_all(parent);
    }

    let mut temp = tempfile::NamedTempFile::new_in(parent)?;
    temp.write_all(content.as_ref())?;
    temp.flush()?;

    #[cfg(windows)]
    {
        let mut file = temp;
        let mut last_err = None;
        for attempt in 0..3 {
            match file.persist(target_path) {
                Ok(_) => return Ok(()),
                Err(e) => {
                    last_err = Some(e.error);
                    file = e.file;
                    std::thread::sleep(std::time::Duration::from_millis(10 * (attempt + 1)));
                }
            }
        }
        Err(last_err.unwrap())
    }
    #[cfg(not(windows))]
    {
        temp.persist(target_path).map_err(|e| e.error)?;
        Ok(())
    }
}

/// Entry file name representing a collocated document bundle inside a directory.
pub const COLLOCATED_INDEX_FILE: &str = "index.md";

/// Returns `true` when `path` points to a collocated bundle entry file (`index.md`, ASCII case-insensitive).
pub fn is_collocated_index_path(path: &std::path::Path) -> bool {
    path.file_name()
        .and_then(|n| n.to_str())
        .is_some_and(|n| n.eq_ignore_ascii_case(COLLOCATED_INDEX_FILE))
}

/// Locates the collocated entry file (`index.md`, ASCII case-insensitive) inside `dir`,
/// if it exists as a regular file.
pub fn find_collocated_index_file(dir: &std::path::Path) -> Option<std::path::PathBuf> {
    if !dir.is_dir() {
        return None;
    }
    let candidate = dir.join(COLLOCATED_INDEX_FILE);
    if candidate.is_file() {
        return Some(candidate);
    }

    // Fallback for case-sensitive filesystems where `INDEX.MD` / `Index.md` differs in case.
    let mut matches: Vec<std::path::PathBuf> = std::fs::read_dir(dir)
        .ok()?
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| path.is_file() && is_collocated_index_path(path))
        .collect();
    matches.sort();
    matches.into_iter().next()
}

/// Returns `true` when `dir` is a collocated document bundle containing `index.md`.
pub fn has_collocated_index(dir: &std::path::Path) -> bool {
    find_collocated_index_file(dir).is_some()
}

/// Returns the collocated bundle directory for `file_path` if `file_path` is an `index.md` file.
pub fn collocated_bundle_dir(file_path: &std::path::Path) -> Option<&std::path::Path> {
    if is_collocated_index_path(file_path) {
        file_path.parent().filter(|p| !p.as_os_str().is_empty())
    } else {
        None
    }
}

/// Returns the collocated bundle directory for `file_path` unless that directory IS `root_dir`
/// itself (preventing `<workspace_root>/index.md` from masquerading as a document bundle).
pub fn collocated_bundle_dir_within<'a>(
    file_path: &'a std::path::Path,
    root_dir: Option<&std::path::Path>,
) -> Option<&'a std::path::Path> {
    let bundle = collocated_bundle_dir(file_path)?;
    if root_dir.is_some_and(|root| bundle == root) {
        return None;
    }
    Some(bundle)
}

/// Resolves a clicked/dropped entry path to an openable markdown file path.
///
/// In collocated mode, clicking or dropping a bundle directory resolves to its `index.md`;
/// a directory without `index.md` (or any directory in standard mode) yields `None`.
pub fn resolve_openable_document_path(
    path: &std::path::Path,
    collocated_mode: bool,
) -> Option<std::path::PathBuf> {
    if path.is_dir() {
        collocated_mode
            .then(|| find_collocated_index_file(path))
            .flatten()
    } else {
        path.is_file().then(|| path.to_path_buf())
    }
}

/// Maps an internal document file path (`<bundle>/index.md`) to its external user-facing path
/// (`<bundle>`) in collocated mode, leaving paths outside bundles (or `<root>/index.md`) unchanged.
pub fn resolve_document_external_path<'a>(
    path: &'a std::path::Path,
    collocated_mode: bool,
    root_dir: Option<&std::path::Path>,
) -> &'a std::path::Path {
    if collocated_mode && let Some(bundle_dir) = collocated_bundle_dir_within(path, root_dir) {
        bundle_dir
    } else {
        path
    }
}

/// Resolves the logical parent directory of a document: the parent of `<bundle>` in collocated
/// mode (avoiding placing exports inside `<bundle>/`), or the parent of `file_path` otherwise.
pub fn resolve_document_parent_dir<'a>(
    file_path: &'a std::path::Path,
    collocated_mode: bool,
    root_dir: Option<&std::path::Path>,
) -> Option<&'a std::path::Path> {
    resolve_document_external_path(file_path, collocated_mode, root_dir)
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
}

/// Resolves the display title for a document path in standard or collocated mode.
pub fn resolve_document_title(
    file_path: Option<&std::path::Path>,
    collocated_mode: bool,
) -> String {
    if collocated_mode
        && let Some(bundle_dir) = file_path.and_then(collocated_bundle_dir)
        && let Some(name) = bundle_dir.file_name().and_then(|n| n.to_str())
        && !name.is_empty()
    {
        return name.to_string();
    }

    file_path
        .and_then(|p| p.file_name())
        .map(|f| f.to_string_lossy().to_string())
        .unwrap_or_else(|| "Untitled.md".to_string())
}

#[cfg(test)]
mod collocated_tests {
    use super::*;

    #[test]
    fn test_find_collocated_index_case_insensitive_and_non_dir() {
        let dir = tempfile::tempdir().unwrap();
        let bundle = dir.path().join("MyNote");
        std::fs::create_dir_all(&bundle).unwrap();
        let upper_index = bundle.join("INDEX.MD");
        std::fs::write(&upper_index, "# Hello\n").unwrap();

        let found = find_collocated_index_file(&bundle).expect("should find INDEX.MD");
        assert!(is_collocated_index_path(&found));
        assert!(has_collocated_index(&bundle));
        assert_eq!(find_collocated_index_file(&upper_index), None);
    }

    #[test]
    fn test_collocated_bundle_dir_within_guards_workspace_root() {
        let root = std::path::Path::new("d:/workspace");
        let root_index = root.join("index.md");
        assert_eq!(collocated_bundle_dir(&root_index), Some(root));
        assert_eq!(collocated_bundle_dir_within(&root_index, Some(root)), None);

        let nested_index = root.join("guide").join("index.md");
        assert_eq!(
            collocated_bundle_dir_within(&nested_index, Some(root)),
            Some(root.join("guide").as_path())
        );
    }

    #[test]
    fn test_resolve_openable_document_path_and_external_paths() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let bundle = root.join("notes").join("chapter_one");
        let folder = root.join("empty_folder");
        std::fs::create_dir_all(&bundle).unwrap();
        std::fs::create_dir_all(&folder).unwrap();

        let index = bundle.join("index.md");
        let loose = root.join("loose.md");
        std::fs::write(&index, "# Chapter 1\n").unwrap();
        std::fs::write(&loose, "# Loose\n").unwrap();

        // Standard vs collocated opening of directories and files
        assert_eq!(
            resolve_openable_document_path(&bundle, true),
            Some(index.clone())
        );
        assert_eq!(resolve_openable_document_path(&bundle, false), None);
        assert_eq!(resolve_openable_document_path(&folder, true), None);
        assert_eq!(
            resolve_openable_document_path(&loose, false),
            Some(loose.clone())
        );
        assert_eq!(
            resolve_openable_document_path(&root.join("missing"), true),
            None
        );

        // External path & parent dir avoid leaking `<bundle>/index.md` and guard `root/index.md`
        assert_eq!(
            resolve_document_external_path(&index, true, Some(root)),
            bundle.as_path()
        );
        assert_eq!(
            resolve_document_external_path(&index, false, Some(root)),
            index.as_path()
        );
        assert_eq!(
            resolve_document_parent_dir(&index, true, Some(root)),
            Some(root.join("notes").as_path())
        );
        assert_eq!(
            resolve_document_parent_dir(&index, false, Some(root)),
            Some(bundle.as_path())
        );

        let root_index = root.join("index.md");
        assert_eq!(
            resolve_document_external_path(&root_index, true, Some(root)),
            root_index.as_path()
        );
        assert_eq!(
            resolve_document_parent_dir(&root_index, true, Some(root)),
            Some(root)
        );

        assert!(is_collocated_index_path(&index));
        assert!(is_collocated_index_path(std::path::Path::new(
            "/a/b/INDEX.MD"
        )));
        assert!(!is_collocated_index_path(&loose));
        assert_eq!(find_collocated_index_file(&bundle), Some(index.clone()));
        assert_eq!(find_collocated_index_file(&folder), None);
        assert!(has_collocated_index(&bundle));
        assert!(!has_collocated_index(&folder));
        assert_eq!(collocated_bundle_dir(&index), Some(bundle.as_path()));
        assert_eq!(collocated_bundle_dir(&loose), None);

        assert_eq!(resolve_document_title(Some(&index), false), "index.md");
        assert_eq!(resolve_document_title(Some(&index), true), "chapter_one");
        assert_eq!(resolve_document_title(Some(&loose), true), "loose.md");
        assert_eq!(resolve_document_title(None, true), "Untitled.md");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_save_document_chunks_streaming() {
        let temp_dir = tempfile::tempdir().expect("create temp dir");
        let target_file = temp_dir.path().join("document.md");

        let chunks = vec!["# Title\n\n", "Line 1\n", "Line 2"];
        let mtime = save_document_chunks(&target_file, chunks).unwrap();

        assert_eq!(
            fs::read_to_string(&target_file).unwrap(),
            "# Title\n\nLine 1\nLine 2"
        );
        assert_eq!(
            fs::metadata(&target_file).unwrap().modified().unwrap(),
            mtime
        );
    }

    #[test]
    fn test_save_document_chunks_creates_parent_dirs() {
        let temp_dir = tempfile::tempdir().expect("create temp dir");
        let nested_file = temp_dir.path().join("a").join("b").join("c.md");

        save_document_chunks(&nested_file, vec!["nested content"]).unwrap();
        assert_eq!(fs::read_to_string(&nested_file).unwrap(), "nested content");
    }

    #[test]
    fn test_atomic_write_basic_and_overwrite() {
        let temp_dir = tempfile::tempdir().expect("create temp dir");
        let target_file = temp_dir.path().join("config.toml");

        atomic_write(&target_file, "initial = 1\n").unwrap();
        assert_eq!(fs::read_to_string(&target_file).unwrap(), "initial = 1\n");

        atomic_write(&target_file, "updated = 2\n").unwrap();
        assert_eq!(fs::read_to_string(&target_file).unwrap(), "updated = 2\n");
    }

    #[test]
    fn test_atomic_write_creates_parent_dirs() {
        let temp_dir = tempfile::tempdir().expect("create temp dir");
        let nested_file = temp_dir.path().join("sub").join("nested.toml");

        atomic_write(&nested_file, "key = true\n").unwrap();
        assert_eq!(fs::read_to_string(&nested_file).unwrap(), "key = true\n");
    }

    #[test]
    fn test_atomic_write_directory_target_fails() {
        let temp_dir = tempfile::tempdir().expect("create temp dir");
        let target_dir = temp_dir.path().join("dir");
        fs::create_dir(&target_dir).unwrap();

        let err = atomic_write(&target_dir, "content").unwrap_err();
        assert_eq!(err.kind(), std::io::ErrorKind::IsADirectory);
    }

    #[test]
    fn test_atomic_write_bare_filename() {
        let rand_suffix = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        let bare_name = format!(
            "inviscid_test_bare_{}_{}.tmp",
            std::process::id(),
            rand_suffix
        );
        let target_path = Path::new(&bare_name);

        struct Cleanup<'a>(&'a Path);
        impl Drop for Cleanup<'_> {
            fn drop(&mut self) {
                let _ = fs::remove_file(self.0);
            }
        }
        let _guard = Cleanup(target_path);

        atomic_write(target_path, "bare content").unwrap();
        assert_eq!(fs::read_to_string(target_path).unwrap(), "bare content");
    }
}
