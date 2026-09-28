use gpui::prelude::*;
use gpui::*;

use crate::app::actions::display_keystroke_for;
use crate::theme::Theme;
use crate::ui::{FontSize, Icon, IconName, IconSize, Radius, Spacing};

pub fn render_welcome_button(
    id: &'static str,
    icon: IconName,
    label: &'static str,
    shortcut: impl Into<SharedString>,
    theme: &Theme,
    on_click: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
) -> impl IntoElement {
    div()
        .id(ElementId::Name(id.into()))
        .w(px(240.0))
        .h(px(34.0))
        .px(px(14.0))
        .flex()
        .flex_row()
        .items_center()
        .justify_between()
        .rounded(Radius::MD)
        .bg(theme.bg_toolbar)
        .hover(|s| s.bg(theme.btn_hover).border_color(theme.border))
        .active(|s| s.bg(theme.btn_active))
        .cursor_pointer()
        .on_click(on_click)
        .child(
            div()
                .flex()
                .flex_row()
                .items_center()
                .gap(Spacing::XS)
                .child(
                    Icon::new(icon)
                        .size(IconSize::Small)
                        .color(theme.text_accent),
                )
                .child(
                    div()
                        .text_size(FontSize::SM)
                        .text_color(theme.text_primary)
                        .child(label),
                ),
        )
        .child(
            div()
                .text_size(FontSize::CAPTION)
                .text_color(theme.text_muted)
                .child(shortcut.into()),
        )
}

pub fn render_welcome_view(
    focus_handle: &FocusHandle,
    theme: &Theme,
    cx: &App,
) -> impl IntoElement {
    let fh_new = focus_handle.clone();
    let fh_open_file = focus_handle.clone();
    let fh_open_folder = focus_handle.clone();
    div()
        .flex_1()
        .w_full()
        .h_full()
        .bg(theme.bg_editor)
        .flex()
        .flex_col()
        .items_center()
        .justify_center()
        .child(
            div()
                .flex()
                .flex_col()
                .items_center()
                .gap(Spacing::MDS)
                .child(render_welcome_button(
                    "welcome_new_file_btn",
                    IconName::FileText,
                    "New File",
                    display_keystroke_for("NewTab", cx),
                    theme,
                    move |_, window, cx| {
                        window.focus(&fh_new);
                        window.dispatch_action(crate::app::actions::NewTab.boxed_clone(), cx);
                    },
                ))
                .child(render_welcome_button(
                    "welcome_open_file_btn",
                    IconName::FileText,
                    "Open File",
                    display_keystroke_for("OpenFile", cx),
                    theme,
                    move |_, window, cx| {
                        window.focus(&fh_open_file);
                        window.dispatch_action(crate::app::actions::OpenFile.boxed_clone(), cx);
                    },
                ))
                .child(render_welcome_button(
                    "welcome_open_folder_btn",
                    IconName::Folder,
                    "Open Folder",
                    display_keystroke_for("OpenFolder", cx),
                    theme,
                    move |_, window, cx| {
                        window.focus(&fh_open_folder);
                        window.dispatch_action(crate::app::actions::OpenFolder.boxed_clone(), cx);
                    },
                )),
        )
}
