use super::Editor;
use gpui::Context;
use std::time::{Duration, Instant};

impl Editor {
    pub(crate) fn schedule_layout_measurement(&mut self, cx: &mut Context<Self>) {
        self.ensure_layout_cache();
        let epoch = self.layout_epoch.get();
        if self.layout_task.is_some() && self.layout_task_epoch == epoch {
            return;
        }
        self.layout_task = None;
        if self.layout_cache.borrow().metrics.unmeasured_count() == 0 {
            return;
        }
        self.layout_task_epoch = epoch;
        self.layout_task = Some(cx.spawn(async move |this, cx| {
            loop {
                // Leave a gap for native input and paint messages between measurement slices.
                cx.background_executor()
                    .timer(Duration::from_millis(8))
                    .await;
                let pending = this.update(cx, |editor, cx| {
                    let current = {
                        let cache = editor.layout_cache.borrow();
                        editor.layout_epoch.get() == epoch
                            && cache.buffer_revision == editor.buffer.revision()
                            && editor.cache_matches_view(
                                &cache,
                                crate::editor::shaping::compute_content_width(
                                    editor.viewport_bounds_val().size.width,
                                    editor.render_mode,
                                ),
                            )
                    };
                    if !current {
                        editor.layout_task = None;
                        editor.schedule_layout_measurement(cx);
                        return false;
                    }
                    let pending = editor.measure_pending_slice(Duration::from_millis(2));
                    editor.layout_notifier.notify();
                    if !pending {
                        editor.layout_task = None;
                    }
                    pending
                });
                if !matches!(pending, Ok(true)) {
                    break;
                }
            }
        }));
    }

    fn measure_pending_slice(&mut self, budget: Duration) -> bool {
        let start = Instant::now();
        let pending = {
            let mut cached = self.layout_cache.borrow_mut();
            let cache = std::sync::Arc::make_mut(&mut cached);
            while let Some(line) = cache.metrics.next_unmeasured(0) {
                self.measure_pending_range(cache, line..line + 1);
                if start.elapsed() >= budget {
                    break;
                }
            }
            cache.metrics.unmeasured_count() > 0
        };
        self.apply_layout_scroll_adjustment();
        self.rebase_scroll();
        pending
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::buffer::TextBuffer;
    use gpui::AppContext;

    fn finish_pending(editor: &gpui::Entity<Editor>, cx: &mut gpui::TestAppContext) {
        for _ in 0..30_000 {
            cx.run_until_parked();
            if editor.read_with(cx, |ed, _| ed.layout_task.is_none()) {
                return;
            }
            cx.executor().advance_clock(Duration::from_millis(8));
        }
        panic!("layout measurement did not finish");
    }

    #[gpui::test]
    fn pending_measurement_restarts_after_edit_and_finishes_idle(cx: &mut gpui::TestAppContext) {
        let text = "# heading\n\nparagraph\n".repeat(7000);
        let editor = cx.new(|cx| Editor::new_with_buffer(TextBuffer::from_str(&text, None), cx));
        editor.update(cx, |ed, cx| {
            ed.schedule_layout_measurement(cx);
            assert!(ed.layout_task.is_some());
            let epoch = ed.layout_task_epoch;
            ed.buffer.insert_text("changed ");
            ed.schedule_layout_measurement(cx);
            assert!(ed.layout_task_epoch > epoch);
        });
        finish_pending(&editor, cx);
        editor.read_with(cx, |ed, _| {
            assert_eq!(ed.layout_snapshot().metrics.unmeasured_count(), 0);
            assert!(ed.layout_task.is_none());
        });
        let measured = editor.read_with(cx, |ed, _| ed.layout_stats.get().lines_measured);
        cx.run_until_parked();
        editor.read_with(cx, |ed, _| {
            assert_eq!(ed.layout_stats.get().lines_measured, measured)
        });
    }

    #[gpui::test]
    fn pending_measurement_does_not_retain_closed_editor(cx: &mut gpui::TestAppContext) {
        let text = "paragraph\n".repeat(20_000);
        let editor = cx.new(|cx| Editor::new_with_buffer(TextBuffer::from_str(&text, None), cx));
        editor.update(cx, |ed, cx| ed.schedule_layout_measurement(cx));
        let weak = editor.downgrade();
        drop(editor);
        cx.run_until_parked();
        assert!(weak.upgrade().is_none());
    }
}
