use serde::{Deserialize, Serialize};

pub const UPDATES_RELEASE_TAG: &str = "updates";
pub const MANIFEST_FILENAME: &str = "update.json";

/// Configured download origin for release manifests and archives.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum UpdateSource {
    Disabled,
    #[default]
    #[serde(other)]
    GitHub,
}

impl UpdateSource {
    pub const GITHUB_BASE_URL: &'static str =
        "https://github.com/KiraKiraAyu/Inviscid/releases/download";

    /// Resolves the effective base URL for this source.
    #[inline]
    pub fn resolve_base_url(self) -> &'static str {
        match self {
            Self::GitHub => Self::GITHUB_BASE_URL,
            Self::Disabled => "",
        }
    }
}

/// Validates that an update base URL uses `https://` (with localhost allowed in unit tests).
pub fn validate_update_base_url(url: &str) -> Result<(), String> {
    let trimmed = url.trim();
    if trimmed.is_empty() {
        return Err("Update base URL cannot be empty".to_string());
    }
    if trimmed.starts_with("https://") {
        return Ok(());
    }
    #[cfg(test)]
    if trimmed.starts_with("http://127.0.0.1") || trimmed.starts_with("http://localhost") {
        return Ok(());
    }
    Err("Update URL must use HTTPS".to_string())
}

pub fn manifest_url(base_url: &str) -> String {
    let base = base_url.trim().trim_end_matches('/');
    format!("{base}/{UPDATES_RELEASE_TAG}/{MANIFEST_FILENAME}")
}

pub fn asset_url(base_url: &str, tag: &str, asset_name: &str) -> String {
    let base = base_url.trim().trim_end_matches('/');
    let clean_tag = tag.trim_matches('/');
    let clean_asset = asset_name.trim_start_matches('/');
    format!("{base}/{clean_tag}/{clean_asset}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_url_builders() {
        assert_eq!(
            manifest_url("https://github.com/KiraKiraAyu/Inviscid/releases/download/"),
            "https://github.com/KiraKiraAyu/Inviscid/releases/download/updates/update.json"
        );
        assert_eq!(
            asset_url(
                "https://github.com/KiraKiraAyu/Inviscid/releases/download/",
                "/v0.2.0/",
                "/inviscid-v0.2.0.zip"
            ),
            "https://github.com/KiraKiraAyu/Inviscid/releases/download/v0.2.0/inviscid-v0.2.0.zip"
        );
    }

    #[test]
    fn test_validate_update_base_url() {
        assert!(validate_update_base_url("https://example.com/releases").is_ok());
        assert!(validate_update_base_url("http://127.0.0.1:8080").is_ok());
        assert!(validate_update_base_url("http://example.com/releases").is_err());
        assert!(validate_update_base_url("").is_err());
        assert!(validate_update_base_url("   ").is_err());
    }

    #[test]
    fn test_source_resolution_and_legacy_fallback() {
        #[derive(Serialize, Deserialize)]
        struct Wrapper {
            update_source: UpdateSource,
        }

        assert_eq!(
            UpdateSource::GitHub.resolve_base_url(),
            UpdateSource::GITHUB_BASE_URL
        );
        assert_eq!(UpdateSource::Disabled.resolve_base_url(), "");

        let legacy: Wrapper = toml::from_str("update_source = \"custom\"\n").unwrap();
        assert_eq!(legacy.update_source, UpdateSource::GitHub);

        let serialized = toml::to_string(&Wrapper {
            update_source: UpdateSource::GitHub,
        })
        .unwrap();
        assert_eq!(serialized.trim(), "update_source = \"github\"");
    }
}
