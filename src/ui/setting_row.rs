use gpui::prelude::FluentBuilder;
use gpui::{
    AnyElement, App, FontWeight, IntoElement, ParentElement, RenderOnce, SharedString, Styled,
    Window, div,
};

use crate::theme::ThemeManager;
use crate::ui::tokens::{FontSize, LineHeight, Spacing};

/// A standardized form row for settings panels displaying a title, optional description, and a trailing control.
#[derive(IntoElement)]
pub struct SettingRow {
    title: SharedString,
    description: Option<SharedString>,
    control: AnyElement,
}

impl SettingRow {
    pub fn new(title: impl Into<SharedString>, control: impl IntoElement) -> Self {
        Self {
            title: title.into(),
            description: None,
            control: control.into_any_element(),
        }
    }

    pub fn description(mut self, desc: impl Into<SharedString>) -> Self {
        self.description = Some(desc.into());
        self
    }
}

impl RenderOnce for SettingRow {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = cx.global::<ThemeManager>().theme();

        div()
            .w_full()
            .py(Spacing::SMD)
            .overflow_hidden()
            .flex()
            .flex_row()
            .items_center()
            .gap(Spacing::LG)
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .flex()
                    .flex_col()
                    .gap(Spacing::XXS)
                    .child(
                        div()
                            .text_size(FontSize::BODY)
                            .font_weight(FontWeight::MEDIUM)
                            .text_color(theme.text_primary)
                            .child(self.title),
                    )
                    .when_some(self.description, |this, desc| {
                        this.child(
                            div()
                                .text_size(FontSize::SM)
                                .text_color(theme.text_muted)
                                .line_height(LineHeight::SM)
                                .child(desc),
                        )
                    }),
            )
            .child(div().flex_none().child(self.control))
    }
}
