use gpui::prelude::FluentBuilder;
use gpui::{
    AnyElement, App, ClickEvent, ElementId, InteractiveElement, IntoElement, ParentElement, Pixels,
    RenderOnce, StatefulInteractiveElement, Styled, Window, div,
};

use crate::theme::ThemeManager;
use crate::ui::tokens::{ControlHeight, Radius, Spacing};

type OnClickCallback = Box<dyn Fn(&ClickEvent, &mut Window, &mut App) + 'static>;

/// A list row with leading and trailing slots and an optional click handler.
#[derive(IntoElement)]
pub struct SelectableRow {
    id: ElementId,
    height: Pixels,
    selected: bool,
    leading: Option<AnyElement>,
    trailing: Option<AnyElement>,
    on_click: Option<OnClickCallback>,
}

impl SelectableRow {
    pub fn new(id: impl Into<ElementId>) -> Self {
        Self {
            id: id.into(),
            height: ControlHeight::LG,
            selected: false,
            leading: None,
            trailing: None,
            on_click: None,
        }
    }

    pub fn height(mut self, height: Pixels) -> Self {
        self.height = height;
        self
    }

    /// Highlights the row with an accent border.
    pub fn selected(mut self, selected: bool) -> Self {
        self.selected = selected;
        self
    }

    pub fn leading(mut self, element: impl IntoElement) -> Self {
        self.leading = Some(element.into_any_element());
        self
    }

    pub fn trailing(mut self, element: impl IntoElement) -> Self {
        self.trailing = Some(element.into_any_element());
        self
    }

    pub fn on_click(
        mut self,
        handler: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.on_click = Some(Box::new(handler));
        self
    }
}

impl RenderOnce for SelectableRow {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = cx.global::<ThemeManager>().theme();

        div()
            .id(self.id)
            .h(self.height)
            .w_full()
            .px(Spacing::MD)
            .rounded(Radius::MD)
            .flex()
            .flex_row()
            .items_center()
            .justify_between()
            .border_1()
            .border_color(if self.selected {
                theme.text_accent
            } else {
                theme.border_subtle
            })
            .when_some(self.leading, |this, element| this.child(element))
            .when_some(self.trailing, |this, element| this.child(element))
            .when_some(self.on_click, |this, on_click| {
                this.cursor_pointer()
                    .hover(|s| s.bg(theme.btn_hover))
                    .on_click(move |event, window, cx| on_click(event, window, cx))
            })
    }
}
