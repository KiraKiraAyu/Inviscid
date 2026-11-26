use std::future::Future;
use std::path::PathBuf;
use std::pin::Pin;

use gpui::{App, Entity, Task, WindowHandle};

use crate::app::InviscidWindow;
use crate::app::workspace::TabKind;
use crate::config::AppConfig;
use crate::editor::Editor;
use crate::i18n::catalog::format_template;
use crate::i18n::{ActiveLocale, TextKey};

#[derive(Clone)]
pub struct UnsavedDocument {
    pub window_handle: WindowHandle<InviscidWindow>,
    pub editor: Entity<Editor>,
    pub title: String,
    pub has_file_path: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UnsavedChangesChoice {
    SaveAll,
    DiscardAll,
    Cancel,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum QuitOutcome {
    Proceeded,
    Cancelled,
    SaveFailed(String),
}

/// Collects every dirty editor tab across all open [`InviscidWindow`] instances.
pub fn collect_unsaved_documents(cx: &App) -> Vec<UnsavedDocument> {
    let mut unsaved = Vec::new();
    for handle in cx.windows() {
        let Some(window_handle) = handle.downcast::<InviscidWindow>() else {
            continue;
        };
        let Ok(app) = window_handle.read(cx) else {
            continue;
        };
        let workspace = app.workspace().read(cx);
        for tab in workspace.tabs() {
            if tab.kind != TabKind::Editor {
                continue;
            }
            let editor = tab.editor.read(cx);
            if editor.is_dirty() {
                unsaved.push(UnsavedDocument {
                    window_handle,
                    editor: tab.editor.clone(),
                    title: editor.title(),
                    has_file_path: editor.file_path().is_some(),
                });
            }
        }
    }
    unsaved
}

/// Synchronously persists all open window layouts and session state, saving the active window last
/// so it remains the primary `last_workspace` on next launch.
pub fn persist_all_windows_state_sync(cx: &mut App) {
    let active = cx.active_window();
    for handle in cx.windows() {
        if Some(handle) != active
            && let Some(window_handle) = handle.downcast::<InviscidWindow>()
        {
            let _ = window_handle.update(cx, |app, window, cx| {
                app.persist_window_state_sync(window, cx);
            });
        }
    }
    if let Some(window_handle) = active.and_then(|handle| handle.downcast::<InviscidWindow>()) {
        let _ = window_handle.update(cx, |app, window, cx| {
            app.persist_window_state_sync(window, cx);
        });
    } else if let Some(config) = cx.try_global::<AppConfig>() {
        config.save_session();
    }
}

fn resolve_save_as_target(raw_path: PathBuf, collocated_mode: bool) -> Result<PathBuf, String> {
    if collocated_mode && !crate::fs::is_collocated_index_path(&raw_path) {
        let bundle_dir = match (raw_path.parent(), raw_path.file_stem()) {
            (Some(parent), Some(stem)) if !stem.is_empty() => parent.join(stem),
            _ => raw_path.with_extension(""),
        };
        std::fs::create_dir_all(&bundle_dir).map_err(|e| e.to_string())?;
        Ok(bundle_dir.join(crate::fs::COLLOCATED_INDEX_FILE))
    } else {
        Ok(raw_path)
    }
}

fn any_inviscid_window_borrowed(cx: &App) -> bool {
    cx.windows()
        .into_iter()
        .filter_map(|handle| handle.downcast::<InviscidWindow>())
        .any(|window_handle| window_handle.read(cx).is_err())
}

/// Checks for unsaved documents across all windows before running `on_confirmed`.
///
/// When no documents are dirty and no window is currently borrowed, persists window state and
/// invokes `on_confirmed` synchronously. Otherwise defers onto the foreground task so all
/// windows are released from any active `update_window` borrow before checking and prompting.
pub fn request_graceful_quit_with_prompt<F, Fut, C>(
    cx: &mut App,
    prompt: F,
    on_confirmed: C,
) -> Task<QuitOutcome>
where
    F: FnOnce(Vec<String>) -> Fut + 'static,
    Fut: Future<Output = UnsavedChangesChoice> + 'static,
    C: FnOnce(&mut App) + 'static,
{
    if !any_inviscid_window_borrowed(cx) {
        let unsaved = collect_unsaved_documents(cx);
        if unsaved.is_empty() {
            persist_all_windows_state_sync(cx);
            on_confirmed(cx);
            return Task::ready(QuitOutcome::Proceeded);
        }
    }

    cx.spawn(async move |cx| {
        let unsaved = cx
            .update(|cx| collect_unsaved_documents(cx))
            .unwrap_or_default();
        if unsaved.is_empty() {
            let _ = cx.update(|cx| {
                persist_all_windows_state_sync(cx);
                on_confirmed(cx);
            });
            return QuitOutcome::Proceeded;
        }

        let titles: Vec<String> = unsaved.iter().map(|doc| doc.title.clone()).collect();
        let choice = prompt(titles).await;
        match choice {
            UnsavedChangesChoice::Cancel => QuitOutcome::Cancelled,
            UnsavedChangesChoice::DiscardAll => {
                let _ = cx.update(|cx| {
                    persist_all_windows_state_sync(cx);
                    on_confirmed(cx);
                });
                QuitOutcome::Proceeded
            }
            UnsavedChangesChoice::SaveAll => {
                for doc in unsaved {
                    if doc.has_file_path {
                        let task_res = doc.editor.update(cx, |ed, cx| ed.save_file_async(cx));
                        match task_res {
                            Ok(Ok(task)) => {
                                if let Err(err) = task.await {
                                    return QuitOutcome::SaveFailed(err.to_string());
                                }
                            }
                            Ok(Err(err)) => return QuitOutcome::SaveFailed(err.to_string()),
                            Err(err) => return QuitOutcome::SaveFailed(err.to_string()),
                        }
                    } else {
                        let default_name = if doc.title.is_empty() {
                            "document.md".to_string()
                        } else {
                            doc.title.clone()
                        };
                        let Some(file) = rfd::AsyncFileDialog::new()
                            .set_file_name(&default_name)
                            .save_file()
                            .await
                        else {
                            return QuitOutcome::Cancelled;
                        };
                        let collocated_mode = cx
                            .update(|cx| {
                                cx.try_global::<AppConfig>()
                                    .is_some_and(|cfg| cfg.collocated_mode)
                            })
                            .unwrap_or(false);
                        let target_path = match resolve_save_as_target(
                            file.path().to_path_buf(),
                            collocated_mode,
                        ) {
                            Ok(path) => path,
                            Err(err) => return QuitOutcome::SaveFailed(err),
                        };
                        let task_res = doc
                            .editor
                            .update(cx, |ed, cx| ed.save_file_as_async(target_path, cx));
                        match task_res {
                            Ok(task) => {
                                if let Err(err) = task.await {
                                    return QuitOutcome::SaveFailed(err.to_string());
                                }
                            }
                            Err(err) => return QuitOutcome::SaveFailed(err.to_string()),
                        }
                    }
                }

                let _ = cx.update(|cx| {
                    persist_all_windows_state_sync(cx);
                    on_confirmed(cx);
                });
                QuitOutcome::Proceeded
            }
        }
    })
}

pub(crate) fn build_unsaved_prompt(
    cx: &App,
) -> impl FnOnce(Vec<String>) -> Pin<Box<dyn Future<Output = UnsavedChangesChoice>>> + use<> {
    let title = cx.t(TextKey::QuitUnsavedTitle).to_string();
    let msg_template = cx.t(TextKey::QuitUnsavedMessage).to_string();
    let save_all = cx.t(TextKey::QuitUnsavedSaveAll).to_string();
    let discard = cx.t(TextKey::QuitUnsavedDiscard).to_string();
    let cancel = cx.t(TextKey::CommonCancel).to_string();

    move |titles| {
        Box::pin(async move {
            let list = titles
                .iter()
                .map(|t| format!("• {t}"))
                .collect::<Vec<_>>()
                .join("\n");
            let count_str = titles.len().to_string();
            let description =
                format_template(&msg_template, &[("count", &count_str), ("files", &list)]);
            let result = rfd::AsyncMessageDialog::new()
                .set_level(rfd::MessageLevel::Warning)
                .set_title(&title)
                .set_description(description)
                .set_buttons(rfd::MessageButtons::YesNoCancelCustom(
                    save_all.clone(),
                    discard.clone(),
                    cancel,
                ))
                .show()
                .await;

            match result {
                rfd::MessageDialogResult::Yes => UnsavedChangesChoice::SaveAll,
                rfd::MessageDialogResult::No => UnsavedChangesChoice::DiscardAll,
                rfd::MessageDialogResult::Custom(label) if label == save_all => {
                    UnsavedChangesChoice::SaveAll
                }
                rfd::MessageDialogResult::Custom(label) if label == discard => {
                    UnsavedChangesChoice::DiscardAll
                }
                _ => UnsavedChangesChoice::Cancel,
            }
        })
    }
}

/// Runs the standard unsaved-changes guard with a native confirmation dialog before executing
/// `on_confirmed`.
pub fn request_graceful_quit(cx: &mut App, on_confirmed: impl FnOnce(&mut App) + 'static) {
    let prompt = build_unsaved_prompt(cx);
    request_graceful_quit_with_prompt(cx, prompt, on_confirmed).detach();
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::actions;
    use crate::theme::ThemeManager;
    use core::prelude::v1::test;
    use gpui::EntityInputHandler;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicBool, Ordering};

    fn init_test_globals(cx: &mut gpui::TestAppContext) {
        cx.update(|cx| {
            let cfg = AppConfig::default();
            cx.set_global(ThemeManager::from_config(&cfg));
            actions::bind_default_bindings(cx);
            cx.set_global(cfg);
        });
    }

    #[gpui::test]
    fn test_graceful_quit_clean_workspace_proceeds_synchronously(cx: &mut gpui::TestAppContext) {
        init_test_globals(cx);
        let (_app, cx) = cx.add_window_view(|_, cx| InviscidWindow::new(cx));
        cx.run_until_parked();

        let prompt_called = Arc::new(AtomicBool::new(false));
        let confirmed = Arc::new(AtomicBool::new(false));
        let prompt_flag = prompt_called.clone();
        let confirm_flag = confirmed.clone();

        cx.cx.update(|cx| {
            assert!(collect_unsaved_documents(cx).is_empty());
            let _task = request_graceful_quit_with_prompt(
                cx,
                move |_| async move {
                    prompt_flag.store(true, Ordering::SeqCst);
                    UnsavedChangesChoice::Cancel
                },
                move |_| {
                    confirm_flag.store(true, Ordering::SeqCst);
                },
            );
            // Clean unborrowed path must run synchronously before returning.
            assert!(confirmed.load(Ordering::SeqCst));
            assert!(!prompt_called.load(Ordering::SeqCst));
        });
    }

    fn open_and_dirty_temp_file(
        app: &Entity<InviscidWindow>,
        cx: &mut gpui::VisualTestContext,
    ) -> (tempfile::TempDir, PathBuf, Entity<Editor>) {
        let dir = tempfile::tempdir().unwrap();
        let file_path = dir.path().join("note.md");
        std::fs::write(&file_path, "initial").unwrap();

        let editor = cx.update(|window, cx| {
            let ws = app.read(cx).workspace().clone();
            ws.update(cx, |ws, cx| ws.open_file(file_path.clone(), cx));
            let editor = ws.read(cx).active_editor().unwrap().clone();
            let _ = window;
            editor
        });
        cx.run_until_parked();

        cx.update(|window, cx| {
            editor.update(cx, |ed, cx| {
                ed.replace_text_in_range(None, " updated", window, cx);
            });
            assert!(editor.read(cx).is_dirty());
        });
        cx.run_until_parked();

        (dir, file_path, editor)
    }

    #[gpui::test]
    fn test_graceful_quit_dirty_doc_cancel_aborts(cx: &mut gpui::TestAppContext) {
        init_test_globals(cx);
        let (app, cx) = cx.add_window_view(|_, cx| InviscidWindow::new(cx));
        cx.run_until_parked();

        let (_dir, file_path, editor) = open_and_dirty_temp_file(&app, cx);
        let confirmed = Arc::new(AtomicBool::new(false));
        let confirm_flag = confirmed.clone();

        let docs = cx.read(|cx| collect_unsaved_documents(cx));
        assert_eq!(docs.len(), 1);
        assert_eq!(docs[0].editor, editor);

        let task = cx.update(|_window, cx| {
            request_graceful_quit_with_prompt(
                cx,
                |titles| async move {
                    assert_eq!(titles, vec!["note.md".to_string()]);
                    UnsavedChangesChoice::Cancel
                },
                move |_| {
                    confirm_flag.store(true, Ordering::SeqCst);
                },
            )
        });
        cx.run_until_parked();

        let outcome = cx.executor().block(task);
        assert_eq!(outcome, QuitOutcome::Cancelled);
        assert!(!confirmed.load(Ordering::SeqCst));
        assert_eq!(std::fs::read_to_string(&file_path).unwrap(), "initial");
        cx.read(|cx| assert!(editor.read(cx).is_dirty()));
    }

    #[gpui::test]
    fn test_graceful_quit_dirty_doc_discard_proceeds(cx: &mut gpui::TestAppContext) {
        init_test_globals(cx);
        let (app, cx) = cx.add_window_view(|_, cx| InviscidWindow::new(cx));
        cx.run_until_parked();

        let (_dir, file_path, _editor) = open_and_dirty_temp_file(&app, cx);
        let confirmed = Arc::new(AtomicBool::new(false));
        let confirm_flag = confirmed.clone();

        let task = cx.update(|_window, cx| {
            request_graceful_quit_with_prompt(
                cx,
                |_| async move { UnsavedChangesChoice::DiscardAll },
                move |_| {
                    confirm_flag.store(true, Ordering::SeqCst);
                },
            )
        });
        cx.run_until_parked();

        let outcome = cx.executor().block(task);
        assert_eq!(outcome, QuitOutcome::Proceeded);
        assert!(confirmed.load(Ordering::SeqCst));
        assert_eq!(std::fs::read_to_string(&file_path).unwrap(), "initial");
    }

    #[gpui::test]
    fn test_graceful_quit_dirty_doc_save_all_persists_to_disk_and_proceeds(
        cx: &mut gpui::TestAppContext,
    ) {
        init_test_globals(cx);
        let (app, cx) = cx.add_window_view(|_, cx| InviscidWindow::new(cx));
        cx.run_until_parked();

        let (_dir, file_path, editor) = open_and_dirty_temp_file(&app, cx);
        let confirmed = Arc::new(AtomicBool::new(false));
        let confirm_flag = confirmed.clone();

        let task = cx.update(|_window, cx| {
            request_graceful_quit_with_prompt(
                cx,
                |_| async move { UnsavedChangesChoice::SaveAll },
                move |_| {
                    confirm_flag.store(true, Ordering::SeqCst);
                },
            )
        });
        cx.run_until_parked();

        let outcome = cx.executor().block(task);
        assert_eq!(outcome, QuitOutcome::Proceeded);
        assert!(confirmed.load(Ordering::SeqCst));
        let saved_disk = std::fs::read_to_string(&file_path).unwrap();
        assert!(saved_disk.contains("updated"), "disk content was: {saved_disk:?}");
        cx.read(|cx| assert!(!editor.read(cx).is_dirty()));
    }
}
