use super::{
    Editor, RenderMode, line_metrics::LineMetrics, parsed_line_store::ParsedLineStore,
    window_cache::EditWindowPatch,
};
use crate::{buffer::SourceLine, markdown::QuoteProjection};
use gpui::{Pixels, px};
use std::sync::Arc;

impl Editor {
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
        let theme_changed =
            current.parsed_lines.is_windowed() && current.is_dark_theme != self.is_dark_theme;
        let same_view = self.cache_matches_view(&current, width);
        if same_buffer
            && same_view
            && current.active_line == active
            && current.active_col == col
            && current.selection_range == self.selection_line_range()
            && current.marked_range == self.marked_range
            && current.image_cache_version == image
            && current.syntax_cache_version == syntax
        {
            return;
        }
        let sel_lines = self.selection_line_range();
        let new_marked_line = self.ime_preedit_line_and_cols().map(|(line, _)| line);
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
        let mut edit_patch = None;
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
                .block_pair(start.min(current.lines_len.saturating_sub(1)));
            let new_pair =
                projection.block_pair(start.min(self.buffer.line_count().saturating_sub(1)));
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

            let (local_new_range, is_code_interior, code_close) = if structural {
                (begin..self.buffer.line_count(), false, None)
            } else if let Some((_open, close)) = self
                .buffer
                .line_index()
                .structure()
                .fence_range(SourceLine::new(start))
            {
                (
                    start..(start + delta.new_line_count).min(self.buffer.line_count()),
                    true,
                    close.map(|c| c.index()),
                )
            } else {
                let line_delta = delta.new_line_count as isize - delta.old_line_count as isize;
                let ctx_begin = start
                    .saturating_sub(1)
                    .min(old_context.start)
                    .min(new_context.start);
                let shifted_old_end =
                    (old_context.end as isize + line_delta).max(ctx_begin as isize) as usize;
                let ctx_end = (start + delta.new_line_count + 1)
                    .max(shifted_old_end)
                    .max(new_context.end)
                    .min(self.buffer.line_count());
                (ctx_begin..ctx_end, false, None)
            };

            edit_patch = Some(EditWindowPatch {
                delta,
                structural,
                local_new_range,
                is_code_interior,
                code_close,
            });
        }
        let prev_parsed_lines = current.parsed_lines.clone();
        drop(current);
        let next_parsed_lines = if !same_buffer {
            if let Some(patch) = &edit_patch
                && !self.is_loading
            {
                prev_parsed_lines.apply_edit(&self.buffer, patch, self.is_dark_theme)
            } else {
                ParsedLineStore::windowed(&self.buffer)
                    .with_theme(self.is_dark_theme)
                    .with_session(self.background_session.clone())
                    .with_prepared(self.prepared_document.clone().filter(|snapshot| {
                        snapshot.revision == self.buffer.revision()
                            && (self.is_loading || snapshot.is_dark_theme == self.is_dark_theme)
                    }))
            }
        } else if theme_changed {
            prev_parsed_lines.update_theme(self.is_dark_theme)
        } else {
            prev_parsed_lines
        };
        let reset_metrics = initial || (!same_buffer && !incremental);
        if !self.is_loading
            && let Some(session) = &self.background_session
            && (reset_metrics
                || theme_changed
                || self
                    .prepared_document
                    .as_ref()
                    .is_none_or(|snapshot| snapshot.revision != self.buffer.revision()))
        {
            if let Some(patch) = &edit_patch
                && !theme_changed
            {
                let donated = next_parsed_lines.snapshot_windows();
                session.submit_target(
                    &self.buffer,
                    Some((patch, donated)),
                    self.is_dark_theme,
                    false,
                );
            } else {
                session.submit_target(
                    &self.buffer,
                    None,
                    self.is_dark_theme,
                    reset_metrics || theme_changed,
                );
            }
        }
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
        if !same_buffer || theme_changed {
            cache.parsed_lines = next_parsed_lines;
        }
        if same_view
            && (cache.active_line != active
                || cache.active_col != col
                || cache.selection_range != sel_lines
                || cache.marked_range != self.marked_range)
        {
            let map_old = |line: usize| {
                if let Some(delta) = delta.filter(|_| incremental) {
                    let start = delta.start_line.index();
                    if line >= start + delta.old_line_count {
                        return line - delta.old_line_count + delta.new_line_count;
                    }
                    if line >= start {
                        return start + (line - start).min(delta.new_line_count.saturating_sub(1));
                    }
                }
                line
            };
            if cache.active_line != active || cache.active_col != col {
                for line in [map_old(cache.active_line.index()), active.index()] {
                    if line < cache.metrics.len() {
                        dirty.push(line..line + 1);
                    }
                }
                for (start, end) in [cache.active_math_block, cache.active_mermaid_block]
                    .into_iter()
                    .flatten()
                {
                    dirty.push(map_old(start.index())..map_old(end.index()) + 1);
                }
                for (start, end) in [math, mermaid].into_iter().flatten() {
                    dirty.push(start.index()..end.index() + 1);
                }
            }
            if cache.selection_range != sel_lines {
                let old_sel = cache.selection_range.map(|(s, e)| {
                    (
                        SourceLine::new(map_old(s.index())),
                        SourceLine::new(map_old(e.index())),
                    )
                });
                for line in cache.parsed_lines.image_line_indices() {
                    let source_line = SourceLine::new(line);
                    let was_selected =
                        old_sel.is_some_and(|(s, e)| source_line >= s && source_line <= e);
                    let is_selected =
                        sel_lines.is_some_and(|(s, e)| source_line >= s && source_line <= e);
                    if was_selected != is_selected && line < cache.metrics.len() {
                        dirty.push(line..line + 1);
                    }
                }
            }
            if cache.marked_range != self.marked_range {
                for line in [
                    cache.marked_line.map(|line| map_old(line.index())),
                    new_marked_line.map(|line| line.index()),
                ]
                .into_iter()
                .flatten()
                {
                    if line < cache.metrics.len() {
                        dirty.push(line..line + 1);
                    }
                }
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
        cache.selection_range = sel_lines;
        cache.active_math_block = math;
        cache.active_mermaid_block = mermaid;
        cache.marked_range = self.marked_range.clone();
        cache.marked_line = new_marked_line;
        cache.doc_path = self.buffer.file_path_buf();
        cache.image_cache_version = image;
        cache.syntax_cache_version = syntax;
        cache.fold_version = self.display_map.version();
        cache.font_size = self.font_size;
        cache.line_height = self.line_height;
        cache.soft_wrap = self.soft_wrap;
        cache.is_dark_theme = self.is_dark_theme;
        cache.space_width = self.space_width();
        cache.quote_projection = projection;
        #[cfg(test)]
        {
            if !cache.separators.is_empty() {
                cache.separators = Arc::default();
            }
            if !cache.quote_ranges.is_empty() {
                cache.quote_ranges = Arc::default();
            }
        }
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
