use anyhow::{Context, Result, anyhow, bail};
use std::cell::RefCell;
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, OnceLock};
use std::time::{Duration, Instant};
use tree_sitter::{Language, Parser, Query, QueryCursor, StreamingIterator};

use crate::config::AppConfig;
use crate::http::{HttpWorkerPool, fetch_bytes_sync};
use crate::markdown::InlineSpan;
use crate::sync::{Condvar, Mutex, RwLock, VersionReceiver, VersionedNotifier};
use crate::wasm::WasmHost;

use super::builtins::{GrammarSpec, resolve_grammar_spec};
use super::queries::{
    compile_resilient_query, normalize_language_id, sanitize_remote_highlights_scm,
    synthesize_highlights_scm,
};
use super::token::SyntaxToken;

const MAX_WASM_SIZE: u64 = 16 * 1024 * 1024; // 16 MiB safety cap
const MAX_SCM_SIZE: u64 = 512 * 1024; // 512 KiB safety cap

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
    pub const DEFAULT: Self = Self::JsDelivr;

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

/// A compiled Tree-sitter WASM language and its highlight query ready for execution.
pub struct LoadedGrammar {
    pub canonical_id: String,
    pub language: Language,
    pub query: Query,
    pub capture_tokens: Vec<Option<SyntaxToken>>,
}

impl std::fmt::Debug for LoadedGrammar {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LoadedGrammar")
            .field("canonical_id", &self.canonical_id)
            .field("capture_count", &self.capture_tokens.len())
            .finish()
    }
}

impl LoadedGrammar {
    /// Loads a `.wasm` grammar module using a built-in [`GrammarSpec`] and compiles its highlight query.
    pub fn from_wasm_bytes(
        spec: GrammarSpec,
        wasm_bytes: &[u8],
        custom_highlights_scm: Option<&str>,
    ) -> Result<Self> {
        let scm = custom_highlights_scm.unwrap_or(spec.default_highlights_scm);
        Self::from_wasm_bytes_with_id(spec.canonical_id, wasm_bytes, Some(scm))
    }

    /// Loads a `.wasm` grammar module for `canonical_id` (`tree_sitter_<canonical_id>`),
    /// using `custom_highlights_scm`, a built-in query, or a query synthesized from the symbol table.
    pub fn from_wasm_bytes_with_id(
        canonical_id: impl Into<String>,
        wasm_bytes: &[u8],
        custom_highlights_scm: Option<&str>,
    ) -> Result<Self> {
        let canonical_id = canonical_id.into();
        let language = WasmHost::global()
            .context("Initializing WASM host")?
            .load_tree_sitter_language(&canonical_id, wasm_bytes)
            .with_context(|| format!("Loading WASM language binary for '{}'", canonical_id))?;

        let query = if let Some(scm) = custom_highlights_scm {
            compile_resilient_query(&language, scm).context("Compiling custom highlight query")?
        } else if let Some(spec) = resolve_grammar_spec(&canonical_id) {
            compile_resilient_query(&language, spec.default_highlights_scm).with_context(|| {
                format!("Compiling default highlight query for '{}'", canonical_id)
            })?
        } else {
            let synthesized = synthesize_highlights_scm(&language);
            compile_resilient_query(&language, &synthesized).with_context(|| {
                format!(
                    "Compiling synthesized highlight query for '{}'",
                    canonical_id
                )
            })?
        };

        let capture_tokens = query
            .capture_names()
            .iter()
            .map(|name| SyntaxToken::from_capture_name(name))
            .collect();

        Ok(Self {
            canonical_id,
            language,
            query,
            capture_tokens,
        })
    }
}

#[derive(Clone, Debug)]
pub enum GrammarState {
    Loaded(Arc<LoadedGrammar>),
    Loading,
    Failed(String),
}

pub struct SyntaxRegistry {
    entries: HashMap<String, GrammarState>,
    base_url: String,
    cache_dir: Option<PathBuf>,
}

impl Default for SyntaxRegistry {
    fn default() -> Self {
        Self::new()
    }
}

impl SyntaxRegistry {
    pub fn new() -> Self {
        let default_url = if cfg!(test) {
            String::new()
        } else {
            GrammarCdnPreset::DEFAULT.url().to_string()
        };
        Self {
            entries: HashMap::new(),
            base_url: default_url,
            cache_dir: AppConfig::grammars_cache_dir(),
        }
    }

    pub fn set_base_url(&mut self, url: impl Into<String>) {
        let normalized = normalize_grammar_base_url(&url.into());
        if self.base_url != normalized {
            self.base_url = normalized;
            self.entries
                .retain(|_, state| !matches!(state, GrammarState::Failed(_)));
        }
    }

    pub fn base_url(&self) -> &str {
        &self.base_url
    }

    pub fn set_cache_dir(&mut self, cache_dir: Option<PathBuf>) {
        self.cache_dir = cache_dir;
    }

    pub fn get_state(&self, canonical_id: &str) -> Option<&GrammarState> {
        self.entries.get(canonical_id)
    }

    pub fn get_loaded(&self, canonical_id: &str) -> Option<Arc<LoadedGrammar>> {
        match self.entries.get(canonical_id) {
            Some(GrammarState::Loaded(grammar)) => Some(grammar.clone()),
            _ => None,
        }
    }

    pub fn mark_loading(&mut self, canonical_id: &str) {
        self.entries
            .insert(canonical_id.to_string(), GrammarState::Loading);
    }

    pub fn mark_loaded(&mut self, grammar: Arc<LoadedGrammar>) {
        self.entries
            .insert(grammar.canonical_id.clone(), GrammarState::Loaded(grammar));
    }

    pub fn mark_failed(&mut self, canonical_id: &str, reason: impl Into<String>) {
        self.entries.insert(
            canonical_id.to_string(),
            GrammarState::Failed(reason.into()),
        );
    }

    /// Synchronously compiles and registers a `.wasm` grammar in this registry.
    pub fn register_wasm_bytes(
        &mut self,
        lang_hint: &str,
        wasm_bytes: &[u8],
        custom_scm: Option<&str>,
    ) -> Result<Arc<LoadedGrammar>> {
        let canonical_id = normalize_language_id(lang_hint)
            .ok_or_else(|| anyhow!("Unsupported language identifier: '{}'", lang_hint))?;
        let loaded = Arc::new(
            LoadedGrammar::from_wasm_bytes_with_id(&canonical_id, wasm_bytes, custom_scm)
                .with_context(|| format!("Registering wasm grammar for '{}'", canonical_id))?,
        );
        self.mark_loaded(loaded.clone());
        Ok(loaded)
    }
}

/// Searches `dirs` in priority order for `<id>/grammar.wasm`, `<id>/<wasm_filename>`, or `<wasm_filename>`,
/// along with an optional `<id>/highlights.scm` or `<id>.scm`.
pub fn find_grammar_in_dirs(
    spec: GrammarSpec,
    dirs: &[&Path],
) -> Option<(Vec<u8>, Option<String>)> {
    find_grammar_in_dirs_by_id(spec.canonical_id, dirs)
}

/// Searches `dirs` in priority order for `<canonical_id>/grammar.wasm`,
/// `<canonical_id>/tree-sitter-<canonical_id>.wasm`, or `tree-sitter-<canonical_id>.wasm`,
/// along with an optional `<canonical_id>/highlights.scm` or `<canonical_id>.scm`.
pub fn find_grammar_in_dirs_by_id(
    canonical_id: &str,
    dirs: &[&Path],
) -> Option<(Vec<u8>, Option<String>)> {
    let wasm_filename = format!("tree-sitter-{canonical_id}.wasm");
    for dir in dirs {
        let id_dir = dir.join(canonical_id);
        let wasm_path = [
            id_dir.join("grammar.wasm"),
            id_dir.join(&wasm_filename),
            dir.join(&wasm_filename),
        ]
        .into_iter()
        .find(|p| p.is_file());

        if let Some(wasm_path) = wasm_path
            && let Ok(wasm_bytes) = fs::read(&wasm_path)
        {
            let scm_path = [
                id_dir.join("highlights.scm"),
                dir.join(format!("{canonical_id}.scm")),
            ]
            .into_iter()
            .find(|p| p.is_file());

            let custom_scm = scm_path.and_then(|p| fs::read_to_string(p).ok());
            return Some((wasm_bytes, custom_scm));
        }
    }
    None
}

#[derive(Clone, Copy, Debug)]
struct RawCapture {
    start_byte: usize,
    end_byte: usize,
    pattern_index: usize,
    token: SyntaxToken,
}

thread_local! {
    static THREAD_PARSER: RefCell<Option<(Parser, QueryCursor)>> = const { RefCell::new(None) };
}

/// Highlights a slice of code lines using a loaded Tree-sitter WASM grammar.
///
/// Each returned `Vec<InlineSpan>` partitions the characters `(0..char_count)` of the corresponding line.
pub fn highlight_lines_with_grammar(
    grammar: &LoadedGrammar,
    lines: &[String],
) -> Vec<Vec<InlineSpan>> {
    if lines.is_empty() {
        return Vec::new();
    }

    let total_bytes: usize = lines.iter().map(|l| l.len() + 1).sum();
    let mut source = String::with_capacity(total_bytes);
    let mut line_byte_ranges = Vec::with_capacity(lines.len());

    for (idx, line) in lines.iter().enumerate() {
        if idx > 0 {
            source.push('\n');
        }
        let start = source.len();
        source.push_str(line);
        let end = source.len();
        line_byte_ranges.push((start, end));
    }

    let captures = THREAD_PARSER.with(|cell| -> Option<Vec<RawCapture>> {
        let mut borrow = cell.borrow_mut();
        if borrow.is_none() {
            let host = match WasmHost::global() {
                Ok(host) => host,
                Err(e) => {
                    eprintln!(
                        "Warning: Failed to initialize WASM host for syntax highlighting: {}",
                        e
                    );
                    return None;
                }
            };
            let store = match host.create_tree_sitter_store() {
                Ok(store) => store,
                Err(e) => {
                    eprintln!("Warning: Failed to create Tree-sitter WASM store: {}", e);
                    return None;
                }
            };
            let mut parser = Parser::new();
            if let Err(e) = parser.set_wasm_store(store) {
                eprintln!(
                    "Warning: Failed to attach WASM store to Tree-sitter parser: {}",
                    e
                );
                return None;
            }
            *borrow = Some((parser, QueryCursor::new()));
        }

        let (parser, cursor) = borrow.as_mut()?;
        if let Err(e) = parser.set_language(&grammar.language) {
            eprintln!(
                "Warning: Failed to set Tree-sitter language '{}': {}",
                grammar.canonical_id, e
            );
            return None;
        }
        let Some(tree) = parser.parse(&source, None) else {
            eprintln!(
                "Warning: Tree-sitter parser returned no syntax tree for '{}'",
                grammar.canonical_id
            );
            return None;
        };

        let mut raw_captures = Vec::new();
        let mut matches = cursor.captures(&grammar.query, tree.root_node(), source.as_bytes());
        while let Some((m, capture_idx)) = matches.next() {
            let capture = m.captures()[*capture_idx];
            if let Some(Some(token)) = grammar.capture_tokens.get(capture.index as usize) {
                let range = capture.node.byte_range();
                if range.start < range.end {
                    raw_captures.push(RawCapture {
                        start_byte: range.start,
                        end_byte: range.end,
                        pattern_index: m.pattern_index,
                        token: *token,
                    });
                }
            }
        }
        Some(raw_captures)
    });

    let Some(mut captures) = captures else {
        return plain_spans_for_lines(lines);
    };

    // Sort so broader outer AST nodes (and earlier general fallback patterns) are applied first,
    // while narrower inner AST nodes (and later specific patterns) overwrite them.
    captures.sort_by(|a, b| {
        let len_a = a.end_byte - a.start_byte;
        let len_b = b.end_byte - b.start_byte;
        len_b
            .cmp(&len_a)
            .then_with(|| a.pattern_index.cmp(&b.pattern_index))
            .then_with(|| a.start_byte.cmp(&b.start_byte))
    });

    let mut result = Vec::with_capacity(lines.len());
    for (line_idx, raw_line) in lines.iter().enumerate() {
        let (line_start, line_end) = line_byte_ranges[line_idx];
        result.push(build_highlighted_line_spans(
            raw_line, line_start, line_end, &captures,
        ));
    }

    result
}

fn build_highlighted_line_spans(
    raw_line: &str,
    line_start_byte: usize,
    line_end_byte: usize,
    captures: &[RawCapture],
) -> Vec<InlineSpan> {
    if raw_line.is_empty() {
        return vec![InlineSpan::plain(String::new(), (0, 0))];
    }

    let mut char_byte_offsets: Vec<usize> = raw_line.char_indices().map(|(b, _)| b).collect();
    let char_count = char_byte_offsets.len();
    char_byte_offsets.push(raw_line.len());

    let byte_to_char_idx = |rel_byte: usize| -> usize {
        if rel_byte >= raw_line.len() {
            char_count
        } else {
            char_byte_offsets[..char_count]
                .partition_point(|&b| b <= rel_byte)
                .saturating_sub(1)
        }
    };

    let mut char_tokens: Vec<Option<SyntaxToken>> = vec![None; char_count];
    for cap in captures {
        if cap.end_byte <= line_start_byte || cap.start_byte >= line_end_byte {
            continue;
        }
        let rel_start = cap
            .start_byte
            .saturating_sub(line_start_byte)
            .min(raw_line.len());
        let rel_end = (cap.end_byte - line_start_byte).min(raw_line.len());
        let c_start = byte_to_char_idx(rel_start);
        let c_end = byte_to_char_idx(rel_end);
        char_tokens[c_start..c_end].fill(Some(cap.token));
    }

    let mut spans = Vec::new();
    let mut run_start = 0usize;
    let mut run_token = char_tokens[0];

    for (c_idx, &token) in char_tokens.iter().enumerate().skip(1) {
        if token != run_token {
            let slice = &raw_line[char_byte_offsets[run_start]..char_byte_offsets[c_idx]];
            spans.push(InlineSpan::syntax(slice, (run_start, c_idx), run_token));
            run_start = c_idx;
            run_token = token;
        }
    }

    let tail_slice = &raw_line[char_byte_offsets[run_start]..raw_line.len()];
    spans.push(InlineSpan::syntax(
        tail_slice,
        (run_start, char_count),
        run_token,
    ));

    spans
}

#[inline]
pub fn plain_spans_for_lines(lines: &[String]) -> Vec<Vec<InlineSpan>> {
    lines
        .iter()
        .map(|line| {
            let char_count = line.chars().count();
            vec![InlineSpan::plain(line.clone(), (0, char_count))]
        })
        .collect()
}

static GLOBAL_SYNTAX_REGISTRY: OnceLock<RwLock<SyntaxRegistry>> = OnceLock::new();
static SYNTAX_NOTIFIER: VersionedNotifier = VersionedNotifier::new();
static SYNTAX_WORKER_POOL: OnceLock<HttpWorkerPool> = OnceLock::new();

pub fn syntax_cache_version() -> usize {
    SYNTAX_NOTIFIER.version()
}

pub(crate) fn notify_syntax_mutation() {
    SYNTAX_NOTIFIER.notify();
}

pub fn subscribe_syntax_updates() -> VersionReceiver {
    SYNTAX_NOTIFIER.subscribe()
}

pub(crate) fn read_registry<R>(f: impl FnOnce(&SyntaxRegistry) -> R) -> R {
    let rwlock = GLOBAL_SYNTAX_REGISTRY.get_or_init(|| RwLock::new(SyntaxRegistry::new()));
    f(&rwlock.read())
}

pub(crate) fn write_registry<R>(f: impl FnOnce(&mut SyntaxRegistry) -> R) -> R {
    let rwlock = GLOBAL_SYNTAX_REGISTRY.get_or_init(|| RwLock::new(SyntaxRegistry::new()));
    f(&mut rwlock.write())
}

/// Synchronously registers a `.wasm` grammar in the global [`SyntaxRegistry`] and increments
/// [`syntax_cache_version`].
pub fn register_global_wasm_grammar(
    lang_hint: &str,
    wasm_bytes: &[u8],
    custom_scm: Option<&str>,
) -> Result<Arc<LoadedGrammar>> {
    let res = write_registry(|reg| reg.register_wasm_bytes(lang_hint, wasm_bytes, custom_scm))?;
    notify_syntax_mutation();
    Ok(res)
}

/// Configures the remote grammar base URL on the global [`SyntaxRegistry`].
pub fn set_global_grammar_base_url(url: impl Into<String>) {
    write_registry(|reg| reg.set_base_url(url));
    notify_syntax_mutation();
}

/// Highlights a fenced code block's content lines for `lang_hint`.
///
/// - If the grammar is already loaded in memory, immediately parses and highlights with Tree-sitter WASM.
/// - If the grammar exists on disk locally, loads it immediately on a background thread.
/// - If the grammar requires remote downloading from a CDN, debounces the fetch (canceling intermediate
///   typing fragments) and returns plain spans for the current frame.
pub fn highlight_code_block(lang_hint: &str, lines: &[String]) -> Vec<Vec<InlineSpan>> {
    if lines.is_empty() {
        return Vec::new();
    }

    let Some(canonical_id) = normalize_language_id(lang_hint) else {
        return plain_spans_for_lines(lines);
    };

    if let Some(loaded) = read_registry(|reg| reg.get_loaded(&canonical_id)) {
        return highlight_lines_with_grammar(&loaded, lines);
    }

    let cache_dir = read_registry(|reg| reg.cache_dir.clone());
    let is_local = if let Some(ref cache_root) = cache_dir {
        find_grammar_in_dirs_by_id(&canonical_id, &[cache_root.as_path()]).is_some()
    } else {
        false
    };

    if is_local {
        let should_spawn = write_registry(|reg| {
            if reg.get_state(&canonical_id).is_some() {
                return false;
            }
            reg.mark_loading(&canonical_id);
            true
        });
        if should_spawn {
            spawn_local_grammar_loader(canonical_id);
        }
    } else {
        let should_schedule = write_registry(|reg| {
            if reg.get_state(&canonical_id).is_some() {
                return false;
            }
            reg.mark_loading(&canonical_id);
            true
        });
        if should_schedule {
            static DEBOUNCED_LOADER: OnceLock<DebouncedGrammarLoader> = OnceLock::new();
            DEBOUNCED_LOADER
                .get_or_init(DebouncedGrammarLoader::new)
                .schedule_remote_fetch(&canonical_id);
        }
    }

    plain_spans_for_lines(lines)
}

fn spawn_local_grammar_loader(canonical_id: String) {
    let cache_dir = read_registry(|reg| reg.cache_dir.clone());
    let pool =
        SYNTAX_WORKER_POOL.get_or_init(|| HttpWorkerPool::with_name_prefix(4, "inviscid-syntax"));
    pool.spawn(move || {
        if let Some(ref cache_root) = cache_dir
            && let Some((wasm_bytes, custom_scm)) =
                find_grammar_in_dirs_by_id(&canonical_id, &[cache_root.as_path()])
        {
            match LoadedGrammar::from_wasm_bytes_with_id(
                &canonical_id,
                &wasm_bytes,
                custom_scm.as_deref(),
            ) {
                Ok(grammar) => {
                    write_registry(|reg| reg.mark_loaded(Arc::new(grammar)));
                    notify_syntax_mutation();
                }
                Err(err) => {
                    eprintln!(
                        "Warning: Failed to load local grammar '{}': {:#}",
                        canonical_id, err
                    );
                    write_registry(|reg| reg.mark_failed(&canonical_id, format!("{err:#}")));
                }
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
}

impl Default for DebouncedGrammarLoader {
    fn default() -> Self {
        Self::new()
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
        std::thread::Builder::new()
            .name("inviscid-syntax-debouncer".to_string())
            .spawn(move || {
                Self::coordinator_loop(coordinator);
            })
            .expect("Failed to spawn syntax debouncer thread");

        Self { inner }
    }

    pub fn schedule_remote_fetch(&self, canonical_id: &str) {
        let (lock, cvar) = &*self.inner;
        let mut inner = lock.lock();
        let now = Instant::now();
        let due_at = now + Duration::from_millis(300);

        // Cancel and purge any pending scheduled fetches that are superseding/superseded prefixes
        // or older keystrokes from interactive typing (> 15ms ago)
        let mut to_cancel = Vec::new();
        for (id, item) in inner.scheduled.iter() {
            if id == canonical_id {
                continue;
            }
            let is_prefix_or_suffix = canonical_id.starts_with(id) || id.starts_with(canonical_id);
            let is_interactive_keystroke =
                now.duration_since(item.scheduled_at) > Duration::from_millis(15);
            if is_prefix_or_suffix || is_interactive_keystroke {
                item.cancel_token.store(true, Ordering::Relaxed);
                to_cancel.push(id.clone());
            }
        }
        for id in to_cancel {
            inner.scheduled.remove(&id);
            write_registry(|reg| {
                if matches!(reg.get_state(&id), Some(GrammarState::Loading)) {
                    reg.entries.remove(&id);
                }
            });
        }

        if let Some(existing) = inner.scheduled.get_mut(canonical_id) {
            existing.due_at = due_at;
        } else {
            let cancel_token = Arc::new(AtomicBool::new(false));
            inner.scheduled.insert(
                canonical_id.to_string(),
                ScheduledRemoteFetch {
                    scheduled_at: now,
                    due_at,
                    cancel_token,
                },
            );
        }

        cvar.notify_one();
    }

    fn coordinator_loop(inner: Arc<(Mutex<DebouncedLoaderInner>, Condvar)>) {
        let (lock, cvar) = &*inner;
        let mut inner_guard = lock.lock();
        loop {
            if inner_guard.shutdown {
                break;
            }

            let now = Instant::now();
            let mut ready = Vec::new();
            let mut next_wake: Option<Duration> = None;

            for (id, item) in inner_guard.scheduled.iter() {
                if item.cancel_token.load(Ordering::Relaxed) {
                    continue;
                }
                if now >= item.due_at {
                    ready.push((id.clone(), item.cancel_token.clone()));
                } else {
                    let remaining = item.due_at - now;
                    next_wake = match next_wake {
                        Some(w) if remaining < w => Some(remaining),
                        Some(w) => Some(w),
                        None => Some(remaining),
                    };
                }
            }

            inner_guard
                .scheduled
                .retain(|_, item| !item.cancel_token.load(Ordering::Relaxed) && item.due_at > now);

            // Dispatch ready jobs to syntax worker pool
            for (id, cancel_token) in ready {
                dispatch_remote_loader(id, cancel_token);
            }

            match next_wake {
                Some(dur) => {
                    let _ = cvar.wait_for(&mut inner_guard, dur);
                }
                None => {
                    cvar.wait(&mut inner_guard);
                }
            }
        }
    }
}

fn dispatch_remote_loader(canonical_id: String, cancel_token: Arc<AtomicBool>) {
    let (base_url, cache_dir) =
        read_registry(|reg| (reg.base_url().to_string(), reg.cache_dir.clone()));

    if base_url.is_empty() {
        write_registry(|reg| reg.mark_failed(&canonical_id, "Remote downloading is disabled"));
        return;
    }

    let pool =
        SYNTAX_WORKER_POOL.get_or_init(|| HttpWorkerPool::with_name_prefix(4, "inviscid-syntax"));

    let ct = cancel_token.clone();
    pool.spawn_front_cancelable(cancel_token.clone(), move || {
        if ct.load(Ordering::Relaxed) {
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
                write_registry(|reg| reg.mark_loaded(Arc::new(grammar)));
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
                write_registry(|reg| reg.mark_failed(&canonical_id, err_msg));
            }
        }
    });
}

/// Downloads and compiles a grammar for `canonical_id` from `base_url`.
pub fn fetch_remote_grammar_by_id(
    base_url: &str,
    canonical_id: &str,
) -> Result<(Vec<u8>, Option<String>, LoadedGrammar)> {
    fetch_remote_grammar_by_id_cancelable(base_url, canonical_id, None)
}

/// Downloads and compiles a grammar for `canonical_id` from `base_url` with cancellation support.
pub fn fetch_remote_grammar_by_id_cancelable(
    base_url: &str,
    canonical_id: &str,
    cancel_token: Option<&AtomicBool>,
) -> Result<(Vec<u8>, Option<String>, LoadedGrammar)> {
    let normalized = normalize_grammar_base_url(base_url);
    if normalized.is_empty() {
        bail!("Grammar not found locally and remote downloading is disabled");
    }

    if let Some(token) = cancel_token {
        if token.load(Ordering::Relaxed) {
            bail!("Grammar download cancelled for '{}'", canonical_id);
        }
    }

    if let Some(spec) = resolve_grammar_spec(canonical_id) {
        let download_url = format!("{}/{}", normalized, spec.cdn_path);
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
        if let Some(token) = cancel_token {
            if token.load(Ordering::Relaxed) {
                bail!("Grammar download cancelled for '{}'", canonical_id);
            }
        }

        let wasm_url = format!("{}/{}", pkg_root, wasm_filename);
        let wasm_bytes = match fetch_bytes_sync(&wasm_url, Duration::from_secs(8), MAX_WASM_SIZE) {
            Ok(bytes) => bytes,
            Err(e) => {
                last_err = e.context(format!("Fetching wasm binary from '{}'", wasm_url));
                continue;
            }
        };

        if let Some(token) = cancel_token {
            if token.load(Ordering::Relaxed) {
                bail!("Grammar download cancelled for '{}'", canonical_id);
            }
        }

        let scm_url = format!("{}/queries/highlights.scm", pkg_root);
        let remote_scm = fetch_bytes_sync(&scm_url, Duration::from_secs(5), MAX_SCM_SIZE)
            .ok()
            .and_then(|bytes| String::from_utf8(bytes).ok())
            .map(|raw| sanitize_remote_highlights_scm(&raw));

        match LoadedGrammar::from_wasm_bytes_with_id(
            canonical_id,
            &wasm_bytes,
            remote_scm.as_deref(),
        ) {
            Ok(grammar) => return Ok((wasm_bytes, remote_scm, grammar)),
            Err(e) if remote_scm.is_some() => {
                let err_str = format!("{e:#}");
                if err_str.contains("failed to parse dylink")
                    || err_str.contains("invalid import")
                    || err_str.contains("WebAssembly validation failed")
                {
                    last_err = e.context(format!("Loading remote grammar from '{}'", wasm_url));
                    continue;
                }
                // If the remote queries/highlights.scm failed, fall back to synthesized AST query
                match LoadedGrammar::from_wasm_bytes_with_id(canonical_id, &wasm_bytes, None) {
                    Ok(grammar) => return Ok((wasm_bytes, None, grammar)),
                    Err(synth_err) => {
                        last_err = synth_err
                            .context(format!("Compiling fallback query for '{}'", wasm_url));
                    }
                }
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
