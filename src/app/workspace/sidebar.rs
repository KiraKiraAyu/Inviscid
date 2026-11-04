use gpui::prelude::*;
use gpui::*;

use super::Workspace;
use crate::theme::Theme;

pub const DEFAULT_SIDEBAR_WIDTH: f32 = 240.0;
pub const MIN_SIDEBAR_WIDTH: f32 = 160.0;
pub const MAX_SIDEBAR_WIDTH: f32 = 800.0;

#[inline]
pub fn clamp_sidebar_width(width: Pixels) -> Pixels {
    width.clamp(px(MIN_SIDEBAR_WIDTH), px(MAX_SIDEBAR_WIDTH))
}

pub fn render_panel_resize_canvas(ws_weak: WeakEntity<Workspace>) -> impl IntoElement {
    let ws_move = ws_weak.clone();
    let ws_up = ws_weak;
    canvas(
        |_bounds, _window, _cx| {},
        move |_bounds, (), window, _cx| {
            window.on_mouse_event(
                move |event: &MouseMoveEvent,
                      phase: DispatchPhase,
                      _window: &mut Window,
                      cx: &mut App| {
                    if phase != DispatchPhase::Capture {
                        return;
                    }
                    let handled = if let Some(ws) = ws_move.upgrade() {
                        ws.update(cx, |this, cx| {
                            if !this.is_resizing_panel() {
                                return false;
                            }
                            if event.pressed_button != Some(MouseButton::Left) {
                                this.finish_panel_resize(cx);
                                return true;
                            }
                            this.handle_panel_resize_drag(event.position.x, cx);
                            true
                        })
                    } else {
                        false
                    };
                    if handled {
                        cx.stop_propagation();
                    }
                },
            );

            window.on_mouse_event(
                move |event: &MouseUpEvent,
                      phase: DispatchPhase,
                      _window: &mut Window,
                      cx: &mut App| {
                    if phase != DispatchPhase::Capture {
                        return;
                    }
                    if event.button == MouseButton::Left {
                        let stopped = if let Some(ws) = ws_up.upgrade() {
                            ws.update(cx, |this, cx| {
                                if this.is_resizing_panel() {
                                    this.finish_panel_resize(cx);
                                    true
                                } else {
                                    false
                                }
                            })
                        } else {
                            false
                        };
                        if stopped {
                            cx.stop_propagation();
                        }
                    }
                },
            );
        },
    )
    .size_0()
    .absolute()
}

/// The sidebar frame; owns the width and the divider so the panels inside only fill it.
pub fn render_sidebar_panel(width: Pixels, content: AnyElement, theme: &Theme) -> impl IntoElement {
    render_docked_panel(
        "sidebar_panel",
        width,
        content,
        theme,
        DockedPanelSide::Left,
    )
}

/// The outline frame docked to the right of the editor.
pub fn render_right_panel(width: Pixels, content: AnyElement, theme: &Theme) -> impl IntoElement {
    render_docked_panel("right_panel", width, content, theme, DockedPanelSide::Right)
}

#[derive(Clone, Copy)]
enum DockedPanelSide {
    Left,
    Right,
}

fn render_docked_panel(
    id: &'static str,
    width: Pixels,
    content: AnyElement,
    theme: &Theme,
    side: DockedPanelSide,
) -> impl IntoElement {
    let panel = div()
        .id(id)
        .w(width)
        .h_full()
        .bg(theme.bg_toolbar)
        .flex()
        .flex_col()
        .overflow_hidden();

    match side {
        DockedPanelSide::Left => panel
            .border_r_1()
            .border_color(theme.border_subtle)
            .child(content),
        DockedPanelSide::Right => panel
            .border_l_1()
            .border_color(theme.border_subtle)
            .child(content),
    }
}

pub fn render_sidebar_resize_handle(
    is_resizing: bool,
    ws_weak: WeakEntity<Workspace>,
    theme: &Theme,
) -> impl IntoElement {
    render_panel_resize_handle(
        "sidebar_resize_handle",
        is_resizing,
        ws_weak,
        theme,
        DockedPanelSide::Left,
    )
}

pub fn render_outline_resize_handle(
    is_resizing: bool,
    ws_weak: WeakEntity<Workspace>,
    theme: &Theme,
) -> impl IntoElement {
    render_panel_resize_handle(
        "outline_resize_handle",
        is_resizing,
        ws_weak,
        theme,
        DockedPanelSide::Right,
    )
}

fn render_panel_resize_handle(
    id: &'static str,
    is_resizing: bool,
    ws_weak: WeakEntity<Workspace>,
    theme: &Theme,
    side: DockedPanelSide,
) -> impl IntoElement {
    let (left_margin, right_margin) = match side {
        DockedPanelSide::Left => (px(-2.0), px(-3.0)),
        DockedPanelSide::Right => (px(-3.0), px(-2.0)),
    };

    div()
        .id(id)
        .h_full()
        .w(px(5.0))
        .ml(left_margin)
        .mr(right_margin)
        .cursor_col_resize()
        .hover(|s| s.bg(theme.border))
        .when(is_resizing, |s| s.bg(theme.text_accent))
        .on_mouse_down(MouseButton::Left, move |event, _window, cx| {
            if let Some(ws) = ws_weak.upgrade() {
                ws.update(cx, |this, cx| {
                    if event.click_count == 2 {
                        match side {
                            DockedPanelSide::Left => this.reset_sidebar_width(cx),
                            DockedPanelSide::Right => this.reset_outline_width(cx),
                        }
                    } else {
                        match side {
                            DockedPanelSide::Left => {
                                this.start_sidebar_resize(event.position.x, cx)
                            }
                            DockedPanelSide::Right => {
                                this.start_outline_resize(event.position.x, cx)
                            }
                        }
                    }
                });
            }
        })
}
