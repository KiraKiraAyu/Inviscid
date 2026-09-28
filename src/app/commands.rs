use gpui::*;
use std::path::PathBuf;

use crate::app::{ActiveMenu, InviscidWindow};
use crate::config::AppConfig;
use crate::editor::RenderMode;
use crate::theme::ThemeManager;

pub fn sync_workspace_to_global(app: &InviscidWindow, cx: &mut Context<InviscidWindow>) {
    if cx.has_global::<AppConfig>() {
        let mut config = cx.global::<AppConfig>().clone();
        app.workspace()
            .read(cx)
            .sync_workspace_state(&mut config, cx);
        let config_to_save = config.clone();
        cx.background_executor()
            .spawn(async move {
                config_to_save.save_session();
            })
            .detach();
        cx.set_global(config);
    }
}

pub fn new_tab(app: &mut InviscidWindow, window: &mut Window, cx: &mut Context<InviscidWindow>) {
    app.workspace().update(cx, |ws, cx| {
        ws.new_tab(cx);
        ws.focus_active_editor(window, cx);
    });
    app.close_menu(cx);
    sync_workspace_to_global(app, cx);
    cx.notify();
}

pub fn close_active_tab(
    app: &mut InviscidWindow,
    window: &mut Window,
    cx: &mut Context<InviscidWindow>,
) {
    app.close_menu(cx);
    app.workspace().update(cx, |ws, cx| {
        ws.close_active_tab(cx);
        ws.focus_active_editor(window, cx);
    });
    sync_workspace_to_global(app, cx);
    cx.notify();
}

pub fn close_workspace(
    app: &mut InviscidWindow,
    window: &mut Window,
    cx: &mut Context<InviscidWindow>,
) {
    app.close_menu(cx);
    app.workspace().update(cx, |ws, cx| {
        ws.close_workspace(cx);
        ws.focus_active_editor(window, cx);
    });
    sync_workspace_to_global(app, cx);
    cx.notify();
}

/// Clears the recent-workspaces list.
///
/// Unlike the commands above this does not call `sync_workspace_to_global`, which would
/// immediately re-add the currently open workspace. `AppConfig::clear_recent_workspaces`
/// persists `state.toml` itself.
pub fn clear_recent_workspaces(cx: &mut App) {
    let mut config_to_save = None;
    cx.update_global::<AppConfig, _>(|config, _cx| {
        config.clear_recent_workspaces();
        config_to_save = Some(config.clone());
    });
    if let Some(cfg) = config_to_save {
        cx.background_executor()
            .spawn(async move {
                cfg.save_session();
            })
            .detach();
    }
}

pub fn next_tab(app: &mut InviscidWindow, window: &mut Window, cx: &mut Context<InviscidWindow>) {
    app.close_menu(cx);
    app.workspace().update(cx, |ws, cx| {
        ws.next_tab(cx);
        ws.focus_active_editor(window, cx);
    });
    sync_workspace_to_global(app, cx);
    cx.notify();
}

pub fn prev_tab(app: &mut InviscidWindow, window: &mut Window, cx: &mut Context<InviscidWindow>) {
    app.close_menu(cx);
    app.workspace().update(cx, |ws, cx| {
        ws.prev_tab(cx);
        ws.focus_active_editor(window, cx);
    });
    sync_workspace_to_global(app, cx);
    cx.notify();
}

pub fn open_file(app: &mut InviscidWindow, path: PathBuf, cx: &mut Context<InviscidWindow>) {
    app.workspace().update(cx, |ws, cx| ws.open_file(path, cx));
    sync_workspace_to_global(app, cx);
    cx.notify();
}

pub fn open_file_prompt(app: &mut InviscidWindow, cx: &mut Context<InviscidWindow>) {
    app.close_menu(cx);
    cx.spawn(async move |this, cx| {
        let file = rfd::AsyncFileDialog::new()
            .add_filter("Markdown Files", &["md", "markdown", "txt"])
            .add_filter("All Files", &["*"])
            .pick_file()
            .await;

        if let Some(file) = file {
            let path = file.path().to_path_buf();
            this.update(cx, |app, cx| {
                open_file(app, path, cx);
            })
            .ok();
        }
    })
    .detach();
}

pub fn open_folder(app: &mut InviscidWindow, path: PathBuf, cx: &mut Context<InviscidWindow>) {
    app.workspace()
        .update(cx, |ws, cx| ws.open_folder(path, cx));
    sync_workspace_to_global(app, cx);
    cx.notify();
}

pub fn open_folder_prompt(app: &mut InviscidWindow, cx: &mut Context<InviscidWindow>) {
    app.close_menu(cx);
    cx.spawn(async move |this, cx| {
        let folder = rfd::AsyncFileDialog::new().pick_folder().await;

        if let Some(folder) = folder {
            let path = folder.path().to_path_buf();
            this.update(cx, |app, cx| {
                open_folder(app, path, cx);
            })
            .ok();
        }
    })
    .detach();
}

pub fn save_file(app: &mut InviscidWindow, cx: &mut Context<InviscidWindow>) {
    app.close_menu(cx);
    let active_editor = match app.workspace().read(cx).active_editor() {
        Some(editor) => editor.clone(),
        None => return,
    };
    let needs_save_as = active_editor.read(cx).file_path().is_none();

    if needs_save_as {
        save_file_as_prompt(app, cx);
    } else {
        cx.spawn(async move |this, cx| {
            let task_res = active_editor.update(cx, |editor, cx| editor.save_file_async(cx));
            match task_res {
                Ok(Ok(task)) => {
                    if let Err(e) = task.await {
                        eprintln!("Failed to save file: {}", e);
                    } else {
                        let _ = this.update(cx, |app, cx| {
                            sync_workspace_to_global(app, cx);
                            cx.notify();
                        });
                    }
                }
                Ok(Err(e)) => eprintln!("Failed to save file: {}", e),
                Err(e) => eprintln!("Editor entity unavailable: {}", e),
            }
        })
        .detach();
    }
}

pub fn save_file_as_prompt(app: &mut InviscidWindow, cx: &mut Context<InviscidWindow>) {
    app.close_menu(cx);
    cx.spawn(async move |this, cx| {
        let file = rfd::AsyncFileDialog::new()
            .add_filter("Markdown Files", &["md"])
            .set_file_name("document.md")
            .save_file()
            .await;

        if let Some(file) = file {
            let path = file.path().to_path_buf();
            let active_editor = this.read_with(cx, |app, cx| {
                app.workspace().read(cx).active_editor().cloned()
            });
            if let Ok(Some(editor)) = active_editor {
                let task =
                    editor.update(cx, |editor, cx| editor.save_file_as_async(path.clone(), cx));
                if let Ok(task) = task {
                    if let Err(e) = task.await {
                        eprintln!("Failed to save file as {:?}: {}", path, e);
                    } else {
                        let _ = this.update(cx, |app, cx| {
                            sync_workspace_to_global(app, cx);
                            cx.notify();
                        });
                    }
                }
            }
        }
    })
    .detach();
}

pub fn new_window(cx: &mut App) {
    let bounds = if let Some(active_window) = cx.active_window() {
        if let Ok(active_bounds) = active_window.update(cx, |_, window, _| window.bounds()) {
            let mut b = active_bounds;
            b.origin.x += px(24.0);
            b.origin.y += px(24.0);
            b
        } else {
            Bounds::centered(None, size(px(1080.0), px(780.0)), cx)
        }
    } else {
        Bounds::centered(None, size(px(1080.0), px(780.0)), cx)
    };

    cx.open_window(
        WindowOptions {
            window_bounds: Some(WindowBounds::Windowed(bounds)),
            titlebar: Some(TitlebarOptions {
                title: Some("Inviscid".into()),
                appears_transparent: true,
                traffic_light_position: None,
            }),
            window_decorations: Some(WindowDecorations::Client),
            window_min_size: Some(size(px(360.0), px(240.0))),
            is_movable: true,
            focus: false,
            show: false,
            ..Default::default()
        },
        |window, cx| {
            let app = cx.new(InviscidWindow::new);
            app.read(cx)
                .workspace()
                .read(cx)
                .focus_active_editor(window, cx);
            window.activate_window();
            app
        },
    )
    .ok();
}

#[derive(Default)]
pub struct SettingsWindowState {
    pub handle: Option<WindowHandle<crate::app::settings::SettingsWindow>>,
}

impl gpui::Global for SettingsWindowState {}

pub fn open_settings_window(cx: &mut App) {
    let existing_handle = cx.default_global::<SettingsWindowState>().handle;
    if let Some(handle) = existing_handle {
        let activated = handle
            .update(cx, |view, window, _cx| {
                window.activate_window();
                view.focus(window);
            })
            .is_ok();
        if activated {
            return;
        } else {
            cx.default_global::<SettingsWindowState>().handle = None;
        }
    }

    let bounds = if let Some(active_window) = cx.active_window() {
        if let Ok(active_bounds) = active_window.update(cx, |_, window, _| window.bounds()) {
            let x = active_bounds.origin.x + (active_bounds.size.width - px(800.0)) / 2.0;
            let y = active_bounds.origin.y + (active_bounds.size.height - px(560.0)) / 2.0;
            Bounds::new(
                point(x.max(px(0.0)), y.max(px(0.0))),
                size(px(800.0), px(560.0)),
            )
        } else {
            Bounds::centered(None, size(px(800.0), px(560.0)), cx)
        }
    } else {
        Bounds::centered(None, size(px(800.0), px(560.0)), cx)
    };

    if let Ok(handle) = cx.open_window(
        WindowOptions {
            window_bounds: Some(WindowBounds::Windowed(bounds)),
            titlebar: Some(TitlebarOptions {
                title: Some("Settings".into()),
                appears_transparent: true,
                traffic_light_position: None,
            }),
            window_decorations: Some(WindowDecorations::Client),
            window_min_size: Some(size(px(600.0), px(400.0))),
            is_movable: true,
            focus: false,
            show: false,
            ..Default::default()
        },
        |window, cx| {
            let view = cx.new(crate::app::settings::SettingsWindow::new);
            view.read(cx).focus(window);
            window.activate_window();
            view
        },
    ) {
        cx.default_global::<SettingsWindowState>().handle = Some(handle);
    }
}

#[derive(Default)]
pub struct AboutWindowState {
    pub handle: Option<WindowHandle<crate::app::about::AboutWindow>>,
}

impl gpui::Global for AboutWindowState {}

pub fn open_about_window(cx: &mut App) {
    let existing_handle = cx.default_global::<AboutWindowState>().handle;
    if let Some(handle) = existing_handle {
        let activated = handle
            .update(cx, |view, window, _cx| {
                window.activate_window();
                view.focus(window);
            })
            .is_ok();
        if activated {
            return;
        } else {
            cx.default_global::<AboutWindowState>().handle = None;
        }
    }

    let width = px(360.0);
    let height = px(320.0);

    let bounds = if let Some(active_window) = cx.active_window() {
        if let Ok(active_bounds) = active_window.update(cx, |_, window, _| window.bounds()) {
            let x = active_bounds.origin.x + (active_bounds.size.width - width) / 2.0;
            let y = active_bounds.origin.y + (active_bounds.size.height - height) / 2.0;
            Bounds::new(point(x.max(px(0.0)), y.max(px(0.0))), size(width, height))
        } else {
            Bounds::centered(None, size(width, height), cx)
        }
    } else {
        Bounds::centered(None, size(width, height), cx)
    };

    if let Ok(handle) = cx.open_window(
        WindowOptions {
            window_bounds: Some(WindowBounds::Windowed(bounds)),
            titlebar: None,
            window_decorations: Some(WindowDecorations::Client),
            window_min_size: Some(size(px(340.0), px(300.0))),
            is_movable: true,
            focus: false,
            show: false,
            ..Default::default()
        },
        |window, cx| {
            let view = cx.new(crate::app::about::AboutWindow::new);
            view.read(cx).focus(window);
            window.activate_window();
            view
        },
    ) {
        cx.default_global::<AboutWindowState>().handle = Some(handle);
    }
}

pub fn open_menu(app: &mut InviscidWindow, menu: ActiveMenu, cx: &mut Context<InviscidWindow>) {
    app.set_active_menu(Some(menu), cx);
}

pub fn toggle_menu(app: &mut InviscidWindow, menu: ActiveMenu, cx: &mut Context<InviscidWindow>) {
    app.toggle_menu(menu, cx);
}

pub fn close_menu(app: &mut InviscidWindow, cx: &mut Context<InviscidWindow>) {
    app.close_menu(cx);
}

pub fn set_render_mode(
    app: &mut InviscidWindow,
    mode: RenderMode,
    cx: &mut Context<InviscidWindow>,
) {
    app.close_menu(cx);
    if let Some(editor) = app.workspace().read(cx).active_editor().cloned() {
        editor.update(cx, |editor, cx| {
            editor.set_render_mode(mode, cx);
        });
        cx.notify();
    }
}

pub fn toggle_render_mode(app: &mut InviscidWindow, cx: &mut Context<InviscidWindow>) {
    let mode = app
        .workspace()
        .read(cx)
        .active_editor()
        .map(|e| e.read(cx).render_mode);
    if let Some(mode) = mode {
        let next_mode = if mode == RenderMode::LivePreview {
            RenderMode::Source
        } else {
            RenderMode::LivePreview
        };
        set_render_mode(app, next_mode, cx);
    }
}

pub fn toggle_sidebar(app: &mut InviscidWindow, cx: &mut Context<InviscidWindow>) {
    app.close_menu(cx);
    app.workspace().update(cx, |ws, cx| {
        ws.toggle_sidebar(cx);
    });
    sync_workspace_to_global(app, cx);
    cx.notify();
}

pub fn switch_theme_global(name: &str, cx: &mut App) {
    if cx.has_global::<ThemeManager>() && cx.has_global::<AppConfig>() {
        cx.update_global::<ThemeManager, _>(|manager, _cx| {
            manager.switch_to(name);
        });
        cx.update_global::<AppConfig, _>(|config, _cx| {
            config.theme = name.to_string();
        });
        let config_to_save = cx.global::<AppConfig>().clone();
        cx.background_executor()
            .spawn(async move {
                config_to_save.save_preferences();
            })
            .detach();
        cx.refresh_windows();
    }
}

pub fn preview_theme_global(name: &str, cx: &mut App) {
    if cx.has_global::<ThemeManager>() {
        let changed = cx.update_global::<ThemeManager, _>(|manager, _cx| manager.preview(name));
        if changed {
            cx.refresh_windows();
        }
    }
}

pub fn cancel_theme_preview_global(cx: &mut App) {
    if cx.has_global::<ThemeManager>() && cx.has_global::<AppConfig>() {
        let saved_theme = cx.global::<AppConfig>().theme.clone();
        let changed = cx
            .update_global::<ThemeManager, _>(|manager, _cx| manager.cancel_preview(&saved_theme));
        if changed {
            cx.refresh_windows();
        }
    }
}

pub fn quit(cx: &mut App) {
    cx.quit();
}
