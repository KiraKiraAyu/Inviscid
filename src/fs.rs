//! File system utilities.

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
