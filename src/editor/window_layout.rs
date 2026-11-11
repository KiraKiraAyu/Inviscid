use super::layout::DocumentLayoutCache;
use super::{Editor, RenderMode, line_metrics::LineMetrics, parsed_line_store::ParsedLineStore};
use crate::{buffer::SourceLine, markdown::QuoteProjection};
use gpui::{Pixels, px};
use std::ops::Range;
use std::sync::Arc;

impl Editor {
    pub(super) fn refresh_eager_resources(
        &self,
        cache: &mut DocumentLayoutCache,
        image: usize,
        syntax: usize,
    ) {
        let syntax_changed = cache.syntax_cache_version != syntax;
        let ranges = cache
            .parsed_lines
            .refresh_resources(cache.image_cache_version != image, syntax_changed);
        for range in ranges {
            if syntax_changed
                && cache.parsed_lines.get(range.start).is_some_and(|row| {
                    matches!(
                        row.kind,
                        crate::markdown::BlockKind::CodeBlock {
                            is_fence_start: true
                        }
                    )
                })
            {
                let (expanded, mut parsed) = cache.quote_projection.scan_window(range.clone());
                self.project_ime_preedit(expanded.start, &mut parsed);
                #[cfg(test)]
                self.layout_stats.update(|mut stats| {
                    stats.lines_scanned += parsed.len();
                    stats
                });
                cache.parsed_lines.make_mut().splice(expanded, parsed);
            }
            for row in range {
                if self
                    .display_map
                    .fold_map()
                    .is_line_folded(SourceLine::new(row))
                {
                    continue;
                }
                let handle = cache.parsed_lines.get(row).unwrap();
                let parsed = &*handle;
                let line = SourceLine::new(row);
                let context = self.line_layout_ctx(
                    parsed,
                    line == cache.active_line,
                    cache.separator(row),
                    cache.active_col,
                    cache.content_width,
                    line,
                    cache.active_math_block,
                    cache.active_mermaid_block,
                );
                cache
                    .metrics
                    .set_height(row, self.measure_line_layout_height(row, parsed, context));
            }
        }
        cache.image_cache_version = image;
        cache.syntax_cache_version = syntax;
    }

    pub(super) fn update_local_separators(
        &self,
        cache: &mut DocumentLayoutCache,
        delta: crate::buffer::EditDelta,
        mut range: Range<usize>,
        projection: &QuoteProjection,
    ) {
        if self.render_mode == RenderMode::Source {
            cache.separators = Arc::default();
            return;
        }
        let start = delta.start_line.index();
        let separators = Arc::make_mut(&mut cache.separators);
        separators.splice(
            start..start + delta.old_line_count,
            std::iter::repeat_n(false, delta.new_line_count),
        );
        range.start = range.start.saturating_sub(1);
        range.end = (range.end + 1).min(projection.len());
        while range.end < projection.len()
            && projection.normalized_line(range.end - 1).trim().is_empty()
        {
            range.end += 1;
        }
        for row in range {
            separators[row] = projection.separator(row);
        }
    }

    pub(super) fn try_incremental_layout_update(
        &self,
        cache: &mut DocumentLayoutCache,
        delta: crate::buffer::EditDelta,
        active: SourceLine,
        width: Pixels,
        projection: Arc<QuoteProjection>,
    ) -> bool {
        let start = delta.start_line.index();
        let old_end = start + delta.old_line_count;
        let new_end = start + delta.new_line_count;
        if old_end > cache.lines_len || new_end > self.buffer.line_count() {
            return false;
        }
        let structural = (start..old_end).any(|row| {
            let text = cache.quote_projection.normalized_line(row);
            let kind = crate::markdown::structure::classify(&text);
            kind.is_fence || kind.is_math_delim
        }) || (start..new_end).any(|row| {
            let text = projection.normalized_line(row);
            let kind = crate::markdown::structure::classify(&text);
            kind.is_fence || kind.is_math_delim
        });
        let mut range = start.saturating_sub(1)..if structural {
            projection.len()
        } else {
            (new_end + 1).min(projection.len())
        };
        let (range, mut parsed) = loop {
            while range.end < projection.len()
                && projection.normalized_line(range.end - 1).trim().is_empty()
            {
                range.end += 1;
            }
            let old_right = range.end.saturating_sub(delta.new_line_count) + delta.old_line_count;
            let (old, _) = cache
                .quote_projection
                .scan_window(range.start..old_right.min(cache.lines_len));
            range.start = range.start.min(old.start);
            range.end = range
                .end
                .max(old.end.saturating_sub(delta.old_line_count) + delta.new_line_count)
                .min(projection.len());
            let (expanded, parsed) = projection.scan_window(range.clone());
            if expanded == range {
                break (range, parsed);
            }
            range = expanded;
        };
        self.project_ime_preedit(range.start, &mut parsed);
        #[cfg(test)]
        self.layout_stats.update(|mut stats| {
            stats.lines_scanned += parsed.len();
            stats
        });
        let replaced_end = range.end.saturating_sub(delta.new_line_count) + delta.old_line_count;
        self.update_local_separators(cache, delta, range.clone(), &projection);
        cache
            .parsed_lines
            .make_mut()
            .splice(range.start..replaced_end, parsed);
        cache
            .parsed_lines
            .update_resources_after_edit(delta, range.clone());
        cache.quote_projection = projection;
        let previous_blocks = [cache.active_math_block, cache.active_mermaid_block];
        cache.active_math_block = (self.render_mode != RenderMode::Source)
            .then(|| {
                crate::markdown::math_block_range_with_projection(&cache.quote_projection, active)
            })
            .flatten()
            .filter(|(start, end)| active > *start && active < *end);
        cache.active_mermaid_block = (self.render_mode != RenderMode::Source)
            .then(|| {
                crate::markdown::mermaid_block_range_with_projection(
                    &cache.quote_projection,
                    active,
                )
            })
            .flatten();
        // Disclosure rows can move across the edited window.
        let old_active = cache.active_line.index();
        let mapped_active = if old_active >= old_end {
            old_active - delta.old_line_count + delta.new_line_count
        } else {
            old_active.min(self.buffer.line_count().saturating_sub(1))
        };
        cache.active_line = active;
        cache.active_col = self.disclosure_col();
        let heights = range
            .clone()
            .map(|row| {
                let handle = cache.parsed_lines.get(row).unwrap();
                let parsed = &*handle;
                let line = SourceLine::new(row);
                let context = self.line_layout_ctx(
                    parsed,
                    line == active,
                    cache.separator(row),
                    cache.active_col,
                    width,
                    line,
                    cache.active_math_block,
                    cache.active_mermaid_block,
                );
                self.measure_line_layout_height(row, parsed, context)
            })
            .collect();
        cache.metrics.splice(range.start..replaced_end, heights);
        let mut disclosure_rows = vec![mapped_active, active.index()];
        for (begin, end) in previous_blocks
            .into_iter()
            .chain([cache.active_math_block, cache.active_mermaid_block])
            .flatten()
        {
            let map = |line: usize| {
                if line >= old_end {
                    line - delta.old_line_count + delta.new_line_count
                } else {
                    line
                }
            };
            disclosure_rows
                .extend(map(begin.index())..=(map(end.index())).min(cache.metrics.len() - 1));
        }
        disclosure_rows.sort_unstable();
        disclosure_rows.dedup();
        for row in disclosure_rows {
            if row >= cache.metrics.len() || range.contains(&row) {
                continue;
            }
            let handle = cache.parsed_lines.get(row).unwrap();
            let parsed = &*handle;
            let line = SourceLine::new(row);
            let context = self.line_layout_ctx(
                parsed,
                line == active,
                cache.separator(row),
                cache.active_col,
                width,
                line,
                cache.active_math_block,
                cache.active_mermaid_block,
            );
            cache
                .metrics
                .set_height(row, self.measure_line_layout_height(row, parsed, context));
        }
        cache.quote_ranges = super::layout::quote_ranges(&cache.parsed_lines).into();
        cache.buffer_revision = self.buffer.revision();
        cache.buffer_version = self.buffer.version();
        cache.lines_len = self.buffer.line_count();
        for region in self.display_map.fold_map().folded_regions() {
            let start = (region.start_line.index() + 1).min(cache.lines_len);
            let end = (region.end_line.index() + 1).min(cache.lines_len);
            cache.metrics.set_uniform_measured(start..end, px(0.0));
        }
        true
    }

    pub(super) fn ensure_window_layout(
        &self,
        width: Pixels,
        active: SourceLine,
        col: usize,
        image: usize,
        syntax: usize,
    ) {
        let current = self.layout_cache.borrow().clone();
        let same_buffer = current.buffer_revision == self.buffer.revision()
            && current.lines_len == self.buffer.line_count()
            && current.parsed_lines.is_windowed();
        let same_view = self.cache_matches_view(&current, width);
        if same_buffer
            && same_view
            && current.active_line == active
            && current.active_col == col
            && current.image_cache_version == image
            && current.syntax_cache_version == syntax
        {
            return;
        }
        self.layout_epoch
            .set(self.layout_epoch.get().wrapping_add(1));
        let projection = if same_buffer {
            current.quote_projection.clone()
        } else {
            Arc::new(QuoteProjection::from_source(&self.buffer))
        };
        let math = (self.render_mode != RenderMode::Source)
            .then(|| crate::markdown::math_block_range_with_projection(&projection, active))
            .flatten()
            .filter(|(start, end)| active > *start && active < *end);
        let mermaid = (self.render_mode != RenderMode::Source)
            .then(|| crate::markdown::mermaid_block_range_with_projection(&projection, active))
            .flatten();
        let initial = !current.parsed_lines.is_windowed();
        let delta = self.buffer.last_edit_delta();
        let incremental = !same_buffer
            && !initial
            && current.buffer_revision.wrapping_add(1) == self.buffer.revision()
            && delta.is_some_and(|delta| {
                current.lines_len.saturating_sub(delta.old_line_count) + delta.new_line_count
                    == self.buffer.line_count()
            });
        let anchor = current.metrics.line_at_relative_y(
            self.scroll.anchor_line,
            self.scroll.current_scroll_top + self.layout_scroll_adjustment.get(),
        );
        let old_top =
            anchor.and_then(|line| current.metrics.top_relative(line, self.scroll.anchor_line));
        let mut mapped_anchor = anchor;
        let mut dirty = Vec::new();
        if incremental {
            let delta = delta.unwrap();
            let start = delta.start_line.index();
            mapped_anchor = anchor.map(|line| {
                if line >= start + delta.old_line_count {
                    line - delta.old_line_count + delta.new_line_count
                } else if line >= start {
                    start + (line - start).min(delta.new_line_count.saturating_sub(1))
                } else {
                    line
                }
            });
            let old_pair = current
                .quote_projection
                .parser_pair(start.min(current.lines_len - 1));
            let new_pair = projection.parser_pair(start.min(self.buffer.line_count() - 1));
            let end = old_pair
                .map_or(start + delta.new_line_count, |(_, end)| {
                    (end.index() + 1).saturating_sub(delta.old_line_count) + delta.new_line_count
                })
                .max(new_pair.map_or(0, |(_, end)| end.index() + 1));
            let begin = start
                .saturating_sub(1)
                .min(old_pair.map_or(start, |(start, _)| start.index()))
                .min(new_pair.map_or(start, |(start, _)| start.index()));
            // Delimiter edits can change pairing through the remaining document.
            let structural = (start..start + delta.old_line_count).any(|line| {
                let text = current.quote_projection.normalized_line(line);
                let kind = crate::markdown::structure::classify(&text);
                kind.is_fence || kind.is_math_delim
            }) || (start..start + delta.new_line_count).any(|line| {
                let text = projection.normalized_line(line);
                let kind = crate::markdown::structure::classify(&text);
                kind.is_fence || kind.is_math_delim
            });
            let old_context = current.quote_projection.context_range(
                start.saturating_sub(1)..(start + delta.old_line_count + 1).min(current.lines_len),
            );
            let new_context = projection.context_range(
                start.saturating_sub(1)
                    ..(start + delta.new_line_count + 1).min(self.buffer.line_count()),
            );
            let begin = begin.min(old_context.start).min(new_context.start);
            let end = end
                .max(old_context.end.saturating_sub(delta.old_line_count) + delta.new_line_count)
                .max(new_context.end);
            dirty.push(
                begin..if structural {
                    self.buffer.line_count()
                } else {
                    (end + 2).min(self.buffer.line_count())
                },
            );
        }
        drop(current);
        let mut cached = self.layout_cache.borrow_mut();
        let cache = Arc::make_mut(&mut cached);
        if initial || (!same_buffer && !incremental) || !same_view {
            cache.metrics = LineMetrics::estimated(
                self.buffer.line_count(),
                px(self.font_size * self.line_height),
            );
            #[cfg(test)]
            if initial || (!same_buffer && !incremental) {
                self.layout_stats.update(|mut stats| {
                    stats.full_rebuilds += 1;
                    stats
                });
            }
        } else if incremental {
            let delta = delta.unwrap();
            let start = delta.start_line.index();
            cache
                .metrics
                .splice_estimated(start..start + delta.old_line_count, delta.new_line_count);
        }
        if !same_buffer {
            let preedit = self.marked_range.clone().and_then(|range| {
                let start = self.buffer.utf16_offset_to_pos(range.start);
                let end = self.buffer.utf16_offset_to_pos(range.end);
                (start.line == end.line).then_some((start.line.index(), start.col..end.col))
            });
            cache.parsed_lines = ParsedLineStore::windowed(&self.buffer).with_preedit(preedit);
        }
        if same_buffer && same_view && (cache.active_line != active || cache.active_col != col) {
            for line in [cache.active_line.index(), active.index()] {
                if line < cache.metrics.len() {
                    dirty.push(line..line + 1);
                }
            }
            for (start, end) in [
                cache.active_math_block,
                math,
                cache.active_mermaid_block,
                mermaid,
            ]
            .into_iter()
            .flatten()
            {
                dirty.push(start.index()..end.index() + 1);
            }
        }
        for range in cache.parsed_lines.refresh_resources(
            cache.image_cache_version != image,
            cache.syntax_cache_version != syntax,
        ) {
            cache.metrics.invalidate_measured(range);
        }
        for range in dirty {
            cache.metrics.mark_unmeasured(range);
        }
        cache.buffer_revision = self.buffer.revision();
        cache.buffer_version = self.buffer.version();
        cache.lines_len = self.buffer.line_count();
        cache.render_mode = self.render_mode;
        cache.content_width = width;
        cache.active_line = active;
        cache.active_col = col;
        cache.active_math_block = math;
        cache.active_mermaid_block = mermaid;
        cache.doc_path = self.buffer.file_path_buf();
        cache.image_cache_version = image;
        cache.syntax_cache_version = syntax;
        cache.fold_version = self.display_map.version();
        cache.font_size = self.font_size;
        cache.line_height = self.line_height;
        cache.soft_wrap = self.soft_wrap;
        cache.space_width = self.space_width();
        cache.quote_projection = projection;
        cache.separators = Arc::default();
        cache.quote_ranges = Arc::default();
        for region in self.display_map.fold_map().folded_regions() {
            let start = (region.start_line.index() + 1).min(cache.metrics.len());
            let end = (region.end_line.index() + 1).min(cache.metrics.len());
            cache.metrics.set_uniform_measured(start..end, px(0.0));
        }
        if let Some((before, after)) = old_top.zip(mapped_anchor.and_then(|line| {
            cache
                .metrics
                .top_relative(line.min(cache.lines_len - 1), self.scroll.anchor_line)
        })) {
            self.layout_scroll_adjustment
                .set(self.layout_scroll_adjustment.get() + after - before);
        }
        self.measure_viewport(cache);
    }
}
