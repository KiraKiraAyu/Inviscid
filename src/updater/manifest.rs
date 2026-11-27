use std::collections::HashMap;
use std::fmt;

use semver::Version;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ArchiveFormat {
    Zip,
    #[serde(rename = "tar.gz")]
    TarGz,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TargetAsset {
    pub asset_name: String,
    pub size: u64,
    pub sha256: String,
    pub format: ArchiveFormat,
    pub entry: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct UpdateManifest {
    pub schema_version: u32,
    pub version: String,
    pub tag: String,
    pub pub_date: String,
    #[serde(default)]
    pub min_updater_version: Option<String>,
    pub release_notes_url: String,
    #[serde(default)]
    pub notes: HashMap<String, String>,
    pub targets: HashMap<String, TargetAsset>,
}

impl UpdateManifest {
    /// Returns localized release notes matching `locale_code`, falling back to `"en"` or any entry.
    pub fn localized_notes(&self, locale_code: &str) -> Option<&str> {
        self.notes
            .get(locale_code)
            .or_else(|| self.notes.get("en"))
            .or_else(|| self.notes.values().next())
            .map(String::as_str)
    }
}

/// Returns the Rust target triple corresponding to the current compilation target.
pub const fn current_target_triple() -> &'static str {
    #[cfg(all(target_os = "windows", target_arch = "x86_64"))]
    {
        "x86_64-pc-windows-msvc"
    }
    #[cfg(all(target_os = "windows", target_arch = "aarch64"))]
    {
        "aarch64-pc-windows-msvc"
    }
    #[cfg(all(target_os = "macos", target_arch = "aarch64"))]
    {
        "aarch64-apple-darwin"
    }
    #[cfg(all(target_os = "macos", target_arch = "x86_64"))]
    {
        "x86_64-apple-darwin"
    }
    #[cfg(all(target_os = "linux", target_arch = "x86_64"))]
    {
        "x86_64-unknown-linux-gnu"
    }
    #[cfg(all(target_os = "linux", target_arch = "aarch64"))]
    {
        "aarch64-unknown-linux-gnu"
    }
    #[cfg(not(any(
        all(target_os = "windows", target_arch = "x86_64"),
        all(target_os = "windows", target_arch = "aarch64"),
        all(target_os = "macos", target_arch = "aarch64"),
        all(target_os = "macos", target_arch = "x86_64"),
        all(target_os = "linux", target_arch = "x86_64"),
        all(target_os = "linux", target_arch = "aarch64"),
    )))]
    {
        "unknown"
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ManifestError {
    UnsupportedSchema(u32),
    InvalidJson(String),
    InvalidVersion(String),
    MissingTarget(String),
    InvalidSha256(String),
    InvalidAssetMetadata(String),
}

impl fmt::Display for ManifestError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnsupportedSchema(v) => write!(f, "Unsupported manifest schema version: {v}"),
            Self::InvalidJson(msg) => write!(f, "Invalid manifest JSON: {msg}"),
            Self::InvalidVersion(msg) => write!(f, "Invalid version string: {msg}"),
            Self::MissingTarget(triple) => {
                write!(f, "No update package available for target '{triple}'")
            }
            Self::InvalidSha256(sha) => write!(f, "Invalid SHA-256 digest in manifest: '{sha}'"),
            Self::InvalidAssetMetadata(msg) => write!(f, "Invalid target asset metadata: {msg}"),
        }
    }
}

impl std::error::Error for ManifestError {}

/// Parses raw manifest JSON and validates schema version and semantic version syntax.
pub fn parse_and_validate_manifest(raw_manifest: &[u8]) -> Result<UpdateManifest, ManifestError> {
    let manifest: UpdateManifest = serde_json::from_slice(raw_manifest)
        .map_err(|err| ManifestError::InvalidJson(err.to_string()))?;

    if manifest.schema_version != 1 {
        return Err(ManifestError::UnsupportedSchema(manifest.schema_version));
    }
    Version::parse(&manifest.version)
        .map_err(|err| ManifestError::InvalidVersion(format!("{}: {err}", manifest.version)))?;
    if let Some(ref min_ver) = manifest.min_updater_version {
        Version::parse(min_ver)
            .map_err(|err| ManifestError::InvalidVersion(format!("{min_ver}: {err}")))?;
    }

    Ok(manifest)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VersionComparison {
    UpToDate,
    UpdateAvailable { requires_manual_upgrade: bool },
}

/// Compares `current_version` against `manifest.version`, guarding against downgrades and
/// checking `min_updater_version`.
pub fn compare_manifest_version(
    current_version: &str,
    manifest: &UpdateManifest,
) -> Result<VersionComparison, ManifestError> {
    let current = Version::parse(current_version)
        .map_err(|err| ManifestError::InvalidVersion(format!("{current_version}: {err}")))?;
    let remote = Version::parse(&manifest.version)
        .map_err(|err| ManifestError::InvalidVersion(format!("{}: {err}", manifest.version)))?;

    if remote <= current {
        return Ok(VersionComparison::UpToDate);
    }

    let requires_manual_upgrade = match manifest.min_updater_version.as_deref() {
        Some(min_ver) => {
            let min = Version::parse(min_ver)
                .map_err(|err| ManifestError::InvalidVersion(format!("{min_ver}: {err}")))?;
            current < min
        }
        None => false,
    };

    Ok(VersionComparison::UpdateAvailable {
        requires_manual_upgrade,
    })
}

/// Selects and validates the target asset for `target_triple`.
pub fn select_target_asset<'a>(
    manifest: &'a UpdateManifest,
    target_triple: &str,
) -> Result<&'a TargetAsset, ManifestError> {
    let asset = manifest
        .targets
        .get(target_triple)
        .ok_or_else(|| ManifestError::MissingTarget(target_triple.to_string()))?;

    if asset.sha256.len() != 64 || !asset.sha256.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err(ManifestError::InvalidSha256(asset.sha256.clone()));
    }
    if asset.size == 0 || asset.asset_name.trim().is_empty() || asset.entry.trim().is_empty() {
        return Err(ManifestError::InvalidAssetMetadata(format!(
            "Target '{target_triple}' has empty name/entry or zero size"
        )));
    }

    Ok(asset)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_manifest_json(version: &str, min_updater: Option<&str>) -> Vec<u8> {
        let min_line = min_updater
            .map(|m| format!(r#""min_updater_version": "{m}","#))
            .unwrap_or_default();
        let json = format!(
            r#"{{
  "schema_version": 1,
  "version": "{version}",
  "tag": "v{version}",
  "pub_date": "2026-12-01T10:00:00Z",
  {min_line}
  "release_notes_url": "https://github.com/KiraKiraAyu/Inviscid/releases/tag/v{version}",
  "notes": {{
    "en": "English notes",
    "zh-Hans": "中文更新说明"
  }},
  "targets": {{
    "{triple}": {{
      "asset_name": "inviscid-v{version}.zip",
      "size": 1024,
      "sha256": "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
      "format": "zip",
      "entry": "inviscid.exe"
    }}
  }}
}}"#,
            triple = current_target_triple()
        );
        json.into_bytes()
    }

    #[test]
    fn test_parse_and_validate_manifest_success_and_rejections() {
        let raw = sample_manifest_json("0.2.0", None);
        let parsed = parse_and_validate_manifest(&raw).unwrap();
        assert_eq!(parsed.version, "0.2.0");
        assert_eq!(parsed.localized_notes("zh-Hans"), Some("中文更新说明"));
        assert_eq!(parsed.localized_notes("fr"), Some("English notes"));

        // Invalid JSON fails cleanly
        assert!(matches!(
            parse_and_validate_manifest(b"{not valid json"),
            Err(ManifestError::InvalidJson(_))
        ));

        // Unsupported schema version fails cleanly
        let raw_text = String::from_utf8(raw).unwrap();
        let bad_schema = raw_text.replacen(r#""schema_version": 1"#, r#""schema_version": 2"#, 1);
        assert!(matches!(
            parse_and_validate_manifest(bad_schema.as_bytes()),
            Err(ManifestError::UnsupportedSchema(2))
        ));

        // Invalid SemVer fails cleanly
        let bad_ver = sample_manifest_json("not-semver", None);
        assert!(matches!(
            parse_and_validate_manifest(&bad_ver),
            Err(ManifestError::InvalidVersion(_))
        ));
    }

    #[test]
    fn test_version_comparison_and_target_selection() {
        let raw_beta = sample_manifest_json("0.2.0-beta.1", None);
        let manifest = parse_and_validate_manifest(&raw_beta).unwrap();
        assert_eq!(
            compare_manifest_version("0.1.0", &manifest).unwrap(),
            VersionComparison::UpdateAvailable {
                requires_manual_upgrade: false
            }
        );
        assert_eq!(
            compare_manifest_version("0.2.0-beta.1", &manifest).unwrap(),
            VersionComparison::UpToDate
        );
        assert_eq!(
            compare_manifest_version("0.2.0", &manifest).unwrap(),
            VersionComparison::UpToDate
        );

        // min_updater_version enforcement
        let raw_min = sample_manifest_json("0.5.0", Some("0.3.0"));
        let min_manifest = parse_and_validate_manifest(&raw_min).unwrap();
        assert_eq!(
            compare_manifest_version("0.2.0", &min_manifest).unwrap(),
            VersionComparison::UpdateAvailable {
                requires_manual_upgrade: true
            }
        );

        let asset = select_target_asset(&min_manifest, current_target_triple()).unwrap();
        assert_eq!(asset.entry, "inviscid.exe");
        assert!(matches!(
            select_target_asset(&min_manifest, "missing-triple"),
            Err(ManifestError::MissingTarget(_))
        ));
    }
}
