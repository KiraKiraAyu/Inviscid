pub mod archive;
pub mod config;
pub mod install_env;
pub mod manifest;

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use futures::channel::mpsc;
use parking_lot::Mutex;

use crate::http::{
    DownloadOptions, DownloadProgress, download_to_file_streaming, fetch_bytes_sync,
};

pub use config::{
    UpdateChannel, UpdateSource, asset_url, manifest_url, normalize_update_base_url,
    signature_url, validate_update_base_url,
};
pub use install_env::{
    InstallCapability, ManualUpdateReason, UPDATE_EXPLANATION_ENV, detect_install_capability,
};
pub use manifest::{
    ArchiveFormat, ManifestError, TRUSTED_MINISIGN_PUBLIC_KEYS, TargetAsset, UpdateManifest,
    VersionComparison, compare_manifest_version, current_target_triple, select_target_asset,
    verify_and_parse_manifest,
};

const MANIFEST_MAX_BYTES: u64 = 1024 * 1024; // 1 MiB
const SIGNATURE_MAX_BYTES: u64 = 64 * 1024; // 64 KiB
const MANIFEST_FETCH_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(15);
const AUTO_CHECK_INTERVAL_SECS: u64 = 24 * 60 * 60;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReleaseInfo {
    pub version: String,
    pub tag: String,
    pub channel: UpdateChannel,
    pub pub_date: String,
    pub release_notes_url: String,
    pub notes: HashMap<String, String>,
    pub target: TargetAsset,
    pub download_url: String,
}

impl ReleaseInfo {
    pub fn localized_notes(&self, locale_code: &str) -> Option<&str> {
        self.notes
            .get(locale_code)
            .or_else(|| self.notes.get("en"))
            .or_else(|| self.notes.values().next())
            .map(String::as_str)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StagedUpdate {
    pub release: ReleaseInfo,
    pub staging_dir: PathBuf,
    pub extracted_entry: PathBuf,
}

#[derive(Debug, Clone, PartialEq)]
pub enum UpdateStatus {
    Idle,
    Checking {
        manual: bool,
    },
    UpToDate {
        checked_at_epoch: u64,
        manual: bool,
    },
    Available(ReleaseInfo),
    ManualUpdateOnly {
        release: ReleaseInfo,
        reason: ManualUpdateReason,
    },
    Downloading {
        release: ReleaseInfo,
        downloaded_bytes: u64,
        total_bytes: Option<u64>,
        bytes_per_sec: u64,
    },
    Verifying {
        release: ReleaseInfo,
    },
    Extracting {
        release: ReleaseInfo,
    },
    ReadyToInstall(StagedUpdate),
    Applying {
        version: String,
    },
    Failed {
        message: String,
        manual: bool,
    },
}

struct UpdateManagerInner {
    status: UpdateStatus,
    banner_dismissed: bool,
    active_cancel: Option<Arc<AtomicBool>>,
    cache_dir: Option<PathBuf>,
    subscribers: Vec<mpsc::UnboundedSender<()>>,
}

/// Thread-safe application update coordinator decoupled from GPUI UI views.
#[derive(Clone)]
pub struct UpdateManager {
    inner: Arc<Mutex<UpdateManagerInner>>,
    version: Arc<AtomicU64>,
}

impl gpui::Global for UpdateManager {}

impl Default for UpdateManager {
    fn default() -> Self {
        Self::new(dirs::cache_dir().map(|p| p.join("inviscid").join("updates")))
    }
}

impl UpdateManager {
    pub fn new(cache_dir: Option<PathBuf>) -> Self {
        Self {
            inner: Arc::new(Mutex::new(UpdateManagerInner {
                status: UpdateStatus::Idle,
                banner_dismissed: false,
                active_cancel: None,
                cache_dir,
                subscribers: Vec::new(),
            })),
            version: Arc::new(AtomicU64::new(0)),
        }
    }

    pub fn current_unix_epoch_secs() -> u64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs()
    }

    /// Returns `true` when enough time (>= 24 hours) has elapsed since `last_check_epoch`.
    pub fn should_auto_check(last_check_epoch: Option<u64>, now_epoch: u64) -> bool {
        match last_check_epoch {
            None => true,
            Some(prev) => now_epoch.saturating_sub(prev) >= AUTO_CHECK_INTERVAL_SECS,
        }
    }

    pub fn status(&self) -> UpdateStatus {
        self.inner.lock().status.clone()
    }

    pub fn version(&self) -> u64 {
        self.version.load(Ordering::Acquire)
    }

    pub fn is_banner_dismissed(&self) -> bool {
        self.inner.lock().banner_dismissed
    }

    pub fn dismiss_banner(&self) {
        let mut inner = self.inner.lock();
        if !inner.banner_dismissed {
            inner.banner_dismissed = true;
            Self::notify_locked(&mut inner, &self.version);
        }
    }

    pub fn restore_banner(&self) {
        let mut inner = self.inner.lock();
        if inner.banner_dismissed {
            inner.banner_dismissed = false;
            Self::notify_locked(&mut inner, &self.version);
        }
    }

    pub fn subscribe(&self) -> mpsc::UnboundedReceiver<()> {
        let (tx, rx) = mpsc::unbounded();
        self.inner.lock().subscribers.push(tx);
        rx
    }

    fn notify_locked(inner: &mut UpdateManagerInner, version: &AtomicU64) {
        version.fetch_add(1, Ordering::Release);
        inner.subscribers.retain(|tx| tx.unbounded_send(()).is_ok());
    }

    fn set_status(&self, status: UpdateStatus) {
        let mut inner = self.inner.lock();
        inner.status = status;
        Self::notify_locked(&mut inner, &self.version);
    }

    /// Starts a background update check unless a check/download/apply is already active.
    pub fn start_check(
        &self,
        source: UpdateSource,
        custom_url: String,
        channel: UpdateChannel,
        skipped_version: Option<String>,
        manual: bool,
    ) {
        if source == UpdateSource::Disabled {
            if manual {
                self.set_status(UpdateStatus::Failed {
                    message: "Update source is disabled".to_string(),
                    manual: true,
                });
            }
            return;
        }

        {
            let mut inner = self.inner.lock();
            match &inner.status {
                UpdateStatus::Checking { .. }
                | UpdateStatus::Downloading { .. }
                | UpdateStatus::Verifying { .. }
                | UpdateStatus::Extracting { .. }
                | UpdateStatus::Applying { .. } => return,
                _ => {}
            }
            if manual {
                inner.banner_dismissed = false;
            }
            inner.status = UpdateStatus::Checking { manual };
            Self::notify_locked(&mut inner, &self.version);
        }

        let mgr = self.clone();
        let _ = std::thread::Builder::new()
            .name("inviscid-update-check".to_string())
            .spawn(move || {
                let effective_base = source.resolve_base_url(&custom_url).to_string();
                let capability = detect_install_capability();
                let next_status = check_for_updates_sync(
                    &effective_base,
                    channel,
                    env!("CARGO_PKG_VERSION"),
                    current_target_triple(),
                    TRUSTED_MINISIGN_PUBLIC_KEYS,
                    capability,
                    skipped_version.as_deref(),
                    manual,
                );

                let mut inner = mgr.inner.lock();
                if matches!(
                    next_status,
                    UpdateStatus::Available(_)
                        | UpdateStatus::ManualUpdateOnly { .. }
                        | UpdateStatus::ReadyToInstall(_)
                ) {
                    inner.banner_dismissed = false;
                }
                inner.status = next_status;
                Self::notify_locked(&mut inner, &mgr.version);
            });
    }

    /// Starts downloading, verifying, and extracting the currently `Available` release.
    pub fn start_download(&self) {
        let (release, cache_dir, cancel_token) = {
            let mut inner = self.inner.lock();
            let UpdateStatus::Available(release) = inner.status.clone() else {
                return;
            };
            let cancel_token = Arc::new(AtomicBool::new(false));
            inner.active_cancel = Some(cancel_token.clone());
            inner.banner_dismissed = false;
            inner.status = UpdateStatus::Downloading {
                release: release.clone(),
                downloaded_bytes: 0,
                total_bytes: Some(release.target.size),
                bytes_per_sec: 0,
            };
            Self::notify_locked(&mut inner, &self.version);
            (release, inner.cache_dir.clone(), cancel_token)
        };

        let mgr = self.clone();
        let _ = std::thread::Builder::new()
            .name("inviscid-update-download".to_string())
            .spawn(move || {
                let updates_root = cache_dir.unwrap_or_else(|| {
                    std::env::temp_dir().join("inviscid").join("updates")
                });
                let result = download_and_stage_sync(
                    &release,
                    &updates_root,
                    cancel_token,
                    |progress| {
                        mgr.set_status(UpdateStatus::Downloading {
                            release: release.clone(),
                            downloaded_bytes: progress.downloaded_bytes,
                            total_bytes: progress.total_bytes,
                            bytes_per_sec: progress.bytes_per_sec,
                        });
                    },
                    |phase| match phase {
                        StagingPhase::Verifying => {
                            mgr.set_status(UpdateStatus::Verifying {
                                release: release.clone(),
                            });
                        }
                        StagingPhase::Extracting => {
                            mgr.set_status(UpdateStatus::Extracting {
                                release: release.clone(),
                            });
                        }
                    },
                );

                let mut inner = mgr.inner.lock();
                inner.active_cancel = None;
                match result {
                    Ok(staged) => {
                        inner.banner_dismissed = false;
                        inner.status = UpdateStatus::ReadyToInstall(staged);
                    }
                    Err(StageError::Cancelled) => {
                        inner.status = UpdateStatus::Available(release);
                    }
                    Err(StageError::Failed(msg)) => {
                        inner.status = UpdateStatus::Failed {
                            message: msg,
                            manual: true,
                        };
                    }
                }
                Self::notify_locked(&mut inner, &mgr.version);
            });
    }

    pub fn cancel_download(&self) {
        let inner = self.inner.lock();
        if let Some(ref token) = inner.active_cancel {
            token.store(true, Ordering::Relaxed);
        }
    }

    /// Marks the currently available release as skipped and returns its version string for
    /// persistence in `state.toml`.
    pub fn skip_version(&self) -> Option<String> {
        let mut inner = self.inner.lock();
        let skipped = match &inner.status {
            UpdateStatus::Available(rel) | UpdateStatus::ManualUpdateOnly { release: rel, .. } => {
                Some(rel.version.clone())
            }
            _ => None,
        };
        if skipped.is_some() {
            inner.status = UpdateStatus::Idle;
            inner.banner_dismissed = false;
            Self::notify_locked(&mut inner, &self.version);
        }
        skipped
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StagingPhase {
    Verifying,
    Extracting,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StageError {
    Cancelled,
    Failed(String),
}

/// Fetches, verifies, and parses the signed manifest synchronously.
#[allow(clippy::too_many_arguments)]
pub fn check_for_updates_sync(
    base_url: &str,
    channel: UpdateChannel,
    current_version: &str,
    target_triple: &str,
    public_keys: &[&str],
    capability: InstallCapability,
    skipped_version: Option<&str>,
    manual: bool,
) -> UpdateStatus {
    if let Err(err) = validate_update_base_url(base_url) {
        return UpdateStatus::Failed {
            message: err,
            manual,
        };
    }

    let json_url = manifest_url(base_url, channel);
    let sig_url = signature_url(base_url, channel);

    let raw_manifest = match fetch_bytes_sync(&json_url, MANIFEST_FETCH_TIMEOUT, MANIFEST_MAX_BYTES)
    {
        Ok(bytes) => bytes,
        Err(err) => {
            return UpdateStatus::Failed {
                message: format!("Failed to fetch update manifest: {err}"),
                manual,
            };
        }
    };

    let raw_sig_bytes =
        match fetch_bytes_sync(&sig_url, MANIFEST_FETCH_TIMEOUT, SIGNATURE_MAX_BYTES) {
            Ok(bytes) => bytes,
            Err(err) => {
                return UpdateStatus::Failed {
                    message: format!("Failed to fetch manifest signature: {err}"),
                    manual,
                };
            }
        };

    let Ok(raw_sig) = std::str::from_utf8(&raw_sig_bytes) else {
        return UpdateStatus::Failed {
            message: "Manifest signature is not valid UTF-8".to_string(),
            manual,
        };
    };

    let manifest = match verify_and_parse_manifest(&raw_manifest, raw_sig, channel, public_keys) {
        Ok(m) => m,
        Err(err) => {
            return UpdateStatus::Failed {
                message: err.to_string(),
                manual,
            };
        }
    };

    let comparison = match compare_manifest_version(current_version, &manifest) {
        Ok(c) => c,
        Err(err) => {
            return UpdateStatus::Failed {
                message: err.to_string(),
                manual,
            };
        }
    };

    let now_epoch = UpdateManager::current_unix_epoch_secs();
    let requires_manual_upgrade = match comparison {
        VersionComparison::UpToDate => {
            return UpdateStatus::UpToDate {
                checked_at_epoch: now_epoch,
                manual,
            };
        }
        VersionComparison::UpdateAvailable {
            requires_manual_upgrade,
        } => requires_manual_upgrade,
    };

    if !manual && skipped_version == Some(manifest.version.as_str()) {
        return UpdateStatus::UpToDate {
            checked_at_epoch: now_epoch,
            manual: false,
        };
    }

    let target = match select_target_asset(&manifest, target_triple) {
        Ok(t) => t.clone(),
        Err(err) => {
            return UpdateStatus::Failed {
                message: err.to_string(),
                manual,
            };
        }
    };

    let download_url = asset_url(base_url, &manifest.tag, &target.asset_name);
    let release = ReleaseInfo {
        version: manifest.version,
        tag: manifest.tag,
        channel: manifest.channel,
        pub_date: manifest.pub_date,
        release_notes_url: manifest.release_notes_url,
        notes: manifest.notes,
        target,
        download_url,
    };

    if requires_manual_upgrade {
        return UpdateStatus::ManualUpdateOnly {
            release,
            reason: ManualUpdateReason::ExternalExplanation(
                "Current version requires a manual upgrade to the latest release.".to_string(),
            ),
        };
    }

    match capability {
        InstallCapability::SelfUpdatable => UpdateStatus::Available(release),
        InstallCapability::ManualOnly(reason) => UpdateStatus::ManualUpdateOnly { release, reason },
    }
}

/// Downloads `release`, verifies its exact byte size and SHA-256 against the signed manifest,
/// and extracts it into a clean staging directory under `updates_root`.
pub fn download_and_stage_sync<FP, FS>(
    release: &ReleaseInfo,
    updates_root: &Path,
    cancel_token: Arc<AtomicBool>,
    on_progress: FP,
    mut on_phase: FS,
) -> Result<StagedUpdate, StageError>
where
    FP: FnMut(DownloadProgress),
    FS: FnMut(StagingPhase),
{
    if let Err(err) = validate_update_base_url(&release.download_url) {
        return Err(StageError::Failed(err));
    }

    let downloads_dir = updates_root.join("downloads");
    let archive_path = downloads_dir.join(&release.target.asset_name);
    let staging_dir = updates_root.join("staging").join(&release.version);

    let options = DownloadOptions {
        expected_bytes: Some(release.target.size),
        cancel_token,
        ..Default::default()
    };

    let dl_res =
        download_to_file_streaming(&release.download_url, &archive_path, &options, on_progress)
            .map_err(|err| match err {
                crate::http::DownloadError::Cancelled => StageError::Cancelled,
                other => StageError::Failed(other.to_string()),
            })?;

    on_phase(StagingPhase::Verifying);

    let expected_sha = release.target.sha256.to_ascii_lowercase();
    if dl_res.sha256_hex != expected_sha {
        let _ = fs::remove_file(&archive_path);
        return Err(StageError::Failed(format!(
            "SHA-256 verification failed: expected {}, got {}",
            expected_sha, dl_res.sha256_hex
        )));
    }

    on_phase(StagingPhase::Extracting);

    let extracted_entry = archive::extract_archive_safely(
        &archive_path,
        release.target.format,
        &staging_dir,
        &release.target.entry,
    )
    .map_err(|err| {
        let _ = fs::remove_file(&archive_path);
        StageError::Failed(err.to_string())
    })?;

    let _ = fs::remove_file(&archive_path);

    Ok(StagedUpdate {
        release: release.clone(),
        staging_dir,
        extracted_entry,
    })
}

#[cfg(test)]
mod tests {
    use super::manifest::test_signing::TestMinisignSigner;
    use super::*;
    use ring::digest::{Context as DigestContext, SHA256};
    use std::io::{Read, Write};
    use std::net::TcpListener;
    use zip::write::SimpleFileOptions;

    fn sha256_hex(bytes: &[u8]) -> String {
        let mut ctx = DigestContext::new(&SHA256);
        ctx.update(bytes);
        let digest = ctx.finish();
        digest
            .as_ref()
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect::<String>()
    }

    fn make_zip_bytes(entry_name: &str, content: &[u8]) -> Vec<u8> {
        let mut cursor = std::io::Cursor::new(Vec::new());
        {
            let mut writer = zip::ZipWriter::new(&mut cursor);
            writer
                .start_file(entry_name, SimpleFileOptions::default())
                .unwrap();
            writer.write_all(content).unwrap();
            writer.finish().unwrap();
        }
        cursor.into_inner()
    }

    fn spawn_mock_server(routes: HashMap<String, Vec<u8>>) -> (String, std::thread::JoinHandle<()>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let expected_requests = routes.len();
        let handle = std::thread::spawn(move || {
            for _ in 0..expected_requests {
                if let Ok((mut stream, _)) = listener.accept() {
                    let mut buf = [0u8; 2048];
                    let n = stream.read(&mut buf).unwrap_or(0);
                    let req = String::from_utf8_lossy(&buf[..n]);
                    let path = req
                        .lines()
                        .next()
                        .and_then(|line| line.split_whitespace().nth(1))
                        .unwrap_or("/");
                    if let Some(body) = routes.get(path) {
                        let header = format!(
                            "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                            body.len()
                        );
                        let _ = stream.write_all(header.as_bytes());
                        let _ = stream.write_all(body);
                    } else {
                        let _ = stream.write_all(
                            b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
                        );
                    }
                    let _ = stream.flush();
                }
            }
        });
        (format!("http://{addr}"), handle)
    }

    #[test]
    fn test_auto_check_24h_throttle() {
        assert!(UpdateManager::should_auto_check(None, 1_000_000));
        assert!(!UpdateManager::should_auto_check(
            Some(1_000_000),
            1_000_000 + 3600
        ));
        assert!(UpdateManager::should_auto_check(
            Some(1_000_000),
            1_000_000 + 86_400
        ));
    }

    #[test]
    fn test_end_to_end_check_download_verify_and_stage_mock_http() {
        let signer = TestMinisignSigner::generate();
        let archive_bytes = make_zip_bytes("inviscid.exe", b"MZfake-new-binary");
        let archive_sha = sha256_hex(&archive_bytes);
        let triple = current_target_triple();

        let manifest_json = format!(
            r#"{{
  "schema_version": 1,
  "channel": "stable",
  "version": "0.2.0",
  "tag": "v0.2.0",
  "pub_date": "2026-12-01T00:00:00Z",
  "release_notes_url": "https://github.com/KiraKiraAyu/Inviscid/releases/tag/v0.2.0",
  "notes": {{ "en": "Release notes" }},
  "targets": {{
    "{triple}": {{
      "asset_name": "inviscid-v0.2.0.zip",
      "size": {},
      "sha256": "{archive_sha}",
      "format": "zip",
      "entry": "inviscid.exe"
    }}
  }}
}}"#,
            archive_bytes.len()
        )
        .into_bytes();
        let sig = signer.sign_prehashed(&manifest_json, "update-stable.json");

        let mut routes = HashMap::new();
        routes.insert(
            "/channel-stable/update-stable.json".to_string(),
            manifest_json,
        );
        routes.insert(
            "/channel-stable/update-stable.json.minisig".to_string(),
            sig.into_bytes(),
        );
        routes.insert(
            "/v0.2.0/inviscid-v0.2.0.zip".to_string(),
            archive_bytes.clone(),
        );

        let (base_url, server) = spawn_mock_server(routes);

        let status = check_for_updates_sync(
            &base_url,
            UpdateChannel::Stable,
            "0.1.0",
            triple,
            &[&signer.public_key_b64],
            InstallCapability::SelfUpdatable,
            None,
            false,
        );
        let UpdateStatus::Available(release) = status else {
            panic!("expected Available status, got {status:?}");
        };

        let temp_cache = tempfile::tempdir().unwrap();
        let mut phases = Vec::new();
        let staged = download_and_stage_sync(
            &release,
            temp_cache.path(),
            Arc::new(AtomicBool::new(false)),
            |_| {},
            |p| phases.push(p),
        )
        .unwrap();
        server.join().unwrap();

        assert_eq!(phases, vec![StagingPhase::Verifying, StagingPhase::Extracting]);
        assert_eq!(fs::read(&staged.extracted_entry).unwrap(), b"MZfake-new-binary");
    }

    #[test]
    fn test_tampered_archive_sha256_rejected_and_cleaned() {
        let signer = TestMinisignSigner::generate();
        let expected_bytes = make_zip_bytes("inviscid.exe", b"MZgood");
        let tampered_bytes = make_zip_bytes("inviscid.exe", b"MZevil");
        assert_eq!(expected_bytes.len(), tampered_bytes.len());
        let expected_sha = sha256_hex(&expected_bytes);
        let triple = current_target_triple();

        let manifest_json = format!(
            r#"{{
  "schema_version": 1,
  "channel": "stable",
  "version": "0.2.0",
  "tag": "v0.2.0",
  "pub_date": "2026-12-01T00:00:00Z",
  "release_notes_url": "https://github.com/KiraKiraAyu/Inviscid/releases/tag/v0.2.0",
  "targets": {{
    "{triple}": {{
      "asset_name": "inviscid-v0.2.0.zip",
      "size": {},
      "sha256": "{expected_sha}",
      "format": "zip",
      "entry": "inviscid.exe"
    }}
  }}
}}"#,
            expected_bytes.len()
        )
        .into_bytes();
        let sig = signer.sign_prehashed(&manifest_json, "update-stable.json");

        let mut routes = HashMap::new();
        routes.insert(
            "/channel-stable/update-stable.json".to_string(),
            manifest_json,
        );
        routes.insert(
            "/channel-stable/update-stable.json.minisig".to_string(),
            sig.into_bytes(),
        );
        routes.insert("/v0.2.0/inviscid-v0.2.0.zip".to_string(), tampered_bytes);

        let (base_url, server) = spawn_mock_server(routes);
        let status = check_for_updates_sync(
            &base_url,
            UpdateChannel::Stable,
            "0.1.0",
            triple,
            &[&signer.public_key_b64],
            InstallCapability::SelfUpdatable,
            None,
            false,
        );
        let UpdateStatus::Available(release) = status else {
            panic!("expected Available status");
        };

        let temp_cache = tempfile::tempdir().unwrap();
        let err = download_and_stage_sync(
            &release,
            temp_cache.path(),
            Arc::new(AtomicBool::new(false)),
            |_| {},
            |_| {},
        )
        .unwrap_err();
        server.join().unwrap();

        assert!(matches!(err, StageError::Failed(msg) if msg.contains("SHA-256")));
        assert!(!temp_cache.path().join("downloads/inviscid-v0.2.0.zip").exists());
        assert!(!temp_cache.path().join("staging/0.2.0").exists());
    }

    #[test]
    fn test_skipped_version_and_manual_only_capability() {
        let signer = TestMinisignSigner::generate();
        let triple = current_target_triple();
        let manifest_json = format!(
            r#"{{
  "schema_version": 1,
  "channel": "stable",
  "version": "0.2.0",
  "tag": "v0.2.0",
  "pub_date": "2026-12-01T00:00:00Z",
  "release_notes_url": "https://github.com/KiraKiraAyu/Inviscid/releases/tag/v0.2.0",
  "targets": {{
    "{triple}": {{
      "asset_name": "inviscid-v0.2.0.zip",
      "size": 100,
      "sha256": "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
      "format": "zip",
      "entry": "inviscid.exe"
    }}
  }}
}}"#
        )
        .into_bytes();
        let sig = signer.sign_prehashed(&manifest_json, "update-stable.json");

        let mut routes = HashMap::new();
        routes.insert(
            "/channel-stable/update-stable.json".to_string(),
            manifest_json.clone(),
        );
        routes.insert(
            "/channel-stable/update-stable.json.minisig".to_string(),
            sig.clone().into_bytes(),
        );
        let (base_url1, s1) = spawn_mock_server(routes);

        // Silent check skips 0.2.0
        let silent = check_for_updates_sync(
            &base_url1,
            UpdateChannel::Stable,
            "0.1.0",
            triple,
            &[&signer.public_key_b64],
            InstallCapability::SelfUpdatable,
            Some("0.2.0"),
            false,
        );
        s1.join().unwrap();
        assert!(matches!(silent, UpdateStatus::UpToDate { manual: false, .. }));

        // Manual check ignores skipped_version and surfaces ManualUpdateOnly when read-only
        let mut routes2 = HashMap::new();
        routes2.insert("/channel-stable/update-stable.json".to_string(), manifest_json);
        routes2.insert(
            "/channel-stable/update-stable.json.minisig".to_string(),
            sig.into_bytes(),
        );
        let (base_url2, s2) = spawn_mock_server(routes2);
        let manual = check_for_updates_sync(
            &base_url2,
            UpdateChannel::Stable,
            "0.1.0",
            triple,
            &[&signer.public_key_b64],
            InstallCapability::ManualOnly(ManualUpdateReason::PackageManager("Scoop".into())),
            Some("0.2.0"),
            true,
        );
        s2.join().unwrap();
        assert!(matches!(
            manual,
            UpdateStatus::ManualUpdateOnly {
                reason: ManualUpdateReason::PackageManager(_),
                ..
            }
        ));
    }
}
