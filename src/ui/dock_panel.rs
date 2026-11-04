use gpui::prelude::*;
use gpui::*;

use crate::theme::Theme;
use crate::ui::tokens::{FontSize, Spacing};

pub const LIST_VERTICAL_PADDING: Pixels = px(4.0);

pub fn row(indent: Pixels, theme: &Theme) -> Div {
    div()
        .h(px(22.0))
        .w_full()
        .pl(indent)
        .pr(Spacing::SM)
        .flex()
        .flex_row()
        .items_center()
        .gap(px(5.0))
        .overflow_hidden()
        .cursor_pointer()
        .hover(|this| this.bg(theme.btn_hover))
}

pub fn empty_state(message: &'static str, theme: &Theme) -> impl IntoElement {
    div()
        .w_full()
        .h_full()
        .flex()
        .flex_col()
        .items_center()
        .justify_center()
        .child(
            div()
                .text_size(FontSize::SM)
                .text_color(theme.text_muted)
                .child(message),
        )
}
