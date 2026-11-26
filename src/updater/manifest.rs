use std::collections::HashMap;
use std::fmt;

use minisign_verify::{PublicKey, Signature};
use semver::Version;
use serde::{Deserialize, Serialize};

use super::config::UpdateChannel;

/// Embedded Minisign public keys trusted for release manifest verification.
///
/// Replace the placeholder key with the real public key generated via `minisign -G`
/// before publishing the first public release (see `docs/RELEASING.md`).
pub const TRUSTED_MINISIGN_PUBLIC_KEYS: &[&str] = &[
    // Placeholder key (all-zero Ed25519 bytes; syntactically valid, no private key exists).
    "RWQAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA",
];

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
    pub channel: UpdateChannel,
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
    InvalidSignature(String),
    InvalidJson(String),
    ChannelMismatch {
        expected: UpdateChannel,
        actual: UpdateChannel,
    },
    InvalidVersion(String),
    MissingTarget(String),
    InvalidSha256(String),
    InvalidAssetMetadata(String),
}

impl fmt::Display for ManifestError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnsupportedSchema(v) => write!(f, "Unsupported manifest schema version: {v}"),
            Self::InvalidSignature(msg) => write!(f, "Manifest signature verification failed: {msg}"),
            Self::InvalidJson(msg) => write!(f, "Invalid manifest JSON: {msg}"),
            Self::ChannelMismatch { expected, actual } => write!(
                f,
                "Manifest channel mismatch: expected '{}', got '{}'",
                expected.as_str(),
                actual.as_str()
            ),
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

/// Verifies the Minisign signature against `public_keys` BEFORE parsing JSON, then validates
/// schema version, channel match, and semantic version syntax.
pub fn verify_and_parse_manifest(
    raw_manifest: &[u8],
    raw_signature: &str,
    expected_channel: UpdateChannel,
    public_keys: &[&str],
) -> Result<UpdateManifest, ManifestError> {
    if public_keys.is_empty() {
        return Err(ManifestError::InvalidSignature(
            "No trusted public keys configured".to_string(),
        ));
    }

    let sig = Signature::decode(raw_signature)
        .map_err(|err| ManifestError::InvalidSignature(err.to_string()))?;

    let mut verified = false;
    let mut last_err = None;
    for key_b64 in public_keys {
        match PublicKey::from_base64(key_b64) {
            Ok(pk) => match pk.verify(raw_manifest, &sig, false) {
                Ok(()) => {
                    verified = true;
                    break;
                }
                Err(err) => last_err = Some(err.to_string()),
            },
            Err(err) => last_err = Some(err.to_string()),
        }
    }

    if !verified {
        return Err(ManifestError::InvalidSignature(
            last_err.unwrap_or_else(|| "Signature did not match any trusted key".to_string()),
        ));
    }

    let manifest: UpdateManifest = serde_json::from_slice(raw_manifest)
        .map_err(|err| ManifestError::InvalidJson(err.to_string()))?;

    if manifest.schema_version != 1 {
        return Err(ManifestError::UnsupportedSchema(manifest.schema_version));
    }
    if manifest.channel != expected_channel {
        return Err(ManifestError::ChannelMismatch {
            expected: expected_channel,
            actual: manifest.channel,
        });
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
pub(crate) mod test_signing {
    use ring::rand::SystemRandom;
    use ring::signature::{Ed25519KeyPair, KeyPair};

    const BASE64_ALPHABET: &[u8; 64] =
        b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

    pub(crate) fn encode_base64(input: &[u8]) -> String {
        let mut out = String::with_capacity(input.len().div_ceil(3) * 4);
        for chunk in input.chunks(3) {
            let b0 = chunk[0] as u32;
            let b1 = if chunk.len() > 1 { chunk[1] as u32 } else { 0 };
            let b2 = if chunk.len() > 2 { chunk[2] as u32 } else { 0 };
            let triple = (b0 << 16) | (b1 << 8) | b2;
            out.push(BASE64_ALPHABET[((triple >> 18) & 0x3F) as usize] as char);
            out.push(BASE64_ALPHABET[((triple >> 12) & 0x3F) as usize] as char);
            out.push(if chunk.len() > 1 {
                BASE64_ALPHABET[((triple >> 6) & 0x3F) as usize] as char
            } else {
                '='
            });
            out.push(if chunk.len() > 2 {
                BASE64_ALPHABET[(triple & 0x3F) as usize] as char
            } else {
                '='
            });
        }
        out
    }

    /// Minimal RFC 7693 unkeyed BLAKE2b-512 implementation used strictly by tests to create
    /// pre-hashed Minisign signatures compatible with `minisign_verify::PublicKey::verify(..., false)`.
    pub(crate) fn blake2b_512(data: &[u8]) -> [u8; 64] {
        const IV: [u64; 8] = [
            0x6a09e667f3bcc908,
            0xbb67ae8584caa73b,
            0x3c6ef372fe94f82b,
            0xa54ff53a5f1d36f1,
            0x510e527fade682d1,
            0x9b05688c2b3e6c1f,
            0x1f83d9abfb41bd6b,
            0x5be0cd19137e2179,
        ];
        const SIGMA: [[usize; 16]; 12] = [
            [0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15],
            [14, 10, 4, 8, 9, 15, 13, 6, 1, 12, 0, 2, 11, 7, 5, 3],
            [11, 8, 12, 0, 5, 2, 15, 13, 10, 14, 3, 6, 7, 1, 9, 4],
            [7, 9, 3, 1, 13, 12, 11, 14, 2, 6, 5, 10, 4, 0, 15, 8],
            [9, 0, 5, 7, 2, 4, 10, 15, 14, 1, 11, 12, 6, 8, 3, 13],
            [2, 12, 6, 10, 0, 11, 8, 3, 4, 13, 7, 5, 15, 14, 1, 9],
            [12, 5, 1, 15, 14, 13, 4, 10, 0, 7, 6, 3, 9, 2, 8, 11],
            [13, 11, 7, 14, 12, 1, 3, 9, 5, 0, 15, 4, 8, 6, 2, 10],
            [6, 15, 14, 9, 11, 3, 0, 8, 12, 2, 13, 7, 1, 4, 10, 5],
            [10, 2, 8, 4, 7, 6, 1, 5, 15, 11, 9, 14, 3, 12, 13, 0],
            [0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15],
            [14, 10, 4, 8, 9, 15, 13, 6, 1, 12, 0, 2, 11, 7, 5, 3],
        ];

        fn g(v: &mut [u64; 16], a: usize, b: usize, c: usize, d: usize, x: u64, y: u64) {
            v[a] = v[a].wrapping_add(v[b]).wrapping_add(x);
            v[d] = (v[d] ^ v[a]).rotate_right(32);
            v[c] = v[c].wrapping_add(v[d]);
            v[b] = (v[b] ^ v[c]).rotate_right(24);
            v[a] = v[a].wrapping_add(v[b]).wrapping_add(y);
            v[d] = (v[d] ^ v[a]).rotate_right(16);
            v[c] = v[c].wrapping_add(v[d]);
            v[b] = (v[b] ^ v[c]).rotate_right(63);
        }

        fn compress(h: &mut [u64; 8], block: &[u8; 128], t: u128, last: bool) {
            let mut m = [0u64; 16];
            for (i, chunk) in block.chunks_exact(8).enumerate() {
                m[i] = u64::from_le_bytes(chunk.try_into().unwrap());
            }
            let mut v = [0u64; 16];
            v[..8].copy_from_slice(h);
            v[8..16].copy_from_slice(&IV);
            v[12] ^= t as u64;
            v[13] ^= (t >> 64) as u64;
            if last {
                v[14] = !v[14];
            }
            for s in &SIGMA {
                g(&mut v, 0, 4, 8, 12, m[s[0]], m[s[1]]);
                g(&mut v, 1, 5, 9, 13, m[s[2]], m[s[3]]);
                g(&mut v, 2, 6, 10, 14, m[s[4]], m[s[5]]);
                g(&mut v, 3, 7, 11, 15, m[s[6]], m[s[7]]);
                g(&mut v, 0, 5, 10, 15, m[s[8]], m[s[9]]);
                g(&mut v, 1, 6, 11, 12, m[s[10]], m[s[11]]);
                g(&mut v, 2, 7, 8, 13, m[s[12]], m[s[13]]);
                g(&mut v, 3, 4, 9, 14, m[s[14]], m[s[15]]);
            }
            for i in 0..8 {
                h[i] ^= v[i] ^ v[i + 8];
            }
        }

        let mut h = IV;
        h[0] ^= 0x0101_0040; // fanout=1, depth=1, digest_len=64

        if data.is_empty() {
            compress(&mut h, &[0u8; 128], 0, true);
        } else {
            let chunks = data.chunks_exact(128);
            let rem = chunks.remainder();
            let mut offset: u128 = 0;
            let full_blocks = data.len() / 128;
            for (idx, block) in chunks.enumerate() {
                offset += 128;
                let is_last = rem.is_empty() && idx + 1 == full_blocks;
                compress(&mut h, block.try_into().unwrap(), offset, is_last);
            }
            if !rem.is_empty() {
                let mut padded = [0u8; 128];
                padded[..rem.len()].copy_from_slice(rem);
                offset += rem.len() as u128;
                compress(&mut h, &padded, offset, true);
            }
        }

        let mut out = [0u8; 64];
        for (i, word) in h.iter().enumerate() {
            out[i * 8..(i + 1) * 8].copy_from_slice(&word.to_le_bytes());
        }
        out
    }

    pub(crate) struct TestMinisignSigner {
        key_pair: Ed25519KeyPair,
        key_id: [u8; 8],
        pub(crate) public_key_b64: String,
    }

    impl TestMinisignSigner {
        pub(crate) fn generate() -> Self {
            let rng = SystemRandom::new();
            let pkcs8 = Ed25519KeyPair::generate_pkcs8(&rng).unwrap();
            let key_pair = Ed25519KeyPair::from_pkcs8(pkcs8.as_ref()).unwrap();
            let key_id = [0x1A, 0x2B, 0x3C, 0x4D, 0x5E, 0x6F, 0x70, 0x81];

            let mut pk_bytes = Vec::with_capacity(42);
            pk_bytes.extend_from_slice(&[0x45, 0x64]); // "Ed"
            pk_bytes.extend_from_slice(&key_id);
            pk_bytes.extend_from_slice(key_pair.public_key().as_ref());

            Self {
                key_pair,
                key_id,
                public_key_b64: encode_base64(&pk_bytes),
            }
        }

        pub(crate) fn sign_prehashed(&self, data: &[u8], filename: &str) -> String {
            let digest = blake2b_512(data);
            let sig1 = self.key_pair.sign(&digest);

            let trusted_comment = format!("timestamp:1700000000\tfile:{filename}\thashed");
            let mut global_msg = Vec::with_capacity(64 + trusted_comment.len());
            global_msg.extend_from_slice(sig1.as_ref());
            global_msg.extend_from_slice(trusted_comment.as_bytes());
            let sig2 = self.key_pair.sign(&global_msg);

            let mut bin1 = Vec::with_capacity(74);
            bin1.extend_from_slice(&[0x45, 0x44]); // "ED" (prehashed)
            bin1.extend_from_slice(&self.key_id);
            bin1.extend_from_slice(sig1.as_ref());

            format!(
                "untrusted comment: signature from test minisign key\n{}\ntrusted comment: {}\n{}\n",
                encode_base64(&bin1),
                trusted_comment,
                encode_base64(sig2.as_ref())
            )
        }
    }
}

#[cfg(test)]
mod tests {
    use super::test_signing::TestMinisignSigner;
    use super::*;

    fn sample_manifest_json(channel: &str, version: &str, min_updater: Option<&str>) -> Vec<u8> {
        let min_line = min_updater
            .map(|m| format!(r#""min_updater_version": "{m}","#))
            .unwrap_or_default();
        let json = format!(
            r#"{{
  "schema_version": 1,
  "channel": "{channel}",
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
    fn test_placeholder_public_key_parses() {
        for key in TRUSTED_MINISIGN_PUBLIC_KEYS {
            PublicKey::from_base64(key).expect("trusted minisign public key must decode");
        }
    }

    #[test]
    fn test_verify_and_parse_manifest_success_and_tamper_rejection() {
        let signer = TestMinisignSigner::generate();
        let raw = sample_manifest_json("stable", "0.2.0", None);
        let sig = signer.sign_prehashed(&raw, "update-stable.json");

        let parsed = verify_and_parse_manifest(
            &raw,
            &sig,
            UpdateChannel::Stable,
            &[&signer.public_key_b64],
        )
        .unwrap();
        assert_eq!(parsed.version, "0.2.0");
        assert_eq!(parsed.localized_notes("zh-Hans"), Some("中文更新说明"));
        assert_eq!(parsed.localized_notes("fr"), Some("English notes"));

        // Tampered payload must fail BEFORE parsing JSON (even if invalid JSON)
        let mut tampered_raw = raw.clone();
        tampered_raw[5] = b'X';
        assert!(matches!(
            verify_and_parse_manifest(
                &tampered_raw,
                &sig,
                UpdateChannel::Stable,
                &[&signer.public_key_b64]
            ),
            Err(ManifestError::InvalidSignature(_))
        ));

        // Tampered signature must fail (deterministically mutate a byte in the signature line)
        let mut sig_lines: Vec<String> = sig.lines().map(str::to_string).collect();
        let mut sig_bytes = sig_lines[1].as_bytes().to_vec();
        sig_bytes[20] = if sig_bytes[20] == b'A' { b'B' } else { b'A' };
        sig_lines[1] = String::from_utf8(sig_bytes).unwrap();
        let tampered_sig = format!("{}\n", sig_lines.join("\n"));
        assert!(matches!(
            verify_and_parse_manifest(
                &raw,
                &tampered_sig,
                UpdateChannel::Stable,
                &[&signer.public_key_b64]
            ),
            Err(ManifestError::InvalidSignature(_))
        ));

        // Wrong public key must fail
        let other = TestMinisignSigner::generate();
        assert!(matches!(
            verify_and_parse_manifest(
                &raw,
                &sig,
                UpdateChannel::Stable,
                &[&other.public_key_b64]
            ),
            Err(ManifestError::InvalidSignature(_))
        ));
    }

    #[test]
    fn test_manifest_channel_mismatch_and_version_comparison() {
        let signer = TestMinisignSigner::generate();
        let raw_beta = sample_manifest_json("beta", "0.2.0-beta.1", None);
        let sig_beta = signer.sign_prehashed(&raw_beta, "update-beta.json");

        let err = verify_and_parse_manifest(
            &raw_beta,
            &sig_beta,
            UpdateChannel::Stable,
            &[&signer.public_key_b64],
        )
        .unwrap_err();
        assert!(matches!(
            err,
            ManifestError::ChannelMismatch {
                expected: UpdateChannel::Stable,
                actual: UpdateChannel::Beta
            }
        ));

        let manifest = verify_and_parse_manifest(
            &raw_beta,
            &sig_beta,
            UpdateChannel::Beta,
            &[&signer.public_key_b64],
        )
        .unwrap();
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
        let raw_min = sample_manifest_json("stable", "0.5.0", Some("0.3.0"));
        let sig_min = signer.sign_prehashed(&raw_min, "update-stable.json");
        let min_manifest = verify_and_parse_manifest(
            &raw_min,
            &sig_min,
            UpdateChannel::Stable,
            &[&signer.public_key_b64],
        )
        .unwrap();
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
