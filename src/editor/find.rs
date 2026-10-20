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
    pub fn open_find_bar(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let seed = self
            .buffer
            .selected_text()
            .filter(|text| !text.contains('\n'));

        let view = match &self.find_bar {
            Some(handle) => handle.view.clone(),
            None => {
                let view = cx.new(FindBar::new);
                let subscription = cx.subscribe_in(&view, window, Self::on_find_bar_event);
                self.find_bar = Some(FindBarHandle {
                    view: view.clone(),
                    _subscription: subscription,
                });
                view
            }
        };
        view.update(cx, |bar, cx| bar.focus_query(seed.as_deref(), window, cx));
        cx.notify();
    }

    pub fn close_find_bar(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.find_bar.take().is_none() {
            return;
        }
        self.find_state.clear();
        window.focus(&self.focus_handle);
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

    fn on_find_bar_event(
        &mut self,
        _bar: &Entity<FindBar>,
        event: &FindBarEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match event {
            FindBarEvent::QueryChanged(query) => self.set_find_query(query.clone(), cx),
            FindBarEvent::Next => self.find_next(cx),
            FindBarEvent::Previous => self.find_previous(cx),
            FindBarEvent::Dismiss => self.close_find_bar(window, cx),
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

    pub fn search_highlights(&self, line_idx: usize) -> Vec<LineHighlight> {
        if self.find_state.is_stale(self.buffer.revision()) {
            return Vec::new();
        }
        let line_len = self.buffer.line_len(line_idx);
        let current = self.find_state.current_match_index();

        self.find_state
            .matches_on_line(line_idx)
            .filter_map(|(idx, m)| {
                let start = if m.start.line == line_idx {
                    m.start.col
                } else {
                    0
                };
                let end = if m.end.line == line_idx {
                    m.end.col
                } else {
                    line_len
                };
                let continues = m.end.line > line_idx;
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
