use anyhow::{Context, Result};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, OnceLock};
use tree_sitter::{Language, Query, StreamingIterator};

use crate::config::AppConfig;
use crate::markdown::InlineSpan;
use crate::sync::{RwLock, VersionReceiver, VersionedNotifier};
use crate::wasm::WasmHost;

use super::builtins::{GrammarSpec, resolve_grammar_spec};
use super::loader::{
    DebouncedGrammarLoader, GrammarSource, has_grammar_in_dirs_by_id,
    normalize_grammar_base_url, spawn_local_grammar_loader,
};
use super::queries::{compile_resilient_query, normalize_language_id, synthesize_highlights_scm};
use super::token::SyntaxToken;

const MAX_LOADED_GRAMMARS: usize = 12;

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
    fn from_language_and_query(canonical_id: String, language: Language, query: Query) -> Self {
        let capture_tokens = query
            .capture_names()
            .iter()
            .map(|name| SyntaxToken::from_capture_name(name))
            .collect();

        Self {
            canonical_id,
            language,
            query,
            capture_tokens,
        }
    }

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

        Ok(Self::from_language_and_query(canonical_id, language, query))
    }

    /// Loads WASM once and compiles `remote_scm`, falling back to a synthesized AST query on the
    /// same loaded `Language` if `remote_scm` fails query compilation.
    pub(crate) fn from_wasm_bytes_with_fallback(
        canonical_id: &str,
        wasm_bytes: &[u8],
        remote_scm: Option<&str>,
    ) -> Result<(Self, bool)> {
        let language = WasmHost::global()
            .context("Initializing WASM host")?
            .load_tree_sitter_language(canonical_id, wasm_bytes)
            .with_context(|| format!("Loading WASM language binary for '{}'", canonical_id))?;

        if let Some(scm) = remote_scm {
            if let Ok(query) = compile_resilient_query(&language, scm) {
                return Ok((
                    Self::from_language_and_query(canonical_id.to_string(), language, query),
                    true,
                ));
            }
        }

        let synthesized = synthesize_highlights_scm(&language);
        let query = compile_resilient_query(&language, &synthesized).with_context(|| {
            format!(
                "Compiling fallback synthesized query for '{}'",
                canonical_id
            )
        })?;
        Ok((
            Self::from_language_and_query(canonical_id.to_string(), language, query),
            false,
        ))
    }
}

#[derive(Clone, Debug)]
pub enum GrammarState {
    Loaded(Arc<LoadedGrammar>),
    Loading,
    Failed,
}

struct RegistrySlot {
    state: GrammarState,
    last_used_seq: AtomicU64,
}

impl RegistrySlot {
    fn new(state: GrammarState, seq: u64) -> Self {
        Self {
            state,
            last_used_seq: AtomicU64::new(seq),
        }
    }

    #[inline]
    fn touch(&self, seq: u64) {
        self.last_used_seq.store(seq, Ordering::Relaxed);
    }
}

pub struct SyntaxRegistry {
    entries: HashMap<String, RegistrySlot>,
    base_url: String,
    #[cfg(test)]
    pub(super) cache_dir: Option<PathBuf>,
    #[cfg(not(test))]
    cache_dir: Option<PathBuf>,
    next_seq: AtomicU64,
    max_loaded: usize,
    retry_epoch: u64,
}

impl Default for SyntaxRegistry {
    fn default() -> Self {
        Self::new()
    }
}

impl SyntaxRegistry {
    pub fn new() -> Self {
        Self::with_limit(MAX_LOADED_GRAMMARS)
    }

    pub(crate) fn with_limit(max_loaded: usize) -> Self {
        let (default_url, cache_dir) = if cfg!(test) {
            (String::new(), None)
        } else {
            (
                GrammarSource::DEFAULT_URL.to_string(),
                AppConfig::grammars_cache_dir(),
            )
        };
        Self {
            entries: HashMap::new(),
            base_url: default_url,
            cache_dir,
            next_seq: AtomicU64::new(1),
            max_loaded: max_loaded.max(1),
            retry_epoch: 0,
        }
    }

    #[inline]
    fn alloc_seq(&self) -> u64 {
        self.next_seq.fetch_add(1, Ordering::Relaxed)
    }

    pub fn set_base_url(&mut self, url: impl Into<String>) {
        let normalized = normalize_grammar_base_url(&url.into());
        if self.base_url != normalized {
            self.base_url = normalized;
            self.entries
                .retain(|_, slot| !matches!(slot.state, GrammarState::Failed));
            self.retry_epoch = self.retry_epoch.wrapping_add(1);
        }
    }

    pub fn base_url(&self) -> &str {
        &self.base_url
    }

    pub fn get_state(&self, canonical_id: &str) -> Option<&GrammarState> {
        self.entries.get(canonical_id).map(|slot| &slot.state)
    }

    /// Reads a loaded grammar without updating its LRU recency sequence.
    pub(crate) fn peek_loaded(&self, canonical_id: &str) -> Option<Arc<LoadedGrammar>> {
        match &self.entries.get(canonical_id)?.state {
            GrammarState::Loaded(grammar) => Some(grammar.clone()),
            _ => None,
        }
    }

    pub fn get_loaded(&self, canonical_id: &str) -> Option<Arc<LoadedGrammar>> {
        let slot = self.entries.get(canonical_id)?;
        match &slot.state {
            GrammarState::Loaded(grammar) => {
                slot.touch(self.alloc_seq());
                Some(grammar.clone())
            }
            _ => None,
        }
    }

    pub fn mark_loading(&mut self, canonical_id: &str) {
        let seq = self.alloc_seq();
        self.entries.insert(
            canonical_id.to_string(),
            RegistrySlot::new(GrammarState::Loading, seq),
        );
    }

    pub(crate) fn clear_loading(&mut self, canonical_id: &str) {
        if matches!(self.get_state(canonical_id), Some(GrammarState::Loading)) {
            self.entries.remove(canonical_id);
        }
    }

    /// Inserts a loaded grammar and evicts the least-recently-used entries over budget.
    /// Returns `true` if at least one older grammar was evicted.
    pub fn mark_loaded(&mut self, grammar: Arc<LoadedGrammar>) -> bool {
        let id = grammar.canonical_id.clone();
        let seq = self.alloc_seq();
        self.entries.insert(
            id.clone(),
            RegistrySlot::new(GrammarState::Loaded(grammar), seq),
        );
        self.evict_lru_over_limit(&id)
    }

    fn evict_lru_over_limit(&mut self, protected_id: &str) -> bool {
        let mut loaded: Vec<(String, u64)> = self
            .entries
            .iter()
            .filter_map(|(id, slot)| {
                matches!(slot.state, GrammarState::Loaded(_))
                    .then(|| (id.clone(), slot.last_used_seq.load(Ordering::Relaxed)))
            })
            .collect();

        if loaded.len() <= self.max_loaded {
            return false;
        }

        loaded.sort_by_key(|&(_, seq)| seq);
        let mut remaining = loaded.len();
        let mut evicted = false;

        for (id, _) in loaded {
            if remaining <= self.max_loaded {
                break;
            }
            if id == protected_id {
                continue;
            }
            self.entries.remove(&id);
            remaining -= 1;
            evicted = true;
        }
        evicted
    }

    pub fn mark_failed(&mut self, canonical_id: &str) {
        let seq = self.alloc_seq();
        self.entries.insert(
            canonical_id.to_string(),
            RegistrySlot::new(GrammarState::Failed, seq),
        );
    }

    /// Transitions `canonical_id` to `Failed` only if it is still `Loading`, so a stale
    /// background failure cannot overwrite a grammar that was synchronously registered or loaded.
    pub fn mark_failed_if_loading(&mut self, canonical_id: &str) {
        if matches!(self.get_state(canonical_id), Some(GrammarState::Loading)) {
            self.mark_failed(canonical_id);
        }
    }

    /// Synchronously compiles and registers a `.wasm` grammar in this registry.
    #[cfg(test)]
    pub fn register_wasm_bytes(
        &mut self,
        lang_hint: &str,
        wasm_bytes: &[u8],
        custom_scm: Option<&str>,
    ) -> Result<Arc<LoadedGrammar>> {
        let canonical_id = normalize_language_id(lang_hint)
            .ok_or_else(|| anyhow::anyhow!("Unsupported language identifier: '{}'", lang_hint))?;
        let loaded = Arc::new(
            LoadedGrammar::from_wasm_bytes_with_id(&canonical_id, wasm_bytes, custom_scm)
                .with_context(|| format!("Registering wasm grammar for '{}'", canonical_id))?,
        );
        self.mark_loaded(loaded.clone());
        Ok(loaded)
    }

    #[cfg(test)]
    pub(crate) fn loaded_count(&self) -> usize {
        self.entries
            .values()
            .filter(|slot| matches!(slot.state, GrammarState::Loaded(_)))
            .count()
    }
}

#[derive(Clone, Copy, Debug)]
struct RawCapture {
    start_byte: usize,
    end_byte: usize,
    pattern_index: usize,
    token: SyntaxToken,
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

    let host = match WasmHost::global() {
        Ok(host) => host,
        Err(e) => {
            eprintln!(
                "Warning: Failed to initialize WASM host for syntax highlighting: {}",
                e
            );
            return plain_spans_for_lines(lines);
        }
    };

    let captures = host
        .with_parser(&grammar.canonical_id, |parser, cursor| {
            if let Err(e) = parser.set_language(&grammar.language) {
                eprintln!(
                    "Warning: Failed to set Tree-sitter language '{}': {}",
                    grammar.canonical_id, e,
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
        })
        .unwrap_or_else(|e| {
            eprintln!(
                "Warning: Failed to borrow Tree-sitter WASM parser for '{}': {:#}",
                grammar.canonical_id, e
            );
            None
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

pub fn syntax_cache_version() -> usize {
    SYNTAX_NOTIFIER.version()
}

pub(crate) fn notify_syntax_mutation() {
    SYNTAX_NOTIFIER.notify();
}

pub fn subscribe_syntax_updates() -> VersionReceiver {
    SYNTAX_NOTIFIER.subscribe()
}

fn read_registry<R>(f: impl FnOnce(&SyntaxRegistry) -> R) -> R {
    let rwlock = GLOBAL_SYNTAX_REGISTRY.get_or_init(|| RwLock::new(SyntaxRegistry::new()));
    f(&rwlock.read())
}

fn write_registry<R>(f: impl FnOnce(&mut SyntaxRegistry) -> R) -> R {
    let rwlock = GLOBAL_SYNTAX_REGISTRY.get_or_init(|| RwLock::new(SyntaxRegistry::new()));
    f(&mut rwlock.write())
}

pub(crate) fn peek_global_loaded(canonical_id: &str) -> Option<Arc<LoadedGrammar>> {
    read_registry(|reg| reg.peek_loaded(canonical_id))
}

pub(crate) fn syntax_retry_epoch() -> u64 {
    read_registry(|reg| reg.retry_epoch)
}

pub(crate) fn is_global_loaded(canonical_id: &str) -> bool {
    read_registry(|reg| reg.peek_loaded(canonical_id).is_some())
}

pub(crate) fn get_global_config_snapshot() -> (String, Option<PathBuf>) {
    read_registry(|reg| (reg.base_url().to_string(), reg.cache_dir.clone()))
}

pub(crate) fn mark_global_loaded(grammar: LoadedGrammar) {
    let evicted = write_registry(|reg| reg.mark_loaded(Arc::new(grammar)));
    if evicted && let Ok(host) = WasmHost::global() {
        host.notify_grammar_evicted();
    }
}

pub(crate) fn clear_global_loading(canonical_id: &str) {
    write_registry(|reg| reg.clear_loading(canonical_id));
}

pub(crate) fn mark_global_failed_if_loading(canonical_id: &str) {
    write_registry(|reg| reg.mark_failed_if_loading(canonical_id));
}

/// Synchronously registers a `.wasm` grammar in the global [`SyntaxRegistry`] and increments
/// [`syntax_cache_version`].
#[cfg(test)]
pub(crate) fn register_global_wasm_grammar(
    lang_hint: &str,
    wasm_bytes: &[u8],
    custom_scm: Option<&str>,
) -> Result<Arc<LoadedGrammar>> {
    let canonical_id = normalize_language_id(lang_hint)
        .ok_or_else(|| anyhow::anyhow!("Unsupported language identifier: '{}'", lang_hint))?;
    let loaded = Arc::new(
        LoadedGrammar::from_wasm_bytes_with_id(&canonical_id, wasm_bytes, custom_scm)
            .with_context(|| format!("Registering wasm grammar for '{}'", canonical_id))?,
    );
    let evicted = write_registry(|reg| reg.mark_loaded(loaded.clone()));
    if evicted && let Ok(host) = WasmHost::global() {
        host.notify_grammar_evicted();
    }
    notify_syntax_mutation();
    Ok(loaded)
}

/// Configures the remote grammar base URL on the global [`SyntaxRegistry`].
pub fn set_global_grammar_base_url(url: impl Into<String>) {
    write_registry(|reg| reg.set_base_url(url));
    notify_syntax_mutation();
}

enum RegistryLookup {
    Loaded(Arc<LoadedGrammar>),
    Missing {
        cache_dir: Option<PathBuf>,
        has_remote_url: bool,
    },
    InFlightOrFailed,
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

    let (cache_dir, has_remote_url) = match read_registry(|reg| {
        if let Some(loaded) = reg.get_loaded(&canonical_id) {
            RegistryLookup::Loaded(loaded)
        } else if reg.get_state(&canonical_id).is_some() {
            RegistryLookup::InFlightOrFailed
        } else {
            RegistryLookup::Missing {
                cache_dir: reg.cache_dir.clone(),
                has_remote_url: !reg.base_url().is_empty(),
            }
        }
    }) {
        RegistryLookup::Loaded(loaded) => return highlight_lines_with_grammar(&loaded, lines),
        RegistryLookup::InFlightOrFailed => return plain_spans_for_lines(lines),
        RegistryLookup::Missing {
            cache_dir,
            has_remote_url,
        } => (cache_dir, has_remote_url),
    };

    let is_local = cache_dir
        .as_deref()
        .is_some_and(|cache_root| has_grammar_in_dirs_by_id(&canonical_id, &[cache_root]));

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
    } else if !has_remote_url {
        write_registry(|reg| {
            if reg.get_state(&canonical_id).is_none() {
                reg.mark_failed(&canonical_id);
            }
        });
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
