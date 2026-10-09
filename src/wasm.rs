use anyhow::{Context, Result, anyhow};
use std::sync::OnceLock;
pub use tree_sitter::wasmtime;
use tree_sitter::{Language, WasmStore};

const MAX_SANDBOX_MEMORY_BYTES: u64 = 128 * 1024 * 1024; // 128 MiB

pub struct WasmHost {
    engine: wasmtime::Engine,
}

static GLOBAL_WASM_HOST: OnceLock<std::result::Result<WasmHost, String>> = OnceLock::new();

impl WasmHost {
    fn init() -> std::result::Result<Self, String> {
        let mut wt_config = wasmtime::Config::new();
        wt_config.wasm_simd(true);
        wt_config.wasm_bulk_memory(true);
        wt_config.memory_reservation(MAX_SANDBOX_MEMORY_BYTES);

        let engine = wasmtime::Engine::new(&wt_config)
            .map_err(|e| format!("Failed to create Wasmtime engine: {}", e))?;

        Ok(Self { engine })
    }

    pub fn global() -> Result<&'static Self> {
        GLOBAL_WASM_HOST
            .get_or_init(Self::init)
            .as_ref()
            .map_err(|err| anyhow!("{err}"))
    }

    pub fn validate_wasm_bytes(&self, wasm_bytes: &[u8]) -> Result<()> {
        wasmtime::Module::validate(&self.engine, wasm_bytes)
            .map_err(|e| anyhow!("{e}"))
            .context("WebAssembly validation failed")
    }

    pub fn create_tree_sitter_store(&self) -> Result<WasmStore> {
        WasmStore::new(&self.engine).context("Failed to create WasmStore")
    }

    /// Instantiates a `tree_sitter::Language` from a `.wasm` grammar binary.
    /// `wasm_lang_name` must match the exported `tree_sitter_<name>` symbol.
    pub fn load_tree_sitter_language(
        &self,
        wasm_lang_name: &str,
        wasm_bytes: &[u8],
    ) -> Result<Language> {
        self.validate_wasm_bytes(wasm_bytes)
            .with_context(|| format!("Validating wasm bytes for '{wasm_lang_name}'"))?;
        let mut store = self
            .create_tree_sitter_store()
            .with_context(|| format!("Creating WasmStore for '{wasm_lang_name}'"))?;
        store
            .load_language(wasm_lang_name, wasm_bytes)
            .with_context(|| format!("Failed to load tree-sitter language '{wasm_lang_name}'"))
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
        assert!(host.validate_wasm_bytes(MINIMAL_WASM_MODULE).is_ok());
    }

    #[test]
    fn test_wasm_host_rejects_invalid_or_corrupted_bytes() {
        let host = WasmHost::global().expect("WasmHost should initialize");
        assert!(host.validate_wasm_bytes(&[]).is_err());
        assert!(host.validate_wasm_bytes(b"not a wasm file").is_err());
        assert!(host.validate_wasm_bytes(b"\0asm\x99\x99\x99\x99").is_err());
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
}
