pub mod builtins;
pub mod queries;
pub mod registry;
pub mod token;

pub use builtins::*;
pub use queries::*;
pub use registry::*;
pub use token::*;

#[cfg(test)]
mod tests {
    use super::*;

    const JSON_GRAMMAR_WASM: &[u8] = include_bytes!("../tests/fixtures/tree-sitter-json.wasm");

    #[test]
    fn test_language_alias_normalization() {
        assert_eq!(resolve_grammar_spec("rs").unwrap().canonical_id, "rust");
        assert_eq!(
            resolve_grammar_spec("rust,ignore").unwrap().canonical_id,
            "rust"
        );
        assert_eq!(resolve_grammar_spec("PY").unwrap().canonical_id, "python");
        assert_eq!(
            resolve_grammar_spec("js {linenos=true}")
                .unwrap()
                .canonical_id,
            "javascript"
        );
        assert_eq!(
            resolve_grammar_spec("ts").unwrap().canonical_id,
            "typescript"
        );
        assert_eq!(resolve_grammar_spec("c++").unwrap().canonical_id, "cpp");
        assert_eq!(resolve_grammar_spec("c#").unwrap().canonical_id, "c_sharp");
        assert_eq!(resolve_grammar_spec("sh").unwrap().canonical_id, "bash");
        assert_eq!(resolve_grammar_spec("yml").unwrap().canonical_id, "yaml");
        assert!(resolve_grammar_spec("").is_none());
        assert!(resolve_grammar_spec("unknown_lang_xyz").is_none());
    }

    #[test]
    fn test_capture_name_to_syntax_token_mapping() {
        assert_eq!(
            SyntaxToken::from_capture_name("keyword.control"),
            Some(SyntaxToken::Keyword)
        );
        assert_eq!(
            SyntaxToken::from_capture_name("@function.method"),
            Some(SyntaxToken::Function)
        );
        assert_eq!(
            SyntaxToken::from_capture_name("type.builtin"),
            Some(SyntaxToken::Type)
        );
        assert_eq!(
            SyntaxToken::from_capture_name("string"),
            Some(SyntaxToken::String)
        );
        assert_eq!(
            SyntaxToken::from_capture_name("string.escape"),
            Some(SyntaxToken::Operator)
        );
        assert_eq!(
            SyntaxToken::from_capture_name("boolean"),
            Some(SyntaxToken::Number)
        );
        assert_eq!(
            SyntaxToken::from_capture_name("constant.builtin"),
            Some(SyntaxToken::Number)
        );
        assert_eq!(
            SyntaxToken::from_capture_name("comment"),
            Some(SyntaxToken::Comment)
        );
        assert!(SyntaxToken::Comment.is_italic());
        assert!(!SyntaxToken::Keyword.is_italic());
        assert_eq!(
            SyntaxToken::from_capture_name("property"),
            Some(SyntaxToken::Variable)
        );
        assert_eq!(
            SyntaxToken::from_capture_name("attribute"),
            Some(SyntaxToken::Attribute)
        );
        assert_eq!(SyntaxToken::from_capture_name("unknown_capture"), None);
    }

    #[test]
    fn test_resilient_query_filters_incompatible_stanzas() {
        let spec = resolve_grammar_spec("json").unwrap();
        let language = crate::wasm::WasmHost::global()
            .unwrap()
            .load_tree_sitter_language(spec.wasm_symbol(), JSON_GRAMMAR_WASM)
            .unwrap();

        let mixed_scm = r#"
            ; Valid stanza for JSON
            (string) @string
            ; Incompatible node type from another grammar version
            (nonexistent_future_node_xyz) @keyword
            (number) @number
        "#;

        let query = compile_resilient_query(&language, mixed_scm)
            .expect("Resilient query compiler should keep valid stanzas");
        assert!(query.capture_names().contains(&"string"));
        assert!(query.capture_names().contains(&"number"));
        assert!(!query.capture_names().contains(&"keyword"));

        let all_invalid = "(nonexistent_future_node_xyz) @keyword";
        let err = compile_resilient_query(&language, all_invalid)
            .expect_err("Completely invalid query must fail");
        assert!(format!("{err:#}").contains("Invalid query"));
    }

    #[test]
    fn test_highlight_json_code_block_with_unicode_and_nesting() {
        let spec = resolve_grammar_spec("json").unwrap();
        let grammar =
            LoadedGrammar::from_wasm_bytes(spec, JSON_GRAMMAR_WASM, None).expect("JSON grammar");

        let lines = vec![
            r#"{"#.to_string(),
            r#"  "标题": "Inviscid 编辑器\n","#.to_string(),
            r#"  "count": 42,"#.to_string(),
            r#"  "enabled": true"#.to_string(),
            r#"}"#.to_string(),
        ];

        let highlighted = highlight_lines_with_grammar(&grammar, &lines);
        assert_eq!(highlighted.len(), lines.len());

        for (line, spans) in lines.iter().zip(highlighted.iter()) {
            let reconstructed: String = spans.iter().map(|s| s.text.as_str()).collect();
            assert_eq!(&reconstructed, line);

            let mut expected_start = 0;
            for span in spans {
                assert_eq!(span.span_range.0, expected_start);
                assert_eq!(
                    span.span_range.1,
                    expected_start + span.text.chars().count()
                );
                expected_start = span.span_range.1;
            }
            assert_eq!(expected_start, line.chars().count());
        }

        let line1_spans = &highlighted[1];
        assert!(
            line1_spans
                .iter()
                .any(|s| s.text == r#""标题""# && s.syntax_token == Some(SyntaxToken::Variable))
        );
        assert!(
            line1_spans
                .iter()
                .any(|s| s.text.contains("Inviscid 编辑器")
                    && s.syntax_token == Some(SyntaxToken::String))
        );
        assert!(
            line1_spans
                .iter()
                .any(|s| s.text == r#"\n"# && s.syntax_token == Some(SyntaxToken::Operator))
        );

        let line2_spans = &highlighted[2];
        assert!(
            line2_spans
                .iter()
                .any(|s| s.text == "42" && s.syntax_token == Some(SyntaxToken::Number))
        );

        let line3_spans = &highlighted[3];
        assert!(
            line3_spans
                .iter()
                .any(|s| s.text == "true" && s.syntax_token == Some(SyntaxToken::Number))
        );
    }

    #[test]
    fn test_local_grammar_directory_discovery() {
        use std::sync::atomic::{AtomicUsize, Ordering};
        static COUNTER: AtomicUsize = AtomicUsize::new(0);
        let id = COUNTER.fetch_add(1, Ordering::Relaxed);
        let temp_dir = std::env::temp_dir().join(format!(
            "inviscid_syntax_dir_test_{}_{}",
            std::process::id(),
            id
        ));
        let json_dir = temp_dir.join("json");
        std::fs::create_dir_all(&json_dir).unwrap();
        std::fs::write(json_dir.join("grammar.wasm"), JSON_GRAMMAR_WASM).unwrap();

        let spec = resolve_grammar_spec("json").unwrap();
        let found = find_grammar_in_dirs(spec, &[temp_dir.as_path()]);
        assert!(found.is_some());
        let (bytes, custom_scm) = found.unwrap();
        assert_eq!(bytes.len(), JSON_GRAMMAR_WASM.len());
        assert!(custom_scm.is_none());

        let _ = std::fs::remove_dir_all(temp_dir);
    }

    #[test]
    fn test_build_grammar_download_url() {
        assert_eq!(GrammarCdnPreset::default(), GrammarCdnPreset::JsDelivr);
        assert_eq!(
            GrammarCdnPreset::default().url(),
            GrammarCdnPreset::DEFAULT_URL
        );

        let js_spec = resolve_grammar_spec("javascript").unwrap();
        let url = build_grammar_download_url(GrammarCdnPreset::default().url(), js_spec);
        assert_eq!(
            url.as_deref(),
            Some(
                "https://cdn.jsdelivr.net/npm/tree-sitter-javascript@latest/tree-sitter-javascript.wasm"
            )
        );

        let mirror_url = build_grammar_download_url("https://unpkg.com/", js_spec);
        assert_eq!(
            mirror_url.as_deref(),
            Some("https://unpkg.com/tree-sitter-javascript@latest/tree-sitter-javascript.wasm")
        );

        assert_eq!(build_grammar_download_url("", js_spec), None);
        assert_eq!(build_grammar_download_url("   ", js_spec), None);
    }

    #[test]
    fn test_normalize_language_id_supports_builtin_and_niche_languages() {
        assert_eq!(normalize_language_id("rs").as_deref(), Some("rust"));
        assert_eq!(normalize_language_id("c#").as_deref(), Some("c_sharp"));
        assert_eq!(normalize_language_id("tf").as_deref(), Some("hcl"));
        assert_eq!(normalize_language_id("terraform").as_deref(), Some("hcl"));
        assert_eq!(normalize_language_id("ex").as_deref(), Some("elixir"));
        assert_eq!(normalize_language_id("hs").as_deref(), Some("haskell"));
        assert_eq!(normalize_language_id("makefile").as_deref(), Some("make"));
        assert_eq!(normalize_language_id("nix").as_deref(), Some("nix"));
        assert_eq!(
            normalize_language_id("common-lisp").as_deref(),
            Some("common_lisp")
        );

        assert!(normalize_language_id("").is_none());
        assert!(normalize_language_id("../etc").is_none());
        assert!(normalize_language_id("foo/bar").is_none());
        assert!(normalize_language_id("123abc").is_none());
    }

    #[test]
    fn test_synthesize_highlights_scm_and_multiline_resilience() {
        let language = crate::wasm::WasmHost::global()
            .unwrap()
            .load_tree_sitter_language("json", JSON_GRAMMAR_WASM)
            .unwrap();

        let synthesized = synthesize_highlights_scm(&language);
        let query = tree_sitter::Query::new(&language, &synthesized)
            .expect("Synthesized query from WASM symbol table must compile");
        assert!(query.pattern_count() > 0);

        let multiline_mixed = r#"
(string) @string
(nonexistent_outer_node
  child: (string) @keyword
)
[
  "nonexistent_keyword_literal"
] @keyword
(number) @number
"#;
        let resilient = compile_resilient_query(&language, multiline_mixed)
            .expect("Multi-line incompatible stanzas should be stripped");
        assert!(resilient.capture_names().contains(&"string"));
        assert!(resilient.capture_names().contains(&"number"));
        assert!(!resilient.capture_names().contains(&"keyword"));
    }

    #[test]
    fn test_debounced_grammar_loader_cancels_superseded_requests() {
        use std::sync::atomic::Ordering;

        let loader = DebouncedGrammarLoader::new();
        loader.schedule_remote_fetch("has");
        // Intermediate typing simulation
        std::thread::sleep(std::time::Duration::from_millis(20));
        loader.schedule_remote_fetch("hask");
        std::thread::sleep(std::time::Duration::from_millis(20));
        loader.schedule_remote_fetch("haske");
        std::thread::sleep(std::time::Duration::from_millis(20));
        loader.schedule_remote_fetch("haskell");

        let (lock, _) = &*loader.inner;
        let inner = lock.lock();
        // Earlier superseded requests must not remain in the scheduled map
        assert!(!inner.scheduled.contains_key("has"));
        assert!(!inner.scheduled.contains_key("hask"));
        assert!(!inner.scheduled.contains_key("haske"));
        // Final stable request must be present and not cancelled
        assert!(inner.scheduled.contains_key("haskell"));
        assert!(
            !inner.scheduled["haskell"]
                .cancel_token
                .load(Ordering::Relaxed)
        );
    }
}
