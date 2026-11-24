use anyhow::{Context, Result, anyhow, bail};
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, OnceLock};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use crate::http::{HttpWorkerPool, fetch_bytes_sync};
use crate::sync::{Condvar, Mutex};

use super::builtins::{GrammarSpec, resolve_grammar_spec};
use super::queries::sanitize_remote_highlights_scm;
use super::registry::{
    LoadedGrammar, clear_global_loading, get_global_config_snapshot, is_global_loaded,
    mark_global_failed_if_loading, mark_global_loaded, notify_syntax_mutation,
};

const MAX_WASM_SIZE: u64 = 16 * 1024 * 1024; // 16 MiB safety cap
const MAX_SCM_SIZE: u64 = 512 * 1024; // 512 KiB safety cap
const TYPING_PREFIX_MIN_GAP: Duration = Duration::from_millis(10);

/// Predefined CDN mirror presets for remote Tree-sitter grammar packages.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum GrammarCdnPreset {
    #[default]
    JsDelivr,
    Unpkg,
    Offline,
}

impl GrammarCdnPreset {
    pub const JSDELIVR: &'static str = "https://cdn.jsdelivr.net/npm";
    pub const UNPKG: &'static str = "https://unpkg.com";
    pub const OFFLINE: &'static str = "";
    pub const DEFAULT_URL: &'static str = Self::JSDELIVR;

    #[inline]
    pub const fn url(self) -> &'static str {
        match self {
            Self::JsDelivr => Self::JSDELIVR,
            Self::Unpkg => Self::UNPKG,
            Self::Offline => Self::OFFLINE,
        }
    }

    /// Matches `url` against known presets after normalization.
    pub fn from_url(url: &str) -> Option<Self> {
        let normalized = normalize_grammar_base_url(url);
        if normalized == Self::JSDELIVR {
            Some(Self::JsDelivr)
        } else if normalized == Self::UNPKG {
            Some(Self::Unpkg)
        } else if normalized.is_empty() {
            Some(Self::Offline)
        } else {
            None
        }
    }
}

/// Trims surrounding whitespace and trailing slashes from a grammar CDN base URL.
pub fn normalize_grammar_base_url(url: &str) -> String {
    url.trim().trim_end_matches('/').to_string()
}

fn locate_in_dir(canonical_id: &str, dir: &Path) -> Option<(PathBuf, Option<PathBuf>)> {
    let id_dir = dir.join(canonical_id);
    let wasm_filename = format!("tree-sitter-{canonical_id}.wasm");
    let wasm_path = [
        id_dir.join("grammar.wasm"),
        id_dir.join(&wasm_filename),
        dir.join(&wasm_filename),
    ]
    .into_iter()
    .find(|p| p.is_file())?;

    let scm_path = [
        id_dir.join("highlights.scm"),
        dir.join(format!("{canonical_id}.scm")),
    ]
    .into_iter()
    .find(|p| p.is_file());

    Some((wasm_path, scm_path))
}

/// Lightweight metadata probe checking whether a `.wasm` grammar exists in `dirs` without
/// reading file contents into memory.
pub(crate) fn has_grammar_in_dirs_by_id(canonical_id: &str, dirs: &[&Path]) -> bool {
    dirs.iter()
        .any(|dir| locate_in_dir(canonical_id, dir).is_some())
}

/// Searches `dirs` in priority order for `<canonical_id>/grammar.wasm`,
/// `<canonical_id>/tree-sitter-<canonical_id>.wasm`, or `tree-sitter-<canonical_id>.wasm`,
/// along with an optional `<canonical_id>/highlights.scm` or `<canonical_id>.scm`.
pub fn find_grammar_in_dirs_by_id(
    canonical_id: &str,
    dirs: &[&Path],
) -> Option<(Vec<u8>, Option<String>)> {
    for dir in dirs {
        if let Some((wasm_path, scm_path)) = locate_in_dir(canonical_id, dir)
            && let Ok(wasm_bytes) = fs::read(&wasm_path)
        {
            let custom_scm = scm_path.and_then(|p| fs::read_to_string(p).ok());
            return Some((wasm_bytes, custom_scm));
        }
    }
    None
}

static SYNTAX_WORKER_POOL: OnceLock<HttpWorkerPool> = OnceLock::new();

fn syntax_worker_pool() -> &'static HttpWorkerPool {
    SYNTAX_WORKER_POOL.get_or_init(|| HttpWorkerPool::with_name_prefix(4, "inviscid-syntax"))
}

pub(crate) fn spawn_local_grammar_loader(canonical_id: String) {
    let (_, cache_dir) = get_global_config_snapshot();
    syntax_worker_pool().spawn(move || {
        let Some(cache_root) = cache_dir else {
            mark_global_failed_if_loading(&canonical_id);
            return;
        };
        let Some((wasm_bytes, custom_scm)) =
            find_grammar_in_dirs_by_id(&canonical_id, &[cache_root.as_path()])
        else {
            mark_global_failed_if_loading(&canonical_id);
            return;
        };

        match LoadedGrammar::from_wasm_bytes_with_id(
            &canonical_id,
            &wasm_bytes,
            custom_scm.as_deref(),
        ) {
            Ok(grammar) => {
                mark_global_loaded(grammar);
                notify_syntax_mutation();
            }
            Err(err) => {
                eprintln!(
                    "Warning: Failed to load local grammar '{}': {:#}",
                    canonical_id, err
                );
                mark_global_failed_if_loading(&canonical_id);
            }
        }
    });
}

pub(crate) struct ScheduledRemoteFetch {
    pub(crate) scheduled_at: Instant,
    pub(crate) due_at: Instant,
    pub(crate) cancel_token: Arc<AtomicBool>,
}

pub(crate) struct DebouncedLoaderInner {
    pub(crate) scheduled: HashMap<String, ScheduledRemoteFetch>,
    pub(crate) shutdown: bool,
}

pub struct DebouncedGrammarLoader {
    pub(crate) inner: Arc<(Mutex<DebouncedLoaderInner>, Condvar)>,
    thread: Option<JoinHandle<()>>,
}

impl Default for DebouncedGrammarLoader {
    fn default() -> Self {
        Self::new()
    }
}

impl Drop for DebouncedGrammarLoader {
    fn drop(&mut self) {
        {
            let (lock, cvar) = &*self.inner;
            let mut guard = lock.lock();
            guard.shutdown = true;
            cvar.notify_all();
        }
        if let Some(handle) = self.thread.take() {
            let _ = handle.join();
        }
    }
}

impl DebouncedGrammarLoader {
    pub fn new() -> Self {
        let inner = Arc::new((
            Mutex::new(DebouncedLoaderInner {
                scheduled: HashMap::new(),
                shutdown: false,
            }),
            Condvar::new(),
        ));

        let coordinator = inner.clone();
        let handle = std::thread::Builder::new()
            .name("inviscid-syntax-debouncer".to_string())
            .spawn(move || {
                Self::coordinator_loop(coordinator);
            })
            .expect("Failed to spawn syntax debouncer thread");

        Self {
            inner,
            thread: Some(handle),
        }
    }

    pub fn schedule_remote_fetch(&self, canonical_id: &str) {
        let cancelled = {
            let (lock, cvar) = &*self.inner;
            let mut inner = lock.lock();
            let now = Instant::now();
            let due_at = now + Duration::from_millis(300);

            // Cancel superseded typing prefixes (e.g. `has` -> `hask` -> `haskell`) only when a
            // real keystroke gap elapsed, so same-frame polyglot scans never cancel `c`/`cpp` or
            // unrelated languages.
            let mut cancelled = Vec::new();
            for (id, item) in inner.scheduled.iter() {
                if id == canonical_id {
                    continue;
                }
                let is_prefix_extension = canonical_id.starts_with(id);
                let is_typing_gap = now.duration_since(item.scheduled_at) >= TYPING_PREFIX_MIN_GAP;
                if is_prefix_extension && is_typing_gap {
                    item.cancel_token.store(true, Ordering::Relaxed);
                    cancelled.push(id.clone());
                }
            }
            for id in &cancelled {
                inner.scheduled.remove(id);
            }

            if let Some(existing) = inner.scheduled.get_mut(canonical_id) {
                existing.scheduled_at = now;
                existing.due_at = due_at;
            } else {
                inner.scheduled.insert(
                    canonical_id.to_string(),
                    ScheduledRemoteFetch {
                        scheduled_at: now,
                        due_at,
                        cancel_token: Arc::new(AtomicBool::new(false)),
                    },
                );
            }

            cvar.notify_one();
            cancelled
        };

        for id in cancelled {
            clear_global_loading(&id);
        }
    }

    fn coordinator_loop(inner: Arc<(Mutex<DebouncedLoaderInner>, Condvar)>) {
        let (lock, cvar) = &*inner;
        let mut guard = lock.lock();
        loop {
            if guard.shutdown {
                break;
            }

            let now = Instant::now();
            let mut ready = Vec::new();
            let mut next_wake: Option<Duration> = None;

            for (id, item) in guard.scheduled.iter() {
                if item.cancel_token.load(Ordering::Relaxed) {
                    continue;
                }
                if now >= item.due_at {
                    ready.push((id.clone(), item.cancel_token.clone()));
                } else {
                    let remaining = item.due_at - now;
                    next_wake = Some(next_wake.map_or(remaining, |w| w.min(remaining)));
                }
            }

            guard
                .scheduled
                .retain(|_, item| !item.cancel_token.load(Ordering::Relaxed) && item.due_at > now);

            if !ready.is_empty() {
                drop(guard);
                for (id, cancel_token) in ready {
                    dispatch_remote_loader(id, cancel_token);
                }
                guard = lock.lock();
                continue;
            }

            match next_wake {
                Some(dur) => {
                    let _ = cvar.wait_for(&mut guard, dur);
                }
                None => {
                    cvar.wait(&mut guard);
                }
            }
        }
    }
}

fn dispatch_remote_loader(canonical_id: String, cancel_token: Arc<AtomicBool>) {
    if is_global_loaded(&canonical_id) {
        return;
    }
    let (base_url, cache_dir) = get_global_config_snapshot();

    if base_url.is_empty() {
        mark_global_failed_if_loading(&canonical_id);
        return;
    }

    let ct = cancel_token.clone();
    syntax_worker_pool().spawn_front_cancelable(cancel_token, move || {
        if ct.load(Ordering::Relaxed) || is_global_loaded(&canonical_id) {
            return;
        }

        match fetch_remote_grammar_by_id_cancelable(&base_url, &canonical_id, Some(&ct)) {
            Ok((wasm_bytes, remote_scm, grammar)) => {
                if ct.load(Ordering::Relaxed) {
                    return;
                }
                if let Some(ref cache_root) = cache_dir {
                    let lang_cache_dir = cache_root.join(&canonical_id);
                    let wasm_path =
                        lang_cache_dir.join(format!("tree-sitter-{}.wasm", canonical_id));
                    if let Err(e) = crate::fs::atomic_write(&wasm_path, &wasm_bytes) {
                        eprintln!(
                            "Warning: Failed to cache grammar '{}' to {:?}: {}",
                            canonical_id, wasm_path, e
                        );
                    }
                    if let Some(ref scm_text) = remote_scm {
                        let scm_path = lang_cache_dir.join("highlights.scm");
                        if let Err(e) = crate::fs::atomic_write(&scm_path, scm_text.as_bytes()) {
                            eprintln!(
                                "Warning: Failed to cache highlights.scm for '{}' to {:?}: {}",
                                canonical_id, scm_path, e
                            );
                        }
                    }
                }
                mark_global_loaded(grammar);
                notify_syntax_mutation();
            }
            Err(err) => {
                if ct.load(Ordering::Relaxed) {
                    return;
                }
                let err_msg = format!("{err:#}");
                if err_msg.contains("failed to parse dylink") {
                    eprintln!(
                        "Warning: Remote Tree-sitter grammar for '{canonical_id}' is incompatible: package uses legacy Emscripten dylink format (Tree-sitter 0.27 requires dylink.0)"
                    );
                } else if !err_msg.contains("HTTP 404") && !err_msg.contains("cancelled") {
                    eprintln!(
                        "Warning: Failed to load Tree-sitter grammar '{canonical_id}': {err_msg}"
                    );
                }
                mark_global_failed_if_loading(&canonical_id);
            }
        }
    });
}

fn fetch_remote_grammar_by_id_cancelable(
    base_url: &str,
    canonical_id: &str,
    cancel_token: Option<&AtomicBool>,
) -> Result<(Vec<u8>, Option<String>, LoadedGrammar)> {
    let normalized = normalize_grammar_base_url(base_url);
    if normalized.is_empty() {
        bail!("Grammar not found locally and remote downloading is disabled");
    }

    if let Some(token) = cancel_token
        && token.load(Ordering::Relaxed)
    {
        bail!("Grammar download cancelled for '{}'", canonical_id);
    }

    if let Some(spec) = resolve_grammar_spec(canonical_id) {
        let download_url = build_grammar_download_url(&normalized, spec).ok_or_else(|| {
            anyhow!("Grammar not found locally and remote downloading is disabled")
        })?;
        let wasm_bytes = fetch_bytes_sync(&download_url, Duration::from_secs(8), MAX_WASM_SIZE)
            .with_context(|| format!("Fetching built-in grammar from '{}'", download_url))?;
        let grammar = LoadedGrammar::from_wasm_bytes(spec, &wasm_bytes, None)
            .with_context(|| format!("Compiling grammar downloaded from '{}'", download_url))?;
        return Ok((wasm_bytes, None, grammar));
    }

    let pkg_name = format!("tree-sitter-{}", canonical_id.replace('_', "-"));
    let wasm_filename = format!("tree-sitter-{}.wasm", canonical_id);
    let candidate_roots = [
        format!("{}/{}@latest", normalized, pkg_name),
        format!("{}/@tree-sitter-grammars/{}@latest", normalized, pkg_name),
    ];

    let mut last_err = anyhow!("No remote grammar package found for '{}'", canonical_id);
    for pkg_root in candidate_roots {
        if let Some(token) = cancel_token
            && token.load(Ordering::Relaxed)
        {
            bail!("Grammar download cancelled for '{}'", canonical_id);
        }

        let wasm_url = format!("{}/{}", pkg_root, wasm_filename);
        let wasm_bytes = match fetch_bytes_sync(&wasm_url, Duration::from_secs(8), MAX_WASM_SIZE) {
            Ok(bytes) => bytes,
            Err(e) => {
                last_err = e.context(format!("Fetching wasm binary from '{}'", wasm_url));
                continue;
            }
        };

        if let Some(token) = cancel_token
            && token.load(Ordering::Relaxed)
        {
            bail!("Grammar download cancelled for '{}'", canonical_id);
        }

        let scm_url = format!("{}/queries/highlights.scm", pkg_root);
        let remote_scm = fetch_bytes_sync(&scm_url, Duration::from_secs(5), MAX_SCM_SIZE)
            .ok()
            .and_then(|bytes| String::from_utf8(bytes).ok())
            .map(|raw| sanitize_remote_highlights_scm(&raw));

        match LoadedGrammar::from_wasm_bytes_with_fallback(
            canonical_id,
            &wasm_bytes,
            remote_scm.as_deref(),
        ) {
            Ok((grammar, used_remote_scm)) => {
                let kept_scm = used_remote_scm.then_some(remote_scm).flatten();
                return Ok((wasm_bytes, kept_scm, grammar));
            }
            Err(e) => {
                last_err = e.context(format!("Loading remote grammar from '{}'", wasm_url));
            }
        }
    }

    Err(last_err)
}

/// Builds the download URL for `spec` from `base_url`, or returns `None` if `base_url` is empty
/// (offline mode).
pub fn build_grammar_download_url(base_url: &str, spec: GrammarSpec) -> Option<String> {
    let normalized = normalize_grammar_base_url(base_url);
    if normalized.is_empty() {
        return None;
    }
    Some(format!("{}/{}", normalized, spec.cdn_path))
}
