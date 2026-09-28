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

pub fn render_sidebar_resize_canvas(ws_weak: WeakEntity<Workspace>) -> impl IntoElement {
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
                            if !this.is_resizing_sidebar() {
                                return false;
                            }
                            if event.pressed_button != Some(MouseButton::Left) {
                                this.finish_sidebar_resize(cx);
                                return true;
                            }
                            this.handle_sidebar_resize_drag(event.position.x, cx);
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
                                if this.is_resizing_sidebar() {
                                    this.finish_sidebar_resize(cx);
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

pub fn render_sidebar_resize_handle(
    is_resizing: bool,
    ws_weak: WeakEntity<Workspace>,
    theme: &Theme,
) -> impl IntoElement {
    div()
        .id("sidebar_resize_handle")
        .h_full()
        .w(px(5.0))
        .ml(px(-2.0))
        .mr(px(-3.0))
        .cursor_col_resize()
        .hover(|s| s.bg(theme.border))
        .when(is_resizing, |s| s.bg(theme.text_accent))
        .on_mouse_down(MouseButton::Left, move |event, _window, cx| {
            if let Some(ws) = ws_weak.upgrade() {
                ws.update(cx, |this, cx| {
                    if event.click_count == 2 {
                        this.reset_sidebar_width(cx);
                    } else {
                        this.start_sidebar_resize(event.position.x, cx);
                    }
                });
            }
        })
}
