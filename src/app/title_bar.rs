use crate::app::actions::{OpenMenu, ToggleMenu};
use crate::app::commands;
use crate::app::menu::{ActiveMenu, MenuTrigger, TextMenu, ViewMenuState};
use crate::app::update_modal::format_manual_reason;
use crate::app::workspace::Workspace;
use crate::config::AppConfig;
use crate::i18n::{ActiveLocale, TextKey};
use crate::theme::ThemeManager;
use crate::ui::tokens::{ControlHeight, FontSize, Radius, Spacing};
use crate::ui::{CircularProgress, Icon, IconButton, IconName, IconSize, Tooltip, WindowTitleBar};
use crate::updater::{UpdateManager, UpdateStatus};
use gpui::prelude::FluentBuilder;
use gpui::*;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TitleBarEvent {
    MenuChanged(Option<ActiveMenu>),
    OpenUpdateChangelog,
}

impl EventEmitter<TitleBarEvent> for TitleBar {}

pub struct TitleBar {
    active_menu: Option<ActiveMenu>,
    workspace: Entity<Workspace>,
}

impl TitleBar {
    pub const HEIGHT: Pixels = WindowTitleBar::HEIGHT;

    pub fn new(workspace: Entity<Workspace>) -> Self {
        Self {
            active_menu: None,
            workspace,
        }
    }

    pub fn set_active_menu(&mut self, menu: Option<ActiveMenu>, cx: &mut Context<Self>) {
        if self.active_menu != menu {
            if let Some(prev) = self.active_menu {
                prev.on_close(cx);
            }
            self.active_menu = menu;
            cx.emit(TitleBarEvent::MenuChanged(menu));
            cx.notify();
        }
    }

    pub fn close_menu(&mut self, cx: &mut Context<Self>) {
        self.set_active_menu(None, cx);
    }

    pub fn notify_workspace_updated(&mut self, cx: &mut Context<Self>) {
        cx.notify();
    }

    fn render_update_controls(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let mgr = cx.try_global::<UpdateManager>()?;
        let status = mgr.status();
        let theme = cx.global::<ThemeManager>().theme().clone();

        let (primary_slot, has_changelog) = match &status {
            UpdateStatus::Available(release) => {
                let tooltip = cx.tf(
                    TextKey::UpdateTooltipDownload,
                    &[("version", release.version.as_str())],
                );
                let slot = IconButton::new("titlebar_update_download_btn", IconName::Download)
                    .debug_selector("titlebar_update_download_btn")
                    .size(ControlHeight::SM)
                    .icon_size(IconSize::Small)
                    .color(theme.text_accent)
                    .tooltip(tooltip)
                    .on_click(|_, _, cx| commands::download_available_update(cx))
                    .into_any_element();
                (slot, true)
            }
            UpdateStatus::ManualUpdateOnly { release, reason } => {
                let tooltip = format_manual_reason(reason, cx);
                let notes_url = release.release_notes_url.clone();
                let slot = IconButton::new("titlebar_update_manual_btn", IconName::ExternalLink)
                    .debug_selector("titlebar_update_manual_btn")
                    .size(ControlHeight::SM)
                    .icon_size(IconSize::Small)
                    .color(theme.text_warning)
                    .tooltip(tooltip)
                    .on_click(move |_, _, _| commands::open_update_release_notes(&notes_url))
                    .into_any_element();
                (slot, true)
            }
            UpdateStatus::Downloading {
                release,
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
                let pct = (fraction * 100.0).round() as u32;
                let pct_str = pct.to_string();
                let tooltip = cx.tf(
                    TextKey::UpdateTooltipDownloading,
                    &[
                        ("version", release.version.as_str()),
                        ("percent", pct_str.as_str()),
                    ],
                );
                let slot = div()
                    .id("titlebar_update_progress")
                    .debug_selector(|| "titlebar_update_progress".to_string())
                    .flex_none()
                    .size(ControlHeight::SM)
                    .flex()
                    .items_center()
                    .justify_center()
                    .tooltip(Tooltip::text(tooltip))
                    .child(CircularProgress::determinate(fraction).size(IconSize::Small.pixels()))
                    .into_any_element();
                (slot, true)
            }
            UpdateStatus::Verifying { release } => {
                let tooltip = cx.tf(
                    TextKey::UpdateVerifying,
                    &[("version", release.version.as_str())],
                );
                let slot = div()
                    .id("titlebar_update_progress")
                    .debug_selector(|| "titlebar_update_progress".to_string())
                    .flex_none()
                    .size(ControlHeight::SM)
                    .flex()
                    .items_center()
                    .justify_center()
                    .tooltip(Tooltip::text(tooltip))
                    .child(
                        CircularProgress::indeterminate("titlebar_update_verifying")
                            .size(IconSize::Small.pixels()),
                    )
                    .into_any_element();
                (slot, true)
            }
            UpdateStatus::Extracting { release } => {
                let tooltip = cx.tf(
                    TextKey::UpdateExtracting,
                    &[("version", release.version.as_str())],
                );
                let slot = div()
                    .id("titlebar_update_progress")
                    .debug_selector(|| "titlebar_update_progress".to_string())
                    .flex_none()
                    .size(ControlHeight::SM)
                    .flex()
                    .items_center()
                    .justify_center()
                    .tooltip(Tooltip::text(tooltip))
                    .child(
                        CircularProgress::indeterminate("titlebar_update_extracting")
                            .size(IconSize::Small.pixels()),
                    )
                    .into_any_element();
                (slot, true)
            }
            UpdateStatus::ReadyToInstall(staged) => {
                let tooltip = cx.tf(
                    TextKey::UpdateTooltipRestart,
                    &[("version", staged.release.version.as_str())],
                );
                let slot = IconButton::new("titlebar_update_restart_btn", IconName::RotateCw)
                    .debug_selector("titlebar_update_restart_btn")
                    .size(ControlHeight::SM)
                    .icon_size(IconSize::Small)
                    .color(theme.text_accent)
                    .tooltip(tooltip)
                    .on_click(|_, _, cx| commands::install_staged_update(cx))
                    .into_any_element();
                (slot, true)
            }
            UpdateStatus::Applying { version } => {
                let tooltip = cx.tf(TextKey::UpdateApplying, &[("version", version.as_str())]);
                let slot = div()
                    .id("titlebar_update_progress")
                    .debug_selector(|| "titlebar_update_progress".to_string())
                    .flex_none()
                    .size(ControlHeight::SM)
                    .flex()
                    .items_center()
                    .justify_center()
                    .tooltip(Tooltip::text(tooltip))
                    .child(
                        CircularProgress::indeterminate("titlebar_update_applying")
                            .size(IconSize::Small.pixels()),
                    )
                    .into_any_element();
                (slot, false)
            }
            UpdateStatus::Idle
            | UpdateStatus::Checking { .. }
            | UpdateStatus::UpToDate { .. }
            | UpdateStatus::Failed { .. } => return None,
        };

        let changelog_slot = if has_changelog {
            IconButton::new("titlebar_update_changelog_btn", IconName::Changelog)
                .debug_selector("titlebar_update_changelog_btn")
                .size(ControlHeight::SM)
                .icon_size(IconSize::Small)
                .tooltip(cx.t(TextKey::UpdateTooltipChangelog).to_string())
                .on_click(cx.listener(|_, _, _, cx| {
                    cx.emit(TitleBarEvent::OpenUpdateChangelog);
                }))
                .into_any_element()
        } else {
            div()
                .flex_none()
                .size(ControlHeight::SM)
                .flex()
                .items_center()
                .justify_center()
                .opacity(0.4)
                .child(
                    Icon::new(IconName::Changelog)
                        .size(IconSize::Small)
                        .color(theme.text_muted),
                )
                .into_any_element()
        };

        Some(
            div()
                .id("titlebar_update_container")
                .debug_selector(|| "titlebar_update_container".to_string())
                .flex_none()
                .h(ControlHeight::MD)
                .px(Spacing::XXS)
                .rounded(Radius::FULL)
                .bg(theme.text_primary.opacity(0.05))
                .border_1()
                .border_color(theme.border_subtle)
                .flex()
                .flex_row()
                .items_center()
                .gap(px(2.0))
                .child(primary_slot)
                .child(changelog_slot)
                .into_any_element(),
        )
    }
}

impl Render for TitleBar {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.global::<ThemeManager>().theme().clone();
        let has_workspace = self.workspace.read(cx).has_workspace(cx);
        let workspace_name = self.workspace.read(cx).workspace_name(cx);
        let is_menu_expanded = self.active_menu.is_some_and(|m| m.is_text_menu());
        let is_workspace_menu_active = self.active_menu == Some(ActiveMenu::Workspace);

        let view_menu_state = ViewMenuState {
            cur_render_mode: self
                .workspace
                .read(cx)
                .active_editor()
                .map(|ed| ed.read(cx).render_mode())
                .unwrap_or(crate::editor::RenderMode::LivePreview),
            is_sidebar_visible: self.workspace.read(cx).sidebar_visible(),
            is_fps_visible: cx.try_global::<AppConfig>().is_some_and(|cfg| cfg.show_fps),
        };

        // Edit menu actions are dispatched at the editor itself; nothing to target without a document.
        let edit_target = self
            .workspace
            .read(cx)
            .active_editor()
            .map(|editor| editor.read(cx).focus_handle(cx));

        let left_group = div()
            .flex()
            .flex_row()
            .items_center()
            .gap(Spacing::SMD)
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .child(if is_menu_expanded {
                div()
                    .flex()
                    .flex_row()
                    .items_center()
                    .gap(Spacing::XXS)
                    .child(MenuTrigger::new(
                        TextMenu::Inviscid,
                        "Inviscid",
                        self.active_menu,
                    ))
                    .child(MenuTrigger::new(
                        TextMenu::File,
                        cx.t(TextKey::MenuFile).to_string(),
                        self.active_menu,
                    ))
                    .child(
                        MenuTrigger::new(
                            TextMenu::Edit,
                            cx.t(TextKey::MenuEdit).to_string(),
                            self.active_menu,
                        )
                        .edit_target(edit_target),
                    )
                    .child(
                        MenuTrigger::new(
                            TextMenu::View,
                            cx.t(TextKey::MenuView).to_string(),
                            self.active_menu,
                        )
                        .view_state(view_menu_state),
                    )
                    .child(MenuTrigger::new(
                        TextMenu::Theme,
                        cx.t(TextKey::MenuTheme).to_string(),
                        self.active_menu,
                    ))
                    .into_any_element()
            } else {
                div()
                    .flex()
                    .flex_row()
                    .items_center()
                    .gap(Spacing::XXS)
                    .child(
                        div()
                            .id("app_menu_hamburger_btn")
                            .debug_selector(|| "app_menu_hamburger_btn".into())
                            .size(ControlHeight::MD)
                            .flex()
                            .items_center()
                            .justify_center()
                            .rounded(Radius::LG)
                            .cursor_pointer()
                            .hover(|s| s.bg(theme.btn_hover))
                            .active(|s| s.bg(theme.btn_active))
                            .on_click(|_, window, cx| {
                                window.dispatch_action(
                                    ToggleMenu(ActiveMenu::Inviscid).boxed_clone(),
                                    cx,
                                );
                            })
                            .child(
                                Icon::new(IconName::Menu)
                                    .size(IconSize::Small)
                                    .color(theme.text_secondary),
                            ),
                    )
                    .child(
                        div()
                            .relative()
                            .id("workspace_selector_btn")
                            .debug_selector(|| "workspace_selector_btn".into())
                            .h(ControlHeight::MD)
                            .px(Spacing::MDS)
                            .rounded(Radius::FULL)
                            .cursor_pointer()
                            .bg(if is_workspace_menu_active {
                                theme.btn_hover
                            } else {
                                theme.text_primary.opacity(0.05)
                            })
                            .hover(|s| s.bg(theme.btn_hover))
                            .active(|s| s.bg(theme.btn_active))
                            .on_mouse_down(MouseButton::Left, move |_, window, cx| {
                                if is_workspace_menu_active {
                                    window.dispatch_action(
                                        crate::app::actions::CloseMenu.boxed_clone(),
                                        cx,
                                    );
                                } else {
                                    window.dispatch_action(
                                        OpenMenu(ActiveMenu::Workspace).boxed_clone(),
                                        cx,
                                    );
                                }
                            })
                            .flex()
                            .flex_row()
                            .items_center()
                            .gap(Spacing::XS)
                            .when(has_workspace, |this| {
                                this.child(
                                    Icon::new(IconName::Folder)
                                        .size(IconSize::Small)
                                        .color(theme.text_accent),
                                )
                            })
                            .child(
                                div()
                                    .text_size(FontSize::SM)
                                    .font_weight(FontWeight::MEDIUM)
                                    .text_color(theme.text_primary)
                                    .child(workspace_name.clone()),
                            )
                            .child(
                                Icon::new(IconName::ChevronDown)
                                    .size(IconSize::Indicator)
                                    .color(theme.text_secondary),
                            )
                            .when(is_workspace_menu_active, |this| {
                                let titlebar = cx.entity().clone();
                                let workspace = self.workspace.clone();
                                let current_root_dir =
                                    workspace.read(cx).root_dir().map(|p| p.to_path_buf());
                                this.child(deferred(crate::app::menu::render_workspace_dropdown(
                                    current_root_dir.as_deref(),
                                    move |ws_state, window, cx| {
                                        titlebar.update(cx, |tb, cx| tb.close_menu(cx));
                                        let wb = crate::config::WindowLayoutState::from_window(
                                            window, cx,
                                        );
                                        crate::app::commands::sync_workspace_state_to_config(
                                            &workspace, wb, cx,
                                        );
                                        workspace.update(cx, |ws, cx| {
                                            ws.restore_from_state(ws_state, cx);
                                            ws.focus_active_editor(window, cx);
                                        });
                                    },
                                    cx,
                                )))
                            }),
                    )
                    .into_any_element()
            });

        let mut titlebar = WindowTitleBar::new().left(left_group);
        if let Some(update_controls) = self.render_update_controls(cx) {
            titlebar = titlebar.right(update_controls);
        }
        titlebar
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use core::prelude::v1::test;

    #[test]
    fn test_active_menu_text_menu_classification() {
        assert!(ActiveMenu::File.is_text_menu());
        assert!(ActiveMenu::Edit.is_text_menu());
        assert!(!ActiveMenu::Workspace.is_text_menu());
    }
}
