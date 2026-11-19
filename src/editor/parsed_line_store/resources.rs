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

impl Resource {
    pub fn is_syntax(&self) -> bool {
        matches!(self.state, State::Syntax { .. })
    }

    pub fn collect(start: usize, rows: &[ParsedLine]) -> Vec<Self> {
        let retry_epoch = crate::syntax::syntax_retry_epoch();
        rows.iter()
            .enumerate()
            .filter_map(|(offset, row)| {
                let line = start + offset;
                match &row.kind {
                    BlockKind::Image { url, .. } => Some(Self {
                        range: line..line + 1,
                        state: State::Image {
                            url: url.clone(),
                            signature: image_signature(url),
                        },
                    }),
                    BlockKind::CodeBlock {
                        is_fence_start: true,
                    } => {
                        let content = crate::markdown::strip_quote_prefix(&row.raw_text);
                        let language = crate::markdown::fence_info(content)?.to_string();
                        let end = rows[offset + 1..]
                            .iter()
                            .position(|row| {
                                matches!(
                                    row.kind,
                                    BlockKind::CodeBlock {
                                        is_fence_start: false
                                    }
                                )
                            })
                            .map_or(start + rows.len(), |close| line + close + 2);
                        Some(Self {
                            range: line..end,
                            state: State::Syntax {
                                grammar: peek_grammar(&language).map(|g| Arc::downgrade(&g)),
                                language,
                                retry_epoch,
                            },
                        })
                    }
                    _ => None,
                }
            })
            .collect()
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
        let rel_start = self.range.start.saturating_sub(window_start) + 1;
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
