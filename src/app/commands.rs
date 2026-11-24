use gpui::*;
use std::path::{Path, PathBuf};

use crate::app::actions::OpenWorkspaceMatch;
use crate::app::{ActiveMenu, InviscidWindow, SidebarView};
use crate::buffer::{LineSource, SourceSnapshot};
use crate::config::{AppConfig, MIN_WINDOW_HEIGHT, MIN_WINDOW_WIDTH, WindowLayoutState};
use crate::editor::{Editor, RenderMode};
use crate::export::{ExportOptions, export_html_to_path, export_pdf_to_path};
use crate::theme::{Theme, ThemeManager};

pub fn main_window_options(
    display_id: Option<DisplayId>,
    window_bounds: WindowBounds,
) -> WindowOptions {
    WindowOptions {
        window_bounds: Some(window_bounds),
        display_id,
        titlebar: Some(TitlebarOptions {
            title: Some("Inviscid".into()),
            appears_transparent: true,
            traffic_light_position: None,
        }),
        window_decorations: Some(WindowDecorations::Client),
        window_min_size: Some(size(px(MIN_WINDOW_WIDTH), px(MIN_WINDOW_HEIGHT))),
        is_movable: true,
        focus: false,
        show: false,
        ..Default::default()
    }
}

pub fn sync_workspace_state_to_config(
    workspace: &Entity<crate::app::workspace::Workspace>,
    window_bounds: Option<WindowLayoutState>,
    cx: &mut App,
) -> bool {
    if !cx.has_global::<AppConfig>() || !workspace.read(cx).persists_session() {
        return false;
    }
    let mut config = cx.global::<AppConfig>().clone();
    workspace
        .read(cx)
        .sync_workspace_state_with_window(&mut config, window_bounds, cx);
    cx.set_global(config);
    true
}

pub fn sync_workspace_in_memory(app: &InviscidWindow, cx: &mut App) -> bool {
    sync_workspace_state_to_config(app.workspace(), app.cached_window_bounds().cloned(), cx)
}

pub fn sync_workspace_to_global(app: &InviscidWindow, cx: &mut Context<InviscidWindow>) {
    if sync_workspace_in_memory(app, cx) {
        cx.global::<AppConfig>()
            .persist_session_async(cx.background_executor());
    }
}

pub fn sync_workspace_to_global_sync(app: &InviscidWindow, cx: &mut Context<InviscidWindow>) {
    if sync_workspace_in_memory(app, cx) {
        cx.global::<AppConfig>().save_session();
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
        cfg.persist_session_async(cx.background_executor());
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
    app.defer_after_menu_close(cx, |_, cx| {
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
    });
}

pub fn open_folder(app: &mut InviscidWindow, path: PathBuf, cx: &mut Context<InviscidWindow>) {
    app.workspace()
        .update(cx, |ws, cx| ws.open_folder(path, cx));
    sync_workspace_to_global(app, cx);
    cx.notify();
}

pub fn open_folder_prompt(app: &mut InviscidWindow, cx: &mut Context<InviscidWindow>) {
    app.defer_after_menu_close(cx, |_, cx| {
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
    });
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
    if app.workspace().read(cx).active_editor().is_none() {
        app.close_menu(cx);
        return;
    }

    app.defer_after_menu_close(cx, |_, cx| {
        cx.spawn(async move |this, cx| {
            let file = rfd::AsyncFileDialog::new()
                .add_filter("Markdown Files", &["md"])
                .set_file_name("document.md")
                .save_file()
                .await;

            if let Some(file) = file {
                let raw_path = file.path().to_path_buf();
                let state = this.read_with(cx, |app, cx| {
                    let editor = app.workspace().read(cx).active_editor().cloned();
                    let collocated_mode = cx
                        .try_global::<AppConfig>()
                        .is_some_and(|cfg| cfg.collocated_mode);
                    (editor, collocated_mode)
                });
                if let Ok((Some(editor), collocated_mode)) = state {
                    let path = if collocated_mode && !crate::fs::is_collocated_index_path(&raw_path)
                    {
                        let bundle_dir = match (raw_path.parent(), raw_path.file_stem()) {
                            (Some(parent), Some(stem)) if !stem.is_empty() => parent.join(stem),
                            _ => raw_path.with_extension(""),
                        };
                        if let Err(e) = std::fs::create_dir_all(&bundle_dir) {
                            eprintln!(
                                "Failed to create bundle directory {}: {}",
                                bundle_dir.display(),
                                e
                            );
                            return;
                        }
                        bundle_dir.join(crate::fs::COLLOCATED_INDEX_FILE)
                    } else {
                        raw_path
                    };
                    let task =
                        editor.update(cx, |editor, cx| editor.save_file_as_async(path.clone(), cx));
                    if let Ok(task) = task {
                        if let Err(e) = task.await {
                            eprintln!("Failed to save file as {:?}: {}", path, e);
                        } else {
                            let _ = this.update(cx, |app, cx| {
                                app.workspace().update(cx, |ws, cx| ws.refresh_panels(cx));
                                sync_workspace_to_global(app, cx);
                                cx.notify();
                            });
                        }
                    }
                }
            }
        })
        .detach();
    });
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ExportTargetKind {
    Html,
    Pdf,
}

impl ExportTargetKind {
    fn extension(self) -> &'static str {
        match self {
            Self::Html => "html",
            Self::Pdf => "pdf",
        }
    }

    fn file_filter(self) -> (&'static str, &'static [&'static str]) {
        match self {
            Self::Html => ("HTML Files", &["html", "htm"]),
            Self::Pdf => ("PDF Files", &["pdf"]),
        }
    }
}

/// Snapshot and metadata prepared on the UI thread for background document export.
#[derive(Clone)]
pub struct PreparedExport {
    pub kind: ExportTargetKind,
    pub snapshot: SourceSnapshot,
    pub doc_path: Option<PathBuf>,
    pub title: String,
    pub default_file_name: String,
    pub initial_dir: Option<PathBuf>,
    pub theme: Theme,
}

/// Returns true when `editor` holds a ready, editable document that can be exported.
fn can_export_editor(editor: &Editor) -> bool {
    editor.can_edit()
}

/// Returns true when `app`'s active tab is an exportable document editor.
pub fn can_export_active_document(app: &InviscidWindow, cx: &App) -> bool {
    app.workspace()
        .read(cx)
        .active_editor()
        .is_some_and(|editor| can_export_editor(editor.read(cx)))
}

/// Captures export metadata and a buffer snapshot for the active editor, if exportable.
pub fn prepare_active_export(
    app: &InviscidWindow,
    kind: ExportTargetKind,
    cx: &App,
) -> Option<PreparedExport> {
    let ws = app.workspace().read(cx);
    let editor = ws.active_editor()?;
    let ed = editor.read(cx);
    if !can_export_editor(ed) {
        return None;
    }

    let doc_path = ed.file_path_buf();
    let collocated_mode = cx
        .try_global::<AppConfig>()
        .is_some_and(|cfg| cfg.collocated_mode);
    let stem = doc_path
        .as_deref()
        .and_then(|p| {
            if collocated_mode
                && let Some(bundle_dir) = crate::fs::collocated_bundle_dir_within(p, ws.root_dir())
            {
                bundle_dir.file_name()
            } else {
                p.file_stem()
            }
        })
        .and_then(|s| s.to_str())
        .filter(|s| !s.is_empty())
        .map(str::to_string)
        .unwrap_or_else(|| "Untitled".to_string());
    let title = stem.clone();
    let default_file_name = format!("{stem}.{}", kind.extension());
    let initial_dir = doc_path
        .as_deref()
        .and_then(|p| crate::fs::resolve_document_parent_dir(p, collocated_mode, ws.root_dir()))
        .map(Path::to_path_buf)
        .or_else(|| ws.root_dir().map(Path::to_path_buf));

    Some(PreparedExport {
        kind,
        snapshot: ed.buffer().source_snapshot(),
        doc_path,
        title,
        default_file_name,
        initial_dir,
        theme: cx.global::<ThemeManager>().theme().clone(),
    })
}

pub fn export_html_prompt(app: &mut InviscidWindow, cx: &mut Context<InviscidWindow>) {
    export_active_document_prompt(app, ExportTargetKind::Html, cx);
}

pub fn export_pdf_prompt(app: &mut InviscidWindow, cx: &mut Context<InviscidWindow>) {
    export_active_document_prompt(app, ExportTargetKind::Pdf, cx);
}

fn export_active_document_prompt(
    app: &mut InviscidWindow,
    kind: ExportTargetKind,
    cx: &mut Context<InviscidWindow>,
) {
    if !can_export_active_document(app, cx) {
        app.close_menu(cx);
        return;
    }

    app.defer_after_menu_close(cx, move |app, cx| {
        let Some(prepared) = prepare_active_export(app, kind, cx) else {
            return;
        };

        cx.spawn(async move |_this, cx| {
            let resolved_dir = match prepared.initial_dir {
                Some(dir) => {
                    cx.background_executor()
                        .spawn(async move { dir.is_dir().then_some(dir) })
                        .await
                }
                None => None,
            };

            let mut dialog = rfd::AsyncFileDialog::new();
            if let Some(dir) = resolved_dir.as_deref() {
                dialog = dialog.set_directory(dir);
            }
            let (filter_label, extensions) = prepared.kind.file_filter();
            dialog = dialog
                .add_filter(filter_label, extensions)
                .set_file_name(&prepared.default_file_name);

            let Some(file) = dialog.save_file().await else {
                return;
            };
            let target_path = file.path().to_path_buf();
            let result = cx
                .background_executor()
                .spawn(async move {
                    let options = ExportOptions {
                        title: &prepared.title,
                        doc_path: prepared.doc_path.as_deref(),
                        theme: &prepared.theme,
                    };
                    match prepared.kind {
                        ExportTargetKind::Html => {
                            export_html_to_path(&prepared.snapshot, &options, &target_path)
                        }
                        ExportTargetKind::Pdf => {
                            export_pdf_to_path(&prepared.snapshot, &options, &target_path)
                        }
                    }
                })
                .await;

            if let Err(err) = result {
                // TODO: Surface export failures in the UI once a unified toast/notification system is implemented.
                eprintln!("Export failed: {err:#}");
            }
        })
        .detach();
    });
}

pub fn reload_file(app: &mut InviscidWindow, cx: &mut Context<InviscidWindow>) {
    let Some(editor) = app.workspace().read(cx).active_editor().cloned() else {
        return;
    };
    if let Err(error) = editor.update(cx, |editor, cx| editor.reload_file(cx)) {
        eprintln!("Failed to reload file: {}", error);
    }
    sync_workspace_to_global(app, cx);
    cx.notify();
}

pub fn keep_local_changes(app: &mut InviscidWindow, cx: &mut Context<InviscidWindow>) {
    let Some(editor) = app.workspace().read(cx).active_editor().cloned() else {
        return;
    };
    editor.update(cx, |editor, cx| editor.keep_local_changes(cx));
    sync_workspace_to_global(app, cx);
    cx.notify();
}

pub fn new_window(cx: &mut App) {
    let (display_id, window_bounds) = cx
        .try_global::<AppConfig>()
        .cloned()
        .map(|cfg| cfg.resolve_new_window_bounds(cx))
        .unwrap_or_else(|| WindowLayoutState::default_window_bounds(cx));

    cx.open_window(
        main_window_options(display_id, window_bounds),
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

fn centered_child_window_bounds(width: Pixels, height: Pixels, cx: &mut App) -> Bounds<Pixels> {
    if let Some(active_window) = cx.active_window()
        && let Ok(active_bounds) = active_window.update(cx, |_, window, _| window.bounds())
    {
        let x = active_bounds.origin.x + (active_bounds.size.width - width) / 2.0;
        let y = active_bounds.origin.y + (active_bounds.size.height - height) / 2.0;
        Bounds::new(point(x.max(px(0.0)), y.max(px(0.0))), size(width, height))
    } else {
        Bounds::centered(None, size(width, height), cx)
    }
}

fn open_singleton_window<S, V>(
    cx: &mut App,
    slot: fn(&mut S) -> &mut Option<WindowHandle<V>>,
    window_size: Size<Pixels>,
    options: impl FnOnce(Bounds<Pixels>) -> WindowOptions,
    build: impl FnOnce(&mut Context<V>) -> V + 'static,
    focus: fn(&V, &mut Window),
) where
    S: gpui::Global + Default,
    V: gpui::Render + 'static,
{
    let existing_handle = *slot(cx.default_global::<S>());
    if let Some(handle) = existing_handle {
        let activated = handle
            .update(cx, |view, window, _cx| {
                window.activate_window();
                focus(view, window);
            })
            .is_ok();
        if activated {
            return;
        }
        *slot(cx.default_global::<S>()) = None;
    }

    let bounds = centered_child_window_bounds(window_size.width, window_size.height, cx);
    if let Ok(handle) = cx.open_window(options(bounds), |window, cx| {
        let view = cx.new(build);
        focus(view.read(cx), window);
        window.activate_window();
        view
    }) {
        *slot(cx.default_global::<S>()) = Some(handle);
    }
}

#[derive(Default)]
pub struct SettingsWindowState {
    pub handle: Option<WindowHandle<crate::app::settings::SettingsWindow>>,
}

impl gpui::Global for SettingsWindowState {}

pub fn open_settings_window(cx: &mut App) {
    open_singleton_window::<SettingsWindowState, crate::app::settings::SettingsWindow>(
        cx,
        |state| &mut state.handle,
        size(px(800.0), px(560.0)),
        |bounds| WindowOptions {
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
        crate::app::settings::SettingsWindow::new,
        |view, window| view.focus(window),
    );
}

#[derive(Default)]
pub struct AboutWindowState {
    pub handle: Option<WindowHandle<crate::app::about::AboutWindow>>,
}

impl gpui::Global for AboutWindowState {}

pub fn open_about_window(cx: &mut App) {
    open_singleton_window::<AboutWindowState, crate::app::about::AboutWindow>(
        cx,
        |state| &mut state.handle,
        size(px(360.0), px(320.0)),
        |bounds| WindowOptions {
            window_bounds: Some(WindowBounds::Windowed(bounds)),
            titlebar: None,
            window_decorations: Some(WindowDecorations::Client),
            window_min_size: Some(size(px(340.0), px(300.0))),
            is_movable: true,
            focus: false,
            show: false,
            ..Default::default()
        },
        crate::app::about::AboutWindow::new,
        |view, window| view.focus(window),
    );
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

pub fn toggle_sidebar(
    app: &mut InviscidWindow,
    window: &mut Window,
    cx: &mut Context<InviscidWindow>,
) {
    app.close_menu(cx);
    app.workspace().update(cx, |ws, cx| {
        ws.toggle_sidebar_view(SidebarView::Files, window, cx);
    });
    sync_workspace_to_global(app, cx);
    cx.notify();
}

pub fn toggle_search_view(
    app: &mut InviscidWindow,
    window: &mut Window,
    cx: &mut Context<InviscidWindow>,
) {
    app.close_menu(cx);
    app.workspace().update(cx, |ws, cx| {
        ws.toggle_sidebar_view(SidebarView::Search, window, cx);
    });
    sync_workspace_to_global(app, cx);
    cx.notify();
}

pub fn toggle_outline_view(
    app: &mut InviscidWindow,
    window: &mut Window,
    cx: &mut Context<InviscidWindow>,
) {
    app.close_menu(cx);
    app.workspace().update(cx, |ws, cx| {
        ws.toggle_outline_panel(window, cx);
    });
    sync_workspace_to_global(app, cx);
    cx.notify();
}

pub fn toggle_fps_display(app: &mut InviscidWindow, cx: &mut Context<InviscidWindow>) {
    app.close_menu(cx);
    let mut cfg = cx.try_global::<AppConfig>().cloned().unwrap_or_default();
    cfg.show_fps = !cfg.show_fps;
    cfg.persist_preferences_async(cx.background_executor());
    cx.set_global(cfg);
    cx.refresh_windows();
    cx.notify();
}

pub fn open_workspace_search(
    app: &mut InviscidWindow,
    window: &mut Window,
    cx: &mut Context<InviscidWindow>,
) {
    app.close_menu(cx);
    app.workspace().update(cx, |ws, cx| {
        ws.open_workspace_search(window, cx);
    });
    sync_workspace_to_global(app, cx);
    cx.notify();
}

pub fn open_workspace_match(
    app: &mut InviscidWindow,
    action: &OpenWorkspaceMatch,
    window: &mut Window,
    cx: &mut Context<InviscidWindow>,
) {
    app.close_menu(cx);
    app.workspace().update(cx, |ws, cx| {
        ws.open_file_at(action.path.clone(), action.position, cx);
        ws.focus_active_editor(window, cx);
    });
    sync_workspace_to_global(app, cx);
    cx.notify();
}

pub fn close_search_panel(
    app: &mut InviscidWindow,
    window: &mut Window,
    cx: &mut Context<InviscidWindow>,
) {
    app.close_menu(cx);
    app.workspace().update(cx, |ws, cx| {
        if ws.is_workspace_search_active() {
            ws.close_active_tab(cx);
        }
        ws.set_sidebar_visible(false, cx);
        ws.focus_active_editor(window, cx);
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
        cx.global::<AppConfig>()
            .persist_preferences_async(cx.background_executor());
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

#[cfg(test)]
mod tests {
    use super::*;
    use core::prelude::v1::test;

    #[gpui::test]
    fn menu_disables_export_items_while_editor_is_loading(cx: &mut TestAppContext) {
        cx.update(|cx| {
            let config = AppConfig::default();
            cx.set_global(ThemeManager::from_config(&config));
            crate::app::actions::bind_default_bindings(cx);
            cx.set_global(config);
        });
        let (app, cx) = cx.add_window_view(|_, cx| InviscidWindow::new(cx));
        cx.update(|_, cx| {
            let workspace = app.read(cx).workspace().clone();
            workspace.update(cx, |workspace, cx| workspace.new_tab(cx));
            let editor = workspace.read(cx).active_editor().unwrap().clone();
            editor.update(cx, |editor, _| editor.begin_loading("loading.md".into()));
            app.update(cx, |app, cx| {
                app.set_active_menu(Some(ActiveMenu::File), cx);
            });
        });
        cx.run_until_parked();

        cx.update(|_, cx| {
            let win = app.read(cx);
            assert!(!can_export_active_document(win, cx));
            assert!(prepare_active_export(win, ExportTargetKind::Html, cx).is_none());
            assert!(prepare_active_export(win, ExportTargetKind::Pdf, cx).is_none());
        });

        for selector in ["menu_item_Export as HTML...", "menu_item_Export as PDF..."] {
            let pos = cx.debug_bounds(selector).expect(selector).center();
            cx.simulate_click(pos, Modifiers::default());
            cx.run_until_parked();
            assert_eq!(
                cx.update(|_, cx| app.read(cx).active_menu()),
                Some(ActiveMenu::File),
                "{selector} must remain disabled while loading"
            );
        }
    }
}
