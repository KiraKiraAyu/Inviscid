use gpui::prelude::FluentBuilder;
use gpui::{
    AnyElement, App, InteractiveElement, IntoElement, ParentElement, Pixels, RenderOnce, Styled,
    Window, WindowControlArea, div,
};

use crate::platform::{should_render_client_window_controls, titlebar_left_padding};
use crate::theme::ThemeManager;
use crate::ui::tokens::ControlHeight;
use crate::ui::window_controls::WindowControls;

/// Platform-aware window titlebar with left, center drag, and right slots plus native or
/// client-side window controls.
#[derive(IntoElement)]
pub struct WindowTitleBar {
    left: Option<AnyElement>,
    center: Option<AnyElement>,
    right: Option<AnyElement>,
}

impl WindowTitleBar {
    pub const HEIGHT: Pixels = ControlHeight::LG;

    pub fn new() -> Self {
        Self {
            left: None,
            center: None,
            right: None,
        }
    }

    pub fn left(mut self, el: impl IntoElement) -> Self {
        self.left = Some(el.into_any_element());
        self
    }

    pub fn center(mut self, el: impl IntoElement) -> Self {
        self.center = Some(el.into_any_element());
        self
    }

    pub fn right(mut self, el: impl IntoElement) -> Self {
        self.right = Some(el.into_any_element());
        self
    }
}

impl Default for WindowTitleBar {
    fn default() -> Self {
        Self::new()
    }
}

impl RenderOnce for WindowTitleBar {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = cx.global::<ThemeManager>().theme();
        let show_controls = should_render_client_window_controls();
        let has_right = self.right.is_some() || show_controls;

        div()
            .id("window_titlebar")
            .w_full()
            .h(Self::HEIGHT)
            .flex_none()
            .bg(theme.bg_toolbar)
            .border_b_1()
            .border_color(theme.border)
            .pl(titlebar_left_padding())
            .flex()
            .flex_row()
            .items_center()
            .window_control_area(WindowControlArea::Drag)
            .when_some(self.left, |this, left| {
                this.child(div().flex().flex_row().items_center().child(left))
            })
            .child(
                div()
                    .id("titlebar_center_drag")
                    .flex_1()
                    .h_full()
                    .flex()
                    .items_center()
                    .justify_center()
                    .window_control_area(WindowControlArea::Drag)
                    .when_some(self.center, |this, center| this.child(center)),
            )
            .when(has_right, |this| {
                this.child(
                    div()
                        .flex()
                        .flex_row()
                        .items_center()
                        .when_some(self.right, |this, right| this.child(right))
                        .when(show_controls, |this| {
                            this.child(WindowControls::new(Self::HEIGHT))
                        }),
                )
            })
    }
}
