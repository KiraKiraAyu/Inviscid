use gpui::prelude::FluentBuilder;
use gpui::{
    App, ClickEvent, ElementId, Hsla, InteractiveElement, IntoElement, MouseButton, ParentElement,
    Pixels, RenderOnce, SharedString, StatefulInteractiveElement, Styled, Window, div,
};

use crate::theme::ActiveTheme;
use crate::ui::tokens::{ControlHeight, Radius};
use crate::ui::{Icon, IconName, IconSize, Tooltip};

type OnClickCallback = Box<dyn Fn(&ClickEvent, &mut Window, &mut App) + 'static>;

/// A compact square icon button with optional tooltip, hover styling, and click handler.
#[derive(IntoElement)]
pub struct IconButton {
    id: ElementId,
    icon: IconName,
    size: Pixels,
    icon_size: IconSize,
    color: Option<Hsla>,
    hover_color: Option<Hsla>,
    hover_bg: Option<Hsla>,
    active_bg: Option<Hsla>,
    tooltip: Option<SharedString>,
    stop_propagation: bool,
    debug_selector: Option<SharedString>,
    on_click: Option<OnClickCallback>,
}

impl IconButton {
    pub fn new(id: impl Into<ElementId>, icon: IconName) -> Self {
        Self {
            id: id.into(),
            icon,
            size: ControlHeight::SM,
            icon_size: IconSize::XSmall,
            color: None,
            hover_color: None,
            hover_bg: None,
            active_bg: None,
            tooltip: None,
            stop_propagation: false,
            debug_selector: None,
            on_click: None,
        }
    }

    pub fn size(mut self, size: Pixels) -> Self {
        self.size = size;
        self
    }

    pub fn icon_size(mut self, icon_size: IconSize) -> Self {
        self.icon_size = icon_size;
        self
    }

    pub fn color(mut self, color: impl Into<Hsla>) -> Self {
        self.color = Some(color.into());
        self
    }

    pub fn hover_color(mut self, hover_color: impl Into<Hsla>) -> Self {
        self.hover_color = Some(hover_color.into());
        self
    }

    pub fn hover_bg(mut self, hover_bg: impl Into<Hsla>) -> Self {
        self.hover_bg = Some(hover_bg.into());
        self
    }

    pub fn active_bg(mut self, active_bg: impl Into<Hsla>) -> Self {
        self.active_bg = Some(active_bg.into());
        self
    }

    pub fn tooltip(mut self, tooltip: impl Into<SharedString>) -> Self {
        self.tooltip = Some(tooltip.into());
        self
    }

    pub fn stop_propagation(mut self, stop: bool) -> Self {
        self.stop_propagation = stop;
        self
    }

    pub fn debug_selector(mut self, selector: impl Into<SharedString>) -> Self {
        self.debug_selector = Some(selector.into());
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

impl RenderOnce for IconButton {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = cx.theme();
        let base_color = self.color.unwrap_or(theme.text_secondary);
        let hover_color = self.hover_color.unwrap_or(theme.text_primary);
        let hover_bg = self.hover_bg.unwrap_or(theme.btn_hover);
        let active_bg = self.active_bg.unwrap_or(theme.btn_active);
        let stop_propagation = self.stop_propagation;

        div()
            .id(self.id)
            .flex_none()
            .size(self.size)
            .flex()
            .items_center()
            .justify_center()
            .rounded(Radius::SM)
            .cursor_pointer()
            .text_color(base_color)
            .hover(move |s| s.bg(hover_bg).text_color(hover_color))
            .active(move |s| s.bg(active_bg))
            .when_some(self.debug_selector, |this, selector| {
                this.debug_selector(move || selector.to_string())
            })
            .when_some(self.tooltip, |this, tip| {
                this.tooltip(Tooltip::text(tip))
            })
            .when(stop_propagation, |this| {
                this.on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            })
            .when_some(self.on_click, |this, on_click| {
                this.on_click(move |event, window, cx| {
                    if stop_propagation {
                        cx.stop_propagation();
                    }
                    on_click(event, window, cx);
                })
            })
            // Omit `.color(...)` so Icon inherits the button's `text_color` and hover color.
            .child(Icon::new(self.icon).size(self.icon_size))
    }
}
