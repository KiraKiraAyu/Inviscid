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
pub(crate) mod update_modal;
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
use crate::theme::ThemeManager;
use crate::updater::UpdateManager;

/// Root view entity for an Inviscid editor window.
pub struct InviscidWindow {
    focus_handle: FocusHandle,
    titlebar: Entity<TitleBar>,
    workspace: Entity<Workspace>,
    active_menu: Option<ActiveMenu>,
    update_changelog_open: bool,
    cached_window_bounds: Option<WindowLayoutState>,
    window_persistence_attached: bool,
    close_prompt_active: bool,
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
                crate::app::title_bar::TitleBarEvent::OpenUpdateChangelog => {
                    this.close_menu(cx);
                    if !this.update_changelog_open {
                        this.update_changelog_open = true;
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
            update_changelog_open: false,
            cached_window_bounds: None,
            window_persistence_attached: false,
            close_prompt_active: false,
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
            weak.update(cx, |this, cx| this.request_close(window, cx))
                .unwrap_or(true)
        });
    }

    pub(crate) fn set_close_prompt_active(&mut self, active: bool) {
        self.close_prompt_active = active;
    }

    /// Checks this window's workspace for unsaved documents before closing.
    ///
    /// Returns `true` when clean (and persists window state), or `false` after scheduling the
    /// unsaved-changes prompt for this window.
    pub(crate) fn request_close(&mut self, window: &mut Window, cx: &mut Context<Self>) -> bool {
        if !lifecycle::has_unsaved_documents(&self.workspace, cx) {
            self.persist_window_state_sync(window, cx);
            return true;
        }
        if !self.close_prompt_active
            && let Some(window_handle) = window.window_handle().downcast::<InviscidWindow>()
        {
            self.close_prompt_active = true;
            lifecycle::request_window_close(window_handle, cx);
        }
        false
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

    pub fn is_update_changelog_open(&self) -> bool {
        self.update_changelog_open
    }

    pub fn close_update_changelog(&mut self, cx: &mut Context<Self>) {
        if self.update_changelog_open {
            self.update_changelog_open = false;
            cx.notify();
        }
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

        let view_entity = cx.entity().downgrade();
        let changelog_modal = if self.update_changelog_open {
            let status = cx
                .try_global::<UpdateManager>()
                .map(UpdateManager::status)
                .unwrap_or(crate::updater::UpdateStatus::Idle);
            let modal = update_modal::render_update_changelog_modal(
                &status,
                move |_, cx| {
                    let _ = view_entity.update(cx, |this, cx| {
                        this.close_update_changelog(cx);
                    });
                },
                cx,
            );
            if modal.is_none() {
                self.update_changelog_open = false;
            }
            modal
        } else {
            None
        };

        // `escape` is only claimable while a menu or modal overlay is open; see `key_context::MENU_OPEN`.
        let app_context = if self.active_menu.is_some() || self.update_changelog_open {
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
                .child(self.workspace.clone())
                .children(changelog_modal),
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
    fn titlebar_update_container_avoids_layout_jitter_and_changelog_modal_works(
        cx: &mut TestAppContext,
    ) {
        use crate::updater::{ArchiveFormat, ReleaseInfo, StagedUpdate, TargetAsset, UpdateChannel, UpdateStatus};
        use std::collections::HashMap;
        use std::path::PathBuf;

        let mgr = UpdateManager::new(None);
        cx.update(|cx| {
            let config = AppConfig::default();
            cx.set_global(ThemeManager::from_config(&config));
            actions::bind_default_bindings(cx);
            cx.set_global(config);
            cx.set_global(mgr.clone());
            commands::start_update_event_pump(cx);
        });

        let (app, cx) = cx.add_window_view(|_, cx| InviscidWindow::new(cx));
        cx.run_until_parked();

        assert!(cx.debug_bounds("titlebar_update_container").is_none());

        let release = ReleaseInfo {
            version: "0.2.0".to_string(),
            tag: "v0.2.0".to_string(),
            channel: UpdateChannel::Stable,
            pub_date: "2026-12-01T00:00:00Z".to_string(),
            release_notes_url: "https://github.com/KiraKiraAyu/Inviscid/releases/tag/v0.2.0"
                .to_string(),
            notes: HashMap::from([(
                "en".to_string(),
                "- Added changelog modal\n- Circular progress indicator".to_string(),
            )]),
            target: TargetAsset {
                asset_name: "inviscid-v0.2.0.zip".to_string(),
                size: 1024,
                sha256: "a".repeat(64),
                format: ArchiveFormat::Zip,
                entry: "inviscid.exe".to_string(),
            },
            download_url: "https://example.com/inviscid-v0.2.0.zip".to_string(),
        };

        // 1. Available: container and both icon buttons visible.
        mgr.set_test_status(UpdateStatus::Available(release.clone()));
        cx.run_until_parked();

        let container_bounds_avail = cx
            .debug_bounds("titlebar_update_container")
            .expect("titlebar update container bounds");
        let changelog_bounds_avail = cx
            .debug_bounds("titlebar_update_changelog_btn")
            .expect("titlebar changelog btn bounds");
        assert!(cx.debug_bounds("titlebar_update_download_btn").is_some());

        // 2. Downloading (50%): icon swaps to read-only CircularProgress with ZERO layout jitter.
        mgr.set_test_status(UpdateStatus::Downloading {
            release: release.clone(),
            downloaded_bytes: 512,
            total_bytes: Some(1024),
            bytes_per_sec: 256,
        });
        cx.run_until_parked();

        let progress_bounds = cx
            .debug_bounds("titlebar_update_progress")
            .expect("titlebar progress bounds");
        let container_bounds_dl = cx.debug_bounds("titlebar_update_container").unwrap();
        let changelog_bounds_dl = cx.debug_bounds("titlebar_update_changelog_btn").unwrap();
        assert_eq!(container_bounds_avail, container_bounds_dl);
        assert_eq!(changelog_bounds_avail, changelog_bounds_dl);

        // Progress indicator must be read-only (clicking it does not alter download status).
        cx.simulate_click(progress_bounds.center(), Modifiers::default());
        cx.run_until_parked();
        assert!(matches!(mgr.status(), UpdateStatus::Downloading { .. }));

        // 3. ReadyToInstall: icon swaps to restart button, still ZERO layout jitter.
        mgr.set_test_status(UpdateStatus::ReadyToInstall(StagedUpdate {
            release: release.clone(),
            staging_dir: PathBuf::from("staging"),
            extracted_entry: PathBuf::from("staging/inviscid.exe"),
        }));
        cx.run_until_parked();

        assert!(cx.debug_bounds("titlebar_update_restart_btn").is_some());
        assert_eq!(
            container_bounds_avail,
            cx.debug_bounds("titlebar_update_container").unwrap()
        );
        assert_eq!(
            changelog_bounds_avail,
            cx.debug_bounds("titlebar_update_changelog_btn").unwrap()
        );

        // 4. Changelog modal: open, close via X, re-open, and skip version.
        mgr.set_test_status(UpdateStatus::Available(release));
        cx.run_until_parked();

        cx.simulate_click(changelog_bounds_avail.center(), Modifiers::default());
        cx.run_until_parked();
        assert!(cx.read(|cx| app.read(cx).is_update_changelog_open()));
        assert!(cx.debug_bounds("update_changelog_modal").is_some());
        assert!(cx.debug_bounds("changelog_action_skip").is_some());
        assert!(cx.debug_bounds("changelog_action_download").is_some());

        let close_pos = cx
            .debug_bounds("update_changelog_close")
            .expect("changelog close btn")
            .center();
        cx.simulate_click(close_pos, Modifiers::default());
        cx.run_until_parked();
        assert!(!cx.read(|cx| app.read(cx).is_update_changelog_open()));

        cx.simulate_click(changelog_bounds_avail.center(), Modifiers::default());
        cx.run_until_parked();
        assert!(cx.read(|cx| app.read(cx).is_update_changelog_open()));

        let skip_pos = cx
            .debug_bounds("changelog_action_skip")
            .expect("changelog skip btn")
            .center();
        cx.simulate_click(skip_pos, Modifiers::default());
        cx.run_until_parked();
        assert!(!cx.read(|cx| app.read(cx).is_update_changelog_open()));
        assert!(matches!(mgr.status(), UpdateStatus::Idle));
        assert_eq!(
            cx.read(|cx| cx.global::<AppConfig>().session.skipped_update_version.clone())
                .as_deref(),
            Some("0.2.0")
        );
    }
}
