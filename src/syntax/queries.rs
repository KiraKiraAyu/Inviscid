use anyhow::{Context, Result, anyhow};
use tree_sitter::{Language, Query};

use super::builtins::{canonical_language_id, extract_fence_token};

/// Normalizes a Markdown code fence info-string (such as `"rs"`, `"hcl"`, `"elixir"`)
/// into a canonical Tree-sitter language identifier (`[a-z][a-z0-9_]*`).
pub fn normalize_language_id(lang_hint: &str) -> Option<String> {
    let token = extract_fence_token(lang_hint)?;
    if let Some(canonical) = canonical_language_id(token.as_ref()) {
        return Some(canonical.to_string());
    }

    let token_ref = token.as_ref();
    if token_ref.is_empty()
        || token_ref.len() > 32
        || !token_ref.starts_with(|c: char| c.is_ascii_lowercase())
        || !token_ref
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_' || c == '-')
    {
        return None;
    }

    Some(token_ref.replace('-', "_"))
}

/// Compiles a Tree-sitter highlight query against `language`, stripping individual incompatible
/// stanzas when a grammar version differs on specific node kinds.
pub fn compile_resilient_query(language: &Language, scm_source: &str) -> Result<Query> {
    let first_err = match Query::new(language, scm_source) {
        Ok(query) => return Ok(query),
        Err(err) => err,
    };

    let first_err_msg = first_err.to_string();
    let mut lines: Vec<&str> = scm_source.lines().collect();
    let mut err = first_err;

    for _ in 0..lines.len() {
        if !blank_enclosing_stanza(&mut lines, err.row) {
            break;
        }
        eprintln!(
            "Warning: Skipping incompatible Tree-sitter query pattern at line {}: {}",
            err.row + 1,
            err.message
        );
        let candidate = lines.join("\n");
        match Query::new(language, &candidate) {
            Ok(query) if query.pattern_count() > 0 => return Ok(query),
            Ok(_) => break,
            Err(next_err) => err = next_err,
        }
    }

    Err(anyhow!("{first_err_msg}")).context("Invalid query")
}

fn blank_enclosing_stanza(lines: &mut [&str], target_row: usize) -> bool {
    if lines.is_empty() {
        return false;
    }
    let clamped_row = target_row.min(lines.len() - 1);
    let mut depth = 0usize;
    let mut start_row = 0usize;
    let mut end_row = lines.len() - 1;
    let mut found_end = false;

    for (r, line) in lines.iter().enumerate() {
        if r <= clamped_row && depth == 0 {
            start_row = r;
        }
        let mut in_string = false;
        let mut escaped = false;
        for ch in line.chars() {
            if in_string {
                if escaped {
                    escaped = false;
                } else if ch == '\\' {
                    escaped = true;
                } else if ch == '"' {
                    in_string = false;
                }
                continue;
            }
            match ch {
                ';' => break,
                '"' => in_string = true,
                '(' | '[' => depth += 1,
                ')' | ']' => depth = depth.saturating_sub(1),
                _ => {}
            }
        }
        if r >= clamped_row && depth == 0 && !found_end {
            end_row = r;
            found_end = true;
        }
    }

    let mut changed = false;
    for slot in &mut lines[start_row..=end_row] {
        if !slot.is_empty() {
            *slot = "";
            changed = true;
        }
    }
    changed
}

/// Removes catch-all `(identifier) @variable` rules from remote `highlights.scm` files so plain
/// identifiers retain `text_primary` and do not shadow function or member captures.
pub fn sanitize_remote_highlights_scm(scm_source: &str) -> String {
    let mut sanitized = String::with_capacity(scm_source.len());
    for line in scm_source.lines() {
        let trimmed = line.trim();
        if trimmed != "(identifier) @variable" && trimmed != "(variable_identifier) @variable" {
            if !sanitized.is_empty() {
                sanitized.push('\n');
            }
            sanitized.push_str(line);
        }
    }
    sanitized
}

fn classify_named_node_kind(kind: &str) -> Option<&'static str> {
    if kind == "comment" || kind.ends_with("_comment") {
        return Some("@comment");
    }
    if kind == "escape_sequence" {
        return Some("@string.escape");
    }
    if matches!(
        kind,
        "string"
            | "string_literal"
            | "raw_string_literal"
            | "interpreted_string_literal"
            | "char_literal"
            | "character"
            | "character_literal"
            | "heredoc"
            | "heredoc_body"
            | "template_string"
            | "quoted_template"
            | "string_lit"
    ) {
        return Some("@string");
    }
    if matches!(
        kind,
        "number"
            | "integer"
            | "float"
            | "int_literal"
            | "float_literal"
            | "integer_literal"
            | "number_literal"
            | "numeric_literal"
            | "numeric_lit"
            | "decimal_literal"
            | "hex_literal"
            | "real_literal"
    ) {
        return Some("@number");
    }
    if matches!(
        kind,
        "boolean" | "boolean_literal" | "bool_lit" | "true" | "false"
    ) {
        return Some("@boolean");
    }
    if matches!(
        kind,
        "null" | "nil" | "none" | "null_literal" | "nil_literal"
    ) {
        return Some("@constant.builtin");
    }
    if matches!(
        kind,
        "type_identifier"
            | "builtin_type"
            | "primitive_type"
            | "predefined_type"
            | "type_specifier"
            | "type_name"
            | "module_name"
            | "constructor"
    ) {
        return Some("@type");
    }
    if matches!(
        kind,
        "function_name" | "function_identifier" | "method_name" | "command_name" | "function_call"
    ) {
        return Some("@function");
    }
    if matches!(
        kind,
        "attribute"
            | "decorator"
            | "annotation"
            | "builtin_identifier"
            | "preproc_include"
            | "preproc_def"
            | "directive"
    ) {
        return Some("@attribute");
    }
    if matches!(
        kind,
        "property_identifier"
            | "field_identifier"
            | "field_name"
            | "property_name"
            | "variable_name"
            | "attribute_name"
    ) {
        return Some("@property");
    }

    None
}

/// Synthesizes a fallback `highlights.scm` query from a [`Language`]'s symbol table when a
/// grammar package does not bundle its own query file.
pub fn synthesize_highlights_scm(language: &Language) -> String {
    use std::collections::HashSet;

    let mut rules = Vec::new();
    let mut keywords = Vec::new();
    let mut seen = HashSet::new();

    for id in 0..language.node_kind_count() as u16 {
        if !language.node_kind_is_visible(id) {
            continue;
        }
        let Some(kind) = language.node_kind_for_id(id) else {
            continue;
        };
        let is_named = language.node_kind_is_named(id);
        if language.id_for_node_kind(kind, is_named) == 0 || !seen.insert((kind, is_named)) {
            continue;
        }

        if is_named {
            if !kind.starts_with(|c: char| c.is_ascii_alphabetic())
                || !kind.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
            {
                continue;
            }

            if let Some(cap) = classify_named_node_kind(kind) {
                let candidate = format!("({}) {}", kind, cap);
                if Query::new(language, &candidate).is_ok() {
                    rules.push(candidate);
                }
            }
        } else if kind.len() >= 2
            && kind.starts_with(|c: char| c.is_ascii_lowercase())
            && kind.chars().all(|c| c.is_ascii_lowercase() || c == '_')
        {
            match kind {
                "true" | "false" => rules.push(format!("\"{}\" @boolean", kind)),
                "null" | "nil" | "none" => rules.push(format!("\"{}\" @constant.builtin", kind)),
                "self" | "this" | "super" => rules.push(format!("\"{}\" @variable.builtin", kind)),
                _ => keywords.push(format!("\"{}\"", kind)),
            }
        } else if matches!(kind, "(" | ")" | "[" | "]" | "{" | "}") {
            rules.push(format!("\"{}\" @punctuation.bracket", kind));
        } else if matches!(kind, "," | "." | ";" | ":") {
            rules.push(format!("\"{}\" @punctuation.delimiter", kind));
        }
    }

    if !keywords.is_empty() {
        rules.push(format!("[{}] @keyword", keywords.join(" ")));
    }

    rules.join("\n")
}
