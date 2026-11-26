use std::path::Path;

/// Environment variable allowing packagers/distros to override or disable in-place self-updates
/// with a custom upgrade explanation (following Zed's `ZED_UPDATE_EXPLANATION` pattern).
pub const UPDATE_EXPLANATION_ENV: &str = "INVISCID_UPDATE_EXPLANATION";

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ManualUpdateReason {
    CompileTimeDisabled,
    MinUpdaterVersion,
    ExternalExplanation(String),
    DevBuild,
    PackageManager(String),
    TranslocatedOrReadOnly(String),
    ReadOnlyLocation,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InstallCapability {
    SelfUpdatable,
    ManualOnly(ManualUpdateReason),
}

fn has_ancestor_cargo_toml(path: &Path) -> bool {
    path.ancestors()
        .skip(1)
        .take(5)
        .any(|dir| dir.join("Cargo.toml").is_file())
}

fn is_dir_writable_probe(dir: &Path) -> bool {
    tempfile::NamedTempFile::new_in(dir).is_ok()
}

fn target_writable_dir(exe_path: &Path) -> Option<&Path> {
    // On macOS app bundles (`.../Inviscid.app/Contents/MacOS/inviscid`), we swap the whole
    // `.app` bundle directory, so the parent of `Inviscid.app` must be writable.
    let mut ancestors = exe_path.ancestors();
    let _exe = ancestors.next()?;
    let parent = ancestors.next()?;
    if parent.file_name().and_then(|n| n.to_str()) == Some("MacOS")
        && let Some(contents) = ancestors.next()
        && contents.file_name().and_then(|n| n.to_str()) == Some("Contents")
        && let Some(bundle) = ancestors.next()
        && bundle
            .extension()
            .and_then(|e| e.to_str())
            .is_some_and(|e| e.eq_ignore_ascii_case("app"))
        && let Some(bundle_parent) = bundle.parent()
    {
        return Some(bundle_parent);
    }
    Some(parent)
}

/// Detects whether the current running executable can safely replace itself in place.
pub fn detect_install_capability() -> InstallCapability {
    if cfg!(feature = "disable-self-update") {
        return InstallCapability::ManualOnly(ManualUpdateReason::CompileTimeDisabled);
    }

    if let Ok(explanation) = std::env::var(UPDATE_EXPLANATION_ENV) {
        let trimmed = explanation.trim();
        if !trimmed.is_empty() {
            return InstallCapability::ManualOnly(ManualUpdateReason::ExternalExplanation(
                trimmed.to_string(),
            ));
        }
    }

    let Ok(exe_path) = std::env::current_exe() else {
        return InstallCapability::ManualOnly(ManualUpdateReason::ReadOnlyLocation);
    };

    detect_install_capability_for(
        &exe_path,
        cfg!(debug_assertions),
        cfg!(target_os = "windows"),
        cfg!(target_os = "macos"),
        cfg!(target_os = "linux"),
        Path::new("/.flatpak-info").exists(),
        std::env::var_os("SNAP").is_some(),
        is_dir_writable_probe,
    )
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn detect_install_capability_for(
    exe_path: &Path,
    is_debug_build: bool,
    is_windows: bool,
    is_macos: bool,
    is_linux: bool,
    has_flatpak_info: bool,
    has_snap_env: bool,
    mut is_dir_writable: impl FnMut(&Path) -> bool,
) -> InstallCapability {
    let raw = exe_path.to_string_lossy();
    let normalized_slash = raw.replace('\\', "/");
    let lower = normalized_slash.to_ascii_lowercase();

    if is_debug_build
        || ((lower.contains("/target/debug/") || lower.contains("/target/release/"))
            && has_ancestor_cargo_toml(exe_path))
    {
        return InstallCapability::ManualOnly(ManualUpdateReason::DevBuild);
    }

    if is_windows {
        if lower.contains("/scoop/apps/") {
            return InstallCapability::ManualOnly(ManualUpdateReason::PackageManager(
                "Scoop".to_string(),
            ));
        }
        if lower.contains("/chocolatey/") {
            return InstallCapability::ManualOnly(ManualUpdateReason::PackageManager(
                "Chocolatey".to_string(),
            ));
        }
        if lower.contains("/windowsapps/") {
            return InstallCapability::ManualOnly(ManualUpdateReason::PackageManager(
                "Microsoft Store".to_string(),
            ));
        }
    }

    if is_macos {
        if normalized_slash.contains("/AppTranslocation/") {
            return InstallCapability::ManualOnly(ManualUpdateReason::TranslocatedOrReadOnly(
                "App Translocation".to_string(),
            ));
        }
        if normalized_slash.starts_with("/Volumes/") {
            return InstallCapability::ManualOnly(ManualUpdateReason::TranslocatedOrReadOnly(
                "Mounted Disk Image".to_string(),
            ));
        }
        if normalized_slash.starts_with("/opt/homebrew/")
            || normalized_slash.starts_with("/usr/local/Caskroom/")
        {
            return InstallCapability::ManualOnly(ManualUpdateReason::PackageManager(
                "Homebrew".to_string(),
            ));
        }
    }

    if is_linux {
        if has_flatpak_info {
            return InstallCapability::ManualOnly(ManualUpdateReason::PackageManager(
                "Flatpak".to_string(),
            ));
        }
        if has_snap_env || normalized_slash.starts_with("/snap/") {
            return InstallCapability::ManualOnly(ManualUpdateReason::PackageManager(
                "Snap".to_string(),
            ));
        }
        if normalized_slash.starts_with("/nix/store/") {
            return InstallCapability::ManualOnly(ManualUpdateReason::PackageManager(
                "Nix".to_string(),
            ));
        }
        if normalized_slash.starts_with("/usr/") || normalized_slash.starts_with("/opt/") {
            return InstallCapability::ManualOnly(ManualUpdateReason::ReadOnlyLocation);
        }
    }

    let Some(writable_dir) = target_writable_dir(exe_path) else {
        return InstallCapability::ManualOnly(ManualUpdateReason::ReadOnlyLocation);
    };

    if !is_dir_writable(writable_dir) {
        return InstallCapability::ManualOnly(ManualUpdateReason::ReadOnlyLocation);
    }

    InstallCapability::SelfUpdatable
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_detect_install_capability_matrix() {
        // Dev build
        assert_eq!(
            detect_install_capability_for(
                Path::new("C:/Apps/Inviscid/inviscid.exe"),
                true,
                true,
                false,
                false,
                false,
                false,
                |_| true
            ),
            InstallCapability::ManualOnly(ManualUpdateReason::DevBuild)
        );

        // Windows Scoop / Chocolatey / Store / ReadOnly / Writable
        assert_eq!(
            detect_install_capability_for(
                Path::new("C:/Users/me/scoop/apps/inviscid/current/inviscid.exe"),
                false,
                true,
                false,
                false,
                false,
                false,
                |_| true
            ),
            InstallCapability::ManualOnly(ManualUpdateReason::PackageManager("Scoop".into()))
        );
        assert_eq!(
            detect_install_capability_for(
                Path::new("C:/Program Files/Inviscid/inviscid.exe"),
                false,
                true,
                false,
                false,
                false,
                false,
                |_| false
            ),
            InstallCapability::ManualOnly(ManualUpdateReason::ReadOnlyLocation)
        );
        assert_eq!(
            detect_install_capability_for(
                Path::new("D:/Portable/Inviscid/inviscid.exe"),
                false,
                true,
                false,
                false,
                false,
                false,
                |_| true
            ),
            InstallCapability::SelfUpdatable
        );

        // macOS AppTranslocation / Volumes / Homebrew / /Applications writable
        assert!(matches!(
            detect_install_capability_for(
                Path::new("/private/var/folders/xx/AppTranslocation/123/d/Inviscid.app/Contents/MacOS/inviscid"),
                false,
                false,
                true,
                false,
                false,
                false,
                |_| true
            ),
            InstallCapability::ManualOnly(ManualUpdateReason::TranslocatedOrReadOnly(_))
        ));
        assert_eq!(
            detect_install_capability_for(
                Path::new("/opt/homebrew/Caskroom/inviscid/0.1.0/Inviscid.app/Contents/MacOS/inviscid"),
                false,
                false,
                true,
                false,
                false,
                false,
                |_| true
            ),
            InstallCapability::ManualOnly(ManualUpdateReason::PackageManager("Homebrew".into()))
        );
        let mut checked_dir = None;
        let mac_cap = detect_install_capability_for(
            Path::new("/Applications/Inviscid.app/Contents/MacOS/inviscid"),
            false,
            false,
            true,
            false,
            false,
            false,
            |dir| {
                checked_dir = Some(dir.to_path_buf());
                true
            },
        );
        assert_eq!(mac_cap, InstallCapability::SelfUpdatable);
        assert_eq!(checked_dir.as_deref(), Some(Path::new("/Applications")));

        // Linux Flatpak / /usr / ~/.local/bin
        assert_eq!(
            detect_install_capability_for(
                Path::new("/app/bin/inviscid"),
                false,
                false,
                false,
                true,
                true,
                false,
                |_| true
            ),
            InstallCapability::ManualOnly(ManualUpdateReason::PackageManager("Flatpak".into()))
        );
        assert_eq!(
            detect_install_capability_for(
                Path::new("/usr/bin/inviscid"),
                false,
                false,
                false,
                true,
                false,
                false,
                |_| true
            ),
            InstallCapability::ManualOnly(ManualUpdateReason::ReadOnlyLocation)
        );
        assert_eq!(
            detect_install_capability_for(
                Path::new("/home/user/.local/bin/inviscid"),
                false,
                false,
                false,
                true,
                false,
                false,
                |_| true
            ),
            InstallCapability::SelfUpdatable
        );
    }
}
