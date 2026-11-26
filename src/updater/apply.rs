use std::fmt;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::process::Command;

use super::StagedUpdate;

#[derive(Debug)]
pub enum ApplyError {
    InvalidArtifact(String),
    BundleNotFound(PathBuf),
    ReplaceFailed(String),
    RelaunchFailed(io::Error),
    Io(io::Error),
}

impl fmt::Display for ApplyError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidArtifact(msg) => write!(f, "Invalid update artifact: {msg}"),
            Self::BundleNotFound(path) => {
                write!(f, "Could not locate application bundle for {}", path.display())
            }
            Self::ReplaceFailed(msg) => write!(f, "Failed to replace application: {msg}"),
            Self::RelaunchFailed(err) => write!(f, "Failed to relaunch updated application: {err}"),
            Self::Io(err) => write!(f, "Apply I/O error: {err}"),
        }
    }
}

impl std::error::Error for ApplyError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::RelaunchFailed(err) | Self::Io(err) => Some(err),
            _ => None,
        }
    }
}

impl From<io::Error> for ApplyError {
    fn from(err: io::Error) -> Self {
        Self::Io(err)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ApplyOutcome {
    pub relaunch_path: PathBuf,
}

#[cfg(any(target_os = "windows", test))]
pub(crate) fn validate_windows_pe_header(path: &Path) -> Result<(), ApplyError> {
    use std::io::Read;
    let mut file = fs::File::open(path)?;
    let mut magic = [0u8; 2];
    file.read_exact(&mut magic).map_err(|err| {
        ApplyError::InvalidArtifact(format!(
            "Could not read PE header from {}: {err}",
            path.display()
        ))
    })?;
    if &magic != b"MZ" {
        return Err(ApplyError::InvalidArtifact(format!(
            "Artifact {} does not have a valid Windows MZ executable header",
            path.display()
        )));
    }
    Ok(())
}

#[cfg(any(target_os = "macos", test))]
pub(crate) fn locate_macos_app_bundle(exe_path: &Path) -> Option<PathBuf> {
    let mut ancestors = exe_path.ancestors();
    let _exe = ancestors.next()?;
    let macos_dir = ancestors.next()?;
    if macos_dir.file_name().and_then(|n| n.to_str()) != Some("MacOS") {
        return None;
    }
    let contents_dir = ancestors.next()?;
    if contents_dir.file_name().and_then(|n| n.to_str()) != Some("Contents") {
        return None;
    }
    let bundle_dir = ancestors.next()?;
    if bundle_dir
        .extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| e.eq_ignore_ascii_case("app"))
    {
        Some(bundle_dir.to_path_buf())
    } else {
        None
    }
}

#[cfg(any(target_os = "macos", test))]
fn copy_dir_recursive(src: &Path, dst: &Path) -> io::Result<()> {
    fs::create_dir_all(dst)?;
    for entry in fs::read_dir(src)? {
        let entry = entry?;
        let file_type = entry.file_type()?;
        let src_path = entry.path();
        let dst_path = dst.join(entry.file_name());
        if file_type.is_dir() {
            copy_dir_recursive(&src_path, &dst_path)?;
        } else if file_type.is_symlink() {
            #[cfg(unix)]
            {
                let target = fs::read_link(&src_path)?;
                std::os::unix::fs::symlink(target, &dst_path)?;
            }
            #[cfg(not(unix))]
            {
                fs::copy(&src_path, &dst_path)?;
            }
        } else {
            fs::copy(&src_path, &dst_path)?;
            #[cfg(unix)]
            {
                let perms = entry.metadata()?.permissions();
                let _ = fs::set_permissions(&dst_path, perms);
            }
        }
    }
    Ok(())
}

/// Atomically swaps `target_app` with `staged_app` on the same parent filesystem,
/// rolling back cleanly if the final rename fails.
#[cfg(any(target_os = "macos", test))]
pub(crate) fn swap_macos_bundle_dirs(
    target_app: &Path,
    staged_app: &Path,
) -> Result<(), ApplyError> {
    if !staged_app.is_dir() || !staged_app.join("Contents/MacOS").is_dir() {
        return Err(ApplyError::InvalidArtifact(format!(
            "Staged bundle {} is missing Contents/MacOS",
            staged_app.display()
        )));
    }
    if !target_app.is_dir() {
        return Err(ApplyError::BundleNotFound(target_app.to_path_buf()));
    }

    let parent = target_app
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .ok_or_else(|| ApplyError::BundleNotFound(target_app.to_path_buf()))?;
    let app_name = target_app
        .file_name()
        .and_then(|n| n.to_str())
        .ok_or_else(|| ApplyError::BundleNotFound(target_app.to_path_buf()))?;

    let pid = std::process::id();
    let temp_new = parent.join(format!(".{app_name}.new.{pid}"));
    let temp_bak = parent.join(format!(".{app_name}.bak.{pid}"));

    if temp_new.exists() {
        let _ = fs::remove_dir_all(&temp_new);
    }
    if temp_bak.exists() {
        let _ = fs::remove_dir_all(&temp_bak);
    }

    if fs::rename(staged_app, &temp_new).is_err() {
        copy_dir_recursive(staged_app, &temp_new).map_err(|err| {
            let _ = fs::remove_dir_all(&temp_new);
            ApplyError::ReplaceFailed(format!("Failed preparing bundle in target directory: {err}"))
        })?;
    }

    if let Err(err) = fs::rename(target_app, &temp_bak) {
        let _ = fs::remove_dir_all(&temp_new);
        return Err(ApplyError::ReplaceFailed(format!(
            "Failed moving existing bundle aside: {err}"
        )));
    }

    if let Err(err) = fs::rename(&temp_new, target_app) {
        let _ = fs::rename(&temp_bak, target_app);
        let _ = fs::remove_dir_all(&temp_new);
        return Err(ApplyError::ReplaceFailed(format!(
            "Failed moving new bundle into place (rolled back): {err}"
        )));
    }

    let _ = fs::remove_dir_all(&temp_bak);
    Ok(())
}

/// Applies a verified and extracted [`StagedUpdate`] in place.
pub fn apply_staged_update(staged: &StagedUpdate) -> Result<ApplyOutcome, ApplyError> {
    #[cfg(target_os = "macos")]
    {
        let current_exe = std::env::current_exe()?;
        let app_bundle = locate_macos_app_bundle(&current_exe)
            .ok_or_else(|| ApplyError::BundleNotFound(current_exe.clone()))?;
        swap_macos_bundle_dirs(&app_bundle, &staged.extracted_entry)?;
        let _ = fs::remove_dir_all(&staged.staging_dir);
        Ok(ApplyOutcome {
            relaunch_path: app_bundle,
        })
    }

    #[cfg(not(target_os = "macos"))]
    {
        if !staged.extracted_entry.is_file() {
            return Err(ApplyError::InvalidArtifact(format!(
                "Staged binary does not exist: {}",
                staged.extracted_entry.display()
            )));
        }

        #[cfg(target_os = "windows")]
        validate_windows_pe_header(&staged.extracted_entry)?;

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = fs::set_permissions(
                &staged.extracted_entry,
                fs::Permissions::from_mode(0o755),
            );
        }

        self_replace::self_replace(&staged.extracted_entry)
            .map_err(|err| ApplyError::ReplaceFailed(err.to_string()))?;

        let _ = fs::remove_dir_all(&staged.staging_dir);
        let current_exe = std::env::current_exe()?;
        Ok(ApplyOutcome {
            relaunch_path: current_exe,
        })
    }
}

/// Spawns a fresh instance of the updated application without waiting for it.
pub fn relaunch_updated_app(relaunch_path: &Path) -> io::Result<()> {
    #[cfg(target_os = "macos")]
    {
        Command::new("open").arg("-n").arg(relaunch_path).spawn()?;
        Ok(())
    }

    #[cfg(not(target_os = "macos"))]
    {
        Command::new(relaunch_path).spawn()?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_locate_macos_app_bundle() {
        assert_eq!(
            locate_macos_app_bundle(Path::new("/Applications/Inviscid.app/Contents/MacOS/inviscid")),
            Some(PathBuf::from("/Applications/Inviscid.app"))
        );
        assert_eq!(
            locate_macos_app_bundle(Path::new("/usr/local/bin/inviscid")),
            None
        );
    }

    #[test]
    fn test_swap_macos_bundle_dirs_replaces_atomically() {
        let dir = tempfile::tempdir().unwrap();
        let target_app = dir.path().join("Inviscid.app");
        let staged_app = dir.path().join("staging").join("Inviscid.app");

        fs::create_dir_all(target_app.join("Contents/MacOS")).unwrap();
        fs::write(target_app.join("Contents/MacOS/inviscid"), b"old_bin").unwrap();
        fs::write(target_app.join("Contents/Info.plist"), b"old_plist").unwrap();

        fs::create_dir_all(staged_app.join("Contents/MacOS")).unwrap();
        fs::write(staged_app.join("Contents/MacOS/inviscid"), b"new_bin").unwrap();
        fs::write(staged_app.join("Contents/Info.plist"), b"new_plist").unwrap();

        swap_macos_bundle_dirs(&target_app, &staged_app).unwrap();

        assert_eq!(
            fs::read(target_app.join("Contents/MacOS/inviscid")).unwrap(),
            b"new_bin"
        );
        assert_eq!(
            fs::read(target_app.join("Contents/Info.plist")).unwrap(),
            b"new_plist"
        );

        let leftover_temp = fs::read_dir(dir.path())
            .unwrap()
            .flatten()
            .any(|e| {
                let name = e.file_name().to_string_lossy().to_string();
                name.starts_with(".Inviscid.app.")
            });
        assert!(!leftover_temp);
    }

    #[test]
    fn test_swap_macos_bundle_dirs_rolls_back_on_missing_staged_contents() {
        let dir = tempfile::tempdir().unwrap();
        let target_app = dir.path().join("Inviscid.app");
        let bad_staged = dir.path().join("staging").join("Inviscid.app");

        fs::create_dir_all(target_app.join("Contents/MacOS")).unwrap();
        fs::write(target_app.join("Contents/MacOS/inviscid"), b"old_bin").unwrap();
        fs::create_dir_all(&bad_staged).unwrap(); // Missing Contents/MacOS

        let err = swap_macos_bundle_dirs(&target_app, &bad_staged).unwrap_err();
        assert!(matches!(err, ApplyError::InvalidArtifact(_)));
        assert_eq!(
            fs::read(target_app.join("Contents/MacOS/inviscid")).unwrap(),
            b"old_bin"
        );
    }

    #[test]
    fn test_validate_windows_pe_header_rejects_non_mz() {
        let dir = tempfile::tempdir().unwrap();
        let valid = dir.path().join("good.exe");
        let invalid = dir.path().join("bad.exe");
        fs::write(&valid, b"MZ\x90\x00").unwrap();
        fs::write(&invalid, b"#!/bin/sh").unwrap();

        assert!(validate_windows_pe_header(&valid).is_ok());
        assert!(matches!(
            validate_windows_pe_header(&invalid),
            Err(ApplyError::InvalidArtifact(_))
        ));
    }
}
