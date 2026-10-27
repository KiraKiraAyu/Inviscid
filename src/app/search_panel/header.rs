use gpui::prelude::*;
use gpui::*;

use super::SearchPanel;
use crate::search::SearchToggle;
use crate::theme::Theme;
use crate::ui::icon::{Icon, IconName, IconSize};
use crate::ui::tokens::{FontSize, Radius, Spacing};

const BUTTON_SIZE: Pixels = px(22.0);
/// The fixed trailing controls area keeps the search and replacement inputs aligned.
const RIGHT_SECTION_WIDTH: Pixels = px(96.0);
const OPTION_TOGGLES: [(SearchToggle, &str); 3] = [
    (SearchToggle::CaseSensitive, "Aa"),
    (SearchToggle::WholeWord, "ab"),
    (SearchToggle::Regex, ".*"),
];

impl SearchPanel {
    pub(super) fn render_header(&self, theme: &Theme, cx: &mut Context<Self>) -> impl IntoElement {
        let has_query = !self.query_text(cx).is_empty();
        let has_results = !self.results.is_empty();
        let all_collapsed = self.is_all_collapsed();
        let (hover, active) = (theme.btn_hover, theme.btn_active);

        div()
            .w_full()
            .p(Spacing::SM)
            .flex()
            .flex_col()
            .gap(Spacing::XS)
            .border_b_1()
            .border_color(theme.border_subtle)
            .child(
                div()
                    .w_full()
                    .flex()
                    .items_center()
                    .gap(Spacing::XS)
                    .child(
                        div()
                            .relative()
                            .flex_1()
                            .min_w_0()
                            .child(self.input.clone())
                            .when(has_query, |this| {
                                this.child(
                                    div()
                                        .absolute()
                                        .top_0()
                                        .bottom_0()
                                        .right(Spacing::XS)
                                        .w(BUTTON_SIZE)
                                        .flex()
                                        .items_center()
                                        .justify_center()
                                        .child(
                                            compact_icon_button(
                                                "search_clear_btn",
                                                IconName::Close,
                                                theme,
                                            )
                                            .bg(theme.bg_toolbar)
                                            .on_mouse_down(
                                                MouseButton::Left,
                                                cx.listener(|this, _: &MouseDownEvent, _, cx| {
                                                    cx.stop_propagation();
                                                    this.clear_query(cx);
                                                }),
                                            ),
                                        ),
                                )
                            }),
                    )
                    .child(
                        div()
                            .w(RIGHT_SECTION_WIDTH)
                            .flex_none()
                            .flex()
                            .items_center()
                            .justify_end()
                            .gap(Spacing::XS)
                            .child(self.render_replace_toggle(theme, cx)),
                    ),
            )
            .when(self.show_replace, |this| {
                this.child(
                    div()
                        .w_full()
                        .flex()
                        .items_center()
                        .gap(Spacing::XS)
                        .child(div().flex_1().min_w_0().child(self.replace_input.clone()))
                        .child(
                            div()
                                .w(RIGHT_SECTION_WIDTH)
                                .flex_none()
                                .flex()
                                .items_center()
                                .justify_end()
                                .child(
                                    div()
                                        .id("search_replace_all_btn")
                                        .h(BUTTON_SIZE)
                                        .px(Spacing::SM)
                                        .flex_none()
                                        .flex()
                                        .items_center()
                                        .justify_center()
                                        .rounded(Radius::SM)
                                        .text_size(FontSize::CAPTION)
                                        .font_weight(FontWeight::MEDIUM)
                                        .text_color(if has_results && !self.replacing {
                                            theme.text_secondary
                                        } else {
                                            theme.text_muted.opacity(0.5)
                                        })
                                        .when(has_results && !self.replacing, |this| {
                                            this.cursor_pointer()
                                                .hover(move |s| s.bg(hover))
                                                .active(move |s| s.bg(active))
                                                .on_click(cx.listener(|this, _, _, cx| {
                                                    this.replace_all(cx)
                                                }))
                                        })
                                        .child(if self.replacing {
                                            "Replacing..."
                                        } else {
                                            "Replace All"
                                        }),
                                ),
                        ),
                )
            })
            .when_some(self.replace_error.as_deref(), |this, error| {
                this.child(
                    div()
                        .text_size(FontSize::CAPTION)
                        .text_color(theme.text_danger)
                        .child(error.to_string()),
                )
            })
            .when_some(self.replace_count, |this, count| {
                this.child(
                    div()
                        .text_size(FontSize::CAPTION)
                        .text_color(theme.text_muted)
                        .child(format!("Replaced {count} matches")),
                )
            })
            .child(
                div()
                    .w_full()
                    .flex()
                    .items_center()
                    .justify_between()
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(4.0))
                            .children(OPTION_TOGGLES.map(|(toggle, label)| {
                                self.render_option_toggle(toggle, label, theme, cx)
                            })),
                    )
                    .when(has_results, |this| {
                        let expand_icon = if all_collapsed {
                            IconName::ChevronRight
                        } else {
                            IconName::ChevronDown
                        };
                        this.child(
                            div()
                                .flex()
                                .items_center()
                                .gap(Spacing::XS)
                                .child(
                                    div()
                                        .text_size(FontSize::CAPTION)
                                        .text_color(theme.text_muted)
                                        .child(self.results_summary()),
                                )
                                .child(
                                    icon_button("search_toggle_expand_all", expand_icon, theme)
                                        .on_click(cx.listener(move |this, _, _, cx| {
                                            this.set_all_collapsed(!all_collapsed, cx);
                                        })),
                                ),
                        )
                    }),
            )
    }

    fn results_summary(&self) -> String {
        let cap = if self.results.is_capped() { "+" } else { "" };
        format!(
            "{}{cap} results in {} files",
            self.results.total_matches(),
            self.results.files().len()
        )
    }

    fn render_replace_toggle(&self, theme: &Theme, cx: &mut Context<Self>) -> Stateful<Div> {
        let (hover, active) = (theme.btn_hover, theme.btn_active);
        div()
            .id("search_toggle_replace")
            .h(BUTTON_SIZE)
            .px(Spacing::XS)
            .flex_none()
            .flex()
            .items_center()
            .gap(Spacing::XXS)
            .rounded(Radius::SM)
            .text_size(FontSize::CAPTION)
            .text_color(theme.text_secondary)
            .cursor_pointer()
            .hover(move |s| s.bg(hover))
            .active(move |s| s.bg(active))
            .on_click(cx.listener(|this, _, _, cx| this.toggle_replace(cx)))
            .child(
                Icon::new(if self.show_replace {
                    IconName::ChevronDown
                } else {
                    IconName::ChevronRight
                })
                .size(IconSize::XSmall)
                .color(theme.text_secondary),
            )
            .child("Replace")
    }

    fn render_option_toggle(
        &self,
        toggle: SearchToggle,
        label: &'static str,
        theme: &Theme,
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        let is_on = self.is_option_on(toggle);
        let (hover, active) = (theme.btn_hover, theme.btn_active);

        div()
            .id(SharedString::from(format!("search_toggle_{toggle:?}")))
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
                gpui::transparent_black()
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
            .on_click(cx.listener(move |this, _, _, cx| this.toggle_option(toggle, cx)))
    }
}

fn icon_button(id: &'static str, icon: IconName, theme: &Theme) -> Stateful<Div> {
    sized_icon_button(id, icon, BUTTON_SIZE, IconSize::XSmall, theme)
}

fn compact_icon_button(id: &'static str, icon: IconName, theme: &Theme) -> Stateful<Div> {
    sized_icon_button(id, icon, px(18.0), IconSize::Indicator, theme)
}

fn sized_icon_button(
    id: &'static str,
    icon: IconName,
    size: Pixels,
    icon_size: IconSize,
    theme: &Theme,
) -> Stateful<Div> {
    let hover = theme.btn_hover;
    div()
        .id(id)
        .size(size)
        .flex()
        .items_center()
        .justify_center()
        .rounded(Radius::SM)
        .cursor_pointer()
        .hover(move |s| s.bg(hover))
        .child(Icon::new(icon).size(icon_size).color(theme.text_secondary))
}
