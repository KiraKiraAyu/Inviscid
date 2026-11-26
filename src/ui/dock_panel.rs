use gpui::prelude::*;
use gpui::*;

use crate::theme::Theme;
use crate::ui::tokens::{FontSize, Radius, Spacing};

pub const LIST_VERTICAL_PADDING: Pixels = px(4.0);

pub fn section_header(title: impl Into<SharedString>, theme: &Theme) -> impl IntoElement {
    div()
        .w_full()
        .flex_none()
        .px(Spacing::MD)
        .pt(Spacing::SM)
        .pb(Spacing::XS)
        .text_size(FontSize::CAPTION)
        .font_weight(FontWeight::SEMIBOLD)
        .text_color(theme.text_muted)
        .child(title.into())
}

pub fn row(indent: Pixels, theme: &Theme) -> Div {
    div()
        .h(px(26.0))
        .w_full()
        .pl(indent)
        .pr(Spacing::SM)
        .rounded(Radius::LG)
        .flex()
        .flex_row()
        .items_center()
        .gap(px(5.0))
        .overflow_hidden()
        .cursor_pointer()
        .hover(|this| this.bg(theme.btn_hover.opacity(0.55)))
}

pub fn empty_state(message: impl Into<SharedString>, theme: &Theme) -> impl IntoElement {
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
                .child(message.into()),
        )
}

pub fn loading_state(id: impl Into<ElementId>, theme: &Theme) -> impl IntoElement {
    div()
        .w_full()
        .h_full()
        .flex()
        .flex_col()
        .items_center()
        .justify_center()
        .child(crate::ui::render_loading_metaball_animation(
            id,
            theme.text_accent,
            px(32.0),
        ))
}
