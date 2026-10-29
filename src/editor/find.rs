use gpui::{AppContext, Context, Entity, Subscription, Window};

use crate::search::{FindState, SearchQuery, find_matches};

use super::find_bar::{FindBar, FindBarEvent, FindBarMatches};
use super::{Editor, HighlightKind, LineHighlight};

const MAX_FIND_MATCHES: usize = 10_000;

pub(crate) struct FindBarHandle {
    pub(crate) view: Entity<FindBar>,
    _subscription: Subscription,
}

impl Editor {
    fn get_or_create_find_bar(&mut self, cx: &mut Context<Self>) -> Entity<FindBar> {
        match &self.find_bar {
            Some(handle) => handle.view.clone(),
            None => {
                let view = cx.new(FindBar::new);
                let subscription = cx.subscribe(&view, |this, _, event, cx| {
                    this.on_find_bar_event(event, cx);
                });
                self.find_bar = Some(FindBarHandle {
                    view: view.clone(),
                    _subscription: subscription,
                });
                view
            }
        }
    }

    pub fn open_find_bar(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let seed = self
            .buffer
            .selected_text()
            .filter(|text| !text.contains('\n'));

        let view = self.get_or_create_find_bar(cx);
        view.update(cx, |bar, cx| bar.focus_query(seed.as_deref(), window, cx));
        cx.notify();
    }

    pub(crate) fn open_replace(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let seed = self
            .buffer
            .selected_text()
            .filter(|text| !text.contains('\n'));

        let view = self.get_or_create_find_bar(cx);
        view.update(cx, |bar, cx| {
            bar.set_replace_visible(true, cx);
            if let Some(text) = seed.as_deref() {
                bar.focus_query(Some(text), window, cx);
            } else if bar.query_text(cx).is_empty() {
                bar.focus_query(None, window, cx);
            } else {
                bar.focus_replace(window, cx);
            }
        });
        cx.notify();
    }

    pub fn close_find_bar(&mut self, window: Option<&mut Window>, cx: &mut Context<Self>) {
        if self.find_bar.take().is_none() {
            return;
        }
        self.find_state.clear();
        if let Some(window) = window {
            window.focus(&self.focus_handle);
        } else {
            self.needs_focus_after_find = true;
        }
        cx.notify();
    }

    pub fn detach_find_bar(&mut self, cx: &mut Context<Self>) -> Option<Entity<FindBar>> {
        let handle = self.find_bar.take()?;
        self.find_state.clear();
        cx.notify();
        Some(handle.view)
    }

    pub fn attach_find_bar(&mut self, view: Entity<FindBar>, cx: &mut Context<Self>) {
        let subscription = cx.subscribe(&view, |this, _, event, cx| {
            this.on_find_bar_event(event, cx);
        });
        self.find_bar = Some(FindBarHandle {
            view: view.clone(),
            _subscription: subscription,
        });

        let query = view.read(cx).search_query(cx);
        match query {
            Some(query) => {
                self.find_state.set_query(query);
                self.refresh_find_matches();
            }
            None => self.find_state.clear(),
        }
        self.sync_find_bar(cx);
        cx.notify();
    }

    pub fn is_find_bar_visible(&self) -> bool {
        self.find_bar.is_some()
    }

    pub fn find_bar(&self) -> Option<&Entity<FindBar>> {
        self.find_bar.as_ref().map(|handle| &handle.view)
    }

    pub fn find_state(&self) -> &FindState {
        &self.find_state
    }

    fn on_find_bar_event(&mut self, event: &FindBarEvent, cx: &mut Context<Self>) {
        match event {
            FindBarEvent::QueryChanged(query) => self.set_find_query(query.clone(), cx),
            FindBarEvent::Next => self.find_next(cx),
            FindBarEvent::Previous => self.find_previous(cx),
            FindBarEvent::Dismiss => self.close_find_bar(None, cx),
            FindBarEvent::ReplaceNext(replacement) => self.replace_next(replacement, cx),
            FindBarEvent::ReplaceAll(replacement) => self.replace_all(replacement, cx),
        }
    }

    pub fn set_find_query(&mut self, query: Option<SearchQuery>, cx: &mut Context<Self>) {
        match query {
            Some(query) => {
                self.find_state.set_query(query);
                self.refresh_find_matches();
                self.reveal_current_match(cx);
            }
            None => self.find_state.clear(),
        }
        self.sync_find_bar(cx);
        cx.notify();
    }

    pub(crate) fn refresh_find_matches(&mut self) {
        let Some(query) = self.find_state.query() else {
            return;
        };
        let text = self.buffer.lines().join("\n");
        let matches = find_matches(&text, query, MAX_FIND_MATCHES);
        let anchor = self.buffer.selection().start();
        self.find_state
            .set_matches(matches, self.buffer.revision(), anchor);
    }

    pub(crate) fn sync_find_bar(&self, cx: &mut Context<Self>) {
        let Some(handle) = &self.find_bar else {
            return;
        };
        let total = self.find_state.match_count();
        let matches = FindBarMatches {
            current: self.find_state.current_match_number(),
            total,
            is_capped: total >= MAX_FIND_MATCHES,
        };
        handle
            .view
            .update(cx, |bar, cx| bar.set_matches(matches, cx));
    }

    pub fn find_next(&mut self, cx: &mut Context<Self>) {
        self.step_match(cx, |state, wrap| state.next_match(wrap));
    }

    pub fn find_previous(&mut self, cx: &mut Context<Self>) {
        self.step_match(cx, |state, wrap| state.prev_match(wrap));
    }

    pub(crate) fn replace_next(&mut self, replacement: &str, cx: &mut Context<Self>) {
        if !self.find_state.is_active() || self.find_state.match_count() == 0 {
            return;
        }
        if self.find_state.is_stale(self.buffer.revision()) {
            self.refresh_find_matches();
        }
        let Some(query) = self.find_state.query().cloned() else {
            return;
        };
        let Some(m) = self.find_state.current_match().copied() else {
            return;
        };

        let matched_text = self.buffer.text_in_range((m.start, m.end));
        let rep_text = query.expand_replacement(&matched_text, replacement);
        self.buffer.replace_range((m.start, m.end), &rep_text);

        self.refresh_find_matches();
        self.reveal_current_match(cx);
        self.sync_find_bar(cx);
        cx.notify();
    }

    pub(crate) fn replace_all(&mut self, replacement: &str, cx: &mut Context<Self>) {
        if !self.find_state.is_active() || self.find_state.match_count() == 0 {
            return;
        }
        let Some(query) = self.find_state.query().cloned() else {
            return;
        };
        let count = self.buffer.replace_all_matches(&query, replacement);
        if count > 0 {
            self.refresh_find_matches();
            self.reveal_current_match(cx);
            self.sync_find_bar(cx);
            cx.notify();
        }
    }

    fn step_match(
        &mut self,
        cx: &mut Context<Self>,
        step: impl FnOnce(&mut FindState, bool) -> Option<usize>,
    ) {
        if !self.find_state.is_active() {
            return;
        }
        if self.find_state.is_stale(self.buffer.revision()) {
            self.refresh_find_matches();
        }

        let wrap_around = self
            .find_state
            .query()
            .map(|q| q.options().wrap_around)
            .unwrap_or(true);

        if step(&mut self.find_state, wrap_around).is_some() {
            self.reveal_current_match(cx);
            self.sync_find_bar(cx);
            cx.notify();
        }
    }

    pub fn search_highlights(&self, source_line: crate::buffer::SourceLine) -> Vec<LineHighlight> {
        if self.find_state.is_stale(self.buffer.revision()) {
            return Vec::new();
        }
        let line_idx = source_line.index();
        let line_len = self.buffer.line_len(source_line);
        let current = self.find_state.current_match_index();

        self.find_state
            .matches_on_line(source_line)
            .filter_map(|(idx, m)| {
                let start = if m.start.line.index() == line_idx {
                    m.start.col
                } else {
                    0
                };
                let end = if m.end.line.index() == line_idx {
                    m.end.col
                } else {
                    line_len
                };
                let continues = m.end.line.index() > line_idx;
                if start >= end && !continues {
                    return None;
                }
                let kind = if current == Some(idx) {
                    HighlightKind::ActiveSearchMatch
                } else {
                    HighlightKind::SearchMatch
                };
                Some(LineHighlight {
                    cols: start.min(line_len)..end.min(line_len),
                    continues,
                    kind,
                })
            })
            .collect()
    }

    fn reveal_current_match(&mut self, cx: &mut Context<Self>) {
        let Some(m) = self.find_state.current_match().copied() else {
            return;
        };
        let prev_head = self.buffer.cursor_pos();
        self.buffer.update_cursor(false, m.start);
        if self.buffer.cursor_pos() != prev_head {
            cx.emit(super::EditorEvent::CursorMoved);
        }
        self.scroll_to_cursor();
        self.start_scroll_animation_if_needed(cx);
    }

    pub(crate) fn pane_key_context(&self) -> &'static str {
        if self.find_bar.is_some() {
            crate::ui::key_context::EDITOR_PANE_WITH_FIND_ACTIVE
        } else {
            crate::ui::key_context::EDITOR_PANE
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::buffer::TextBuffer;
    use crate::config::AppConfig;
    use crate::theme::ThemeManager;
    use gpui::TestAppContext;

    fn init_globals(cx: &mut TestAppContext) {
        let config = AppConfig::default().with_theme("Dracula");
        let theme_manager = ThemeManager::from_config(&config);
        cx.update(|cx| {
            cx.set_global(theme_manager);
            cx.set_global(config);
            crate::app::actions::bind_default_bindings(cx);
        });
    }

    #[gpui::test]
    fn test_attach_and_detach_find_bar(cx: &mut TestAppContext) {
        init_globals(cx);

        let buf1 = TextBuffer::from_str("apple orange apple", None);
        let (ed1, cx) = cx.add_window_view(|_window, cx| Editor::new_with_buffer(buf1, cx));

        let buf2 = TextBuffer::from_str("banana apple grape", None);
        let (ed2, cx) = cx.add_window_view(|_window, cx| Editor::new_with_buffer(buf2, cx));

        cx.update(|window, cx| {
            ed1.update(cx, |ed, cx| {
                ed.open_find_bar(window, cx);
                let bar = ed.find_bar().cloned().expect("find bar opened");
                bar.update(cx, |bar, cx| bar.focus_query(Some("apple"), window, cx));
            });
        });

        ed1.read_with(cx, |ed, cx| {
            assert!(ed.is_find_bar_visible());
            assert_eq!(ed.find_state().match_count(), 2);
            assert_eq!(
                ed.search_highlights(crate::buffer::SourceLine::new(0))
                    .len(),
                2
            );
            assert_eq!(ed.find_bar().unwrap().read(cx).query_text(cx), "apple");
        });

        let bar = ed1
            .update(cx, |ed, cx| ed.detach_find_bar(cx))
            .expect("find bar was detached");

        ed1.read_with(cx, |ed, _| {
            assert!(!ed.is_find_bar_visible());
            assert_eq!(ed.find_state().match_count(), 0);
            assert!(
                ed.search_highlights(crate::buffer::SourceLine::new(0))
                    .is_empty()
            );
        });

        ed2.update(cx, |ed, cx| ed.attach_find_bar(bar, cx));

        ed2.read_with(cx, |ed, cx| {
            assert!(ed.is_find_bar_visible());
            assert_eq!(ed.find_state().match_count(), 1);
            assert_eq!(
                ed.search_highlights(crate::buffer::SourceLine::new(0))
                    .len(),
                1
            );
            assert_eq!(ed.find_bar().unwrap().read(cx).query_text(cx), "apple");
        });

        cx.update(|window, cx| {
            let bar = ed2.read(cx).find_bar().cloned().unwrap();
            bar.update(cx, |bar, cx| bar.focus_query(Some("grape"), window, cx));
        });

        ed2.read_with(cx, |ed, _| {
            assert_eq!(ed.find_state().match_count(), 1);
        });
        ed1.read_with(cx, |ed, _| {
            assert_eq!(ed.find_state().match_count(), 0);
        });
    }

    #[gpui::test]
    fn test_close_find_bar_clears_state(cx: &mut TestAppContext) {
        init_globals(cx);

        let buf = TextBuffer::from_str("testing close", None);
        let (ed, cx) = cx.add_window_view(|_window, cx| Editor::new_with_buffer(buf, cx));

        cx.update(|window, cx| {
            ed.update(cx, |ed, cx| {
                ed.open_find_bar(window, cx);
                let bar = ed.find_bar().cloned().unwrap();
                bar.update(cx, |bar, cx| bar.focus_query(Some("close"), window, cx));
            });
        });

        ed.read_with(cx, |ed, _| {
            assert!(ed.is_find_bar_visible());
            assert_eq!(ed.find_state().match_count(), 1);
        });

        ed.update(cx, |ed, cx| ed.close_find_bar(None, cx));

        ed.read_with(cx, |ed, _| {
            assert!(!ed.is_find_bar_visible());
            assert_eq!(ed.find_state().match_count(), 0);
        });
    }
}
