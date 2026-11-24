use gpui::*;
use std::cell::RefCell;
use std::collections::HashMap;
use std::ops::Range;
use std::rc::Rc;
use std::sync::Arc;

use crate::markdown::InlineSpan;
use crate::theme::Theme;

use super::runs::{LineShapingContext, build_line_runs};

const SHAPED_ROW_CACHE_LIMIT: usize = 512;

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub(crate) struct ShapedLineKey {
    pub buffer_revision: u64,
    pub line_idx: usize,
    pub cell_idx: Option<usize>,
    pub theme_fingerprint: u64,
    pub font_size_bits: u32,
    pub wrap_width_bits: Option<u32>,
    pub override_color_bits: Option<(u32, u32, u32, u32)>,
    pub is_source: bool,
    pub is_monospace: bool,
    pub is_heading: bool,
    pub is_active_caret: bool,
    pub disclosure_col: usize,
    pub is_selection_disclosure: bool,
    pub ime_preedit_cols: Option<(usize, usize)>,
    pub hovered_link_range: Option<(usize, usize)>,
}

impl ShapedLineKey {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        buffer_revision: u64,
        line_idx: usize,
        cell_idx: Option<usize>,
        theme: &Theme,
        font_size: Pixels,
        wrap_width: Option<Pixels>,
        override_color: Option<Hsla>,
        scx: &LineShapingContext<'_>,
        hovered_link_range: Option<(usize, usize)>,
    ) -> Self {
        let effective_active = !scx.is_source && scx.is_active_caret;
        Self {
            buffer_revision,
            line_idx,
            cell_idx,
            theme_fingerprint: theme.fingerprint(),
            font_size_bits: f32::from(font_size).to_bits(),
            wrap_width_bits: wrap_width.map(|w| f32::from(w).to_bits()),
            override_color_bits: override_color
                .map(|c| (c.h.to_bits(), c.s.to_bits(), c.l.to_bits(), c.a.to_bits())),
            is_source: scx.is_source,
            is_monospace: scx.is_monospace,
            is_heading: scx.is_heading,
            is_active_caret: effective_active,
            disclosure_col: if effective_active { scx.cursor_col } else { 0 },
            is_selection_disclosure: effective_active && scx.is_selection_disclosure,
            ime_preedit_cols: scx.ime_preedit_cols,
            hovered_link_range,
        }
    }
}

#[derive(Clone)]
pub(crate) struct CachedShapedLine {
    pub wrapped_line: Option<Arc<WrappedLine>>,
    pub visual_lines: usize,
}

#[derive(Default)]
pub(crate) struct ShapedRowCache {
    entries: HashMap<ShapedLineKey, (CachedShapedLine, u64)>,
    tick: u64,
}

impl ShapedRowCache {
    pub fn get(&mut self, key: &ShapedLineKey) -> Option<CachedShapedLine> {
        let (entry, last_used) = self.entries.get_mut(key)?;
        self.tick = self.tick.wrapping_add(1);
        *last_used = self.tick;
        Some(entry.clone())
    }

    pub fn insert(&mut self, key: ShapedLineKey, value: CachedShapedLine) {
        if !self.entries.contains_key(&key)
            && self.entries.len() >= SHAPED_ROW_CACHE_LIMIT
            && let Some(oldest_key) = self
                .entries
                .iter()
                .min_by_key(|(_, (_, used))| *used)
                .map(|(k, _)| k.clone())
        {
            self.entries.remove(&oldest_key);
        }
        self.tick = self.tick.wrapping_add(1);
        self.entries.insert(key, (value, self.tick));
    }

    pub fn clear(&mut self) {
        self.entries.clear();
    }

    pub fn invalidate_range(&mut self, range: Range<usize>) {
        self.entries
            .retain(|key, _| key.line_idx < range.start || key.line_idx >= range.end);
    }

    #[allow(clippy::too_many_arguments)]
    pub fn apply_edit(
        &mut self,
        old_rev: u64,
        new_rev: u64,
        line_delta: isize,
        dirty_old_range: Range<usize>,
        edit_old_range: Range<usize>,
        structural: bool,
        mut can_reuse_dirty_neighbor: impl FnMut(usize, usize) -> bool,
    ) {
        let previous: Vec<_> = self.entries.drain().collect();
        for (mut key, value) in previous {
            if key.buffer_revision != old_rev {
                continue;
            }
            if key.line_idx < dirty_old_range.start {
                key.buffer_revision = new_rev;
                self.entries.insert(key, value);
            } else if !structural && key.line_idx >= dirty_old_range.end {
                key.buffer_revision = new_rev;
                key.line_idx = ((key.line_idx as isize) + line_delta).max(0) as usize;
                self.entries.insert(key, value);
            } else if !structural
                && (key.line_idx < edit_old_range.start || key.line_idx >= edit_old_range.end)
            {
                let new_line_idx = if key.line_idx < edit_old_range.start {
                    key.line_idx
                } else {
                    ((key.line_idx as isize) + line_delta).max(0) as usize
                };
                if can_reuse_dirty_neighbor(key.line_idx, new_line_idx) {
                    key.buffer_revision = new_rev;
                    key.line_idx = new_line_idx;
                    self.entries.insert(key, value);
                }
            }
        }
    }

    #[cfg(test)]
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    #[cfg(test)]
    pub fn contains_line(&self, line_idx: usize, buffer_revision: u64) -> bool {
        self.entries
            .keys()
            .any(|key| key.line_idx == line_idx && key.buffer_revision == buffer_revision)
    }
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn shape_line_or_cached(
    key: &ShapedLineKey,
    spans: &[InlineSpan],
    scx: LineShapingContext<'_>,
    hovered_link_range: Option<(usize, usize)>,
    font_size: Pixels,
    wrap_width: Option<Pixels>,
    text_system: &Arc<WindowTextSystem>,
    base_style: &TextStyle,
    cache: Option<&Rc<RefCell<ShapedRowCache>>>,
) -> Option<CachedShapedLine> {
    if let Some(cache) = cache
        && let Some(hit) = cache.borrow_mut().get(key)
    {
        return Some(hit);
    }

    let (line_str, runs) = build_line_runs(spans, scx, hovered_link_range, base_style);
    let entry = if line_str.is_empty() || runs.is_empty() {
        CachedShapedLine {
            wrapped_line: None,
            visual_lines: 1,
        }
    } else {
        let shaped = text_system
            .shape_text(line_str.into(), font_size, &runs, wrap_width, None)
            .ok()?;
        let mut iter = shaped.into_iter();
        let wrapped_line = iter.next()?;
        let visual_lines = wrapped_line.wrap_boundaries.len() + 1;
        CachedShapedLine {
            wrapped_line: Some(Arc::new(wrapped_line)),
            visual_lines,
        }
    };

    if let Some(cache) = cache {
        cache.borrow_mut().insert(key.clone(), entry.clone());
    }
    Some(entry)
}
