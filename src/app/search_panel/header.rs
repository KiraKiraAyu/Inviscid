use gpui::prelude::*;
use gpui::*;

use super::SearchPanel;
use crate::search::SearchToggle;
use crate::theme::Theme;
use crate::ui::icon::{Icon, IconName, IconSize};
use crate::ui::tokens::{FontSize, Radius, Spacing};

const BUTTON_SIZE: Pixels = px(22.0);
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
                    .child(div().flex_1().min_w_0().child(self.input.clone()))
                    .when(has_query, |this| {
                        this.child(
                            icon_button("search_clear_btn", IconName::Close, theme)
                                .on_click(cx.listener(|this, _, _, cx| this.clear_query(cx))),
                        )
                    }),
            )
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
    let hover = theme.btn_hover;
    div()
        .id(id)
        .size(BUTTON_SIZE)
        .flex()
        .items_center()
        .justify_center()
        .rounded(Radius::SM)
        .cursor_pointer()
        .hover(move |s| s.bg(hover))
        .child(
            Icon::new(icon)
                .size(IconSize::XSmall)
                .color(theme.text_secondary),
        )
}
