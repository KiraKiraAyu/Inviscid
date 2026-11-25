use gpui::*;
use std::path::PathBuf;

pub mod about;
pub mod actions;
pub mod commands;
pub(crate) mod image_cache;
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
use crate::theme::ThemeManager;

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
}
