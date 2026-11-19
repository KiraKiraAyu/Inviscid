use anyhow::{Context, Result, anyhow};
use std::collections::HashSet;
use std::path::Path;
use std::sync::OnceLock;
use std::sync::atomic::{AtomicBool, Ordering};
use tree_sitter::{Language, Parser, QueryCursor, WasmStore};

use crate::config::AppConfig;
use crate::sync::{Condvar, Mutex};

const MAX_SANDBOX_MEMORY_BYTES: u64 = 128 * 1024 * 1024; // 128 MiB
const MAX_DISK_CACHE_BYTES: u64 = 128 * 1024 * 1024; // 128 MiB
const MAX_PARSER_SLOTS: usize = 2;
const MAX_LANGUAGES_PER_SLOT: usize = 8;

/// Process-wide WASM grammar compiler/runtime host.
///
/// Compilation uses a short-lived ephemeral `Engine` backed by Wasmtime's on-disk artifact
/// cache so Cranelift scratch buffers are immediately released back to the OS, while the
/// long-lived `runtime_engine` loads precompiled artifacts without retaining compiler state.
pub struct WasmHost {
    runtime_engine: wasmtime::Engine,
    cache: Option<wasmtime::Cache>,
    compile_lock: Mutex<()>,
    stdlib_prewarmed: AtomicBool,
    parser_pool: WasmParserPool,
}

static GLOBAL_WASM_HOST: OnceLock<std::result::Result<WasmHost, String>> = OnceLock::new();

impl WasmHost {
    fn init() -> std::result::Result<Self, String> {
        let cache_dir = if cfg!(test) {
            None
        } else {
            AppConfig::grammars_cache_dir().map(|dir| dir.join(".wasmtime-cache"))
        };
        Self::new_with_cache_dir(cache_dir.as_deref()).map_err(|err| format!("{err:#}"))
    }

    fn new_with_cache_dir(cache_dir: Option<&Path>) -> Result<Self> {
        let cache = cache_dir.and_then(init_disk_cache);
        let runtime_engine = Self::build_engine(cache.as_ref())
            .context("Failed to create Wasmtime runtime engine")?;

        Ok(Self {
            runtime_engine,
            cache,
            compile_lock: Mutex::new(()),
            stdlib_prewarmed: AtomicBool::new(false),
            parser_pool: WasmParserPool::new(),
        })
    }

    fn build_engine(cache: Option<&wasmtime::Cache>) -> Result<wasmtime::Engine> {
        let mut wt_config = wasmtime::Config::new();
        wt_config.wasm_simd(true);
        wt_config.wasm_bulk_memory(true);
        wt_config.memory_reservation(MAX_SANDBOX_MEMORY_BYTES);
        // Single-pass regalloc keeps cold-compile peak memory and compile latency bounded for
        // large Tree-sitter lex state machines while keeping optimized runtime code.
        wt_config.cranelift_opt_level(wasmtime::OptLevel::Speed);
        wt_config.cranelift_regalloc_algorithm(wasmtime::RegallocAlgorithm::SinglePass);
        wt_config.cache(cache.cloned());
        wasmtime::Engine::new(&wt_config).map_err(|e| anyhow!("{e}"))
    }

    pub fn global() -> Result<&'static Self> {
        GLOBAL_WASM_HOST
            .get_or_init(Self::init)
            .as_ref()
            .map_err(|err| anyhow!("{err}"))
    }

    fn create_tree_sitter_store(&self) -> Result<WasmStore> {
        self.ensure_stdlib_prewarmed();
        WasmStore::new(&self.runtime_engine).context("Failed to create WasmStore")
    }

    fn warm_stdlib_with_engine(&self, compile_engine: &wasmtime::Engine) {
        if !self.stdlib_prewarmed.load(Ordering::Acquire)
            && let Ok(store) = WasmStore::new(compile_engine)
        {
            drop(store);
            self.stdlib_prewarmed.store(true, Ordering::Release);
        }
    }

    fn ensure_stdlib_prewarmed(&self) {
        let Some(cache) = &self.cache else {
            return;
        };
        if self.stdlib_prewarmed.load(Ordering::Acquire) {
            return;
        }
        let _guard = self.compile_lock.lock();
        if self.stdlib_prewarmed.load(Ordering::Acquire) {
            return;
        }
        if let Ok(compile_engine) = Self::build_engine(Some(cache)) {
            self.warm_stdlib_with_engine(&compile_engine);
        }
    }

    /// Serializes Cranelift compilation into an ephemeral `Engine` so that compiled artifacts land
    /// in the on-disk cache and all temporary `CompilerContext` allocations are dropped immediately.
    fn precompile_to_cache(&self, wasm_bytes: &[u8]) -> Result<()> {
        let Some(cache) = &self.cache else {
            return Ok(());
        };
        let _guard = self.compile_lock.lock();
        let compile_engine =
            Self::build_engine(Some(cache)).context("Failed to create ephemeral compile engine")?;
        self.warm_stdlib_with_engine(&compile_engine);
        let compiled = wasmtime::Module::from_binary(&compile_engine, wasm_bytes)
            .map_err(|e| anyhow!("{e}"))
            .context("WebAssembly validation or compilation failed")?;
        drop(compiled);
        drop(compile_engine);
        Ok(())
    }

    /// Instantiates a `tree_sitter::Language` from a `.wasm` grammar binary.
    /// `wasm_lang_name` must match the exported `tree_sitter_<name>` symbol.
    pub fn load_tree_sitter_language(
        &self,
        wasm_lang_name: &str,
        wasm_bytes: &[u8],
    ) -> Result<Language> {
        self.precompile_to_cache(wasm_bytes)
            .with_context(|| format!("Precompiling wasm bytes for '{wasm_lang_name}'"))?;
        let mut store = self
            .create_tree_sitter_store()
            .with_context(|| format!("Creating WasmStore for '{wasm_lang_name}'"))?;
        store
            .load_language(wasm_lang_name, wasm_bytes)
            .with_context(|| format!("Failed to load tree-sitter language '{wasm_lang_name}'"))
    }

    /// Borrows a bounded reusable `(Parser, QueryCursor)` slot backed by `WasmStore`.
    pub fn with_parser<R>(
        &self,
        lang_key: &str,
        f: impl FnOnce(&mut Parser, &mut QueryCursor) -> R,
    ) -> Result<R> {
        let mut lease = self.parser_pool.checkout(self, lang_key)?;
        let slot = lease.slot_mut();
        Ok(f(&mut slot.parser, &mut slot.cursor))
    }

    /// Bumps the parser pool generation and discards idle `WasmStore`s so linear memory held
    /// by evicted grammars can be reclaimed by the OS.
    pub fn notify_grammar_evicted(&self) {
        self.parser_pool.invalidate_stores();
    }
}

fn init_disk_cache(dir: &Path) -> Option<wasmtime::Cache> {
    if let Err(err) = std::fs::create_dir_all(dir) {
        eprintln!(
            "Warning: Failed to create wasmtime cache directory {:?}: {}",
            dir, err
        );
        return None;
    }
    let mut cache_cfg = wasmtime::CacheConfig::new();
    cache_cfg.with_directory(dir);
    cache_cfg.with_files_total_size_soft_limit(MAX_DISK_CACHE_BYTES);
    match wasmtime::Cache::new(cache_cfg) {
        Ok(cache) => Some(cache),
        Err(err) => {
            eprintln!(
                "Warning: Failed to initialize wasmtime disk cache at {:?}: {}",
                dir, err
            );
            None
        }
    }
}

struct ParserSlot {
    parser: Parser,
    cursor: QueryCursor,
    generation: u64,
    seen_languages: HashSet<String>,
}

impl ParserSlot {
    fn new(host: &WasmHost, generation: u64) -> Result<Self> {
        let store = host.create_tree_sitter_store()?;
        let mut parser = Parser::new();
        parser
            .set_wasm_store(store)
            .context("Failed to attach WASM store to Tree-sitter parser")?;
        Ok(Self {
            parser,
            cursor: QueryCursor::new(),
            generation,
            seen_languages: HashSet::new(),
        })
    }

    fn accepts(&self, lang_key: &str) -> bool {
        self.seen_languages.contains(lang_key) || self.seen_languages.len() < MAX_LANGUAGES_PER_SLOT
    }

    fn record_language(&mut self, lang_key: &str) {
        if !self.seen_languages.contains(lang_key) {
            self.seen_languages.insert(lang_key.to_string());
        }
    }
}

struct PoolInner {
    idle: Vec<ParserSlot>,
    active: usize,
    generation: u64,
}

struct WasmParserPool {
    state: Mutex<PoolInner>,
    available: Condvar,
}

impl WasmParserPool {
    fn new() -> Self {
        Self {
            state: Mutex::new(PoolInner {
                idle: Vec::with_capacity(MAX_PARSER_SLOTS),
                active: 0,
                generation: 1,
            }),
            available: Condvar::new(),
        }
    }

    fn invalidate_stores(&self) {
        let dropped_idle = {
            let mut guard = self.state.lock();
            guard.generation = guard.generation.wrapping_add(1);
            std::mem::take(&mut guard.idle)
        };
        drop(dropped_idle);
    }

    fn checkout<'a>(&'a self, host: &WasmHost, lang_key: &str) -> Result<ParserLease<'a>> {
        enum Acquired {
            Existing(ParserSlot),
            CreateNew {
                generation: u64,
                discarded: Option<ParserSlot>,
            },
        }

        let acquired = {
            let mut guard = self.state.lock();
            loop {
                let current_gen = guard.generation;
                guard.idle.retain(|slot| slot.generation == current_gen);

                if let Some(pos) = guard
                    .idle
                    .iter()
                    .rposition(|slot| slot.seen_languages.contains(lang_key))
                {
                    let slot = guard.idle.swap_remove(pos);
                    guard.active += 1;
                    break Acquired::Existing(slot);
                }
                if let Some(pos) = guard.idle.iter().rposition(|slot| slot.accepts(lang_key)) {
                    let slot = guard.idle.swap_remove(pos);
                    guard.active += 1;
                    break Acquired::Existing(slot);
                }
                if guard.active < MAX_PARSER_SLOTS {
                    // Replace a saturated idle slot rather than exceeding `MAX_LANGUAGES_PER_SLOT`.
                    let discarded = guard.idle.pop();
                    guard.active += 1;
                    break Acquired::CreateNew {
                        generation: current_gen,
                        discarded,
                    };
                }
                self.available.wait(&mut guard);
            }
        };

        let mut lease = ParserLease {
            pool: self,
            slot: None,
        };
        let mut slot = match acquired {
            Acquired::Existing(slot) => slot,
            Acquired::CreateNew {
                generation,
                discarded,
            } => {
                drop(discarded);
                ParserSlot::new(host, generation)?
            }
        };
        slot.record_language(lang_key);
        lease.slot = Some(slot);
        Ok(lease)
    }

    fn checkin(&self, slot: Option<ParserSlot>) {
        let discard = {
            let mut guard = self.state.lock();
            guard.active = guard.active.saturating_sub(1);
            let keep = slot.and_then(|s| {
                if s.generation == guard.generation
                    && s.seen_languages.len() <= MAX_LANGUAGES_PER_SLOT
                    && guard.idle.len() < MAX_PARSER_SLOTS
                {
                    guard.idle.push(s);
                    None
                } else {
                    Some(s)
                }
            });
            self.available.notify_one();
            keep
        };
        drop(discard);
    }
}

struct ParserLease<'a> {
    pool: &'a WasmParserPool,
    slot: Option<ParserSlot>,
}

impl ParserLease<'_> {
    fn slot_mut(&mut self) -> &mut ParserSlot {
        self.slot.as_mut().expect("parser lease already consumed")
    }
}

impl Drop for ParserLease<'_> {
    fn drop(&mut self) {
        let slot = self.slot.take();
        if std::thread::panicking() {
            self.pool.checkin(None);
            drop(slot);
        } else {
            self.pool.checkin(slot);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const MINIMAL_WASM_MODULE: &[u8] = b"\0asm\x01\x00\x00\x00";
    const JSON_GRAMMAR_WASM: &[u8] = include_bytes!("../tests/fixtures/tree-sitter-json.wasm");

    #[test]
    fn test_wasm_host_validates_minimal_module() {
        let host = WasmHost::global().expect("WasmHost should initialize");
        assert!(wasmtime::Module::validate(&host.runtime_engine, MINIMAL_WASM_MODULE).is_ok());
    }

    #[test]
    fn test_wasm_host_rejects_invalid_or_corrupted_bytes() {
        let host = WasmHost::global().expect("WasmHost should initialize");
        assert!(wasmtime::Module::validate(&host.runtime_engine, &[]).is_err());
        assert!(wasmtime::Module::validate(&host.runtime_engine, b"not a wasm file").is_err());
        assert!(
            wasmtime::Module::validate(&host.runtime_engine, b"\0asm\x99\x99\x99\x99").is_err()
        );
    }

    #[test]
    fn test_wasm_host_loads_and_executes_tree_sitter_wasm_grammar() {
        let host = WasmHost::global().expect("WasmHost should initialize");
        let language = host
            .load_tree_sitter_language("json", JSON_GRAMMAR_WASM)
            .expect("tree-sitter-json.wasm should load cleanly");
        assert!(language.is_wasm());

        let store = host
            .create_tree_sitter_store()
            .expect("WasmStore creation should succeed");
        let mut parser = tree_sitter::Parser::new();
        parser
            .set_wasm_store(store)
            .expect("Attaching WasmStore to Parser should succeed");
        parser
            .set_language(&language)
            .expect("Setting WASM language on Parser should succeed");

        let source = r#"{"name": "inviscid", "version": 1, "active": true}"#;
        let tree = parser
            .parse(source, None)
            .expect("Parsing JSON via WASM grammar should produce an AST");
        let root = tree.root_node();
        assert_eq!(root.kind(), "document");
        assert!(!root.has_error());
    }

    #[test]
    fn test_wasm_host_rejects_mismatched_language_symbol() {
        let host = WasmHost::global().expect("WasmHost should initialize");
        let err = host
            .load_tree_sitter_language("rust", JSON_GRAMMAR_WASM)
            .unwrap_err();
        assert!(format!("{err:#}").contains("tree_sitter_rust"));
    }

    #[test]
    fn test_wasm_host_with_disk_cache_precompiles_and_parses() {
        let dir = tempfile::tempdir().expect("temp dir");
        let host = WasmHost::new_with_cache_dir(Some(dir.path()))
            .expect("WasmHost with disk cache should initialize");
        assert!(host.cache.is_some());

        let language = host
            .load_tree_sitter_language("json", JSON_GRAMMAR_WASM)
            .expect("Cached tree-sitter-json.wasm should load cleanly");
        let kind = host
            .with_parser("json", |parser, _| {
                parser.set_language(&language).unwrap();
                let tree = parser.parse(r#"{"ok": true}"#, None).unwrap();
                tree.root_node().kind().to_string()
            })
            .expect("with_parser should succeed");
        assert_eq!(kind, "document");

        let (idle, active, gen_before) = {
            let guard = host.parser_pool.state.lock();
            (guard.idle.len(), guard.active, guard.generation)
        };
        assert_eq!(idle, 1);
        assert_eq!(active, 0);

        host.notify_grammar_evicted();
        let (idle_after, active_after, gen_after) = {
            let guard = host.parser_pool.state.lock();
            (guard.idle.len(), guard.active, guard.generation)
        };
        assert_eq!(idle_after, 0);
        assert_eq!(active_after, 0);
        assert_eq!(gen_after, gen_before + 1);
    }
}
