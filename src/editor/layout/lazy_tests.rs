use super::{Editor, LineMetrics, SourceLine};
use crate::buffer::TextBuffer;
use gpui::{AppContext, Bounds, point, px, size};
use std::sync::Arc;

#[gpui::test]
fn editing_a_large_paragraph_keeps_height_measurement_local(cx: &mut gpui::TestAppContext) {
    let text = "paragraph with words\n".repeat(20_000);
    for mode in [
        crate::editor::RenderMode::Source,
        crate::editor::RenderMode::LivePreview,
    ] {
        for insertion in ["changed ", "first\nsecond"] {
            let editor =
                cx.new(|cx| Editor::new_with_buffer(TextBuffer::from_str(&text, None), cx));
            editor.update(cx, |ed, _| {
                ed.render_mode = mode;
                ed.ensure_layout_cache();
                ed.scroll.set_direct(px(400_000.0), px(1_000_000.0));
                ed.prepare_viewport_layout();
                let before = ed.first_visible_source_line().unwrap();
                let before_y = ed
                    .line_screen_bounds(before, ed.viewport_bounds_val())
                    .origin
                    .y;
                ed.buffer
                    .set_cursor(crate::buffer::Position::new(SourceLine::new(10_000), 0));
                ed.layout_stats.set(super::super::LayoutStats::default());
                ed.buffer.insert_text(insertion);
                ed.prepare_viewport_layout();
                assert_eq!(ed.layout_stats.get().full_rebuilds, 0);
                assert!(
                    ed.layout_stats.get().lines_measured <= 128,
                    "{:?}",
                    ed.layout_stats.get()
                );
                assert!(ed.layout_snapshot().metrics.unmeasured_count() > 19_000);
                let after = SourceLine::new(before.index() + usize::from(insertion.contains('\n')));
                assert_eq!(ed.first_visible_source_line(), Some(after));
                assert!(
                    (ed.line_screen_bounds(after, ed.viewport_bounds_val())
                        .origin
                        .y
                        - before_y)
                        .abs()
                        < px(0.01)
                );
                ed.measure_all_pending_for_test();
                let layout = ed.layout_snapshot();
                let eager = LineMetrics::from_heights(ed.measure_range(
                    0..layout.lines_len,
                    &layout.parsed_lines,
                    &layout.separators,
                    layout.content_width,
                    layout.active_math_block,
                    layout.active_mermaid_block,
                ));
                assert_eq!(layout.metrics, eager);
            });
        }
    }
}

#[gpui::test]
fn viewport_updates_reuse_validation_and_separator_storage(cx: &mut gpui::TestAppContext) {
    let text = "paragraph\n".repeat(20_000);
    let editor = cx.new(|cx| Editor::new_with_buffer(TextBuffer::from_str(&text, None), cx));
    editor.update(cx, |ed, cx| {
        ed.ensure_layout_cache();
        let old = ed.layout_snapshot();
        ed.layout_validation_scans.set(0);
        ed.scroll.set_direct(px(1200.0), px(100_000.0));
        ed.prepare_viewport_layout();
        ed.schedule_layout_measurement(cx);
        let current = ed.layout_snapshot();
        assert_eq!(ed.layout_validation_scans.get(), 0);
        assert_eq!(old.separators.as_ptr(), current.separators.as_ptr());
        assert!(Arc::ptr_eq(&old.quote_ranges, &current.quote_ranges));
    });
}

#[gpui::test]
fn view_changes_reuse_parsing_and_measure_only_viewport(cx: &mut gpui::TestAppContext) {
    let text = "# heading\n\n> quote\n> next\n\n| a | b |\n| --- | --- |\n| c | d |\n\nparagraph with words\n".repeat(2100);
    let editor = cx.new(|cx| Editor::new_with_buffer(TextBuffer::from_str(&text, None), cx));
    editor.update(cx, |ed, _| {
        ed.viewport_bounds = Bounds::new(point(px(0.0), px(0.0)), size(px(900.0), px(700.0)));
        ed.ensure_layout_cache();
        let parsed = ed.layout_snapshot().parsed_lines.clone();
        for change in 0..6 {
            ed.layout_stats.set(super::super::LayoutStats::default());
            match change {
                0 => ed.viewport_bounds.size.width = px(600.0),
                1 => ed.font_size = 21.0,
                2 => ed.line_height = 1.9,
                3 => ed.soft_wrap = false,
                4 => ed.fold_internal(SourceLine::new(0), SourceLine::new(10_000)),
                5 => {
                    ed.unfold_internal(SourceLine::new(0));
                }
                _ => unreachable!(),
            }
            ed.prepare_viewport_layout();
            let stats = ed.layout_stats.get();
            assert_eq!(stats.lines_scanned, 0, "change {change}: {stats:?}");
            assert_eq!(stats.full_rebuilds, 0, "change {change}: {stats:?}");
            assert!(stats.lines_measured <= 128, "change {change}: {stats:?}");
            assert!(Arc::ptr_eq(&parsed, &ed.layout_snapshot().parsed_lines));
            ed.measure_all_pending_for_test();
            let layout = ed.layout_snapshot();
            let eager = LineMetrics::from_heights(ed.measure_range(
                0..layout.lines_len,
                &layout.parsed_lines,
                &layout.separators,
                layout.content_width,
                layout.active_math_block,
                layout.active_mermaid_block,
            ));
            assert_eq!(layout.metrics, eager, "change {change}");
            for line in 0..eager.len() {
                assert_eq!(layout.metrics.top(line), eager.top(line));
            }
        }
    });
}
