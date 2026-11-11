use crate::markdown::{BlockKind, ParsedLine};
use std::{ops::Range, sync::Arc};

#[derive(Clone, Debug)]
pub(super) struct Resource {
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
        grammar: Option<Arc<crate::syntax::LoadedGrammar>>,
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

fn grammar(language: &str) -> Option<Arc<crate::syntax::LoadedGrammar>> {
    let id = crate::syntax::normalize_language_id(language)?;
    crate::syntax::read_registry(|registry| registry.get_loaded(&id))
}

impl Resource {
    pub fn is_syntax(&self) -> bool {
        matches!(self.state, State::Syntax { .. })
    }

    pub fn collect(start: usize, rows: &[ParsedLine]) -> Vec<Self> {
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
                                grammar: grammar(&language),
                                language,
                            },
                        })
                    }
                    _ => None,
                }
            })
            .collect()
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
            } if syntax => {
                let next = grammar(language);
                let changed = match (&*previous, &next) {
                    (Some(a), Some(b)) => !Arc::ptr_eq(a, b),
                    (None, None) => false,
                    _ => true,
                };
                *previous = next;
                changed
            }
            _ => false,
        }
    }
}
