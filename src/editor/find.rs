use gpui::{AppContext, Context, Entity, Subscription, Task, Window};
use std::collections::VecDeque;
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

use crate::buffer::{Position, SourceLine};
use crate::search::{FindState, SearchMatch, SearchQuery, find_matches, find_matches_in_rope};

use super::find_bar::{FindBar, FindBarEvent, FindBarMatches};
use super::{Editor, HighlightKind, LineHighlight};

const MAX_FIND_MATCHES: usize = 10_000;

#[derive(Default)]
pub(crate) struct FindSearch {
    generation: u64,
    pending: Option<PendingFindSearch>,
    actions: VecDeque<FindAction>,
    reveal: bool,
}

impl FindSearch {
    fn cancel(&mut self) {
        self.generation = self.generation.wrapping_add(1);
        self.pending = None;
    }
}

struct PendingFindSearch {
    revision: u64,
    cancel: Arc<AtomicBool>,
    _task: Task<()>,
}

impl Drop for PendingFindSearch {
    fn drop(&mut self) {
        self.cancel.store(true, Ordering::Relaxed);
    }
}

enum FindAction {
    Next,
    Previous,
    ReplaceNext(String),
    ReplaceAll(String),
}

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
        self.reset_find_search();
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
        self.reset_find_search();
        self.find_state.clear();
        cx.notify();
        Some(handle.view)
    }

    pub fn attach_find_bar(&mut self, view: Entity<FindBar>, cx: &mut Context<Self>) {
        self.reset_find_search();
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
                self.refresh_find_matches(cx);
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
        self.reset_find_search();
        match query {
            Some(query) => {
                self.find_state.set_query(query);
                self.find_search.reveal = true;
                self.refresh_find_matches(cx);
            }
            None => self.find_state.clear(),
        }
        self.sync_find_bar(cx);
        cx.notify();
    }

    pub(crate) fn reset_find_search(&mut self) {
        self.find_search.cancel();
        self.find_search.actions.clear();
        self.find_search.reveal = false;
        if let Some(query) = self.find_state.query().cloned() {
            self.find_state.set_query(query);
        }
    }

    pub(crate) fn clear_pending_find_actions(&mut self) {
        self.find_search.actions.clear();
        self.find_search.reveal = false;
    }

    pub(crate) fn refresh_find_matches(&mut self, cx: &mut Context<Self>) {
        let Some(query) = self.find_state.query().cloned() else {
            return;
        };
        let revision = self.buffer.revision();
        if self
            .find_search
            .pending
            .as_ref()
            .is_some_and(|pending| pending.revision == revision)
        {
            return;
        }
        self.find_search.cancel();
        if let Some(matches) = self.incremental_find_matches(&query) {
            self.find_state
                .set_matches(matches, revision, self.buffer.selection().start());
            return;
        }

        let snapshot = self.buffer.text_snapshot();
        let generation = self.find_search.generation;
        let selection = self.buffer.selection();
        let cancel = Arc::new(AtomicBool::new(false));
        let worker_cancel = cancel.clone();
        let search = cx.background_executor().spawn(async move {
            find_matches_in_rope(&snapshot, &query, MAX_FIND_MATCHES, &worker_cancel)
        });
        let task = cx.spawn(async move |this, cx| {
            let Some(matches) = search.await else {
                return;
            };
            // The editor may have been closed while its immutable snapshot was searched.
            let _ = this.update(cx, |ed, cx| {
                if ed.find_search.generation != generation {
                    return;
                }
                ed.find_search.pending = None;
                if ed.buffer.revision() != revision {
                    ed.clear_pending_find_actions();
                    ed.refresh_find_matches(cx);
                    return;
                }
                ed.find_state
                    .set_matches(matches, revision, ed.buffer.selection().start());
                if ed.find_search.reveal && ed.buffer.selection() == selection {
                    ed.reveal_current_match(cx);
                }
                ed.find_search.reveal = false;
                while !ed.find_state.is_stale(ed.buffer.revision()) {
                    let Some(action) = ed.find_search.actions.pop_front() else {
                        break;
                    };
                    match action {
                        FindAction::Next => ed.find_next(cx),
                        FindAction::Previous => ed.find_previous(cx),
                        FindAction::ReplaceNext(text) => ed.replace_next(&text, cx),
                        FindAction::ReplaceAll(text) => ed.replace_all(&text, cx),
                    }
                }
                ed.sync_find_bar(cx);
                cx.notify();
            });
        });
        self.find_search.pending = Some(PendingFindSearch {
            revision,
            cancel,
            _task: task,
        });
    }

    fn defer_find_action(&mut self, action: FindAction, cx: &mut Context<Self>) -> bool {
        if !self.find_state.is_stale(self.buffer.revision()) {
            return false;
        }
        self.refresh_find_matches(cx);
        if !self.find_state.is_stale(self.buffer.revision()) {
            return false;
        }
        self.find_search.actions.push_back(action);
        true
    }

    fn incremental_find_matches(&self, query: &SearchQuery) -> Option<Vec<SearchMatch>> {
        if query.options().use_regex || query.raw().contains('\n') {
            return None;
        }

        let previous_revision = self.find_state.revision()?;
        if previous_revision.wrapping_add(1) != self.buffer.revision() {
            return None;
        }

        let delta = self.buffer.last_edit_delta()?;
        let old_matches = self.find_state.matches();
        if old_matches.len() >= MAX_FIND_MATCHES {
            return None;
        }

        let start = delta.start_line.index();
        let old_end = start.checked_add(delta.old_line_count)?;
        let line_delta = isize::try_from(delta.new_line_count)
            .ok()?
            .checked_sub(isize::try_from(delta.old_line_count).ok()?)?;
        let new_end = start.checked_add(delta.new_line_count)?;
        let mut before = Vec::with_capacity(old_matches.len());
        let mut after = Vec::with_capacity(old_matches.len());

        for found in old_matches {
            if found.end.line.index() < start {
                before.push(*found);
            } else if found.start.line.index() >= old_end {
                let start_line = found.start.line.index().checked_add_signed(line_delta)?;
                let end_line = found.end.line.index().checked_add_signed(line_delta)?;
                after.push(SearchMatch::new(
                    Position::new(SourceLine::new(start_line), found.start.col),
                    Position::new(SourceLine::new(end_line), found.end.col),
                ));
            }
        }

        let mut changed = Vec::new();
        for line in start..new_end {
            let line = SourceLine::new(line);
            let text = self.buffer.line_text(line)?;
            for mut found in find_matches(&text, query, MAX_FIND_MATCHES) {
                found.start.line = line;
                found.end.line = line;
                changed.push(found);
                if before.len() + changed.len() + after.len() >= MAX_FIND_MATCHES {
                    return None;
                }
            }
        }

        let mut matches = before;
        matches.extend(changed);
        matches.extend(after);
        Some(matches)
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
        if self.defer_find_action(FindAction::Next, cx) {
            return;
        }
        self.step_match(cx, |state, wrap| state.next_match(wrap));
    }

    pub fn find_previous(&mut self, cx: &mut Context<Self>) {
        if self.defer_find_action(FindAction::Previous, cx) {
            return;
        }
        self.step_match(cx, |state, wrap| state.prev_match(wrap));
    }

    pub(crate) fn replace_next(&mut self, replacement: &str, cx: &mut Context<Self>) {
        if !self.find_state.is_active() {
            return;
        }
        if self.defer_find_action(FindAction::ReplaceNext(replacement.to_owned()), cx) {
            return;
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

        self.find_search.reveal = true;
        self.refresh_find_matches(cx);
        if !self.find_state.is_stale(self.buffer.revision()) {
            self.find_search.reveal = false;
            self.reveal_current_match(cx);
        }
        self.sync_find_bar(cx);
        cx.notify();
    }

    pub(crate) fn replace_all(&mut self, replacement: &str, cx: &mut Context<Self>) {
        if !self.find_state.is_active() {
            return;
        }
        if self.defer_find_action(FindAction::ReplaceAll(replacement.to_owned()), cx) {
            return;
        }
        let Some(query) = self.find_state.query().cloned() else {
            return;
        };
        let count = self.buffer.replace_all_matches(&query, replacement);
        if count > 0 {
            self.find_search.reveal = true;
            self.refresh_find_matches(cx);
            if !self.find_state.is_stale(self.buffer.revision()) {
                self.find_search.reveal = false;
                self.reveal_current_match(cx);
            }
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
        let line_len = self.buffer.line_len(source_line);
        let current = self.find_state.current_match_index();

        self.find_state
            .matches_on_line(source_line)
            .filter_map(|(idx, m)| {
                let start = if m.start.line == source_line {
                    m.start.col
                } else {
                    0
                };
                let end = if m.end.line == source_line {
                    m.end.col
                } else {
                    line_len
                };
                let continues = m.end.line > source_line;
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
    fn full_search_is_deferred_and_only_the_latest_query_is_applied(cx: &mut TestAppContext) {
        init_globals(cx);
        let (editor, cx) = cx.add_window_view(|_, cx| {
            Editor::new_with_buffer(TextBuffer::from_str("alpha beta beta", None), cx)
        });
        editor.update(cx, |ed, cx| {
            ed.set_find_query(
                Some(SearchQuery::new("alpha", Default::default()).unwrap()),
                cx,
            );
            assert!(ed.find_state().is_stale(ed.buffer.revision()));
            ed.set_find_query(
                Some(SearchQuery::new("beta", Default::default()).unwrap()),
                cx,
            );
        });
        cx.run_until_parked();
        editor.read_with(cx, |ed, _| {
            assert_eq!(ed.find_state().match_count(), 2);
            assert_eq!(ed.find_state().query().unwrap().raw(), "beta");
            assert!(!ed.find_state().is_stale(ed.buffer.revision()));
        });
    }

    #[gpui::test]
    fn pending_navigation_and_replacements_wait_for_current_results(cx: &mut TestAppContext) {
        init_globals(cx);
        let (editor, cx) = cx.add_window_view(|_, cx| {
            Editor::new_with_buffer(TextBuffer::from_str("foo foo foo", None), cx)
        });
        let query = || {
            SearchQuery::new(
                "foo",
                crate::search::SearchOptions {
                    use_regex: true,
                    ..Default::default()
                },
            )
            .unwrap()
        };
        editor.update(cx, |ed, cx| {
            ed.set_find_query(Some(query()), cx);
            ed.find_next(cx);
            ed.find_previous(cx);
            ed.find_next(cx);
            ed.replace_next("bar", cx);
            ed.replace_next("baz", cx);
        });
        cx.run_until_parked();
        editor.read_with(cx, |ed, _| {
            assert_eq!(ed.buffer.to_string_normalized(), "foo bar baz");
            assert_eq!(ed.find_state().match_count(), 1);
        });
        editor.update(cx, |ed, cx| {
            ed.set_find_query(Some(query()), cx);
            ed.replace_all("done", cx);
        });
        cx.run_until_parked();
        editor.read_with(cx, |ed, _| {
            assert_eq!(ed.buffer.to_string_normalized(), "done bar baz");
            assert_eq!(ed.find_state().match_count(), 0);
        });
    }

    #[gpui::test]
    fn closing_find_cancels_worker_and_drops_deferred_replacements(cx: &mut TestAppContext) {
        init_globals(cx);
        let (editor, cx) = cx.add_window_view(|_, cx| {
            Editor::new_with_buffer(TextBuffer::from_str("foo foo", None), cx)
        });
        let cancel = cx.update(|window, cx| {
            editor.update(cx, |ed, cx| {
                ed.open_find_bar(window, cx);
                ed.set_find_query(
                    Some(SearchQuery::new("foo", Default::default()).unwrap()),
                    cx,
                );
                ed.replace_all("bar", cx);
                let cancel = ed.find_search.pending.as_ref().unwrap().cancel.clone();
                ed.close_find_bar(None, cx);
                cancel
            })
        });
        assert!(cancel.load(Ordering::Relaxed));
        cx.run_until_parked();
        editor.read_with(cx, |ed, _| {
            assert!(!ed.find_state().is_active());
            assert_eq!(ed.buffer.to_string_normalized(), "foo foo");
        });
    }

    #[gpui::test]
    fn background_results_do_not_move_a_cursor_changed_by_the_user(cx: &mut TestAppContext) {
        init_globals(cx);
        let (editor, cx) = cx.add_window_view(|_, cx| {
            Editor::new_with_buffer(TextBuffer::from_str("foo last", None), cx)
        });
        let cursor = Position::new(SourceLine::new(0), 8);
        editor.update(cx, |ed, cx| {
            ed.set_find_query(
                Some(SearchQuery::new("foo", Default::default()).unwrap()),
                cx,
            );
            ed.buffer.set_cursor(cursor);
        });
        cx.run_until_parked();
        editor.read_with(cx, |ed, _| assert_eq!(ed.buffer.cursor_pos(), cursor));
    }

    #[gpui::test]
    fn pending_search_cannot_publish_after_edit_clear_or_document_reload(cx: &mut TestAppContext) {
        init_globals(cx);
        let (editor, cx) = cx.add_window_view(|_, cx| {
            Editor::new_with_buffer(TextBuffer::from_str("alpha", None), cx)
        });
        editor.update(cx, |ed, cx| {
            ed.set_find_query(
                Some(SearchQuery::new("alpha", Default::default()).unwrap()),
                cx,
            );
            ed.perform_edit(cx, |ed| ed.buffer.insert_text("alpha "));
        });
        cx.run_until_parked();
        editor.read_with(cx, |ed, _| assert_eq!(ed.find_state().match_count(), 2));
        editor.update(cx, |ed, cx| {
            ed.set_find_query(
                Some(SearchQuery::new("alpha", Default::default()).unwrap()),
                cx,
            );
            ed.set_find_query(None, cx);
        });
        cx.run_until_parked();
        editor.read_with(cx, |ed, _| assert!(!ed.find_state().is_active()));
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("new.txt");
        std::fs::write(&path, "beta").unwrap();
        editor.update(cx, |ed, cx| {
            ed.set_find_query(
                Some(SearchQuery::new("alpha", Default::default()).unwrap()),
                cx,
            );
            ed.load_file(path, cx).unwrap();
        });
        cx.run_until_parked();
        editor.read_with(cx, |ed, _| {
            assert_eq!(ed.find_state().match_count(), 0);
            assert!(!ed.find_state().is_stale(ed.buffer.revision()));
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

        cx.run_until_parked();
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
        cx.run_until_parked();

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

        cx.run_until_parked();
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

        cx.run_until_parked();
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
