//! File system utilities.

use std::fs;
use std::io::Write;
use std::path::Path;
use std::time::SystemTime;

/// Saves a document directly to disk by streaming string chunks into a buffered writer.
///
/// Overwrites the file in place to preserve file identity, hard links, and file watchers.
pub fn save_document_chunks<'a>(
    path: &Path,
    chunks: impl IntoIterator<Item = &'a str>,
) -> std::io::Result<SystemTime> {
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
        writer.write_all(chunk.as_bytes())?;
    }
    writer.flush()?;

    let mtime = writer.get_ref().metadata()?.modified()?;
    Ok(mtime)
}

/// Atomically writes content to `target_path` using a temporary file and rename.
pub fn atomic_write(target_path: &Path, content: impl AsRef<[u8]>) -> std::io::Result<()> {
    if fs::metadata(target_path).map(|m| m.is_dir()).unwrap_or(false) {
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
        let bare_name = format!("inviscid_test_bare_{}_{}.tmp", std::process::id(), rand_suffix);
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
