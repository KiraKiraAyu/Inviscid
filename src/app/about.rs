use gpui::prelude::*;
use gpui::*;

use crate::config::AppConfig;
use crate::theme::ThemeManager;
use crate::ui::{ControlHeight, FontSize, Radius, Spacing};

pub struct AboutWindow {
    focus_handle: FocusHandle,
}

impl Focusable for AboutWindow {
    fn focus_handle(&self, _cx: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl AboutWindow {
    pub fn new(cx: &mut Context<Self>) -> Self {
        Self {
            focus_handle: cx.focus_handle(),
        }
    }

    pub fn focus(&self, window: &mut Window) {
        window.focus(&self.focus_handle);
    }
}

impl Render for AboutWindow {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.global::<ThemeManager>().theme();
        let config = cx.global::<AppConfig>();
        window.set_rem_size(px(config.ui_font_size));
        let version = env!("CARGO_PKG_VERSION");

        div()
            .id("about_window")
            .key_context(crate::ui::key_context::ABOUT_WINDOW)
            .on_key_down(cx.listener(|_, event: &KeyDownEvent, window, _| {
                if event.keystroke.key.eq_ignore_ascii_case("escape")
                    || event.keystroke.key.eq_ignore_ascii_case("enter")
                {
                    window.remove_window();
                }
            }))
            .size_full()
            .bg(theme.bg_editor)
            .rounded(Radius::XL)
            .flex()
            .flex_col()
            .items_center()
            .justify_between()
            .p(Spacing::XXL)
            .window_control_area(WindowControlArea::Drag)
            // Content Area (Centered: Icon -> Title -> Version -> License)
            .child(
                div()
                    .w_full()
                    .flex()
                    .flex_col()
                    .items_center()
                    // App Logo (Squircle container)
                    .child(
                        div()
                            .w(px(58.0))
                            .h(px(58.0))
                            .rounded(Radius::XXL)
                            .bg(theme.bg_toolbar)
                            .flex()
                            .items_center()
                            .justify_center()
                            .child(
                                // TODO: Logo
                                div()
                                    .text_size(FontSize::HERO)
                                    .font_weight(FontWeight::BOLD)
                                    .text_color(theme.text_accent)
                                    .child("I"),
                            ),
                    )
                    // Product Title
                    .child(
                        div()
                            .mt(Spacing::LG)
                            .text_size(FontSize::H2)
                            .font_weight(FontWeight::BOLD)
                            .text_color(theme.text_primary)
                            .child("Inviscid"),
                    )
                    // Metadata Info
                    .child(
                        div()
                            .mt(Spacing::LG)
                            .flex()
                            .flex_col()
                            .items_center()
                            .gap(Spacing::XXS)
                            .child(
                                div()
                                    .text_size(FontSize::CAPTION)
                                    .text_color(theme.text_muted)
                                    .child("Version"),
                            )
                            .child(
                                div()
                                    .text_size(FontSize::SM)
                                    .text_color(theme.text_secondary)
                                    .font_weight(FontWeight::MEDIUM)
                                    .child(version),
                            )
                            .child(
                                div()
                                    .mt(Spacing::MDS)
                                    .text_size(FontSize::CAPTION)
                                    .text_color(theme.text_muted)
                                    .child("License"),
                            )
                            .child(
                                div()
                                    .text_size(FontSize::SM)
                                    .text_color(theme.text_secondary)
                                    .font_weight(FontWeight::MEDIUM)
                                    .child("MIT"),
                            ),
                    ),
            )
            .child(
                div()
                    .id("about_actions")
                    .track_focus(&self.focus_handle)
                    .w_full()
                    .mt(Spacing::XL)
                    .flex()
                    .flex_row()
                    .items_center()
                    .gap(Spacing::MD)
                    .child(
                        div()
                            .id("about_ok_btn")
                            .flex_1()
                            .h(ControlHeight::LG)
                            .bg(theme.btn_primary_bg)
                            .hover(|s| s.bg(theme.btn_primary_hover))
                            .active(|s| s.bg(theme.btn_primary_active))
                            .text_color(theme.btn_primary_text)
                            .text_size(FontSize::SM)
                            .font_weight(FontWeight::SEMIBOLD)
                            .rounded(Radius::MD)
                            .flex()
                            .items_center()
                            .justify_center()
                            .cursor_pointer()
                            .child("OK")
                            .on_click(|_, window, _| {
                                window.remove_window();
                            }),
                    ),
            )
    }
}
