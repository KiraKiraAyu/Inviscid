use gpui::prelude::FluentBuilder;
use gpui::{
    App, ClickEvent, ElementId, InteractiveElement, IntoElement, ParentElement, RenderOnce,
    StatefulInteractiveElement, Styled, Window, div,
};

use crate::theme::ThemeManager;
use crate::ui::tokens::{Radius, SwitchMetrics};

type OnClickCallback = Box<dyn Fn(&ClickEvent, &mut Window, &mut App) + 'static>;

/// A toggle switch component for boolean settings.
#[derive(IntoElement)]
pub struct Switch {
    id: ElementId,
    checked: bool,
    on_click: Option<OnClickCallback>,
}

impl Switch {
    pub fn new(id: impl Into<ElementId>, checked: bool) -> Self {
        Self {
            id: id.into(),
            checked,
            on_click: None,
        }
    }

    pub fn on_click(
        mut self,
        handler: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.on_click = Some(Box::new(handler));
        self
    }
}

impl RenderOnce for Switch {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = cx.global::<ThemeManager>().theme();
        let is_checked = self.checked;

        div()
            .id(self.id)
            .w(SwitchMetrics::TRACK_WIDTH)
            .h(SwitchMetrics::TRACK_HEIGHT)
            .flex_none()
            .rounded(Radius::FULL)
            .p(SwitchMetrics::TRACK_PADDING)
            .bg(if is_checked {
                theme.text_accent
            } else {
                theme.border
            })
            .flex()
            .flex_row()
            .items_center()
            .child(
                div()
                    .w(SwitchMetrics::THUMB_SIZE)
                    .h(SwitchMetrics::THUMB_SIZE)
                    .rounded_full()
                    .bg(if is_checked {
                        theme.bg_editor
                    } else {
                        theme.text_secondary
                    })
                    .when(is_checked, |this| this.ml(SwitchMetrics::THUMB_TRAVEL)),
            )
            .cursor_pointer()
            .hover(|s| {
                if is_checked {
                    s.bg(theme.btn_primary_hover)
                } else {
                    s.bg(theme.btn_hover)
                }
            })
            .when_some(self.on_click, |this, on_click| {
                this.on_click(move |event, window, cx| on_click(event, window, cx))
            })
    }
}
