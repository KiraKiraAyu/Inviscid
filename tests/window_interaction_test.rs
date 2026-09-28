use gpui::{BorrowAppContext, EntityInputHandler, Focusable, TestAppContext};
use inviscid::app::InviscidWindow;
use inviscid::app::actions::*;
use inviscid::buffer::{Position, Selection, TextBuffer};
use inviscid::config::AppConfig;
use inviscid::editor::Editor;
use inviscid::editor::actions::*;
use inviscid::theme::ThemeManager;

fn init_test_globals(cx: &mut TestAppContext) {
    let config = AppConfig::default().with_theme("Dracula");
    let theme_manager = ThemeManager::from_config(&config);
    cx.update(|cx| {
        cx.set_global(theme_manager);
        cx.set_global(config);
        inviscid::app::actions::bind_default_bindings(cx);
    });
}

#[test]
fn test_gpui_global_theme_resolution_and_propagation() {
    let mut cx = TestAppContext::single();
    init_test_globals(&mut cx);

    let initial_theme = cx.update(|cx| cx.global::<ThemeManager>().theme().name.clone());
    assert_eq!(initial_theme, "Dracula");

    let try_theme = cx.update(|cx| {
        cx.try_global::<ThemeManager>()
            .map(|m| m.theme().name.clone())
    });
    assert_eq!(try_theme, Some("Dracula".to_string()));

    let (app, cx) = cx.add_window_view(|_window, cx| InviscidWindow::new(cx));
    cx.run_until_parked();

    let tab_count = cx.update(|_window, cx| app.read(cx).workspace().read(cx).tab_count());
    assert_eq!(tab_count, 0);

    cx.update(|_window, cx| {
        cx.update_global::<ThemeManager, _>(|tm, _cx| {
            assert!(tm.preview("Nord"));
        });
    });
    cx.run_until_parked();

    let active_theme = cx.update(|_window, cx| cx.global::<ThemeManager>().theme().name.clone());
    assert_eq!(active_theme, "Nord");

    cx.update(|_window, cx| {
        cx.update_global::<ThemeManager, _>(|tm, _cx| {
            tm.cancel_preview("Dracula");
        });
    });
    cx.run_until_parked();

    let restored_theme = cx.update(|_window, cx| cx.global::<ThemeManager>().theme().name.clone());
    assert_eq!(restored_theme, "Dracula");
}

#[test]
fn test_gpui_window_action_dispatch_and_buffer_mutation() {
    let mut cx = TestAppContext::single();
    init_test_globals(&mut cx);

    let initial_buf = TextBuffer::from_str("Hello World", None);
    let (editor, cx) = cx.add_window_view(|_window, cx| Editor::new_with_buffer(initial_buf, cx));
    cx.run_until_parked();

    let focus_handle = cx.update(|_window, cx| editor.read(cx).focus_handle(cx));
    cx.update(|window, _| window.focus(&focus_handle));
    cx.run_until_parked();

    let (chars, cursor) = cx.update(|_window, cx| {
        let ed = editor.read(cx);
        (ed.buffer().char_count(), ed.cursor_pos())
    });
    assert_eq!(chars, 11);
    assert_eq!(cursor, Position::new(0, 0));

    cx.dispatch_action(MoveRight);
    let cursor = cx.update(|_window, cx| editor.read(cx).cursor_pos());
    assert_eq!(cursor, Position::new(0, 1));

    cx.dispatch_action(MoveToEndOfLine);
    let cursor = cx.update(|_window, cx| editor.read(cx).cursor_pos());
    assert_eq!(cursor, Position::new(0, 11));

    cx.dispatch_action(MoveToBeginningOfLine);
    let cursor = cx.update(|_window, cx| editor.read(cx).cursor_pos());
    assert_eq!(cursor, Position::new(0, 0));

    cx.dispatch_action(SelectAll);
    let sel = cx.update(|_window, cx| editor.read(cx).buffer().selection());
    assert_eq!(sel.anchor, Position::new(0, 0));
    assert_eq!(sel.head, Position::new(0, 11));

    cx.dispatch_action(Backspace);
    let (empty, is_dirty) = cx.update(|_window, cx| {
        let ed = editor.read(cx);
        (ed.is_empty(), ed.is_dirty())
    });
    assert!(empty);
    assert!(is_dirty);

    cx.dispatch_action(UndoAction);
    let restored_text =
        cx.update(|_window, cx| editor.read(cx).buffer().line(0).unwrap_or("").to_string());
    assert_eq!(restored_text, "Hello World");
}

#[test]
fn test_gpui_focus_management_and_window_lifecycle() {
    let mut cx = TestAppContext::single();
    init_test_globals(&mut cx);

    let (app, cx) = cx.add_window_view(|_window, cx| InviscidWindow::new(cx));
    cx.run_until_parked();

    cx.update(|_window, cx| {
        app.update(cx, |app, cx| {
            app.workspace().update(cx, |ws, cx| ws.new_tab(cx));
        });
    });
    cx.run_until_parked();

    let editor_focus = cx.update(|_window, cx| {
        app.read(cx)
            .workspace()
            .read(cx)
            .active_editor()
            .expect("active editor should exist")
            .read(cx)
            .focus_handle(cx)
    });

    cx.update(|window, _| window.focus(&editor_focus));
    cx.run_until_parked();
    cx.update(|window, _| assert!(editor_focus.is_focused(window)));

    cx.update(|_window, cx| {
        app.update(cx, |_app, cx| {
            inviscid::app::commands::open_about_window(cx);
        });
    });
    cx.run_until_parked();

    cx.update(|_window, cx| {
        assert!(cx.has_global::<inviscid::app::commands::AboutWindowState>());
        assert!(
            cx.global::<inviscid::app::commands::AboutWindowState>()
                .handle
                .is_some()
        );
    });

    cx.update(|_window, cx| {
        app.update(cx, |_app, cx| {
            inviscid::app::commands::open_settings_window(cx);
        });
    });
    cx.run_until_parked();

    cx.update(|_window, cx| {
        assert!(cx.has_global::<inviscid::app::commands::SettingsWindowState>());
        assert!(
            cx.global::<inviscid::app::commands::SettingsWindowState>()
                .handle
                .is_some()
        );
    });

    cx.update(|_window, cx| {
        let ws = app.read(cx).workspace().clone();
        ws.update(cx, |ws, cx| ws.new_tab(cx));
    });
    cx.run_until_parked();

    let (tab_count, active_idx, tab1_focus) = cx.update(|_window, cx| {
        let ws = app.read(cx).workspace().read(cx);
        (
            ws.tab_count(),
            ws.active_tab_idx(),
            ws.active_editor()
                .expect("active editor should exist")
                .read(cx)
                .focus_handle(cx),
        )
    });
    assert_eq!(tab_count, 2);
    assert_eq!(active_idx, 1);

    cx.update(|window, cx| {
        let ws = app.read(cx).workspace().clone();
        ws.read(cx).focus_active_editor(window, cx);
    });
    cx.run_until_parked();
    cx.update(|window, _| assert!(tab1_focus.is_focused(window)));

    cx.update(|_window, cx| {
        let ws = app.read(cx).workspace().clone();
        ws.update(cx, |ws, cx| ws.switch_tab(0, cx));
    });
    cx.update(|window, cx| {
        let ws = app.read(cx).workspace().clone();
        ws.read(cx).focus_active_editor(window, cx);
    });
    cx.run_until_parked();

    let active_idx = cx.update(|_window, cx| app.read(cx).workspace().read(cx).active_tab_idx());
    assert_eq!(active_idx, 0);
    cx.update(|window, _| assert!(editor_focus.is_focused(window)));
}

#[test]
fn test_gpui_ime_input_handler_lifecycle_contract() {
    let mut cx = TestAppContext::single();
    init_test_globals(&mut cx);

    let (editor, cx) = cx.add_window_view(|_window, cx| Editor::new(cx));
    cx.run_until_parked();

    cx.update(|window, cx| {
        editor.update(cx, |ed, cx| {
            ed.replace_and_mark_text_in_range(None, "pinyin", Some(0..6), window, cx);
        });
    });
    cx.run_until_parked();

    cx.update(|window, cx| {
        editor.update(cx, |ed, cx| {
            assert_eq!(ed.marked_text_range(window, cx), Some(0..6));
            assert_eq!(ed.buffer().line(0), Some("pinyin"));
            let mut adj = None;
            assert_eq!(
                ed.text_for_range(0..6, &mut adj, window, cx),
                Some("pinyin".to_string())
            );
            assert_eq!(adj, Some(0..6));
        });
    });

    cx.update(|window, cx| {
        editor.update(cx, |ed, cx| {
            ed.replace_text_in_range(Some(0..6), "拼音", window, cx);
        });
    });
    cx.run_until_parked();

    cx.update(|window, cx| {
        editor.update(cx, |ed, cx| {
            assert_eq!(ed.marked_text_range(window, cx), None);
            assert_eq!(ed.buffer().line(0), Some("拼音"));
            assert_eq!(ed.buffer().line(0).unwrap().chars().count(), 2);
            assert_eq!(ed.buffer().char_count(), 3);
            assert_eq!(ed.cursor_pos(), Position::new(0, 2));
        });
    });

    cx.update(|window, cx| {
        editor.update(cx, |ed, cx| {
            ed.replace_and_mark_text_in_range(None, "ceshi", Some(2..7), window, cx);
        });
    });
    cx.run_until_parked();

    cx.update(|window, cx| {
        editor.update(cx, |ed, cx| {
            assert_eq!(ed.marked_text_range(window, cx), Some(2..7));
        });
    });

    cx.update(|window, cx| {
        editor.update(cx, |ed, cx| {
            ed.unmark_text(window, cx);
        });
    });
    cx.run_until_parked();

    cx.update(|window, cx| {
        editor.update(cx, |ed, cx| {
            assert_eq!(ed.marked_text_range(window, cx), None);
            ed.replace_text_in_range(Some(2..7), "", window, cx);
            assert_eq!(ed.buffer().line(0), Some("拼音"));
        });
    });
    cx.run_until_parked();

    cx.update(|_window, cx| {
        editor.update(cx, |ed, _cx| {
            ed.buffer_mut().set_selection(Selection {
                anchor: Position::new(0, 0),
                head: Position::new(0, 2),
            });
        });
    });
    cx.run_until_parked();

    cx.update(|window, cx| {
        editor.update(cx, |ed, cx| {
            ed.replace_text_in_range(None, "(", window, cx);
        });
    });
    cx.run_until_parked();

    cx.update(|_window, cx| {
        let ed = editor.read(cx);
        assert_eq!(ed.buffer().line(0), Some("(拼音)"));
        assert_eq!(ed.cursor_pos(), Position::new(0, 4));
    });
}

#[test]
fn test_window_action_dispatch_and_menu_bypass() {
    let mut cx = TestAppContext::single();
    init_test_globals(&mut cx);

    let (app, cx) = cx.add_window_view(|_window, cx| InviscidWindow::new(cx));
    cx.run_until_parked();

    cx.update(|_window, cx| {
        let ws = app.read(cx).workspace().clone();
        ws.update(cx, |ws, cx| ws.new_tab(cx));
    });
    cx.run_until_parked();
    cx.update(|window, cx| {
        let ws = app.read(cx).workspace().clone();
        ws.read(cx).focus_active_editor(window, cx);
    });
    cx.run_until_parked();

    assert!(cx.update(|_window, cx| app.read(cx).active_menu().is_none()));
    cx.dispatch_action(NewWindow);
    cx.run_until_parked();

    cx.update(|_window, cx| {
        app.update(cx, |app, cx| {
            app.set_active_menu(Some(inviscid::app::ActiveMenu::File), cx);
        });
    });
    assert!(cx.update(|_window, cx| app.read(cx).active_menu().is_some()));

    cx.dispatch_action(NewWindow);
    cx.run_until_parked();

    assert!(cx.update(|_window, cx| app.read(cx).active_menu().is_none()));
}

#[test]
fn test_editor_cursor_blink_lifecycle_on_focus_and_blur() {
    let mut cx = TestAppContext::single();
    init_test_globals(&mut cx);

    let (editor, cx) = cx.add_window_view(|_window, cx| Editor::new(cx));
    cx.run_until_parked();

    // 1. Initial render without window focus cancels initial ghost task
    cx.update(|_window, cx| {
        let ed = editor.read(cx);
        assert_eq!(ed.cursor_opacity(), 0.0);
        assert!(!ed.has_blink_task());
    });

    // 2. When focused: starts blink task and sets opacity to 1.0
    let focus_handle = cx.update(|_window, cx| editor.read(cx).focus_handle(cx));
    cx.update(|window, cx| {
        window.focus(&focus_handle);
        editor.update(cx, |ed, cx| ed.on_focus(cx));
    });
    cx.run_until_parked();

    cx.update(|_window, cx| {
        let ed = editor.read(cx);
        assert_eq!(ed.cursor_opacity(), 1.0);
        assert!(ed.has_blink_task());
    });

    // 3. On blur: blink task is immediately aborted and opacity set to 0.0
    cx.update(|_window, cx| {
        editor.update(cx, |ed, cx| {
            ed.on_blur(cx);
            assert_eq!(ed.cursor_opacity(), 0.0);
            assert!(!ed.has_blink_task());
        });
    });

    // 4. On re-focus: blink task is restored to solid 1.0 and restarts breathing
    cx.update(|_window, cx| {
        editor.update(cx, |ed, cx| {
            ed.on_focus(cx);
            assert_eq!(ed.cursor_opacity(), 1.0);
            assert!(ed.has_blink_task());
        });
    });
}

#[test]
fn test_inviscid_window_open_initial_and_lib_reexport() {
    use inviscid::InviscidWindow;
    use inviscid::config::WorkspaceState;
    use std::path::PathBuf;

    let mut cx = TestAppContext::single();
    init_test_globals(&mut cx);

    // 1. When initial_path is None and restore_workspace is false -> clean default workspace (0 tabs)
    let (app1, cx) = cx.add_window_view(|_window, cx| InviscidWindow::open_initial(None, cx));
    cx.run_until_parked();
    let tab_count1 = cx.update(|_window, cx| app1.read(cx).workspace().read(cx).tab_count());
    assert_eq!(tab_count1, 0);

    // 2. When initial_path is Some(file) -> opens with that file
    let sample_file = PathBuf::from("test_readme.md");
    let (app2, cx) = cx
        .add_window_view(|_window, cx| InviscidWindow::open_initial(Some(sample_file.clone()), cx));
    cx.run_until_parked();
    let tab_count2 = cx.update(|_window, cx| app2.read(cx).workspace().read(cx).tab_count());
    assert_eq!(tab_count2, 1);
    let active_path = cx.update(|_window, cx| {
        app2.read(cx)
            .workspace()
            .read(cx)
            .active_file_path(cx)
            .map(std::path::Path::to_path_buf)
    });
    assert_eq!(active_path, Some(sample_file));

    // 3. When initial_path is None, restore_workspace is true, and last_workspace is present
    let temp_dir = std::env::temp_dir().join(format!("inviscid_open_init_{}", std::process::id()));
    let _ = std::fs::create_dir_all(&temp_dir);
    let doc1 = temp_dir.join("doc1.md");
    let doc2 = temp_dir.join("doc2.md");
    let _ = std::fs::write(&doc1, "# Doc 1\n");
    let _ = std::fs::write(&doc2, "# Doc 2\n");

    let mock_ws = WorkspaceState {
        root_dir: Some(temp_dir.clone()),
        active_file: Some(doc1.clone()),
        open_files: vec![doc1.clone(), doc2.clone()],
        sidebar_visible: true,
        sidebar_width: None,
    };

    cx.update(|_window, cx| {
        cx.update_global::<AppConfig, _>(|cfg, _cx| {
            cfg.restore_workspace = true;
            cfg.session.last_workspace = Some(mock_ws.clone());
        });
    });

    let (app3, cx) = cx.add_window_view(|_window, cx| InviscidWindow::open_initial(None, cx));
    cx.run_until_parked();
    let tab_count3 = cx.update(|_window, cx| app3.read(cx).workspace().read(cx).tab_count());
    assert_eq!(tab_count3, 2);
    let ws_root = cx.update(|_window, cx| {
        app3.read(cx)
            .workspace()
            .read(cx)
            .root_dir()
            .map(std::path::Path::to_path_buf)
    });
    assert_eq!(ws_root, Some(temp_dir.clone()));

    let _ = std::fs::remove_dir_all(&temp_dir);
}
