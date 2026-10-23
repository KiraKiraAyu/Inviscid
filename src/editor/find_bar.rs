use gpui::prelude::FluentBuilder;
use gpui::*;

use crate::search::{SearchOptions, SearchQuery, SearchToggle};
use crate::theme::ActiveTheme;
use crate::ui::tokens::{ControlHeight, FontSize, Radius, Spacing};
use crate::ui::{Icon, IconName, IconSize, TextInput, TextInputEvent};

const BUTTON_SIZE: Pixels = ControlHeight::SM;
const STATUS_MIN_WIDTH: Pixels = px(76.0);
/// Total width of the right controls container on both rows, ensuring both
/// Find and Replace inputs have identical width and alignment.
/// Calculated as: status (76px) + 3 dividers (3*9px) + 3 toggles (3*24px)
/// + 2 nav buttons (2*24px) + close button (24px) = 271px.
const RIGHT_SECTION_WIDTH: Pixels = px(271.0);

#[derive(Clone, Debug)]
pub enum FindBarEvent {
    QueryChanged(Option<SearchQuery>),
    Next,
    Previous,
    Dismiss,
    ReplaceNext(String),
    ReplaceAll(String),
}

impl EventEmitter<FindBarEvent> for FindBar {}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct FindBarMatches {
    pub current: Option<usize>,
    pub total: usize,
    pub is_capped: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum QueryState {
    Empty,
    Invalid,
    Valid,
}

pub struct FindBar {
    input: Entity<TextInput>,
    replace_input: Entity<TextInput>,
    options: SearchOptions,
    query_state: QueryState,
    matches: FindBarMatches,
    show_replace: bool,
    _input_sub: Subscription,
    _replace_sub: Subscription,
}

impl FindBar {
    pub const HEIGHT: Pixels = ControlHeight::TITLEBAR;

    pub fn new(cx: &mut Context<Self>) -> Self {
        let input = cx.new(|cx| {
            TextInput::new("", cx)
                .id("find_bar_input")
                .placeholder("Find")
                .height(ControlHeight::SM)
                .radius(Radius::SM)
                .padding_x(Spacing::SM)
                .bg(crate::ui::TextInputBg::Toolbar)
        });
        let replace_input = cx.new(|cx| {
            TextInput::new("", cx)
                .id("find_bar_replace_input")
                .placeholder("Replace")
                .height(ControlHeight::SM)
                .radius(Radius::SM)
                .padding_x(Spacing::SM)
                .bg(crate::ui::TextInputBg::Toolbar)
        });
        let _input_sub = cx.subscribe(&input, |this, _, event: &TextInputEvent, cx| match event {
            TextInputEvent::Changed(_) => this.emit_query(cx),
            TextInputEvent::Commit(_) => cx.emit(FindBarEvent::Next),
            TextInputEvent::Cancel => cx.emit(FindBarEvent::Dismiss),
        });
        let _replace_sub =
            cx.subscribe(
                &replace_input,
                |this, _, event: &TextInputEvent, cx| match event {
                    TextInputEvent::Changed(_) => {}
                    TextInputEvent::Commit(_) => {
                        let rep = this.replace_text(cx);
                        cx.emit(FindBarEvent::ReplaceNext(rep));
                    }
                    TextInputEvent::Cancel => cx.emit(FindBarEvent::Dismiss),
                },
            );

        Self {
            input,
            replace_input,
            options: SearchOptions::default(),
            query_state: QueryState::Empty,
            matches: FindBarMatches::default(),
            show_replace: false,
            _input_sub,
            _replace_sub,
        }
    }

    pub fn focus_query(&mut self, seed: Option<&str>, window: &mut Window, cx: &mut Context<Self>) {
        self.input.update(cx, |input, cx| match seed {
            Some(text) => input.set_text(text, cx),
            None => {
                input.select_all();
                cx.notify();
            }
        });
        window.focus(&self.input.focus_handle(cx));
    }

    pub fn query_text(&self, cx: &App) -> String {
        self.input.read(cx).text()
    }

    pub fn replace_text(&self, cx: &App) -> String {
        self.replace_input.read(cx).text()
    }

    pub fn set_replace_visible(&mut self, visible: bool, cx: &mut Context<Self>) {
        if self.show_replace != visible {
            self.show_replace = visible;
            cx.notify();
        }
    }

    fn toggle_replace(&mut self, cx: &mut Context<Self>) {
        self.show_replace = !self.show_replace;
        cx.notify();
    }

    pub fn focus_replace(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.show_replace = true;
        self.replace_input.update(cx, |input, cx| {
            input.select_all();
            cx.notify();
        });
        window.focus(&self.replace_input.focus_handle(cx));
        cx.notify();
    }

    pub fn options(&self) -> &SearchOptions {
        &self.options
    }

    pub fn search_query(&self, cx: &App) -> Option<SearchQuery> {
        let text = self.query_text(cx);
        if text.is_empty() {
            None
        } else {
            SearchQuery::new(text, self.options.clone()).ok()
        }
    }

    pub fn set_matches(&mut self, matches: FindBarMatches, cx: &mut Context<Self>) {
        if self.matches != matches {
            self.matches = matches;
            cx.notify();
        }
    }

    pub fn toggle(&mut self, toggle: SearchToggle, cx: &mut Context<Self>) {
        let flag = match toggle {
            SearchToggle::CaseSensitive => &mut self.options.case_sensitive,
            SearchToggle::WholeWord => &mut self.options.whole_word,
            SearchToggle::Regex => &mut self.options.use_regex,
        };
        *flag = !*flag;
        self.emit_query(cx);
        cx.notify();
    }

    fn emit_query(&mut self, cx: &mut Context<Self>) {
        let text = self.query_text(cx);
        let query = if text.is_empty() {
            self.query_state = QueryState::Empty;
            None
        } else {
            let query = SearchQuery::new(text, self.options.clone()).ok();
            self.query_state = if query.is_some() {
                QueryState::Valid
            } else {
                QueryState::Invalid
            };
            query
        };
        cx.emit(FindBarEvent::QueryChanged(query));
        cx.notify();
    }

    fn status(&self) -> (SharedString, bool) {
        let cap = |n: usize| {
            if self.matches.is_capped {
                format!("{n}+")
            } else {
                n.to_string()
            }
        };
        match self.query_state {
            QueryState::Empty => ("0/0".into(), false),
            QueryState::Invalid => ("Invalid regex".into(), true),
            QueryState::Valid if self.matches.total == 0 => ("0/0".into(), false),
            QueryState::Valid => {
                let current = self.matches.current.map_or("0".into(), |n| n.to_string());
                (
                    format!("{current}/{}", cap(self.matches.total)).into(),
                    false,
                )
            }
        }
    }

    fn render_toggle(
        &self,
        toggle: SearchToggle,
        label: &'static str,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let theme = cx.theme();
        let is_on = match toggle {
            SearchToggle::CaseSensitive => self.options.case_sensitive,
            SearchToggle::WholeWord => self.options.whole_word,
            SearchToggle::Regex => self.options.use_regex,
        };
        let (hover, active) = (theme.btn_hover, theme.btn_active);

        div()
            .id(SharedString::from(format!("find_toggle_{toggle:?}")))
            .size(BUTTON_SIZE)
            .flex_none()
            .flex()
            .items_center()
            .justify_center()
            .rounded(Radius::SM)
            .border_1()
            .border_color(if is_on {
                theme.text_accent
            } else {
                transparent_black()
            })
            .when(is_on, |this| this.bg(theme.btn_active))
            .text_size(FontSize::CAPTION)
            .font_weight(if is_on {
                FontWeight::SEMIBOLD
            } else {
                FontWeight::MEDIUM
            })
            .text_color(if is_on {
                theme.text_accent
            } else {
                theme.text_secondary
            })
            .cursor_pointer()
            .hover(move |s| s.bg(hover))
            .active(move |s| s.bg(active))
            .child(label)
            .on_click(cx.listener(move |this, _, _, cx| this.toggle(toggle, cx)))
    }

    fn render_icon_button(
        &self,
        id: &'static str,
        icon: IconName,
        enabled: bool,
        event: FindBarEvent,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let theme = cx.theme();
        let (hover, active) = (theme.btn_hover, theme.btn_active);
        let color = if enabled {
            theme.text_secondary
        } else {
            theme.text_muted.opacity(0.5)
        };

        div()
            .id(id)
            .size(BUTTON_SIZE)
            .flex_none()
            .flex()
            .items_center()
            .justify_center()
            .rounded(Radius::SM)
            .when(enabled, |this| {
                this.cursor_pointer()
                    .hover(move |s| s.bg(hover))
                    .active(move |s| s.bg(active))
                    .on_click(cx.listener(move |_, _, _, cx| cx.emit(event.clone())))
            })
            .child(Icon::new(icon).size(IconSize::Small).color(color))
    }

    fn render_chevron_toggle(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme();
        let (hover, active) = (theme.btn_hover, theme.btn_active);
        let icon = if self.show_replace {
            IconName::ChevronDown
        } else {
            IconName::ChevronRight
        };
        div()
            .id("find_toggle_replace")
            .size(BUTTON_SIZE)
            .flex_none()
            .flex()
            .items_center()
            .justify_center()
            .rounded(Radius::SM)
            .cursor_pointer()
            .hover(move |s| s.bg(hover))
            .active(move |s| s.bg(active))
            .on_click(cx.listener(|this, _, _, cx| {
                this.toggle_replace(cx);
            }))
            .child(
                Icon::new(icon)
                    .size(IconSize::Small)
                    .color(theme.text_secondary),
            )
    }

    fn render_text_button(
        &self,
        id: &'static str,
        label: &'static str,
        enabled: bool,
        on_click: impl Fn(&mut Self, &mut Context<Self>) + 'static,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let theme = cx.theme();
        let (hover, active) = (theme.btn_hover, theme.btn_active);
        let color = if enabled {
            theme.text_secondary
        } else {
            theme.text_muted.opacity(0.5)
        };

        div()
            .id(id)
            .h(BUTTON_SIZE)
            .px(Spacing::SM)
            .flex_none()
            .flex()
            .items_center()
            .justify_center()
            .rounded(Radius::SM)
            .text_size(FontSize::CAPTION)
            .font_weight(FontWeight::MEDIUM)
            .text_color(color)
            .when(enabled, |this| {
                this.cursor_pointer()
                    .hover(move |s| s.bg(hover))
                    .active(move |s| s.bg(active))
                    .on_click(cx.listener(move |this, _, _, cx| on_click(this, cx)))
            })
            .child(label)
    }
}

fn render_divider(color: Hsla) -> impl IntoElement {
    div().w(px(1.0)).h(px(14.0)).mx(Spacing::XXS).bg(color)
}

impl Render for FindBar {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme();
        let (bg, border_subtle, muted, danger) = (
            theme.bg_toolbar,
            theme.border_subtle,
            theme.text_muted,
            theme.text_danger,
        );
        let has_matches = self.matches.total > 0;
        let (status_text, is_error) = self.status();

        div()
            .id("find_bar")
            .key_context(crate::ui::key_context::FIND_BAR)
            .w_full()
            .flex_none()
            .bg(bg)
            .border_b_1()
            .border_color(border_subtle)
            .flex()
            .flex_col()
            .capture_key_down(cx.listener(|this, event: &KeyDownEvent, window, cx| {
                let keystroke = &event.keystroke;
                if keystroke.modifiers.shift && keystroke.key.eq_ignore_ascii_case("enter") {
                    cx.stop_propagation();
                    cx.emit(FindBarEvent::Previous);
                } else if (keystroke.modifiers.platform || keystroke.modifiers.control)
                    && keystroke.modifiers.alt
                    && keystroke.key.eq_ignore_ascii_case("enter")
                {
                    if this.show_replace {
                        cx.stop_propagation();
                        let rep = this.replace_text(cx);
                        cx.emit(FindBarEvent::ReplaceAll(rep));
                    }
                } else if keystroke.key.eq_ignore_ascii_case("tab") && this.show_replace {
                    if keystroke.modifiers.shift {
                        if this
                            .replace_input
                            .read(cx)
                            .focus_handle(cx)
                            .is_focused(window)
                        {
                            cx.stop_propagation();
                            window.focus(&this.input.read(cx).focus_handle(cx));
                        }
                    } else if this.input.read(cx).focus_handle(cx).is_focused(window) {
                        cx.stop_propagation();
                        window.focus(&this.replace_input.read(cx).focus_handle(cx));
                    }
                }
            }))
            .on_action(cx.listener(
                |this, _: &crate::app::actions::ToggleCaseSensitive, _, cx| {
                    this.toggle(SearchToggle::CaseSensitive, cx)
                },
            ))
            .on_action(
                cx.listener(|this, _: &crate::app::actions::ToggleWholeWord, _, cx| {
                    this.toggle(SearchToggle::WholeWord, cx)
                }),
            )
            .on_action(
                cx.listener(|this, _: &crate::app::actions::ToggleRegex, _, cx| {
                    this.toggle(SearchToggle::Regex, cx)
                }),
            )
            .child(
                div()
                    .w_full()
                    .h(Self::HEIGHT)
                    .px(Spacing::SM)
                    .flex()
                    .flex_row()
                    .items_center()
                    .gap(Spacing::XS)
                    .child(self.render_chevron_toggle(cx))
                    .child(self.input.clone())
                    .child(
                        div()
                            .w(RIGHT_SECTION_WIDTH)
                            .flex_none()
                            .flex()
                            .flex_row()
                            .items_center()
                            .gap(Spacing::XS)
                            .child(
                                div()
                                    .w(STATUS_MIN_WIDTH)
                                    .flex_none()
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .px(Spacing::XS)
                                    .text_size(FontSize::CAPTION)
                                    .whitespace_nowrap()
                                    .text_color(if is_error { danger } else { muted })
                                    .child(status_text),
                            )
                            .child(render_divider(border_subtle))
                            .child(self.render_toggle(SearchToggle::CaseSensitive, "Aa", cx))
                            .child(self.render_toggle(SearchToggle::WholeWord, "ab", cx))
                            .child(self.render_toggle(SearchToggle::Regex, ".*", cx))
                            .child(render_divider(border_subtle))
                            .child(self.render_icon_button(
                                "find_prev",
                                IconName::ChevronUp,
                                has_matches,
                                FindBarEvent::Previous,
                                cx,
                            ))
                            .child(self.render_icon_button(
                                "find_next",
                                IconName::ChevronDown,
                                has_matches,
                                FindBarEvent::Next,
                                cx,
                            ))
                            .child(render_divider(border_subtle))
                            .child(self.render_icon_button(
                                "find_close",
                                IconName::Close,
                                true,
                                FindBarEvent::Dismiss,
                                cx,
                            )),
                    ),
            )
            .when(self.show_replace, |this| {
                this.child(
                    div()
                        .w_full()
                        .h(Self::HEIGHT)
                        .border_t_1()
                        .border_color(border_subtle)
                        .px(Spacing::SM)
                        .flex()
                        .flex_row()
                        .items_center()
                        .gap(Spacing::XS)
                        .child(div().size(BUTTON_SIZE).flex_none())
                        .child(self.replace_input.clone())
                        .child(
                            div()
                                .w(RIGHT_SECTION_WIDTH)
                                .flex_none()
                                .flex()
                                .flex_row()
                                .items_center()
                                .justify_end()
                                .gap(Spacing::XS)
                                .child(self.render_text_button(
                                    "find_replace_btn",
                                    "Replace",
                                    has_matches,
                                    |this, cx| {
                                        let rep = this.replace_text(cx);
                                        cx.emit(FindBarEvent::ReplaceNext(rep));
                                    },
                                    cx,
                                ))
                                .child(self.render_text_button(
                                    "find_replace_all_btn",
                                    "Replace All",
                                    has_matches,
                                    |this, cx| {
                                        let rep = this.replace_text(cx);
                                        cx.emit(FindBarEvent::ReplaceAll(rep));
                                    },
                                    cx,
                                )),
                        ),
                )
            })
    }
}

#[cfg(test)]
mod tests {
    // Not `use super::*`: that would pull in `gpui::test`, which shadows the built-in `#[test]`
    // that `#[gpui::test]` expands to and recurses.
    use super::{FindBar, FindBarEvent, FindBarMatches, SearchToggle};
    use crate::search::SearchOptions;
    use gpui::{Entity, Subscription, TestAppContext, VisualTestContext};
    use std::cell::RefCell;
    use std::rc::Rc;

    type Seen = Rc<RefCell<Vec<Option<(String, SearchOptions)>>>>;

    fn record_queries(bar: &Entity<FindBar>, cx: &mut VisualTestContext) -> (Seen, Subscription) {
        let seen: Seen = Rc::default();
        let sink = seen.clone();
        let sub = cx.update(|_window, cx| {
            cx.subscribe(bar, move |_, event: &FindBarEvent, _| {
                if let FindBarEvent::QueryChanged(query) = event {
                    sink.borrow_mut().push(
                        query
                            .as_ref()
                            .map(|q| (q.raw().to_string(), q.options().clone())),
                    );
                }
            })
        });
        (seen, sub)
    }

    #[gpui::test]
    fn test_typing_emits_compiled_queries_and_clearing_emits_none(cx: &mut TestAppContext) {
        let (bar, cx) = cx.add_window_view(|_window, cx| FindBar::new(cx));
        let (seen, _sub) = record_queries(&bar, cx);

        cx.update(|window, cx| {
            bar.update(cx, |bar, cx| bar.focus_query(Some("foo"), window, cx));
        });
        cx.update(|window, cx| {
            bar.update(cx, |bar, cx| bar.focus_query(Some(""), window, cx));
        });

        assert_eq!(
            *seen.borrow(),
            vec![Some(("foo".into(), SearchOptions::default())), None]
        );
    }

    #[gpui::test]
    fn test_invalid_regex_is_reported_in_the_bar_not_sent(cx: &mut TestAppContext) {
        let (bar, cx) = cx.add_window_view(|_window, cx| FindBar::new(cx));
        let (seen, _sub) = record_queries(&bar, cx);

        cx.update(|window, cx| {
            bar.update(cx, |bar, cx| {
                bar.toggle(SearchToggle::Regex, cx);
                bar.focus_query(Some("[oops"), window, cx);
            });
        });

        assert_eq!(seen.borrow().last(), Some(&None));
        bar.read_with(cx, |bar, _| {
            assert_eq!(bar.status(), ("Invalid regex".into(), true));
        });
    }

    #[gpui::test]
    fn test_toggling_an_option_re_emits_the_query(cx: &mut TestAppContext) {
        let (bar, cx) = cx.add_window_view(|_window, cx| FindBar::new(cx));
        cx.update(|window, cx| {
            bar.update(cx, |bar, cx| bar.focus_query(Some("foo"), window, cx));
        });
        let (seen, _sub) = record_queries(&bar, cx);

        cx.update(|_window, cx| {
            bar.update(cx, |bar, cx| bar.toggle(SearchToggle::CaseSensitive, cx));
        });

        let expected = SearchOptions {
            case_sensitive: true,
            ..Default::default()
        };
        assert_eq!(*seen.borrow(), vec![Some(("foo".into(), expected))]);
    }

    #[gpui::test]
    fn test_status_text(cx: &mut TestAppContext) {
        let (bar, cx) = cx.add_window_view(|_window, cx| FindBar::new(cx));
        bar.read_with(cx, |bar, _| {
            assert_eq!(
                bar.status(),
                ("0/0".into(), false),
                "default to 0/0 when empty"
            );
        });

        cx.update(|window, cx| {
            bar.update(cx, |bar, cx| bar.focus_query(Some("foo"), window, cx));
        });
        bar.read_with(cx, |bar, _| {
            assert_eq!(bar.status(), ("0/0".into(), false));
        });

        cx.update(|_window, cx| {
            bar.update(cx, |bar, cx| {
                bar.set_matches(
                    FindBarMatches {
                        current: Some(3),
                        total: 12,
                        is_capped: false,
                    },
                    cx,
                );
                assert_eq!(bar.status(), ("3/12".into(), false));

                bar.set_matches(
                    FindBarMatches {
                        current: Some(1),
                        total: 10_000,
                        is_capped: true,
                    },
                    cx,
                );
                assert_eq!(bar.status(), ("1/10000+".into(), false));
            });
        });
    }

    #[gpui::test]
    fn test_toggle_replace_mode(cx: &mut TestAppContext) {
        let (bar, cx) = cx.add_window_view(|_window, cx| FindBar::new(cx));
        assert!(!bar.read_with(cx, |bar, _| bar.show_replace));

        cx.update(|_window, cx| {
            bar.update(cx, |bar, cx| bar.toggle_replace(cx));
        });
        assert!(bar.read_with(cx, |bar, _| bar.show_replace));

        cx.update(|_window, cx| {
            bar.update(cx, |bar, cx| bar.toggle_replace(cx));
        });
        assert!(!bar.read_with(cx, |bar, _| bar.show_replace));
    }
}
