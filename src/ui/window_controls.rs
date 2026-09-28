use gpui::{
    App, InteractiveElement, IntoElement, ParentElement, Pixels, RenderOnce,
    StatefulInteractiveElement, Styled, Window, div, px, white,
};

use crate::platform::{
    CaptionButtonKind, caption_button_icon, minimize_window, toggle_maximize_window,
    window_control_close_hover_bg, window_controls_font,
};
use crate::theme::ThemeManager;
use crate::ui::IconSize;
use crate::ui::tokens::ControlHeight;

/// Client-side window caption controls (Minimize, Maximize / Restore, Close).
#[derive(IntoElement)]
pub struct WindowControls {
    button_height: Pixels,
}

impl WindowControls {
    pub fn new(button_height: Pixels) -> Self {
        Self { button_height }
    }
}

impl Default for WindowControls {
    fn default() -> Self {
        Self::new(ControlHeight::TITLEBAR)
    }
}

impl RenderOnce for WindowControls {
    fn render(self, window: &mut Window, _: &mut App) -> impl IntoElement {
        div()
            .id("window-controls")
            .font_family(window_controls_font())
            .flex()
            .flex_row()
            .content_stretch()
            .max_h(self.button_height)
            .min_h(self.button_height)
            .child(CaptionButton::new(CaptionButtonKind::Minimize))
            .child(if window.is_maximized() {
                CaptionButton::new(CaptionButtonKind::Restore)
            } else {
                CaptionButton::new(CaptionButtonKind::Maximize)
            })
            .child(CaptionButton::new(CaptionButtonKind::Close))
    }
}

#[derive(Clone, Copy, IntoElement)]
struct CaptionButton {
    kind: CaptionButtonKind,
}

impl CaptionButton {
    const ICON_SIZE: Pixels = IconSize::Indicator.pixels();

    pub fn new(kind: CaptionButtonKind) -> Self {
        Self { kind }
    }

    #[inline]
    fn id(&self) -> &'static str {
        match self.kind {
            CaptionButtonKind::Minimize => "minimize",
            CaptionButtonKind::Restore => "restore",
            CaptionButtonKind::Maximize => "maximize",
            CaptionButtonKind::Close => "close",
        }
    }

    #[inline]
    fn icon(&self) -> &'static str {
        caption_button_icon(self.kind)
    }
}

impl RenderOnce for CaptionButton {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = cx.global::<ThemeManager>().theme();

        let (hover_bg, hover_fg, active_bg, active_fg) = match self.kind {
            CaptionButtonKind::Close => {
                let color = window_control_close_hover_bg();
                (color, white(), color.opacity(0.8), white().opacity(0.8))
            }
            _ => (
                theme.btn_hover,
                theme.text_primary,
                theme.btn_active,
                theme.text_primary,
            ),
        };

        div()
            .flex()
            .flex_row()
            .items_center()
            .id(self.id())
            .justify_center()
            .occlude()
            .w(px(36.))
            .h_full()
            .text_size(Self::ICON_SIZE)
            .line_height(Self::ICON_SIZE)
            .text_color(theme.text_primary)
            .hover(move |s| s.bg(hover_bg).text_color(hover_fg))
            .active(move |s| s.bg(active_bg).text_color(active_fg))
            .on_click(move |_, window, _| match self.kind {
                CaptionButtonKind::Minimize => minimize_window(window),
                CaptionButtonKind::Restore | CaptionButtonKind::Maximize => {
                    toggle_maximize_window(window)
                }
                CaptionButtonKind::Close => window.remove_window(),
            })
            .child(self.icon())
    }
}
