use gpui::{
    AnyView, App, AppContext, IntoElement, ParentElement, Render, SharedString, Styled, Window, div,
};

use crate::theme::ActiveTheme;
use crate::ui::tokens::{FontSize, LineHeight, Radius, Spacing};

/// A themed text tooltip view for GPUI `.tooltip(...)` builders.
pub struct Tooltip {
    text: SharedString,
}

impl Tooltip {
    pub fn new(text: impl Into<SharedString>) -> Self {
        Self { text: text.into() }
    }

    /// Returns a GPUI tooltip builder closure that renders a single-line text tooltip.
    pub fn text(text: impl Into<SharedString>) -> impl Fn(&mut Window, &mut App) -> AnyView + 'static {
        let text: SharedString = text.into();
        move |_window, cx| {
            let text = text.clone();
            cx.new(|_| Self::new(text)).into()
        }
    }
}

impl Render for Tooltip {
    fn render(&mut self, _window: &mut Window, cx: &mut gpui::Context<Self>) -> impl IntoElement {
        let theme = cx.theme();

        // GPUI places tooltips at `mouse_position + (1px, 1px)`, so the transparent
        // outer padding keeps the cursor from obscuring the tooltip text.
        div().pl(Spacing::SM).pt(Spacing::MD).child(
            div()
                .px(Spacing::SM)
                .py(Spacing::XS)
                .bg(theme.bg_elevated)
                .border_1()
                .border_color(theme.border)
                .rounded(Radius::MD)
                .shadow_md()
                .text_size(FontSize::CAPTION)
                .line_height(LineHeight::CAPTION)
                .text_color(theme.text_primary)
                .whitespace_nowrap()
                .child(self.text.clone()),
        )
    }
}
