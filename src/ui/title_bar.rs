use gpui::prelude::FluentBuilder;
use gpui::{
    AnyElement, App, InteractiveElement, IntoElement, MouseButton, ParentElement, Pixels,
    RenderOnce, Styled, Window, WindowControlArea, div,
};

use crate::platform::{should_render_client_window_controls, titlebar_left_padding};
use crate::theme::ThemeManager;
use crate::ui::tokens::{ControlHeight, Spacing};
use crate::ui::window_controls::WindowControls;

/// Platform-aware window titlebar with left slot, center drag region, optional right slot,
/// and native or client-side window controls.
#[derive(IntoElement)]
pub struct WindowTitleBar {
    left: Option<AnyElement>,
    right: Option<AnyElement>,
}

impl WindowTitleBar {
    pub const HEIGHT: Pixels = ControlHeight::XL;

    pub fn new() -> Self {
        Self {
            left: None,
            right: None,
        }
    }

    pub fn left(mut self, el: impl IntoElement) -> Self {
        self.left = Some(el.into_any_element());
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

        div()
            .id("window_titlebar")
            .occlude()
            .w_full()
            .h(Self::HEIGHT)
            .flex_none()
            .bg(theme.bg_app)
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
                    .window_control_area(WindowControlArea::Drag),
            )
            .when_some(self.right, |this, right| {
                this.child(
                    div()
                        .flex_none()
                        .flex()
                        .flex_row()
                        .items_center()
                        .when(show_controls, |d| d.mr(Spacing::XS))
                        .when(!show_controls, |d| d.mr(Spacing::SM))
                        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                        .child(right),
                )
            })
            .when(show_controls, |this| {
                this.child(WindowControls::new(Self::HEIGHT))
            })
    }
}
