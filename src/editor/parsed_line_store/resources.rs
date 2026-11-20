use crate::buffer::{LineSource, SourceLine, SourceSnapshot, StructureIndex};
use crate::markdown::{BlockKind, ParsedLine};
use std::{
    ops::Range,
    sync::{Arc, Weak},
};

#[derive(Clone, Debug)]
pub(in crate::editor) struct Resource {
    pub range: Range<usize>,
    state: State,
}

#[derive(Clone, Debug)]
enum State {
    Image {
        url: String,
        signature: (bool, bool, Option<(u32, u32)>),
    },
    Syntax {
        language: String,
        grammar: Option<Weak<crate::syntax::LoadedGrammar>>,
        retry_epoch: u64,
    },
}

fn image_signature(url: &str) -> (bool, bool, Option<(u32, u32)>) {
    crate::http::read_cache(|cache| {
        (
            cache.is_loaded(url),
            cache.is_failed(url),
            cache.get_size(url),
        )
    })
}

fn peek_grammar(language: &str) -> Option<Arc<crate::syntax::LoadedGrammar>> {
    let id = crate::syntax::normalize_language_id(language)?;
    crate::syntax::peek_global_loaded(&id)
}

fn syntax_block_end(window_start: usize, rows: &[ParsedLine], search_from: usize) -> usize {
    rows[search_from..]
        .iter()
        .position(|row| {
            matches!(
                row.kind,
                BlockKind::CodeBlock {
                    is_fence_start: false
                }
            )
        })
        .map_or(window_start + rows.len(), |close| {
            window_start + search_from + close + 1
        })
}

fn fence_language(raw_text: &str) -> Option<String> {
    let content = crate::markdown::strip_quote_prefix(raw_text);
    crate::markdown::fence_info(content).map(str::to_string)
}

fn syntax_resource(range: Range<usize>, language: String, retry_epoch: u64) -> Resource {
    Resource {
        range,
        state: State::Syntax {
            grammar: peek_grammar(&language).map(|g| Arc::downgrade(&g)),
            language,
            retry_epoch,
        },
    }
}

impl Resource {
    pub fn is_syntax(&self) -> bool {
        matches!(self.state, State::Syntax { .. })
    }

    pub fn collect(
        start: usize,
        rows: &[ParsedLine],
        context: Option<(&SourceSnapshot, &StructureIndex)>,
    ) -> Vec<Self> {
        let retry_epoch = crate::syntax::syntax_retry_epoch();
        let mut resources = Vec::new();

        // When a window starts inside a multi-window fenced code block, recover its opening
        // language from the buffer's structure index so late grammar loads refresh this slice too.
        if let Some((source, index)) = context
            && let Some(first) = rows.first()
            && matches!(
                first.kind,
                BlockKind::CodeBlockContent
                    | BlockKind::CodeBlock {
                        is_fence_start: false
                    }
            )
            && let Some((open_line, _)) = index.fence_range(SourceLine::new(start))
            && open_line.index() < start
            && let Some(language) = fence_language(&source.source_line(open_line))
        {
            let end = syntax_block_end(start, rows, 0);
            resources.push(syntax_resource(start..end, language, retry_epoch));
        }

        for (offset, row) in rows.iter().enumerate() {
            let line = start + offset;
            match &row.kind {
                BlockKind::Image { url, .. } => resources.push(Self {
                    range: line..line + 1,
                    state: State::Image {
                        url: url.clone(),
                        signature: image_signature(url),
                    },
                }),
                BlockKind::CodeBlock {
                    is_fence_start: true,
                } => {
                    if let Some(language) = fence_language(&row.raw_text) {
                        let end = syntax_block_end(start, rows, offset + 1);
                        resources.push(syntax_resource(line..end, language, retry_epoch));
                    }
                }
                _ => {}
            }
        }
        resources
    }

    /// Clears captured grammar pointers for code blocks whose content rows have no syntax tokens
    /// when a grammar finished loading concurrently during window scanning.
    pub fn clear_raced_unhighlighted(&mut self, window_start: usize, rows: &[ParsedLine]) {
        let State::Syntax { grammar, .. } = &mut self.state else {
            return;
        };
        if grammar.is_none() {
            return;
        }
        let rel_offset = self.range.start.saturating_sub(window_start);
        let rel_start = if matches!(
            rows.get(rel_offset).map(|row| &row.kind),
            Some(BlockKind::CodeBlock {
                is_fence_start: true
            })
        ) {
            rel_offset + 1
        } else {
            rel_offset
        };
        let rel_end = self.range.end.saturating_sub(window_start).min(rows.len());
        if rel_start >= rel_end {
            return;
        }
        let content_rows = &rows[rel_start..rel_end];
        let has_content = content_rows
            .iter()
            .any(|row| matches!(row.kind, BlockKind::CodeBlockContent));
        let has_tokens = content_rows.iter().any(|row| {
            matches!(row.kind, BlockKind::CodeBlockContent)
                && row.spans.iter().any(|span| span.syntax_token.is_some())
        });
        if has_content && !has_tokens {
            *grammar = None;
        }
    }

    pub fn changed(&mut self, images: bool, syntax: bool) -> bool {
        match &mut self.state {
            State::Image { url, signature } if images => {
                let next = image_signature(url);
                let changed = *signature != next;
                *signature = next;
                changed
            }
            State::Syntax {
                language,
                grammar: previous,
                retry_epoch,
            } if syntax => {
                let next_epoch = crate::syntax::syntax_retry_epoch();
                let next = peek_grammar(language);
                if *retry_epoch != next_epoch {
                    *retry_epoch = next_epoch;
                    if next.is_none() {
                        *previous = None;
                        return true;
                    }
                }
                let Some(next_arc) = next else {
                    // Evicted or still-loading grammars do not force already-rendered windows back
                    // to plain text.
                    return false;
                };
                let same = previous
                    .as_ref()
                    .and_then(Weak::upgrade)
                    .is_some_and(|prev| Arc::ptr_eq(&prev, &next_arc));
                *previous = Some(Arc::downgrade(&next_arc));
                !same
            }
            _ => false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::buffer::TextBuffer;
    use crate::markdown::MarkdownScanner;

    const JSON_GRAMMAR_WASM: &[u8] =
        include_bytes!("../../../tests/fixtures/tree-sitter-json.wasm");

    #[test]
    fn split_code_block_window_collects_syntax_resource_and_detects_grammar_load() {
        let mut doc = String::from("```json\n");
        for _ in 0..10 {
            doc.push_str("{\"k\": 1}\n");
        }
        doc.push_str("```\n");
        let buffer = TextBuffer::from_str(&doc, None);
        let source = buffer.source_snapshot();
        let index = buffer.line_index().structure_snapshot();
        let (range, rows) = MarkdownScanner::scan_snapshot_window(&source, &index, 5..12);

        let mut resources = Resource::collect(range.start, &rows, Some((&source, &index)));
        assert_eq!(resources.len(), 1);
        assert_eq!(resources[0].range, 5..12);
        assert!(resources[0].is_syntax());

        let changed_before = resources[0].changed(false, true);
        let already_loaded = crate::syntax::peek_global_loaded("json").is_some();
        assert!(!changed_before);

        if !already_loaded {
            crate::syntax::register_global_wasm_grammar("json", JSON_GRAMMAR_WASM, None)
                .expect("register json grammar");
            assert!(resources[0].changed(false, true));
            assert!(!resources[0].changed(false, true));
        }
    }
}
