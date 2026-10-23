use std::fs;
use std::path::PathBuf;
use std::time::Duration;

use gpui::{Focusable, TestAppContext};
use inviscid::app::actions::OpenWorkspaceMatch;
use inviscid::app::{InviscidWindow, SidebarView};
use inviscid::buffer::Position;
use inviscid::config::AppConfig;
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

fn create_test_workspace(prefix: &str) -> (PathBuf, PathBuf, PathBuf) {
    let dir = std::env::temp_dir().join(format!(
        "inviscid_test_search_{}_{}",
        prefix,
        std::process::id()
    ));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(dir.join("src")).unwrap();

    let file1 = dir.join("README.md");
    let file2 = dir.join("src").join("notes.txt");

    fs::write(&file1, "# Main Project\nIntro text\napple banana\n").unwrap();
    fs::write(&file2, "first line\n    let apple = 42;\nthird line\n").unwrap();

    (dir, file1, file2)
}

#[test]
fn test_workspace_search_shortcut_opens_and_switches_view() {
    let mut tcx = TestAppContext::single();
    init_test_globals(&mut tcx);

    let (dir, _, _) = create_test_workspace("shortcut");
    let (app, cx) = tcx.add_window_view(|_window, cx| InviscidWindow::new(cx));
    cx.run_until_parked();

    let ws = cx.update(|_window, cx| app.read(cx).workspace().clone());
    cx.update(|window, cx| {
        ws.update(cx, |ws, cx| {
            ws.open_folder(dir.clone(), cx);
            ws.focus_active_editor(window, cx);
        });
    });
    cx.run_until_parked();

    // Verify initial sidebar view is Files
    ws.read_with(cx, |ws, _| {
        assert_eq!(ws.active_sidebar(), Some(SidebarView::Files));
    });

    // Simulate FindInWorkspace action
    cx.update(|window, cx| {
        window.dispatch_action(Box::new(inviscid::app::actions::FindInWorkspace), cx);
    });
    cx.run_until_parked();

    // Verify sidebar is visible and switched to Search
    ws.read_with(cx, |ws, _| {
        assert_eq!(ws.active_sidebar(), Some(SidebarView::Search));
    });

    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn test_escape_in_search_input_closes_panel_and_focuses_editor() {
    let mut tcx = TestAppContext::single();
    init_test_globals(&mut tcx);

    let (dir, _, _) = create_test_workspace("escape");
    let (app, cx) = tcx.add_window_view(|_window, cx| InviscidWindow::new(cx));
    cx.run_until_parked();

    let ws = cx.update(|_window, cx| app.read(cx).workspace().clone());
    cx.update(|window, cx| {
        ws.update(cx, |ws, cx| {
            ws.open_folder(dir.clone(), cx);
            ws.focus_active_editor(window, cx);
        });
    });
    cx.run_until_parked();

    cx.update(|window, cx| {
        window.dispatch_action(Box::new(inviscid::app::actions::FindInWorkspace), cx);
    });
    cx.run_until_parked();
    assert_eq!(
        ws.read_with(cx, |ws, _| ws.active_sidebar()),
        Some(SidebarView::Search)
    );

    cx.simulate_keystrokes("escape");
    cx.run_until_parked();

    assert_eq!(ws.read_with(cx, |ws, _| ws.active_sidebar()), None);
    cx.update(|window, cx| {
        let editor = ws.read(cx).active_editor().expect("active editor").clone();
        assert!(editor.read(cx).focus_handle(cx).is_focused(window));
    });

    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn test_workspace_search_finds_matches_and_opens_at_location() {
    let mut cx = TestAppContext::single();
    init_test_globals(&mut cx);

    let (dir, file1, file2) = create_test_workspace("match_open");
    let (app, cx) = cx.add_window_view(|_window, cx| InviscidWindow::new(cx));
    cx.run_until_parked();

    let ws = cx.update(|_window, cx| app.read(cx).workspace().clone());
    cx.update(|window, cx| {
        ws.update(cx, |ws, cx| {
            ws.open_folder(dir.clone(), cx);
            ws.open_workspace_search(window, cx);
        });
    });
    cx.run_until_parked();

    let search_panel = ws.read_with(cx, |ws, _| ws.search_panel().clone());

    // Enter search query "apple"
    cx.update(|_window, cx| {
        search_panel.update(cx, |sp, cx| {
            sp.set_query_text("apple", cx);
        });
    });

    // Advance clock past debounce and run background tasks
    cx.background_executor
        .advance_clock(Duration::from_millis(200));
    cx.background_executor.run_until_parked();
    cx.run_until_parked();

    // Results arrive sorted by relative path: README.md before src/notes.txt
    let hit = search_panel.read_with(cx, |sp, _| {
        let results = sp.results();
        assert_eq!(results.total_matches(), 2);
        let paths: Vec<_> = results.files().iter().map(|f| f.path.clone()).collect();
        assert_eq!(paths, [file1.clone(), file2.clone()]);
        results.files()[1].matches[0].clone()
    });
    assert_eq!((hit.line_number, hit.char_range.0), (2, 8));

    // Dispatch the action a result row dispatches when clicked
    cx.update(|window, cx| {
        window.dispatch_action(
            Box::new(OpenWorkspaceMatch {
                path: file2.clone(),
                position: Position::new(hit.line_number - 1, hit.char_range.0),
            }),
            cx,
        );
    });
    cx.run_until_parked();

    // Verify active editor opened file2 at the target position and received window focus
    ws.read_with(cx, |ws, cx| {
        let ed_entity = ws.active_editor().expect("active editor open");
        let ed = ed_entity.read(cx);
        assert_eq!(ed.file_path(), Some(file2.as_path()));
        assert_eq!(ed.buffer().cursor_pos(), Position::new(1, 8));
    });
    cx.update(|window, cx| {
        let ed_entity = ws
            .read(cx)
            .active_editor()
            .expect("active editor open")
            .clone();
        assert!(ed_entity.read(cx).focus_handle(cx).is_focused(window));
    });

    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn test_workspace_search_switcher_toggle() {
    let mut cx = TestAppContext::single();
    init_test_globals(&mut cx);

    let (dir, _, _) = create_test_workspace("toggle");
    let (app, cx) = cx.add_window_view(|_window, cx| InviscidWindow::new(cx));
    cx.run_until_parked();

    let ws = cx.update(|_window, cx| app.read(cx).workspace().clone());
    cx.update(|window, cx| {
        ws.update(cx, |ws, cx| {
            ws.open_folder(dir.clone(), cx);
            ws.focus_active_editor(window, cx);
        });
    });
    cx.run_until_parked();

    // Toggle to Search
    cx.update(|_window, cx| {
        ws.update(cx, |ws, cx| {
            ws.show_sidebar(SidebarView::Search, cx);
        });
    });
    assert_eq!(
        ws.read_with(cx, |ws, _| ws.active_sidebar()),
        Some(SidebarView::Search)
    );

    // Toggle to Files
    cx.update(|_window, cx| {
        ws.update(cx, |ws, cx| {
            ws.show_sidebar(SidebarView::Files, cx);
        });
    });
    assert_eq!(
        ws.read_with(cx, |ws, _| ws.active_sidebar()),
        Some(SidebarView::Files)
    );

    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn test_workspace_toggle_sidebar_actions() {
    let mut cx = TestAppContext::single();
    init_test_globals(&mut cx);

    let (dir, _, _) = create_test_workspace("actions");
    let (app, cx) = cx.add_window_view(|_window, cx| InviscidWindow::new(cx));
    cx.run_until_parked();

    let ws = cx.update(|_window, cx| app.read(cx).workspace().clone());
    cx.update(|window, cx| {
        ws.update(cx, |ws, cx| {
            ws.open_folder(dir.clone(), cx);
            ws.focus_active_editor(window, cx);
        });
    });
    cx.run_until_parked();

    let active_sidebar =
        |cx: &mut gpui::VisualTestContext| ws.read_with(cx, |ws, _| ws.active_sidebar());

    // Initial state: visible, Files view
    assert_eq!(active_sidebar(cx), Some(SidebarView::Files));

    // ToggleSearchSidebar switches to Search
    cx.update(|window, cx| {
        window.dispatch_action(Box::new(inviscid::app::actions::ToggleSearchSidebar), cx);
    });
    cx.run_until_parked();
    assert_eq!(active_sidebar(cx), Some(SidebarView::Search));

    // ToggleSearchSidebar while on Search closes the sidebar and returns focus to the editor
    cx.update(|window, cx| {
        window.dispatch_action(Box::new(inviscid::app::actions::ToggleSearchSidebar), cx);
    });
    cx.run_until_parked();
    assert_eq!(active_sidebar(cx), None);
    cx.update(|window, cx| {
        let editor = ws.read(cx).active_editor().expect("active editor").clone();
        assert!(editor.read(cx).focus_handle(cx).is_focused(window));
    });

    // ToggleSidebar while closed opens in Files view
    cx.update(|window, cx| {
        window.dispatch_action(Box::new(inviscid::app::actions::ToggleSidebar), cx);
    });
    cx.run_until_parked();
    assert_eq!(active_sidebar(cx), Some(SidebarView::Files));

    // ToggleSidebar while on Files closes sidebar
    cx.update(|window, cx| {
        window.dispatch_action(Box::new(inviscid::app::actions::ToggleSidebar), cx);
    });
    cx.run_until_parked();
    assert_eq!(active_sidebar(cx), None);

    let _ = fs::remove_dir_all(&dir);
}
