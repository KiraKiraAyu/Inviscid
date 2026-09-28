use gpui::prelude::FluentBuilder;
use gpui::{
    App, ClickEvent, ElementId, FontWeight, InteractiveElement, IntoElement, ParentElement,
    RenderOnce, SharedString, StatefulInteractiveElement, Styled, Window, div,
};

use crate::theme::ThemeManager;
use crate::ui::tokens::{ControlHeight, FontSize, Radius, Spacing};

type OnClickCallback = Box<dyn Fn(&ClickEvent, &mut Window, &mut App) + 'static>;

/// A selectable pill button representing an option in a segmented group.
#[derive(IntoElement)]
pub struct OptionPill {
    id: ElementId,
    label: SharedString,
    selected: bool,
    on_click: Option<OnClickCallback>,
}

impl OptionPill {
    pub fn new(id: impl Into<ElementId>, label: impl Into<SharedString>, selected: bool) -> Self {
        Self {
            id: id.into(),
            label: label.into(),
            selected,
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

impl RenderOnce for OptionPill {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = cx.global::<ThemeManager>().theme();
        let is_selected = self.selected;

        div()
            .id(self.id)
            .h(ControlHeight::MD)
            .px(Spacing::MDS)
            .rounded(Radius::SM)
            .bg(if is_selected {
                theme.btn_active
            } else {
                theme.bg_editor
            })
            .border_1()
            .border_color(if is_selected {
                theme.text_accent
            } else {
                theme.border_subtle
            })
            .text_color(if is_selected {
                theme.text_accent
            } else {
                theme.text_secondary
            })
            .text_size(FontSize::SM)
            .font_weight(if is_selected {
                FontWeight::MEDIUM
            } else {
                FontWeight::NORMAL
            })
            .whitespace_nowrap()
            .flex_none()
            .flex()
            .flex_row()
            .items_center()
            .justify_center()
            .gap(Spacing::XS)
            .when(!self.label.is_empty(), |this| this.child(self.label))
            .cursor_pointer()
            .hover(|s| s.bg(theme.btn_hover))
            .active(|s| s.bg(theme.btn_active))
            .when_some(self.on_click, |this, on_click| {
                this.on_click(move |event, window, cx| on_click(event, window, cx))
            })
    }
}
