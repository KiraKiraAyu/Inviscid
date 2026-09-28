use gpui::prelude::FluentBuilder;
use gpui::{
    App, ClickEvent, ElementId, FontWeight, InteractiveElement, IntoElement, ParentElement,
    RenderOnce, SharedString, StatefulInteractiveElement, Styled, Window, div,
};

use crate::theme::{Theme, ThemeManager};
use crate::ui::tokens::{ControlHeight, FontSize, Radius, Spacing};

type OnClickCallback = Box<dyn Fn(&ClickEvent, &mut Window, &mut App) + 'static>;

/// A stepper component for incrementing and decrementing numeric settings.
#[derive(IntoElement)]
pub struct Stepper {
    id: SharedString,
    value: SharedString,
    on_decrement: Option<OnClickCallback>,
    on_increment: Option<OnClickCallback>,
}

impl Stepper {
    pub fn new(id: impl Into<SharedString>, value: impl Into<SharedString>) -> Self {
        Self {
            id: id.into(),
            value: value.into(),
            on_decrement: None,
            on_increment: None,
        }
    }

    pub fn on_decrement(
        mut self,
        handler: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.on_decrement = Some(Box::new(handler));
        self
    }

    pub fn on_increment(
        mut self,
        handler: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.on_increment = Some(Box::new(handler));
        self
    }
}

fn render_step_button(
    id: ElementId,
    label: &'static str,
    handler: Option<OnClickCallback>,
    theme: &Theme,
) -> impl IntoElement {
    let hover_bg = theme.btn_hover;
    let active_bg = theme.btn_active;
    div()
        .id(id)
        .w(ControlHeight::SM)
        .h(ControlHeight::SM)
        .rounded(Radius::XS)
        .flex()
        .items_center()
        .justify_center()
        .text_size(FontSize::BODY)
        .text_color(theme.text_primary)
        .cursor_pointer()
        .hover(move |s| s.bg(hover_bg))
        .active(move |s| s.bg(active_bg))
        .when_some(handler, |this, on_click| {
            this.on_click(move |event, window, cx| on_click(event, window, cx))
        })
        .child(label)
}

impl RenderOnce for Stepper {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = cx.global::<ThemeManager>().theme();

        let dec_btn = render_step_button(
            ElementId::Name(format!("{}_dec", self.id).into()),
            "-",
            self.on_decrement,
            theme,
        );
        let inc_btn = render_step_button(
            ElementId::Name(format!("{}_inc", self.id).into()),
            "+",
            self.on_increment,
            theme,
        );

        div()
            .id(ElementId::Name(self.id))
            .flex_none()
            .flex()
            .flex_row()
            .items_center()
            .gap(Spacing::SMD)
            .bg(theme.bg_editor)
            .border_1()
            .border_color(theme.border_subtle)
            .rounded(Radius::MD)
            .p(Spacing::XXS)
            .child(dec_btn)
            .child(
                div()
                    .px(Spacing::SMD)
                    .text_size(FontSize::SM)
                    .font_weight(FontWeight::MEDIUM)
                    .text_color(theme.text_primary)
                    .child(self.value),
            )
            .child(inc_btn)
    }
}
