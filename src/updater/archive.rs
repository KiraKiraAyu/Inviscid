use std::fmt;
use std::fs;
use std::io::{self, Read};
use std::path::{Component, Path, PathBuf};

use super::manifest::ArchiveFormat;

/// Hard upper bound on total uncompressed bytes written during archive extraction (512 MiB).
pub const MAX_EXTRACTED_BYTES: u64 = 512 * 1024 * 1024;

#[derive(Debug)]
pub enum ArchiveError {
    InvalidPath(String),
    SymlinkEscape(String),
    SizeLimitExceeded(u64),
    EntryNotFound(String),
    UnsupportedFormat(ArchiveFormat),
    Io(io::Error),
    Format(String),
}

impl fmt::Display for ArchiveError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidPath(p) => write!(f, "Unsafe or invalid archive path: '{p}'"),
            Self::SymlinkEscape(p) => write!(f, "Symlink escapes extraction root: '{p}'"),
            Self::SizeLimitExceeded(limit) => {
                write!(f, "Extracted archive exceeded limit of {limit} bytes")
            }
            Self::EntryNotFound(entry) => {
                write!(f, "Expected entry '{entry}' was not found in archive")
            }
            Self::UnsupportedFormat(fmt_kind) => {
                write!(f, "Archive format {fmt_kind:?} is not supported on this platform")
            }
            Self::Io(err) => write!(f, "Archive I/O error: {err}"),
            Self::Format(msg) => write!(f, "Archive format error: {msg}"),
        }
    }
}

impl std::error::Error for ArchiveError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io(err) => Some(err),
            _ => None,
        }
    }
}

impl From<io::Error> for ArchiveError {
    fn from(err: io::Error) -> Self {
        Self::Io(err)
    }
}

/// Validates that `raw` is a clean relative path without root, prefix, or `..` components.
pub fn validate_relative_archive_path(raw: &Path) -> Result<PathBuf, ArchiveError> {
    let mut clean = PathBuf::new();
    for component in raw.components() {
        match component {
            Component::Normal(part) => clean.push(part),
            Component::CurDir => {}
            Component::ParentDir | Component::RootDir | Component::Prefix(_) => {
                return Err(ArchiveError::InvalidPath(raw.display().to_string()));
            }
        }
    }
    if clean.as_os_str().is_empty() {
        return Err(ArchiveError::InvalidPath(raw.display().to_string()));
    }
    Ok(clean)
}

#[cfg(any(unix, test))]
fn validate_relative_symlink_target(
    entry_rel_path: &Path,
    link_target: &Path,
) -> Result<(), ArchiveError> {
    if link_target.is_absolute() {
        return Err(ArchiveError::SymlinkEscape(
            link_target.display().to_string(),
        ));
    }

    let mut depth: usize = entry_rel_path
        .parent()
        .map(|p| {
            p.components()
                .filter(|c| matches!(c, Component::Normal(_)))
                .count()
        })
        .unwrap_or(0);

    for component in link_target.components() {
        match component {
            Component::CurDir => {}
            Component::Normal(_) => depth += 1,
            Component::ParentDir => {
                if depth == 0 {
                    return Err(ArchiveError::SymlinkEscape(
                        link_target.display().to_string(),
                    ));
                }
                depth -= 1;
            }
            Component::RootDir | Component::Prefix(_) => {
                return Err(ArchiveError::SymlinkEscape(
                    link_target.display().to_string(),
                ));
            }
        }
    }
    Ok(())
}

fn copy_bounded(
    reader: &mut dyn Read,
    dest_file: &mut fs::File,
    extracted_total: &mut u64,
    max_total_bytes: u64,
) -> Result<(), ArchiveError> {
    let mut buf = [0u8; 64 * 1024];
    loop {
        let count = match reader.read(&mut buf) {
            Err(err) if err.kind() == io::ErrorKind::Interrupted => continue,
            result => result?,
        };
        if count == 0 {
            break;
        }
        *extracted_total = extracted_total.saturating_add(count as u64);
        if *extracted_total > max_total_bytes {
            return Err(ArchiveError::SizeLimitExceeded(max_total_bytes));
        }
        io::Write::write_all(dest_file, &buf[..count])?;
    }
    Ok(())
}

/// Safely extracts `archive_path` into `staging_dir` and returns the resolved path to
/// `expected_entry`, enforcing path traversal, symlink escape, and size limits.
pub fn extract_archive_safely(
    archive_path: &Path,
    format: ArchiveFormat,
    staging_dir: &Path,
    expected_entry: &str,
) -> Result<PathBuf, ArchiveError> {
    extract_archive_with_limit(
        archive_path,
        format,
        staging_dir,
        expected_entry,
        MAX_EXTRACTED_BYTES,
    )
}

pub(crate) fn extract_archive_with_limit(
    archive_path: &Path,
    format: ArchiveFormat,
    staging_dir: &Path,
    expected_entry: &str,
    max_total_bytes: u64,
) -> Result<PathBuf, ArchiveError> {
    if staging_dir.exists() {
        fs::remove_dir_all(staging_dir)?;
    }
    fs::create_dir_all(staging_dir)?;

    let extract_res = match format {
        ArchiveFormat::Zip => {
            #[cfg(any(target_os = "windows", test))]
            {
                extract_zip_safely(archive_path, staging_dir, max_total_bytes)
            }
            #[cfg(not(any(target_os = "windows", test)))]
            {
                Err(ArchiveError::UnsupportedFormat(ArchiveFormat::Zip))
            }
        }
        ArchiveFormat::TarGz => {
            #[cfg(any(unix, test))]
            {
                extract_tar_gz_safely(archive_path, staging_dir, max_total_bytes)
            }
            #[cfg(not(any(unix, test)))]
            {
                Err(ArchiveError::UnsupportedFormat(ArchiveFormat::TarGz))
            }
        }
    };

    if let Err(err) = extract_res {
        let _ = fs::remove_dir_all(staging_dir);
        return Err(err);
    }

    let expected_rel = validate_relative_archive_path(Path::new(expected_entry))?;
    let resolved_entry = staging_dir.join(&expected_rel);
    if !resolved_entry.exists() {
        let _ = fs::remove_dir_all(staging_dir);
        return Err(ArchiveError::EntryNotFound(expected_entry.to_string()));
    }

    Ok(resolved_entry)
}

#[cfg(any(target_os = "windows", test))]
fn extract_zip_safely(
    archive_path: &Path,
    staging_dir: &Path,
    max_total_bytes: u64,
) -> Result<(), ArchiveError> {
    let file = fs::File::open(archive_path)?;
    let mut archive =
        zip::ZipArchive::new(file).map_err(|err| ArchiveError::Format(err.to_string()))?;
    let mut extracted_total: u64 = 0;

    for idx in 0..archive.len() {
        let mut entry = archive
            .by_index(idx)
            .map_err(|err| ArchiveError::Format(err.to_string()))?;

        if entry.is_symlink() {
            let name = entry
                .name()
                .map(|n| n.into_owned())
                .unwrap_or_else(|_| format!("entry#{idx}"));
            return Err(ArchiveError::SymlinkEscape(name));
        }

        let raw_name = entry
            .name()
            .map_err(|err| ArchiveError::Format(err.to_string()))?
            .into_owned();
        if raw_name.contains('\\') && !cfg!(windows) {
            return Err(ArchiveError::InvalidPath(raw_name));
        }
        let rel_path = validate_relative_archive_path(Path::new(&raw_name))?;
        let Some(enclosed) = entry.enclosed_name() else {
            return Err(ArchiveError::InvalidPath(raw_name));
        };
        if enclosed != rel_path {
            return Err(ArchiveError::InvalidPath(raw_name));
        }

        let out_path = staging_dir.join(&rel_path);
        if entry.is_dir() {
            fs::create_dir_all(&out_path)?;
            continue;
        }

        if let Some(parent) = out_path.parent() {
            fs::create_dir_all(parent)?;
        }
        let mut out_file = fs::File::create(&out_path)?;
        copy_bounded(
            &mut entry,
            &mut out_file,
            &mut extracted_total,
            max_total_bytes,
        )?;
        io::Write::flush(&mut out_file)?;

        #[cfg(unix)]
        if let Some(mode) = entry.unix_mode() {
            use std::os::unix::fs::PermissionsExt;
            let perms = fs::Permissions::from_mode(mode & 0o777);
            let _ = fs::set_permissions(&out_path, perms);
        }
    }

    Ok(())
}

#[cfg(any(unix, test))]
fn extract_tar_gz_safely(
    archive_path: &Path,
    staging_dir: &Path,
    max_total_bytes: u64,
) -> Result<(), ArchiveError> {
    let file = fs::File::open(archive_path)?;
    let gz = flate2::read::GzDecoder::new(file);
    let mut archive = tar::Archive::new(gz);
    let mut extracted_total: u64 = 0;

    let entries = archive.entries()?;
    for entry_res in entries {
        let mut entry = entry_res?;
        let raw_path = entry.path()?.into_owned();
        let rel_path = validate_relative_archive_path(&raw_path)?;
        let out_path = staging_dir.join(&rel_path);
        let entry_type = entry.header().entry_type();

        if entry_type.is_dir() {
            fs::create_dir_all(&out_path)?;
            continue;
        }

        if entry_type.is_symlink() {
            let Some(link_target) = entry.link_name()?.map(|p| p.into_owned()) else {
                return Err(ArchiveError::SymlinkEscape(raw_path.display().to_string()));
            };
            validate_relative_symlink_target(&rel_path, &link_target)?;
            if let Some(parent) = out_path.parent() {
                fs::create_dir_all(parent)?;
            }
            #[cfg(unix)]
            {
                std::os::unix::fs::symlink(&link_target, &out_path)?;
            }
            continue;
        }

        if !entry_type.is_file() {
            return Err(ArchiveError::InvalidPath(format!(
                "Unsupported tar entry type at '{}'",
                raw_path.display()
            )));
        }

        if let Some(parent) = out_path.parent() {
            fs::create_dir_all(parent)?;
        }
        let mut out_file = fs::File::create(&out_path)?;
        copy_bounded(
            &mut entry,
            &mut out_file,
            &mut extracted_total,
            max_total_bytes,
        )?;
        io::Write::flush(&mut out_file)?;

        #[cfg(unix)]
        if let Ok(mode) = entry.header().mode() {
            use std::os::unix::fs::PermissionsExt;
            let perms = fs::Permissions::from_mode(mode & 0o777);
            let _ = fs::set_permissions(&out_path, perms);
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use flate2::Compression;
    use flate2::write::GzEncoder;
    use std::io::Write;
    use zip::write::SimpleFileOptions;

    fn write_test_zip(path: &Path, entries: &[(&str, &[u8])]) {
        let file = fs::File::create(path).unwrap();
        let mut writer = zip::ZipWriter::new(file);
        for (name, data) in entries {
            writer
                .start_file(*name, SimpleFileOptions::default())
                .unwrap();
            writer.write_all(data).unwrap();
        }
        writer.finish().unwrap();
    }

    fn write_test_tar_gz(path: &Path, entries: &[(&str, &[u8])]) {
        let file = fs::File::create(path).unwrap();
        let gz = GzEncoder::new(file, Compression::default());
        let mut builder = tar::Builder::new(gz);
        for (name, data) in entries {
            let mut header = tar::Header::new_gnu();
            header.set_size(data.len() as u64);
            header.set_mode(0o755);
            header.set_cksum();
            builder.append_data(&mut header, *name, *data).unwrap();
        }
        let gz = builder.into_inner().unwrap();
        gz.finish().unwrap();
    }

    #[test]
    fn test_validate_relative_archive_path_rejects_traversal() {
        assert_eq!(
            validate_relative_archive_path(Path::new("Inviscid.app/Contents/MacOS/inviscid"))
                .unwrap(),
            PathBuf::from("Inviscid.app/Contents/MacOS/inviscid")
        );
        assert!(validate_relative_archive_path(Path::new("../evil.exe")).is_err());
        assert!(validate_relative_archive_path(Path::new("a/../../evil.exe")).is_err());
        assert!(validate_relative_archive_path(Path::new("/etc/passwd")).is_err());
        assert!(validate_relative_archive_path(Path::new("")).is_err());
    }

    #[test]
    fn test_zip_extraction_success_slip_and_size_limit() {
        let dir = tempfile::tempdir().unwrap();
        let zip_path = dir.path().join("valid.zip");
        write_test_zip(&zip_path, &[("inviscid.exe", b"MZbinary")]);

        let staging = dir.path().join("staging_ok");
        let entry = extract_archive_safely(&zip_path, ArchiveFormat::Zip, &staging, "inviscid.exe")
            .unwrap();
        assert_eq!(fs::read(&entry).unwrap(), b"MZbinary");

        // Zip Slip rejected
        let slip_zip = dir.path().join("slip.zip");
        write_test_zip(&slip_zip, &[("../escaped.exe", b"bad")]);
        let staging_slip = dir.path().join("staging_slip");
        let err = extract_archive_safely(
            &slip_zip,
            ArchiveFormat::Zip,
            &staging_slip,
            "inviscid.exe",
        )
        .unwrap_err();
        assert!(matches!(err, ArchiveError::InvalidPath(_)));
        assert!(!dir.path().join("escaped.exe").exists());

        // Size limit enforced
        let staging_big = dir.path().join("staging_big");
        let err_big = extract_archive_with_limit(
            &zip_path,
            ArchiveFormat::Zip,
            &staging_big,
            "inviscid.exe",
            4,
        )
        .unwrap_err();
        assert!(matches!(err_big, ArchiveError::SizeLimitExceeded(4)));
    }

    #[test]
    fn test_tar_gz_extraction_and_symlink_escape_guards() {
        let dir = tempfile::tempdir().unwrap();
        let tar_path = dir.path().join("macos.tar.gz");
        write_test_tar_gz(
            &tar_path,
            &[
                ("Inviscid.app/Contents/Info.plist", b"<plist/>"),
                ("Inviscid.app/Contents/MacOS/inviscid", b"macho"),
            ],
        );

        let staging = dir.path().join("staging_tar");
        let app_dir =
            extract_archive_safely(&tar_path, ArchiveFormat::TarGz, &staging, "Inviscid.app")
                .unwrap();
        assert!(app_dir.join("Contents/MacOS/inviscid").is_file());

        // Symlink escape detection
        assert!(
            validate_relative_symlink_target(
                Path::new("Inviscid.app/Contents/MacOS/link"),
                Path::new("../Resources/data")
            )
            .is_ok()
        );
        assert!(
            validate_relative_symlink_target(
                Path::new("Inviscid.app/link"),
                Path::new("../../outside")
            )
            .is_err()
        );
        assert!(
            validate_relative_symlink_target(Path::new("link"), Path::new("/etc/passwd")).is_err()
        );

        // Tar slip via raw header
        let slip_tar = dir.path().join("slip.tar.gz");
        let file = fs::File::create(&slip_tar).unwrap();
        let gz = GzEncoder::new(file, Compression::default());
        let mut builder = tar::Builder::new(gz);
        let mut header = tar::Header::new_gnu();
        header.set_size(3);
        header.set_mode(0o644);
        header
            .set_path("Inviscid.app/../../../escape")
            .unwrap_or_else(|_| {
                // Force raw path bytes in header if tar crate rejects ..
                header.as_gnu_mut().unwrap().name[..9].copy_from_slice(b"../escape");
            });
        header.set_cksum();
        builder.append(&header, &b"bad"[..]).unwrap();
        let gz = builder.into_inner().unwrap();
        gz.finish().unwrap();

        let staging_slip = dir.path().join("staging_tar_slip");
        let err = extract_archive_safely(
            &slip_tar,
            ArchiveFormat::TarGz,
            &staging_slip,
            "Inviscid.app",
        )
        .unwrap_err();
        assert!(matches!(err, ArchiveError::InvalidPath(_)));
    }
}
