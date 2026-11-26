use serde::{Deserialize, Serialize};

/// Release channel tracked by the updater.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum UpdateChannel {
    #[default]
    Stable,
    Beta,
}

impl UpdateChannel {
    #[inline]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Stable => "stable",
            Self::Beta => "beta",
        }
    }

    #[inline]
    pub const fn channel_tag(self) -> &'static str {
        match self {
            Self::Stable => "channel-stable",
            Self::Beta => "channel-beta",
        }
    }
}

/// Configured download origin for release manifests and archives.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum UpdateSource {
    #[default]
    GitHub,
    Disabled,
    Custom,
}

impl UpdateSource {
    pub const GITHUB_BASE_URL: &'static str =
        "https://github.com/KiraKiraAyu/Inviscid/releases/download";

    /// Resolves the effective base URL for this source and custom URL.
    #[inline]
    pub fn resolve_base_url<'a>(self, custom_url: &'a str) -> &'a str {
        match self {
            Self::GitHub => Self::GITHUB_BASE_URL,
            Self::Disabled => "",
            Self::Custom => custom_url,
        }
    }
}

/// Trims surrounding whitespace and trailing slashes from an update base URL.
pub fn normalize_update_base_url(url: &str) -> String {
    url.trim().trim_end_matches('/').to_string()
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

pub fn manifest_url(base_url: &str, channel: UpdateChannel) -> String {
    let base = normalize_update_base_url(base_url);
    format!(
        "{}/{}/update-{}.json",
        base,
        channel.channel_tag(),
        channel.as_str()
    )
}

pub fn signature_url(base_url: &str, channel: UpdateChannel) -> String {
    let base = normalize_update_base_url(base_url);
    format!(
        "{}/{}/update-{}.json.minisig",
        base,
        channel.channel_tag(),
        channel.as_str()
    )
}

pub fn asset_url(base_url: &str, tag: &str, asset_name: &str) -> String {
    let base = normalize_update_base_url(base_url);
    let clean_tag = tag.trim_matches('/');
    let clean_asset = asset_name.trim_start_matches('/');
    format!("{base}/{clean_tag}/{clean_asset}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_channel_tags_and_url_builders() {
        assert_eq!(UpdateChannel::Stable.as_str(), "stable");
        assert_eq!(UpdateChannel::Stable.channel_tag(), "channel-stable");
        assert_eq!(UpdateChannel::Beta.as_str(), "beta");
        assert_eq!(UpdateChannel::Beta.channel_tag(), "channel-beta");

        assert_eq!(
            manifest_url(
                "https://github.com/KiraKiraAyu/Inviscid/releases/download/",
                UpdateChannel::Stable
            ),
            "https://github.com/KiraKiraAyu/Inviscid/releases/download/channel-stable/update-stable.json"
        );
        assert_eq!(
            signature_url("https://mirror.example.com/inviscid/", UpdateChannel::Beta),
            "https://mirror.example.com/inviscid/channel-beta/update-beta.json.minisig"
        );
        assert_eq!(
            asset_url(
                "https://mirror.example.com/inviscid/",
                "/v0.2.0/",
                "/inviscid-v0.2.0.zip"
            ),
            "https://mirror.example.com/inviscid/v0.2.0/inviscid-v0.2.0.zip"
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
    fn test_source_resolution_and_normalization() {
        assert_eq!(
            UpdateSource::GitHub.resolve_base_url("https://unused"),
            UpdateSource::GITHUB_BASE_URL
        );
        assert_eq!(UpdateSource::Disabled.resolve_base_url("https://unused"), "");
        assert_eq!(
            UpdateSource::Custom.resolve_base_url("https://mirror.example.com"),
            "https://mirror.example.com"
        );
        assert_eq!(
            normalize_update_base_url("  https://mirror.example.com///  "),
            "https://mirror.example.com"
        );
    }
}
