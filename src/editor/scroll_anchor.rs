use super::Editor;
use crate::buffer::SourceLine;
use gpui::{Pixels, px};

const PRECISE_SCROLL_THRESHOLD: f64 = 16_000_000.0;

impl Editor {
    pub(crate) fn first_visible_source_line(&self) -> Option<SourceLine> {
        self.layout_snapshot()
            .metrics
            .line_at_relative_y(self.scroll.anchor_line, self.scroll.current_scroll_top)
            .map(SourceLine::new)
    }

    pub(super) fn rebase_scroll(&mut self) {
        let cached = self.layout_cache.borrow();
        let metrics = &cached.metrics;
        if metrics.is_empty()
            || (self.scroll.anchor_line.is_none()
                && metrics.total_height_precise() < PRECISE_SCROLL_THRESHOLD)
        {
            return;
        }
        let old_origin = self.scroll.anchor_line;
        let line = metrics
            .line_at_relative_y(old_origin, self.scroll.current_scroll_top)
            .unwrap();
        let delta = -metrics.top_relative(line, old_origin).unwrap();
        self.scroll.physics.compensate_layout(delta);
        self.scroll.anchor_line = Some(line);
        self.scroll.physics.min_scroll_top = px(-metrics.top_precise(line).unwrap() as f32);
        if let Some(start) = self.scroll.scrollbar_drag_start_scroll.as_mut() {
            *start += delta;
        }
    }

    pub(super) fn scroll_origin_y(&self) -> f64 {
        let cached = self.layout_cache.borrow();
        self.scroll
            .anchor_line
            .and_then(|line| {
                cached
                    .metrics
                    .top_precise(line.min(cached.metrics.len().saturating_sub(1)))
            })
            .unwrap_or(0.0)
    }

    pub(super) fn set_scroll_document_y(&mut self, position: f64) {
        self.ensure_layout_cache();
        let old_origin = self.scroll_origin_y();
        let max = self
            .scrollbar_metrics(self.viewport_height(), self.get_total_content_height())
            .max_scroll;
        let position = position.clamp(0.0, f64::from(f32::from(max)));
        let layout = self.layout_cache.borrow();
        let metrics = &layout.metrics;
        let (origin, offset) = if !metrics.is_empty()
            && (self.scroll.anchor_line.is_some()
                || metrics.total_height_precise() >= PRECISE_SCROLL_THRESHOLD)
        {
            let line = metrics.line_at_precise_y(position).unwrap();
            let origin_y = metrics.top_precise(line).unwrap();
            self.scroll.anchor_line = Some(line);
            (origin_y, px((position - origin_y) as f32))
        } else {
            self.scroll.anchor_line = None;
            (0.0, px(position as f32))
        };
        self.scroll.physics.min_scroll_top = px(-origin as f32);
        self.scroll
            .physics
            .set_direct(offset, px((f64::from(f32::from(max)) - origin) as f32));
        if let Some(start) = self.scroll.scrollbar_drag_start_scroll.as_mut() {
            *start += px((old_origin - origin) as f32);
        }
    }

    pub(super) fn line_scroll_y_range(&self, line: SourceLine) -> (Pixels, Pixels) {
        let layout = self.layout_snapshot();
        let top = layout
            .metrics
            .top_relative(line.index(), self.scroll.anchor_line)
            .unwrap_or(px(24.0));
        (
            top,
            top + layout.metrics.height(line.index()).unwrap_or(px(24.0)),
        )
    }
}

#[cfg(test)]
mod tests {
    use crate::buffer::{SourceLine, TextBuffer};
    use crate::editor::{Editor, RenderMode, line_metrics::LineMetrics};
    use gpui::{AppContext, Bounds, point, px, size};

    #[test]
    fn million_line_relative_geometry_retains_fractional_pixels() {
        let metrics = LineMetrics::estimated(1_000_000, px(24.125));
        let origin = Some(900_000);
        assert_eq!(metrics.top_relative(900_001, origin), Some(px(24.125)));
        assert_eq!(
            metrics.line_at_relative_y(origin, px(24.375)),
            Some(900_001)
        );
        assert_eq!(
            metrics.visible_lines_relative(origin, px(0.25), px(48.0)),
            900_000..900_002
        );
        assert_eq!(metrics.top_relative(899_999, origin), Some(px(-24.125)));
    }

    #[test]
    fn relative_queries_match_precise_oracle_with_collapsed_runs() {
        let heights = (0..5000)
            .map(|line| {
                if line == 0 {
                    px(20_000_000.0)
                } else if line % 7 < 3 {
                    px(0.0)
                } else {
                    px(24.125 + (line % 5) as f32 / 1024.0)
                }
            })
            .collect::<Vec<_>>();
        let metrics = LineMetrics::from_heights(heights.clone());
        let mut top = 24.0_f64;
        let offsets = heights
            .iter()
            .map(|height| {
                let result = top;
                top += f64::from(f32::from(*height));
                result
            })
            .collect::<Vec<_>>();
        let mut seed = 0x5eed_u64;
        for _ in 0..10_000 {
            seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1);
            let origin = 1 + (seed >> 32) as usize % 4999;
            let y = px(((seed >> 16) % 2000) as f32 / 16.0 - 62.5);
            let absolute = offsets[origin] + f64::from(f32::from(y));
            let expected = offsets
                .partition_point(|&top| top <= absolute)
                .saturating_sub(1);
            assert_eq!(metrics.line_at_relative_y(Some(origin), y), Some(expected));
            let end = offsets.partition_point(|&top| top < absolute + 700.0);
            let start = expected
                + usize::from(
                    offsets[expected] + f64::from(f32::from(heights[expected])) <= absolute,
                );
            assert_eq!(
                metrics.visible_lines_relative(Some(origin), y, px(700.0)),
                start.min(end)..end
            );
        }
    }

    #[gpui::test]
    fn scrollbar_jumps_and_smooth_scroll_keep_document_bounds(cx: &mut gpui::TestAppContext) {
        let text = vec!["x"; 20_000].join("\n");
        let editor = cx.new(|cx| Editor::new_with_buffer(TextBuffer::from_str(&text, None), cx));
        editor.update(cx, |ed, _| {
            ed.render_mode = RenderMode::Source;
            ed.font_size = 1000.0;
            ed.ensure_layout_cache();
            ed.set_scroll_document_y(20_000_000.25);
            ed.prepare_viewport_layout();
            let before = ed.scroll_origin_y() + f64::from(f32::from(ed.scroll.current_scroll_top));
            let max = ed.get_max_scroll_top();
            ed.scroll.scroll_by_wheel(px(-100.0), max);
            ed.scroll
                .step_interpolation_with_elapsed(std::time::Duration::from_millis(80));
            ed.rebase_scroll();
            let after = ed.scroll_origin_y() + f64::from(f32::from(ed.scroll.current_scroll_top));
            assert_eq!(after - before, 87.5);
            ed.set_scroll_document_y(0.0);
            ed.prepare_viewport_layout();
            assert_eq!(
                ed.scroll_origin_y() + f64::from(f32::from(ed.scroll.current_scroll_top)),
                0.0
            );
            ed.set_scroll_document_y(f64::MAX);
            let max = ed
                .scrollbar_metrics(ed.viewport_height(), ed.get_total_content_height())
                .max_scroll;
            assert!(
                (ed.scroll_origin_y() + f64::from(f32::from(ed.scroll.current_scroll_top))
                    - f64::from(f32::from(max)))
                .abs()
                    < 0.01
            );
        });
    }

    #[gpui::test]
    fn anchored_touchpad_and_hit_testing_keep_subpixel_offsets(cx: &mut gpui::TestAppContext) {
        let text = vec!["x"; 20_000].join("\n");
        let editor = cx.new(|cx| Editor::new_with_buffer(TextBuffer::from_str(&text, None), cx));
        editor.update(cx, |ed, _| {
            ed.render_mode = RenderMode::Source;
            ed.font_size = 1000.0;
            ed.viewport_bounds = Bounds::new(point(px(0.0), px(0.0)), size(px(900.0), px(700.0)));
            ed.ensure_layout_cache();
            ed.scroll.anchor_line = Some(11_000);
            ed.scroll.physics.set_direct(px(0.25), px(1_000_000.0));
            ed.prepare_viewport_layout();
            let line = SourceLine::new(11_000);
            assert_eq!(ed.first_visible_source_line(), Some(line));
            assert_eq!(
                ed.line_screen_bounds(line, ed.viewport_bounds).origin.y,
                px(-0.25)
            );
            assert_eq!(
                ed.find_line_at_point(point(px(100.0), px(100.0)), true)
                    .unwrap()
                    .0,
                line
            );
            for _ in 0..16 {
                let max = ed.get_max_scroll_top();
                ed.scroll.scroll_direct(px(-0.25), max);
                ed.prepare_viewport_layout();
            }
            assert_eq!(
                ed.line_screen_bounds(line, ed.viewport_bounds).origin.y,
                px(-4.25)
            );
            ed.measure_all_pending_for_test();
            assert_eq!(
                ed.line_screen_bounds(line, ed.viewport_bounds).origin.y,
                px(-4.25)
            );
        });
    }
}
