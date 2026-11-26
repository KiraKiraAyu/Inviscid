use gpui::prelude::*;
use gpui::*;

use crate::app::commands;
use crate::config::AppConfig;
use crate::i18n::{ActiveLocale, TextKey};
use crate::theme::ThemeManager;
use crate::ui::{ControlHeight, FontSize, Radius, Spacing};
use crate::updater::{UpdateManager, UpdateStatus};

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
        let version_label = cx.t(TextKey::AboutVersion).to_string();
        let license_label = cx.t(TextKey::AboutLicense).to_string();
        let ok_label = cx.t(TextKey::CommonOk).to_string();

        let update_status = cx
            .try_global::<UpdateManager>()
            .map(UpdateManager::status)
            .unwrap_or(UpdateStatus::Idle);
        let (update_summary, update_action_label, update_action_enabled, notes_url_opt) =
            match &update_status {
                UpdateStatus::Checking { .. } => (
                    Some(cx.t(TextKey::UpdateChecking).to_string()),
                    cx.t(TextKey::UpdateChecking).to_string(),
                    false,
                    None,
                ),
                UpdateStatus::UpToDate { .. } => (
                    Some(cx.t(TextKey::UpdateUpToDate).to_string()),
                    cx.t(TextKey::AboutCheckForUpdates).to_string(),
                    true,
                    None,
                ),
                UpdateStatus::Available(rel) => (
                    Some(cx.tf(
                        TextKey::UpdateAvailableTitle,
                        &[("version", rel.version.as_str())],
                    )),
                    cx.t(TextKey::UpdateActionDownload).to_string(),
                    true,
                    None,
                ),
                UpdateStatus::ManualUpdateOnly { release, .. } => (
                    Some(cx.tf(
                        TextKey::UpdateManualTitle,
                        &[("version", release.version.as_str())],
                    )),
                    cx.t(TextKey::UpdateActionReleaseNotes).to_string(),
                    true,
                    Some(release.release_notes_url.clone()),
                ),
                UpdateStatus::Downloading { release, .. } => (
                    Some(cx.tf(
                        TextKey::UpdateDownloading,
                        &[("version", release.version.as_str())],
                    )),
                    cx.t(TextKey::UpdateDownloading)
                        .replace("{version}", release.version.as_str()),
                    false,
                    None,
                ),
                UpdateStatus::Verifying { release } | UpdateStatus::Extracting { release } => (
                    Some(cx.tf(
                        TextKey::UpdateVerifying,
                        &[("version", release.version.as_str())],
                    )),
                    cx.t(TextKey::UpdateVerifying)
                        .replace("{version}", release.version.as_str()),
                    false,
                    None,
                ),
                UpdateStatus::ReadyToInstall(staged) => (
                    Some(cx.tf(
                        TextKey::UpdateReadyTitle,
                        &[("version", staged.release.version.as_str())],
                    )),
                    cx.t(TextKey::UpdateActionRestart).to_string(),
                    true,
                    None,
                ),
                UpdateStatus::Applying { version } => (
                    Some(cx.tf(TextKey::UpdateApplying, &[("version", version.as_str())])),
                    cx.t(TextKey::UpdateApplying)
                        .replace("{version}", version.as_str()),
                    false,
                    None,
                ),
                UpdateStatus::Failed { message, .. } => (
                    Some(cx.tf(TextKey::UpdateFailed, &[("error", message.as_str())])),
                    cx.t(TextKey::UpdateActionRetry).to_string(),
                    true,
                    None,
                ),
                UpdateStatus::Idle => (
                    None,
                    cx.t(TextKey::AboutCheckForUpdates).to_string(),
                    true,
                    None,
                ),
            };
        let is_ready_to_install = matches!(update_status, UpdateStatus::ReadyToInstall(_));
        let is_available = matches!(update_status, UpdateStatus::Available(_));
        let is_manual = matches!(update_status, UpdateStatus::ManualUpdateOnly { .. });

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
                    // App Logo
                    .child(img("images/Inviscid.svg").size(px(58.0)))
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
                                    .child(version_label),
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
                                    .child(license_label),
                            )
                            .child(
                                div()
                                    .text_size(FontSize::SM)
                                    .text_color(theme.text_secondary)
                                    .font_weight(FontWeight::MEDIUM)
                                    .child("GPL-3.0"),
                            )
                            .when_some(update_summary, |this, summary| {
                                this.child(
                                    div()
                                        .mt(Spacing::MDS)
                                        .text_size(FontSize::CAPTION)
                                        .text_color(theme.text_accent)
                                        .child(summary),
                                )
                            }),
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
                            .id("about_update_btn")
                            .flex_1()
                            .h(ControlHeight::LG)
                            .bg(theme.btn_bg)
                            .when(update_action_enabled, |s| {
                                s.hover(|h| h.bg(theme.btn_hover))
                                    .active(|a| a.bg(theme.btn_active))
                                    .cursor_pointer()
                            })
                            .text_color(if update_action_enabled {
                                theme.btn_text
                            } else {
                                theme.text_muted
                            })
                            .text_size(FontSize::SM)
                            .font_weight(FontWeight::MEDIUM)
                            .rounded(Radius::MD)
                            .flex()
                            .items_center()
                            .justify_center()
                            .child(update_action_label)
                            .on_click(move |_, _, cx| {
                                if !update_action_enabled {
                                    return;
                                }
                                if is_ready_to_install {
                                    commands::install_staged_update(cx);
                                } else if is_available {
                                    commands::download_available_update(cx);
                                } else if is_manual {
                                    if let Some(ref url) = notes_url_opt {
                                        commands::open_update_release_notes(url);
                                    }
                                } else {
                                    commands::check_for_updates(cx);
                                }
                            }),
                    )
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
                            .child(ok_label)
                            .on_click(|_, window, _| {
                                window.remove_window();
                            }),
                    ),
            )
    }
}
