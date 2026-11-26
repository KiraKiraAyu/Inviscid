use gpui::*;
use std::path::PathBuf;

pub mod about;
pub mod actions;
pub mod commands;
pub(crate) mod image_cache;
pub mod lifecycle;
pub(crate) mod menu;
pub(crate) mod outline_panel;
pub mod project_panel;
pub(crate) mod search_panel;
pub mod settings;
mod sidebar_view;
pub mod status_bar;
pub mod tab_bar;
pub(crate) mod title_bar;
pub mod workspace;

pub use about::AboutWindow;
pub(crate) use image_cache::BoundedImageCache;
pub use menu::ActiveMenu;
pub(crate) use outline_panel::{OutlinePanel, OutlinePanelEvent};
pub use project_panel::ProjectPanel;
pub use settings::SettingsWindow;
pub use sidebar_view::SidebarView;
pub use status_bar::StatusBar;
pub use tab_bar::TabBar;
pub use title_bar::TitleBar;
pub use workspace::Workspace;

use crate::config::{AppConfig, WindowLayoutState, WorkspaceState};
use crate::i18n::{ActiveLocale, TextKey};
use crate::theme::ThemeManager;
use crate::ui::{AppBanner, BannerAction, BannerTone, IconName};
use crate::updater::{ManualUpdateReason, UpdateManager, UpdateStatus};

/// Root view entity for an Inviscid editor window.
pub struct InviscidWindow {
    focus_handle: FocusHandle,
    titlebar: Entity<TitleBar>,
    workspace: Entity<Workspace>,
    active_menu: Option<ActiveMenu>,
    cached_window_bounds: Option<WindowLayoutState>,
    window_persistence_attached: bool,
    _workspace_subscription: Subscription,
    _titlebar_subscription: Subscription,
    _window_subscriptions: Vec<Subscription>,
    _bounds_persist_task: Option<Task<()>>,
}

impl InviscidWindow {
    fn init(
        initial_path: Option<PathBuf>,
        workspace: Option<&WorkspaceState>,
        cx: &mut Context<Self>,
    ) -> Self {
        let workspace = if let Some(ws) = workspace {
            cx.new(|cx| Workspace::from_state(ws, cx))
        } else {
            cx.new(|cx| Workspace::new(initial_path, cx))
        };
        let titlebar = cx.new(|_cx| TitleBar::new(workspace.clone()));

        let _workspace_subscription = cx.subscribe(
            &workspace,
            |this, _workspace, event: &crate::app::workspace::WorkspaceEvent, cx| match event {
                crate::app::workspace::WorkspaceEvent::TitleChanged => {
                    commands::sync_workspace_to_global(this, cx);
                    this.titlebar
                        .update(cx, |tb, cx| tb.notify_workspace_updated(cx));
                }
                crate::app::workspace::WorkspaceEvent::LayoutChanged => {
                    commands::sync_workspace_to_global(this, cx);
                }
            },
        );

        let _titlebar_subscription = cx.subscribe(
            &titlebar,
            |this, _titlebar, event: &crate::app::title_bar::TitleBarEvent, cx| match event {
                crate::app::title_bar::TitleBarEvent::MenuChanged(menu) => {
                    if this.active_menu != *menu {
                        this.active_menu = *menu;
                        cx.notify();
                    }
                }
            },
        );

        Self {
            focus_handle: cx.focus_handle(),
            titlebar,
            workspace,
            active_menu: None,
            cached_window_bounds: None,
            window_persistence_attached: false,
            _workspace_subscription,
            _titlebar_subscription,
            _window_subscriptions: Vec::new(),
            _bounds_persist_task: None,
        }
    }

    pub fn new(cx: &mut Context<Self>) -> Self {
        Self::init(None, None, cx)
    }

    pub fn new_with_file(path: PathBuf, cx: &mut Context<Self>) -> Self {
        Self::init(Some(path), None, cx)
    }

    pub(crate) fn new_from_workspace(ws: &WorkspaceState, cx: &mut Context<Self>) -> Self {
        Self::init(None, Some(ws), cx)
    }

    /// Creates a new window according to CLI arguments and application configuration.
    ///
    /// If an initial file path is provided via CLI argument, it will be opened directly.
    /// Otherwise, if workspace restoration is enabled in [`AppConfig`], the last workspace state is restored.
    /// If no workspace is restored, a clean default workspace is created.
    pub fn open_initial(initial_path: Option<PathBuf>, cx: &mut Context<Self>) -> Self {
        if let Some(file) = initial_path {
            Self::new_with_file(file, cx)
        } else {
            let last_ws = cx.try_global::<AppConfig>().and_then(|config| {
                if config.restore_workspace {
                    config.session.last_workspace.clone()
                } else {
                    None
                }
            });

            if let Some(ws) = &last_ws {
                Self::new_from_workspace(ws, cx)
            } else {
                Self::new(cx)
            }
        }
    }

    pub fn workspace(&self) -> &Entity<Workspace> {
        &self.workspace
    }

    pub fn titlebar(&self) -> &Entity<TitleBar> {
        &self.titlebar
    }

    pub fn cached_window_bounds(&self) -> Option<&WindowLayoutState> {
        self.cached_window_bounds.as_ref()
    }

    fn attach_window_observers(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.window_persistence_attached {
            return;
        }
        self.window_persistence_attached = true;

        let bounds_sub = cx.observe_window_bounds(window, |this, window, cx| {
            this.handle_window_bounds_changed(window, cx);
        });
        let activation_sub = cx.observe_window_activation(window, |this, window, cx| {
            if window.is_window_active() {
                this.refresh_window_bounds_in_memory(window, cx);
            }
        });
        self._window_subscriptions.push(bounds_sub);
        self._window_subscriptions.push(activation_sub);

        let weak = cx.entity().downgrade();
        window.on_window_should_close(cx, move |window, cx| {
            let _ = weak.update(cx, |this, cx| {
                this.persist_window_state_sync(window, cx);
            });
            true
        });
    }

    fn handle_window_bounds_changed(&mut self, window: &Window, cx: &mut Context<Self>) {
        let Some(bounds) = WindowLayoutState::from_window(window, cx) else {
            return;
        };
        if self.cached_window_bounds.as_ref() != Some(&bounds) {
            self.cached_window_bounds = Some(bounds);
            if commands::sync_workspace_in_memory(self, cx) {
                self._bounds_persist_task = Some(cx.spawn(async move |this, cx| {
                    cx.background_executor()
                        .timer(std::time::Duration::from_millis(250))
                        .await;
                    let _ = this.update(cx, |this, cx| {
                        commands::sync_workspace_to_global(this, cx);
                    });
                }));
            }
        }
    }

    fn refresh_window_bounds_in_memory(&mut self, window: &Window, cx: &mut Context<Self>) {
        if let Some(bounds) = WindowLayoutState::from_window(window, cx) {
            self.cached_window_bounds = Some(bounds);
        }
        commands::sync_workspace_in_memory(self, cx);
    }

    pub fn persist_window_state_sync(&mut self, window: &Window, cx: &mut Context<Self>) {
        self._bounds_persist_task = None;
        if let Some(bounds) = WindowLayoutState::from_window(window, cx) {
            self.cached_window_bounds = Some(bounds);
        }
        commands::sync_workspace_to_global_sync(self, cx);
    }

    pub fn active_menu(&self) -> Option<ActiveMenu> {
        self.active_menu
    }

    pub fn set_active_menu(&mut self, menu: Option<ActiveMenu>, cx: &mut Context<Self>) {
        if self.active_menu != menu {
            if let Some(prev) = self.active_menu {
                prev.on_close(cx);
            }
            self.active_menu = menu;
            self.titlebar
                .update(cx, |tb, cx| tb.set_active_menu(menu, cx));
            cx.notify();
        }
    }

    pub fn toggle_menu(&mut self, menu: ActiveMenu, cx: &mut Context<Self>) {
        if self.active_menu == Some(menu) {
            self.close_menu(cx);
        } else {
            self.set_active_menu(Some(menu), cx);
        }
    }

    pub fn close_menu(&mut self, cx: &mut Context<Self>) {
        self.set_active_menu(None, cx);
    }

    /// Closes the dropdown menu, then runs `f` once the closing repaint has been presented.
    ///
    /// gpui runs `Context::defer_in` and `Context::on_next_frame` callbacks *before*
    /// `Window::draw`/`present` (`gpui-0.2.2/src/window.rs:1030` vs `:1048`), so neither can
    /// express "after the next presented frame". Windows queues `WM_PAINT` at the lowest
    /// priority, so opening a window before the closed menu is presented leaves the menu
    /// frozen on screen — that is what `platform::yield_frame` buys us.
    pub(crate) fn defer_after_menu_close<F>(&mut self, cx: &mut Context<Self>, f: F)
    where
        F: FnOnce(&mut Self, &mut Context<Self>) + 'static,
    {
        let had_menu = self.active_menu.is_some();
        self.close_menu(cx);
        if !had_menu {
            f(self, cx);
            return;
        }
        cx.spawn(async move |this, cx| {
            crate::platform::yield_frame(cx).await;
            this.update(cx, f).ok();
        })
        .detach();
    }

    fn format_manual_reason(reason: &ManualUpdateReason, cx: &App) -> String {
        match reason {
            ManualUpdateReason::CompileTimeDisabled => {
                "Self-update is disabled in this build.".to_string()
            }
            ManualUpdateReason::ExternalExplanation(msg) => msg.clone(),
            ManualUpdateReason::DevBuild => cx.t(TextKey::UpdateManualDevBuild).to_string(),
            ManualUpdateReason::PackageManager(mgr) => cx.tf(
                TextKey::UpdateManualPackageManager,
                &[("manager", mgr.as_str())],
            ),
            ManualUpdateReason::TranslocatedOrReadOnly(desc) => desc.clone(),
            ManualUpdateReason::ReadOnlyLocation => cx.t(TextKey::UpdateManualReadOnly).to_string(),
        }
    }

    fn format_download_detail(
        downloaded_bytes: u64,
        total_bytes: Option<u64>,
        bytes_per_sec: u64,
    ) -> String {
        let mb = |b: u64| format!("{:.1} MB", b as f64 / (1024.0 * 1024.0));
        let speed = format!("{:.1} MB/s", bytes_per_sec as f64 / (1024.0 * 1024.0));
        match total_bytes {
            Some(total) if total > 0 => {
                let pct = ((downloaded_bytes as f64 / total as f64) * 100.0).clamp(0.0, 100.0);
                format!("{pct:.0}% ({} / {}, {speed})", mb(downloaded_bytes), mb(total))
            }
            _ => format!("{} ({speed})", mb(downloaded_bytes)),
        }
    }

    fn render_update_banner(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let mgr = cx.try_global::<UpdateManager>()?;
        if mgr.is_banner_dismissed() {
            return None;
        }

        let cancel_label = cx.t(TextKey::CommonCancel).to_string();
        match mgr.status() {
            UpdateStatus::Idle
            | UpdateStatus::Checking { manual: false }
            | UpdateStatus::UpToDate { manual: false, .. }
            | UpdateStatus::Failed { manual: false, .. } => None,
            UpdateStatus::Checking { manual: true } => Some(
                AppBanner::new("update_banner", cx.t(TextKey::UpdateChecking).to_string())
                    .tone(BannerTone::Info)
                    .icon(IconName::Settings)
                    .on_dismiss(|_, _, cx| commands::dismiss_update_banner(cx))
                    .into_any_element(),
            ),
            UpdateStatus::UpToDate { manual: true, .. } => Some(
                AppBanner::new("update_banner", cx.t(TextKey::UpdateUpToDate).to_string())
                    .tone(BannerTone::Info)
                    .icon(IconName::Check)
                    .on_dismiss(|_, _, cx| commands::dismiss_update_banner(cx))
                    .into_any_element(),
            ),
            UpdateStatus::Available(release) => {
                let title = cx.tf(
                    TextKey::UpdateAvailableTitle,
                    &[("version", release.version.as_str())],
                );
                let locale_code = cx.locale().code();
                let notes_url = release.release_notes_url.clone();
                let mut banner = AppBanner::new("update_banner", title)
                    .tone(BannerTone::Accent)
                    .icon(IconName::Settings)
                    .action(
                        BannerAction::new(
                            "download",
                            cx.t(TextKey::UpdateActionDownload).to_string(),
                            |_, _, cx| commands::download_available_update(cx),
                        )
                        .primary(),
                    )
                    .action(BannerAction::new(
                        "notes",
                        cx.t(TextKey::UpdateActionReleaseNotes).to_string(),
                        move |_, _, _| commands::open_update_release_notes(&notes_url),
                    ))
                    .action(BannerAction::new(
                        "skip",
                        cx.t(TextKey::UpdateActionSkip).to_string(),
                        |_, _, cx| commands::skip_available_update(cx),
                    ))
                    .on_dismiss(|_, _, cx| commands::dismiss_update_banner(cx));
                if let Some(note) = release.localized_notes(locale_code) {
                    let first_line = note.lines().next().unwrap_or(note).trim();
                    if !first_line.is_empty() {
                        banner = banner.detail(first_line.to_string());
                    }
                }
                Some(banner.into_any_element())
            }
            UpdateStatus::ManualUpdateOnly { release, reason } => {
                let title = cx.tf(
                    TextKey::UpdateManualTitle,
                    &[("version", release.version.as_str())],
                );
                let detail = Self::format_manual_reason(&reason, cx);
                let notes_url = release.release_notes_url.clone();
                Some(
                    AppBanner::new("update_banner", title)
                        .tone(BannerTone::Warning)
                        .icon(IconName::ExternalLink)
                        .detail(detail)
                        .action(
                            BannerAction::new(
                                "notes",
                                cx.t(TextKey::UpdateActionReleaseNotes).to_string(),
                                move |_, _, _| commands::open_update_release_notes(&notes_url),
                            )
                            .primary(),
                        )
                        .action(BannerAction::new(
                            "skip",
                            cx.t(TextKey::UpdateActionSkip).to_string(),
                            |_, _, cx| commands::skip_available_update(cx),
                        ))
                        .on_dismiss(|_, _, cx| commands::dismiss_update_banner(cx))
                        .into_any_element(),
                )
            }
            UpdateStatus::Downloading {
                release,
                downloaded_bytes,
                total_bytes,
                bytes_per_sec,
            } => {
                let title = cx.tf(
                    TextKey::UpdateDownloading,
                    &[("version", release.version.as_str())],
                );
                let detail =
                    Self::format_download_detail(downloaded_bytes, total_bytes, bytes_per_sec);
                let fraction = match total_bytes {
                    Some(total) if total > 0 => (downloaded_bytes as f32 / total as f32).clamp(0.0, 1.0),
                    _ => 0.0,
                };
                Some(
                    AppBanner::new("update_banner", title)
                        .tone(BannerTone::Accent)
                        .icon(IconName::Settings)
                        .detail(detail)
                        .progress(fraction)
                        .action(BannerAction::new("cancel", cancel_label, |_, _, cx| {
                            commands::cancel_update_download(cx)
                        }))
                        .into_any_element(),
                )
            }
            UpdateStatus::Verifying { release } => Some(
                AppBanner::new(
                    "update_banner",
                    cx.tf(
                        TextKey::UpdateVerifying,
                        &[("version", release.version.as_str())],
                    ),
                )
                .tone(BannerTone::Accent)
                .icon(IconName::Lock)
                .progress(1.0)
                .into_any_element(),
            ),
            UpdateStatus::Extracting { release } => Some(
                AppBanner::new(
                    "update_banner",
                    cx.tf(
                        TextKey::UpdateExtracting,
                        &[("version", release.version.as_str())],
                    ),
                )
                .tone(BannerTone::Accent)
                .icon(IconName::Settings)
                .progress(1.0)
                .into_any_element(),
            ),
            UpdateStatus::ReadyToInstall(staged) => Some(
                AppBanner::new(
                    "update_banner",
                    cx.tf(
                        TextKey::UpdateReadyTitle,
                        &[("version", staged.release.version.as_str())],
                    ),
                )
                .tone(BannerTone::Accent)
                .icon(IconName::Check)
                .action(
                    BannerAction::new(
                        "restart",
                        cx.t(TextKey::UpdateActionRestart).to_string(),
                        |_, _, cx| commands::install_staged_update(cx),
                    )
                    .primary(),
                )
                .on_dismiss(|_, _, cx| commands::dismiss_update_banner(cx))
                .into_any_element(),
            ),
            UpdateStatus::Applying { version } => Some(
                AppBanner::new(
                    "update_banner",
                    cx.tf(TextKey::UpdateApplying, &[("version", version.as_str())]),
                )
                .tone(BannerTone::Accent)
                .icon(IconName::Settings)
                .progress(1.0)
                .into_any_element(),
            ),
            UpdateStatus::Failed {
                message,
                manual: true,
            } => Some(
                AppBanner::new(
                    "update_banner",
                    cx.tf(TextKey::UpdateFailed, &[("error", message.as_str())]),
                )
                .tone(BannerTone::Error)
                .icon(IconName::Close)
                .action(BannerAction::new(
                    "retry",
                    cx.t(TextKey::UpdateActionRetry).to_string(),
                    |_, _, cx| commands::check_for_updates(cx),
                ))
                .on_dismiss(|_, _, cx| commands::dismiss_update_banner(cx))
                .into_any_element(),
            ),
        }
    }
}

impl Render for InviscidWindow {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.attach_window_observers(window, cx);
        // App actions need a dispatch path even before an editor or panel takes focus.
        if window.focused(cx).is_none() {
            window.focus(&self.focus_handle);
        }
        let bg_app = cx.global::<ThemeManager>().theme().bg_app;
        let config = cx.global::<AppConfig>();
        let ui_font_size = config.ui_font_size;

        // `escape` is only claimable while a menu is actually open; see `key_context::MENU_OPEN`.
        let app_context = if self.active_menu.is_some() {
            crate::ui::key_context::APP_WITH_MENU_OPEN
        } else {
            crate::ui::key_context::APP
        };

        window.set_rem_size(px(ui_font_size));

        actions::attach_app_actions(
            div()
                .track_focus(&self.focus_handle)
                .key_context(app_context)
                .size_full()
                .bg(bg_app)
                .text_size(crate::ui::FontSize::BODY)
                .line_height(crate::ui::LineHeight::BODY)
                .flex()
                .flex_col()
                .relative()
                .child(self.titlebar.clone())
                .children(self.render_update_banner(cx))
                .child(self.workspace.clone()),
            cx,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use core::prelude::v1::test;

    #[gpui::test]
    fn menu_commands_work_while_document_is_loading(cx: &mut TestAppContext) {
        cx.update(|cx| {
            let config = AppConfig::default();
            cx.set_global(ThemeManager::from_config(&config));
            actions::bind_default_bindings(cx);
            cx.set_global(config);
        });
        let (app, cx) = cx.add_window_view(|_, cx| InviscidWindow::new(cx));
        let editor = cx.update(|window, cx| {
            let workspace = app.read(cx).workspace().clone();
            workspace.update(cx, |workspace, cx| workspace.new_tab(cx));
            let editor = workspace.read(cx).active_editor().unwrap().clone();
            // Park loading independently of file I/O so the menu must work before EOF.
            editor.update(cx, |editor, _| editor.begin_loading("loading.md".into()));
            window.blur();
            editor
        });
        cx.run_until_parked();
        let position = cx.debug_bounds("app_menu_hamburger_btn").unwrap().center();
        cx.simulate_click(position, Modifiers::default());
        cx.run_until_parked();
        assert_eq!(
            cx.update(|_, cx| app.read(cx).active_menu()),
            Some(ActiveMenu::Inviscid)
        );

        let position = cx.debug_bounds("menu_trigger_View").unwrap().center();
        cx.simulate_event(MouseMoveEvent {
            position,
            pressed_button: None,
            modifiers: Modifiers::default(),
        });
        cx.run_until_parked();
        assert_eq!(
            cx.update(|_, cx| app.read(cx).active_menu()),
            Some(ActiveMenu::View)
        );

        let position = cx.debug_bounds("menu_item_Source Mode").unwrap().center();
        cx.simulate_click(position, Modifiers::default());
        cx.run_until_parked();
        cx.update(|window, cx| {
            let editor = editor.read(cx);
            assert_eq!(editor.render_mode(), crate::editor::RenderMode::Source);
            assert!(editor.is_loading());
            assert!(!editor.focus_handle(cx).is_focused(window));
            assert!(app.read(cx).active_menu().is_none());
        });
    }

    #[gpui::test]
    fn fps_counter_toggles_via_view_menu_and_shortcut(cx: &mut TestAppContext) {
        cx.update(|cx| {
            let config = AppConfig::default();
            cx.set_global(ThemeManager::from_config(&config));
            actions::bind_default_bindings(cx);
            cx.set_global(config);
        });
        let (app, cx) = cx.add_window_view(|_, cx| InviscidWindow::new(cx));
        cx.run_until_parked();

        assert!(
            !cx.read(|cx| cx.global::<AppConfig>().show_fps)
                && cx.debug_bounds("status_bar_fps").is_none(),
            "FPS counter must default to disabled"
        );

        cx.update(|_, cx| {
            app.update(cx, |app, cx| {
                app.set_active_menu(Some(ActiveMenu::View), cx);
            });
        });
        cx.run_until_parked();

        let position = cx.debug_bounds("menu_item_FPS Counter").unwrap().center();
        cx.simulate_click(position, Modifiers::default());
        cx.run_until_parked();
        assert!(
            cx.read(|cx| cx.global::<AppConfig>().show_fps)
                && cx.debug_bounds("status_bar_fps").is_some()
        );

        cx.simulate_keystrokes("f8");
        cx.run_until_parked();
        assert!(
            !cx.read(|cx| cx.global::<AppConfig>().show_fps)
                && cx.debug_bounds("status_bar_fps").is_none()
        );
    }

    #[gpui::test]
    fn update_banner_dismisses_and_restores_from_status_bar_badge(cx: &mut TestAppContext) {
        use crate::updater::{ArchiveFormat, ReleaseInfo, TargetAsset, UpdateChannel};
        use std::collections::HashMap;

        let mgr = UpdateManager::new(None);
        cx.update(|cx| {
            let config = AppConfig::default();
            cx.set_global(ThemeManager::from_config(&config));
            actions::bind_default_bindings(cx);
            cx.set_global(config);
            cx.set_global(mgr.clone());
            commands::start_update_event_pump(cx);
        });

        let (_app, cx) = cx.add_window_view(|_, cx| InviscidWindow::new(cx));
        cx.run_until_parked();

        assert!(cx.debug_bounds("banner_action_download").is_none());
        assert!(cx.debug_bounds("status_bar_update_badge").is_none());

        mgr.set_test_status(UpdateStatus::Available(ReleaseInfo {
            version: "0.2.0".to_string(),
            tag: "v0.2.0".to_string(),
            channel: UpdateChannel::Stable,
            pub_date: "2026-12-01T00:00:00Z".to_string(),
            release_notes_url: "https://github.com/KiraKiraAyu/Inviscid/releases/tag/v0.2.0"
                .to_string(),
            notes: HashMap::from([("en".to_string(), "New features".to_string())]),
            target: TargetAsset {
                asset_name: "inviscid-v0.2.0.zip".to_string(),
                size: 1024,
                sha256: "a".repeat(64),
                format: ArchiveFormat::Zip,
                entry: "inviscid.exe".to_string(),
            },
            download_url: "https://example.com/inviscid-v0.2.0.zip".to_string(),
        }));
        cx.run_until_parked();

        assert!(cx.debug_bounds("banner_action_download").is_some());
        assert!(cx.debug_bounds("status_bar_update_badge").is_some());

        let skip_pos = cx.debug_bounds("banner_action_skip").unwrap().center();
        let dismiss_pos = cx.debug_bounds("banner_dismiss").unwrap().center();
        let badge_pos = cx.debug_bounds("status_bar_update_badge").unwrap().center();

        cx.simulate_click(dismiss_pos, Modifiers::default());
        cx.run_until_parked();

        assert!(
            cx.read(|cx| cx.global::<UpdateManager>().is_banner_dismissed()),
            "clicking banner_dismiss at {dismiss_pos:?} must mark banner dismissed"
        );
        // GPUI 0.2.2 `Frame::clear` omits `debug_bounds.clear()`, leaving stale entries in the
        // double-buffered frame map after unmount; verify unmount directly via click hit-testing.
        cx.simulate_click(skip_pos, Modifiers::default());
        cx.run_until_parked();
        assert!(matches!(mgr.status(), UpdateStatus::Available(_)));
        assert!(cx.debug_bounds("status_bar_update_badge").is_some());

        cx.simulate_click(badge_pos, Modifiers::default());
        cx.run_until_parked();

        assert!(!cx.read(|cx| cx.global::<UpdateManager>().is_banner_dismissed()));
        cx.simulate_click(skip_pos, Modifiers::default());
        cx.run_until_parked();
        assert!(matches!(mgr.status(), UpdateStatus::Idle));
        assert_eq!(
            cx.read(|cx| cx.global::<AppConfig>().session.skipped_update_version.clone())
                .as_deref(),
            Some("0.2.0")
        );
    }
}
