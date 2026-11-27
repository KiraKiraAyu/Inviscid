use std::rc::Rc;

use gpui::prelude::FluentBuilder;
use gpui::*;

use crate::app::commands;
use crate::i18n::{ActiveLocale, TextKey};
use crate::theme::ThemeManager;
use crate::ui::{
    CircularProgress, ControlHeight, FontSize, Icon, IconButton, IconName, IconSize, LineHeight,
    Radius, Spacing,
};
use crate::updater::{ManualUpdateReason, ReleaseInfo, UpdateStatus};

pub(crate) fn format_manual_reason(reason: &ManualUpdateReason, cx: &App) -> String {
    match reason {
        ManualUpdateReason::CompileTimeDisabled => {
            cx.t(TextKey::UpdateManualCompileDisabled).to_string()
        }
        ManualUpdateReason::MinUpdaterVersion => cx.t(TextKey::UpdateManualMinVersion).to_string(),
        ManualUpdateReason::ExternalExplanation(msg) => msg.clone(),
        ManualUpdateReason::DevBuild => cx.t(TextKey::UpdateManualDevBuild).to_string(),
        ManualUpdateReason::PackageManager(mgr) => cx.tf(
            TextKey::UpdateManualPackageManager,
            &[("manager", mgr.as_str())],
        ),
        ManualUpdateReason::TranslocatedOrReadOnly(desc) => cx.tf(
            TextKey::UpdateManualTranslocated,
            &[("reason", desc.as_str())],
        ),
        ManualUpdateReason::ReadOnlyLocation => cx.t(TextKey::UpdateManualReadOnly).to_string(),
    }
}

fn release_from_status(status: &UpdateStatus) -> Option<&ReleaseInfo> {
    match status {
        UpdateStatus::Available(release)
        | UpdateStatus::ManualUpdateOnly { release, .. }
        | UpdateStatus::Downloading { release, .. }
        | UpdateStatus::Verifying { release }
        | UpdateStatus::Extracting { release } => Some(release),
        UpdateStatus::ReadyToInstall(staged) => Some(&staged.release),
        UpdateStatus::Idle
        | UpdateStatus::Checking { .. }
        | UpdateStatus::UpToDate { .. }
        | UpdateStatus::Applying { .. }
        | UpdateStatus::Failed { .. } => None,
    }
}

fn render_changelog_lines(notes: Option<&str>, empty_text: String, theme: &crate::theme::Theme) -> Vec<AnyElement> {
    let Some(raw) = notes else {
        return vec![
            div()
                .text_size(FontSize::SM)
                .text_color(theme.text_muted)
                .child(empty_text)
                .into_any_element(),
        ];
    };

    let lines: Vec<AnyElement> = raw
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(|line| {
            let bullet = line.strip_prefix("- ").or_else(|| line.strip_prefix("* "));
            if let Some(item) = bullet {
                div()
                    .w_full()
                    .flex()
                    .flex_row()
                    .items_start()
                    .gap(Spacing::SM)
                    .child(
                        div()
                            .flex_none()
                            .text_size(FontSize::SM)
                            .line_height(LineHeight::SM)
                            .text_color(theme.text_accent)
                            .child("•"),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .text_size(FontSize::SM)
                            .line_height(LineHeight::SM)
                            .text_color(theme.text_primary)
                            .child(item.to_string()),
                    )
                    .into_any_element()
            } else {
                div()
                    .w_full()
                    .text_size(FontSize::SM)
                    .line_height(LineHeight::SM)
                    .text_color(theme.text_primary)
                    .child(line.to_string())
                    .into_any_element()
            }
        })
        .collect();

    if lines.is_empty() {
        vec![
            div()
                .text_size(FontSize::SM)
                .text_color(theme.text_muted)
                .child(empty_text)
                .into_any_element(),
        ]
    } else {
        lines
    }
}

/// Renders the centered Release Changelog modal when `status` represents an update with metadata.
pub fn render_update_changelog_modal(
    status: &UpdateStatus,
    on_close: impl Fn(&mut Window, &mut App) + 'static,
    cx: &App,
) -> Option<AnyElement> {
    let release = release_from_status(status)?;
    let theme = cx.global::<ThemeManager>().theme().clone();
    let on_close = Rc::new(on_close);
    let on_backdrop_close = on_close.clone();
    let on_x_close = on_close.clone();

    let title = cx.tf(
        TextKey::UpdateChangelogTitle,
        &[("version", release.version.as_str())],
    );
    let date_label = release
        .pub_date
        .split('T')
        .next()
        .filter(|d| !d.is_empty())
        .map(str::to_string);
    let notes_text = release.localized_notes(cx.locale().code());
    let empty_text = cx.t(TextKey::UpdateChangelogEmpty).to_string();
    let open_external_label = cx.t(TextKey::UpdateChangelogOpenExternal).to_string();
    let notes_url = release.release_notes_url.clone();
    let notes_url_for_footer = release.release_notes_url.clone();

    let manual_notice = if let UpdateStatus::ManualUpdateOnly { reason, .. } = status {
        Some(format_manual_reason(reason, cx))
    } else {
        None
    };

    let footer = match status {
        UpdateStatus::Available(_) => {
            let skip_label = cx.t(TextKey::UpdateActionSkip).to_string();
            let download_label = cx.t(TextKey::UpdateActionDownload).to_string();
            let close_on_skip = on_close.clone();
            let close_on_download = on_close.clone();
            div()
                .w_full()
                .flex()
                .flex_row()
                .items_center()
                .justify_end()
                .gap(Spacing::SM)
                .child(
                    div()
                        .id("changelog_action_skip")
                        .debug_selector(|| "changelog_action_skip".to_string())
                        .px(px(14.0))
                        .h(ControlHeight::LG)
                        .bg(theme.btn_bg)
                        .hover(|s| s.bg(theme.btn_hover))
                        .active(|s| s.bg(theme.btn_active))
                        .text_color(theme.btn_text)
                        .text_size(FontSize::SM)
                        .font_weight(FontWeight::MEDIUM)
                        .rounded(Radius::MD)
                        .border_1()
                        .border_color(theme.border)
                        .flex()
                        .items_center()
                        .justify_center()
                        .cursor_pointer()
                        .child(skip_label)
                        .on_click(move |_, window, cx| {
                            commands::skip_available_update(cx);
                            (close_on_skip)(window, cx);
                        }),
                )
                .child(
                    div()
                        .id("changelog_action_download")
                        .debug_selector(|| "changelog_action_download".to_string())
                        .px(px(14.0))
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
                        .child(download_label)
                        .on_click(move |_, window, cx| {
                            (close_on_download)(window, cx);
                            commands::download_available_update(cx);
                        }),
                )
                .into_any_element()
        }
        UpdateStatus::ManualUpdateOnly { .. } => {
            let skip_label = cx.t(TextKey::UpdateActionSkip).to_string();
            let notes_btn_label = cx.t(TextKey::UpdateActionReleaseNotes).to_string();
            let close_on_skip = on_close.clone();
            let close_on_notes = on_close.clone();
            div()
                .w_full()
                .flex()
                .flex_row()
                .items_center()
                .justify_end()
                .gap(Spacing::SM)
                .child(
                    div()
                        .id("changelog_action_skip")
                        .debug_selector(|| "changelog_action_skip".to_string())
                        .px(px(14.0))
                        .h(ControlHeight::LG)
                        .bg(theme.btn_bg)
                        .hover(|s| s.bg(theme.btn_hover))
                        .active(|s| s.bg(theme.btn_active))
                        .text_color(theme.btn_text)
                        .text_size(FontSize::SM)
                        .font_weight(FontWeight::MEDIUM)
                        .rounded(Radius::MD)
                        .border_1()
                        .border_color(theme.border)
                        .flex()
                        .items_center()
                        .justify_center()
                        .cursor_pointer()
                        .child(skip_label)
                        .on_click(move |_, window, cx| {
                            commands::skip_available_update(cx);
                            (close_on_skip)(window, cx);
                        }),
                )
                .child(
                    div()
                        .id("changelog_action_manual_download")
                        .px(px(14.0))
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
                        .child(notes_btn_label)
                        .on_click(move |_, window, cx| {
                            commands::open_update_release_notes(&notes_url_for_footer);
                            (close_on_notes)(window, cx);
                        }),
                )
                .into_any_element()
        }
        UpdateStatus::ReadyToInstall(_) => {
            let restart_label = cx.t(TextKey::UpdateActionRestart).to_string();
            let close_on_restart = on_close.clone();
            div()
                .w_full()
                .flex()
                .flex_row()
                .items_center()
                .justify_end()
                .child(
                    div()
                        .id("changelog_action_restart")
                        .debug_selector(|| "changelog_action_restart".to_string())
                        .px(px(14.0))
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
                        .child(restart_label)
                        .on_click(move |_, window, cx| {
                            (close_on_restart)(window, cx);
                            commands::install_staged_update(cx);
                        }),
                )
                .into_any_element()
        }
        UpdateStatus::Downloading {
            downloaded_bytes,
            total_bytes,
            ..
        } => {
            let fraction = match total_bytes {
                Some(total) if *total > 0 => {
                    (*downloaded_bytes as f32 / *total as f32).clamp(0.0, 1.0)
                }
                _ => 0.0,
            };
            let status_text = cx.tf(
                TextKey::UpdateDownloading,
                &[("version", release.version.as_str())],
            );
            div()
                .w_full()
                .flex()
                .flex_row()
                .items_center()
                .gap(Spacing::SM)
                .child(CircularProgress::determinate(fraction).size(IconSize::Small.pixels()))
                .child(
                    div()
                        .text_size(FontSize::SM)
                        .text_color(theme.text_secondary)
                        .child(status_text),
                )
                .into_any_element()
        }
        UpdateStatus::Verifying { .. } => {
            let status_text = cx.tf(
                TextKey::UpdateVerifying,
                &[("version", release.version.as_str())],
            );
            div()
                .w_full()
                .flex()
                .flex_row()
                .items_center()
                .gap(Spacing::SM)
                .child(
                    CircularProgress::indeterminate("changelog_verifying_spinner")
                        .size(IconSize::Small.pixels()),
                )
                .child(
                    div()
                        .text_size(FontSize::SM)
                        .text_color(theme.text_secondary)
                        .child(status_text),
                )
                .into_any_element()
        }
        UpdateStatus::Extracting { .. } => {
            let status_text = cx.tf(
                TextKey::UpdateExtracting,
                &[("version", release.version.as_str())],
            );
            div()
                .w_full()
                .flex()
                .flex_row()
                .items_center()
                .gap(Spacing::SM)
                .child(
                    CircularProgress::indeterminate("changelog_extracting_spinner")
                        .size(IconSize::Small.pixels()),
                )
                .child(
                    div()
                        .text_size(FontSize::SM)
                        .text_color(theme.text_secondary)
                        .child(status_text),
                )
                .into_any_element()
        }
        _ => div().into_any_element(),
    };

    Some(
        div()
            .id("update_changelog_backdrop")
            .occlude()
            .absolute()
            .inset_0()
            .bg(theme.bg_app.opacity(0.75))
            .flex()
            .items_center()
            .justify_center()
            .on_mouse_down(MouseButton::Left, move |_, window, cx| {
                (on_backdrop_close)(window, cx);
            })
            .child(
                div()
                    .id("update_changelog_modal")
                    .debug_selector(|| "update_changelog_modal".to_string())
                    .w(px(460.0))
                    .max_w(px(520.0))
                    .bg(theme.bg_elevated)
                    .border_1()
                    .border_color(theme.border)
                    .rounded(Radius::XL)
                    .shadow_lg()
                    .p(Spacing::XL)
                    .flex()
                    .flex_col()
                    .gap(Spacing::MD)
                    .on_mouse_down(MouseButton::Left, |_, _, cx| {
                        cx.stop_propagation();
                    })
                    .child(
                        div()
                            .w_full()
                            .flex()
                            .flex_row()
                            .items_start()
                            .justify_between()
                            .gap(Spacing::SM)
                            .child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .flex()
                                    .flex_col()
                                    .gap(px(2.0))
                                    .child(
                                        div()
                                            .text_size(FontSize::SUBTITLE)
                                            .font_weight(FontWeight::SEMIBOLD)
                                            .text_color(theme.text_primary)
                                            .child(title),
                                    )
                                    .when_some(date_label, |this, date| {
                                        this.child(
                                            div()
                                                .text_size(FontSize::CAPTION)
                                                .text_color(theme.text_muted)
                                                .child(date),
                                        )
                                    }),
                            )
                            .child(
                                IconButton::new("update_changelog_close_btn", IconName::Close)
                                    .debug_selector("update_changelog_close")
                                    .size(ControlHeight::SM)
                                    .icon_size(IconSize::XSmall)
                                    .on_click(move |_, window, cx| {
                                        (on_x_close)(window, cx);
                                    }),
                            ),
                    )
                    .when_some(manual_notice, |this, notice| {
                        this.child(
                            div()
                                .w_full()
                                .px(Spacing::MD)
                                .py(Spacing::SM)
                                .rounded(Radius::MD)
                                .bg(theme.bg_surface)
                                .border_1()
                                .border_color(theme.text_warning.opacity(0.4))
                                .text_size(FontSize::CAPTION)
                                .line_height(LineHeight::CAPTION)
                                .text_color(theme.text_warning)
                                .child(notice),
                        )
                    })
                    .child(
                        div()
                            .id("update_changelog_body")
                            .w_full()
                            .max_h(px(260.0))
                            .overflow_y_scroll()
                            .bg(theme.bg_surface)
                            .border_1()
                            .border_color(theme.border_subtle)
                            .rounded(Radius::MD)
                            .p(Spacing::MD)
                            .flex()
                            .flex_col()
                            .gap(Spacing::XS)
                            .children(render_changelog_lines(notes_text, empty_text, &theme)),
                    )
                    .child(
                        div()
                            .id("update_changelog_external_link")
                            .flex()
                            .flex_row()
                            .items_center()
                            .gap(Spacing::XS)
                            .cursor_pointer()
                            .text_size(FontSize::CAPTION)
                            .text_color(theme.text_accent)
                            .hover(|s| s.text_color(theme.text_primary))
                            .on_click(move |_, _, _| {
                                commands::open_update_release_notes(&notes_url);
                            })
                            .child(
                                Icon::new(IconName::ExternalLink)
                                    .size(IconSize::XSmall)
                                    .color(theme.text_accent),
                            )
                            .child(open_external_label),
                    )
                    .child(footer),
            )
            .into_any_element(),
    )
}
