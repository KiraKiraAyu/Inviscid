use gpui::prelude::FluentBuilder;
use gpui::{
    App, ClickEvent, ElementId, InteractiveElement, IntoElement, ParentElement, RenderOnce,
    SharedString, StatefulInteractiveElement, Styled, Window, div,
};

use crate::theme::ThemeManager;
use crate::ui::tokens::{ControlHeight, FontSize, Radius, Spacing};
use crate::ui::{Icon, IconName, IconSize};

type OnClickCallback = Box<dyn Fn(&ClickEvent, &mut Window, &mut App) + 'static>;
type OnHoverCallback = Box<dyn Fn(&bool, &mut Window, &mut App) + 'static>;

/// A dropdown menu item with optional shortcut, selection checkmark, and click/hover handlers.
#[derive(IntoElement)]
pub struct MenuItem {
    id: ElementId,
    label: SharedString,
    shortcut: Option<SharedString>,
    selected: bool,
    disabled: bool,
    on_click: Option<OnClickCallback>,
    on_hover: Option<OnHoverCallback>,
}

impl MenuItem {
    pub fn new(id: impl Into<ElementId>, label: impl Into<SharedString>) -> Self {
        Self {
            id: id.into(),
            label: label.into(),
            shortcut: None,
            selected: false,
            disabled: false,
            on_click: None,
            on_hover: None,
        }
    }

    pub fn from_name(name: impl Into<SharedString>) -> Self {
        let label = name.into();
        Self::new(ElementId::Name(label.clone()), label)
    }

    pub fn shortcut(mut self, shortcut: impl Into<SharedString>) -> Self {
        let sc = shortcut.into();
        if !sc.trim().is_empty() {
            self.shortcut = Some(sc);
        }
        self
    }

    pub fn selected(mut self, selected: bool) -> Self {
        self.selected = selected;
        self
    }

    pub fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }

    pub fn on_click(
        mut self,
        handler: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.on_click = Some(Box::new(handler));
        self
    }

    pub fn on_hover(mut self, handler: impl Fn(&bool, &mut Window, &mut App) + 'static) -> Self {
        self.on_hover = Some(Box::new(handler));
        self
    }
}

impl RenderOnce for MenuItem {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = cx.global::<ThemeManager>().theme();
        let is_selected = self.selected;
        let is_disabled = self.disabled;

        let label_color = if is_disabled {
            theme.text_muted
        } else if is_selected {
            theme.text_accent
        } else {
            theme.text_primary
        };

        div()
            .id(self.id)
            .w_full()
            .h(ControlHeight::MD)
            .px(Spacing::MDS)
            .flex()
            .flex_row()
            .items_center()
            .gap(Spacing::SM)
            .rounded(Radius::SM)
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .overflow_hidden()
                    .whitespace_nowrap()
                    .text_size(FontSize::BODY)
                    .text_color(label_color)
                    .child(self.label),
            )
            .when_some(self.shortcut, |this, sc| {
                this.child(
                    div()
                        .flex_none()
                        .whitespace_nowrap()
                        .text_size(FontSize::CAPTION)
                        .text_color(theme.text_muted)
                        .child(sc),
                )
            })
            .when(is_selected, |this| {
                this.child(
                    Icon::new(IconName::Check)
                        .size(IconSize::Small)
                        .color(if is_disabled {
                            theme.text_muted
                        } else {
                            theme.text_accent
                        }),
                )
            })
            .when(is_disabled, |this| this.opacity(0.5))
            .when(!is_disabled, |this| {
                this.cursor_pointer()
                    .hover(|s| s.bg(theme.btn_hover))
                    .active(|s| s.bg(theme.btn_active))
                    .when_some(self.on_click, |this, on_click| {
                        this.on_click(move |event, window, cx| on_click(event, window, cx))
                    })
                    .when_some(self.on_hover, |this, on_hover| {
                        this.on_hover(move |is_hovered, window, cx| {
                            on_hover(is_hovered, window, cx)
                        })
                    })
            })
    }
}
