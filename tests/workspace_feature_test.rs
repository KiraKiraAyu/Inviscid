use gpui::{Action, AppContext, Focusable, TestAppContext, px};
use std::fs;
use std::path::PathBuf;

use inviscid::app::InviscidWindow;
use inviscid::app::actions::{ClearRecentWorkspaces, CloseWorkspace, ToggleSidebar};
use inviscid::app::project_panel::ProjectPanel;
use inviscid::app::workspace::{
    DEFAULT_SIDEBAR_WIDTH, MAX_SIDEBAR_WIDTH, MIN_SIDEBAR_WIDTH, Workspace,
};
use inviscid::config::{AppConfig, WorkspaceState};
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

fn create_temp_workspace_dir(prefix: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "inviscid_test_ws_{}_{}",
        prefix,
        std::process::id()
    ));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(dir.join(".git")).unwrap();
    fs::write(dir.join(".git/HEAD"), "ref: refs/heads/main").unwrap();
    fs::write(dir.join(".hidden_file"), "secret").unwrap();

    fs::create_dir_all(dir.join("target/debug")).unwrap();
    fs::write(dir.join("target/debug/dummy"), "binary").unwrap();

    fs::create_dir_all(dir.join("docs")).unwrap();
    fs::write(dir.join("docs/intro.md"), "# Intro").unwrap();
    fs::write(dir.join("docs/architecture.md"), "# Architecture").unwrap();

    fs::create_dir_all(dir.join("src")).unwrap();
    fs::write(dir.join("src/main.rs"), "fn main() {}").unwrap();

    fs::write(dir.join("README.md"), "# My Project\nHello World").unwrap();
    fs::write(dir.join("z_notes.md"), "# Notes").unwrap();
    dir
}

#[test]
fn test_project_panel_directory_scanning_and_filtering() {
    let mut cx = TestAppContext::single();
    init_test_globals(&mut cx);

    let temp_dir = create_temp_workspace_dir("scan");

    let (_app, cx) = cx.add_window_view(|_window, cx| InviscidWindow::new(cx));
    cx.run_until_parked();

    let fh = cx.update(|_window, cx| cx.focus_handle());

    let panel = ProjectPanel::new(Some(temp_dir.clone()), fh);

    // Verify root_dir
    assert_eq!(panel.root_dir(), Some(temp_dir.as_path()));

    let entries = panel.entries();
    let names: Vec<&str> = entries.iter().map(|e| e.name.as_str()).collect();

    // 1. Dirs first (docs, src), then files (README.md, z_notes.md)
    assert_eq!(names, vec!["docs", "src", "README.md", "z_notes.md"]);

    // 2. Hidden entries (.git, .hidden_file) and target build dir MUST be filtered out
    assert!(!names.contains(&".git"));
    assert!(!names.contains(&".hidden_file"));
    assert!(!names.contains(&"target"));

    // 3. Initial depth of top-level items is 0, and subdirs are collapsed
    for entry in entries {
        assert_eq!(entry.depth, 0);
        if entry.is_dir() {
            assert!(!entry.is_expanded());
        }
    }

    let _ = fs::remove_dir_all(&temp_dir);
}

#[test]
fn test_project_panel_expand_collapse_and_reveal() {
    let mut cx = TestAppContext::single();
    init_test_globals(&mut cx);

    let temp_dir = create_temp_workspace_dir("expand");
    let (_app, cx) = cx.add_window_view(|_window, cx| InviscidWindow::new(cx));
    cx.run_until_parked();

    let panel_entity = cx.update(|_window, cx| {
        cx.new(|cx| ProjectPanel::new(Some(temp_dir.clone()), cx.focus_handle()))
    });

    // Expand 'docs'
    let docs_path = temp_dir.join("docs");
    cx.update(|_window, cx| {
        panel_entity.update(cx, |panel, cx| {
            panel.toggle_expand(&docs_path, cx);
        });
    });

    let entries_after_expand = cx.update(|_window, cx| {
        panel_entity
            .read(cx)
            .entries()
            .iter()
            .map(|e| (e.name.clone(), e.depth, e.is_dir()))
            .collect::<Vec<_>>()
    });

    // 'docs' should now be expanded, with intro.md and architecture.md listed under it at depth 1
    assert_eq!(
        entries_after_expand,
        vec![
            ("docs".to_string(), 0, true),
            ("architecture.md".to_string(), 1, false),
            ("intro.md".to_string(), 1, false),
            ("src".to_string(), 0, true),
            ("README.md".to_string(), 0, false),
            ("z_notes.md".to_string(), 0, false),
        ]
    );

    // Collapse All
    cx.update(|_window, cx| {
        panel_entity.update(cx, |panel, cx| {
            panel.collapse_all(cx);
        });
    });

    let entries_after_collapse = cx.update(|_window, cx| {
        panel_entity
            .read(cx)
            .entries()
            .iter()
            .map(|e| e.name.clone())
            .collect::<Vec<_>>()
    });
    assert_eq!(
        entries_after_collapse,
        vec!["docs", "src", "README.md", "z_notes.md"]
    );

    // Reveal path 'docs/intro.md'
    let intro_path = temp_dir.join("docs/intro.md");
    cx.update(|_window, cx| {
        panel_entity.update(cx, |panel, cx| {
            panel.reveal_path(&intro_path, cx);
        });
    });

    let is_docs_expanded = cx.update(|_window, cx| panel_entity.read(cx).is_expanded(&docs_path));
    assert!(is_docs_expanded, "reveal_path must expand ancestor folders");

    let _ = fs::remove_dir_all(&temp_dir);
}

#[test]
fn test_workspace_open_folder_and_auto_open_target() {
    let mut cx = TestAppContext::single();
    init_test_globals(&mut cx);

    let temp_dir = create_temp_workspace_dir("open_folder");
    let (app, cx) = cx.add_window_view(|_window, cx| InviscidWindow::new(cx));
    cx.run_until_parked();

    // Open workspace folder
    cx.update(|_window, cx| {
        app.update(cx, |app, cx| {
            app.workspace().update(cx, |ws, cx| {
                ws.open_folder(temp_dir.clone(), cx);
            });
        });
    });
    cx.run_until_parked();

    // Verify root_dir is set on workspace
    let ws_root = cx.update(|_window, cx| {
        app.read(cx)
            .workspace()
            .read(cx)
            .root_dir()
            .map(|p| p.to_path_buf())
    });
    assert_eq!(ws_root, Some(temp_dir.clone()));

    // Verify README.md was automatically opened as the active document
    let active_file = cx.update(|_window, cx| {
        app.read(cx)
            .workspace()
            .read(cx)
            .active_editor()
            .expect("active editor should exist")
            .read(cx)
            .file_path()
            .map(|p| p.to_path_buf())
    });
    assert_eq!(active_file, Some(temp_dir.join("README.md")));

    // Verify workspace_name reflects the folder name
    let ws_name = cx.update(|_window, cx| app.read(cx).workspace().read(cx).workspace_name(cx));
    let expected_name = temp_dir.file_name().unwrap().to_str().unwrap();
    assert_eq!(ws_name, expected_name);

    let _ = fs::remove_dir_all(&temp_dir);
}

#[test]
fn test_sidebar_toggle_action_and_visibility() {
    let mut cx = TestAppContext::single();
    init_test_globals(&mut cx);

    let (app, cx) = cx.add_window_view(|_window, cx| InviscidWindow::new(cx));
    cx.run_until_parked();

    // Open a tab so editor focus can be established
    cx.update(|_window, cx| {
        app.update(cx, |app, cx| {
            app.workspace().update(cx, |ws, cx| ws.new_tab(cx));
        });
    });
    cx.run_until_parked();

    // Sidebar should be visible by default
    let (initial_visible, sb_initial_visible) = cx.update(|_window, cx| {
        let ws = app.read(cx).workspace().read(cx);
        (
            ws.sidebar_visible(),
            ws.status_bar().read(cx).sidebar_visible(),
        )
    });
    assert!(initial_visible);
    assert!(sb_initial_visible);

    // Focus the editor so action propagation chain is active
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

    // Dispatch ToggleSidebar action
    cx.dispatch_action(ToggleSidebar);
    cx.run_until_parked();

    // Sidebar and status bar toggle button should now be hidden
    let (hidden, sb_hidden) = cx.update(|_window, cx| {
        let ws = app.read(cx).workspace().read(cx);
        (
            ws.sidebar_visible(),
            ws.status_bar().read(cx).sidebar_visible(),
        )
    });
    assert!(
        !hidden,
        "Sidebar should be hidden after ToggleSidebar action"
    );
    assert!(!sb_hidden, "StatusBar should sync hidden state");

    // Dispatch ToggleSidebar action again
    cx.dispatch_action(ToggleSidebar);
    cx.run_until_parked();

    // Sidebar should be visible again
    let (shown_again, sb_shown_again) = cx.update(|_window, cx| {
        let ws = app.read(cx).workspace().read(cx);
        (
            ws.sidebar_visible(),
            ws.status_bar().read(cx).sidebar_visible(),
        )
    });
    assert!(
        shown_again,
        "Sidebar should be visible again after second ToggleSidebar action"
    );
    assert!(sb_shown_again, "StatusBar should sync visible state");
}

#[test]
fn test_workspace_state_persistence_roundtrip() {
    let mut cx = TestAppContext::single();
    init_test_globals(&mut cx);

    let temp_dir = create_temp_workspace_dir("persist");
    let (app, cx) = cx.add_window_view(|_window, cx| InviscidWindow::new(cx));
    cx.run_until_parked();

    // Open folder and hide sidebar
    cx.update(|_window, cx| {
        app.update(cx, |app, cx| {
            app.workspace().update(cx, |ws, cx| {
                ws.open_folder(temp_dir.clone(), cx);
                ws.set_sidebar_visible(false, cx);
            });
        });
    });
    cx.run_until_parked();

    // Sync to config
    let mut config = AppConfig::default();
    cx.update(|_window, cx| {
        app.read(cx)
            .workspace()
            .read(cx)
            .sync_workspace_state(&mut config, cx);
    });

    let saved_state = config
        .session
        .last_workspace
        .clone()
        .expect("last_workspace should be saved");
    assert_eq!(saved_state.root_dir, Some(temp_dir.clone()));
    assert!(!saved_state.sidebar_visible);
    assert_eq!(saved_state.active_file, Some(temp_dir.join("README.md")));

    // Serialize and deserialize TOML
    let toml_str = toml::to_string_pretty(&saved_state).unwrap();
    let restored_state: WorkspaceState = toml::from_str(&toml_str).unwrap();
    assert_eq!(restored_state, saved_state);

    // Restore into a new workspace
    cx.update(|_window, cx| {
        let restored_ws_entity =
            cx.new(|cx| inviscid::app::workspace::Workspace::from_state(&restored_state, cx));
        assert_eq!(
            restored_ws_entity.read(cx).root_dir(),
            Some(temp_dir.as_path())
        );
        assert!(!restored_ws_entity.read(cx).sidebar_visible());
    });

    let _ = fs::remove_dir_all(&temp_dir);
}

#[test]
fn test_project_panel_nested_subfolder_and_file_activation() {
    let mut cx = TestAppContext::single();
    init_test_globals(&mut cx);

    // Create a directory structure similar to the user's case:
    // root/
    //   sub/
    //     index.md
    //   index.md
    let dir = std::env::temp_dir().join(format!("inviscid_test_nested_{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(dir.join("sub")).unwrap();
    fs::write(dir.join("sub/index.md"), "# Sub Index").unwrap();
    fs::write(dir.join("index.md"), "# Root Index").unwrap();

    // 1. Test initializing Workspace directly with a directory path (CLI scenario)
    let (app, cx) =
        cx.add_window_view(|_window, cx| InviscidWindow::new_with_file(dir.clone(), cx));
    cx.run_until_parked();

    cx.update(|_window, cx| {
        let ws = app.read(cx).workspace().read(cx);
        // root_dir MUST be the directory itself, not its parent!
        assert_eq!(ws.root_dir(), Some(dir.as_path()));
        // Active file should be index.md (since no README.md exists)
        assert_eq!(
            ws.active_file_path(cx),
            Some(dir.join("index.md").as_path())
        );
    });

    // 2. Test ProjectPanel entries before and after expanding 'sub'
    let panel_entity =
        cx.update(|_window, cx| app.read(cx).workspace().read(cx).project_panel().clone());

    cx.update(|_window, cx| {
        let panel = panel_entity.read(cx);
        let names: Vec<&str> = panel.entries().iter().map(|e| e.name.as_str()).collect();
        // Dirs first ("sub"), then root files ("index.md")
        assert_eq!(names, vec!["sub", "index.md"]);
    });

    // Expand 'sub' directory
    cx.update(|_window, cx| {
        panel_entity.update(cx, |panel, cx| {
            panel.toggle_expand(&dir.join("sub"), cx);
        });
    });
    cx.run_until_parked();

    cx.update(|_window, cx| {
        let panel = panel_entity.read(cx);
        let entries: Vec<(&str, usize)> = panel
            .entries()
            .iter()
            .map(|e| (e.name.as_str(), e.depth))
            .collect();
        // Should have "sub" (depth 0), "index.md" (depth 1), "index.md" (depth 0)
        assert_eq!(entries, vec![("sub", 0), ("index.md", 1), ("index.md", 0),]);
    });

    // 3. Open sub/index.md - must succeed without double-lease panic!
    cx.update(|_window, cx| {
        app.update(cx, |app, cx| {
            app.workspace().update(cx, |ws, cx| {
                ws.open_file(dir.join("sub/index.md"), cx);
            });
        });
    });
    cx.run_until_parked();

    cx.update(|_window, cx| {
        let ws = app.read(cx).workspace().read(cx);
        assert_eq!(
            ws.active_file_path(cx),
            Some(dir.join("sub/index.md").as_path())
        );
        assert_eq!(ws.tab_count(), 2);
    });

    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn test_empty_workspace_zero_tabs_lifecycle() {
    let mut cx = TestAppContext::single();
    init_test_globals(&mut cx);

    let (app, cx) = cx.add_window_view(|window, cx| {
        let app = InviscidWindow::new(cx);
        app.workspace().read(cx).focus_active_editor(window, cx);
        app
    });
    cx.run_until_parked();

    // 1. Initial state: 0 tabs, no active editor, workspace is Open Workspace
    cx.update(|_window, cx| {
        let ws = app.read(cx).workspace().read(cx);
        assert_eq!(ws.tab_count(), 0);
        assert!(ws.active_editor().is_none());
        assert_eq!(ws.workspace_name(cx), "Open Workspace");
        assert!(!ws.has_workspace(cx));
    });

    // Try dispatching NewTab action via window when 0 tabs are open
    cx.update(|window, cx| {
        window.dispatch_action(inviscid::app::actions::NewTab.boxed_clone(), cx);
    });
    cx.run_until_parked();

    let tab_count_after_dispatch =
        cx.update(|_window, cx| app.read(cx).workspace().read(cx).tab_count());
    assert_eq!(
        tab_count_after_dispatch, 1,
        "Dispatching NewTab via window must work in 0-tab state"
    );

    // 2. Dispatched NewTab action created tab 0
    cx.update(|_window, cx| {
        let ws = app.read(cx).workspace().read(cx);
        assert_eq!(ws.tab_count(), 1);
        let editor = ws.active_editor().expect("tab 0 must exist");
        assert_eq!(editor.read(cx).title(), "Untitled.md");
        assert_eq!(editor.read(cx).file_path(), None);
    });

    // 3. Close the tab via close_active_tab -> tabs should become 0 and focus returns to workspace
    cx.update(|window, cx| {
        app.update(cx, |app, cx| {
            inviscid::app::commands::close_active_tab(app, window, cx);
        });
    });
    cx.run_until_parked();

    cx.update(|_window, cx| {
        let ws = app.read(cx).workspace().read(cx);
        assert_eq!(ws.tab_count(), 0);
        assert!(ws.active_editor().is_none());
    });

    // 4. Dispatch NewTab action AGAIN in 0-tab state -> must succeed again
    cx.update(|window, cx| {
        window.dispatch_action(inviscid::app::actions::NewTab.boxed_clone(), cx);
    });
    cx.run_until_parked();

    cx.update(|_window, cx| {
        let ws = app.read(cx).workspace().read(cx);
        assert_eq!(
            ws.tab_count(),
            1,
            "Second NewTab dispatch after closing all tabs must also work"
        );
    });
}

#[test]
fn test_open_empty_folder_without_md_files() {
    let mut cx = TestAppContext::single();
    init_test_globals(&mut cx);

    let empty_dir =
        std::env::temp_dir().join(format!("inviscid_test_empty_folder_{}", std::process::id()));
    let _ = fs::remove_dir_all(&empty_dir);
    fs::create_dir_all(&empty_dir).unwrap();

    let (app, cx) = cx.add_window_view(|_window, cx| InviscidWindow::new(cx));
    cx.run_until_parked();

    // Open the empty directory
    cx.update(|_window, cx| {
        app.update(cx, |app, cx| {
            app.workspace().update(cx, |ws, cx| {
                ws.open_folder(empty_dir.clone(), cx);
            });
        });
    });
    cx.run_until_parked();

    // Verify root_dir is set, but tab_count is 0 (no md file in folder)
    cx.update(|_window, cx| {
        let ws = app.read(cx).workspace().read(cx);
        assert_eq!(ws.root_dir(), Some(empty_dir.as_path()));
        assert_eq!(ws.tab_count(), 0);
        assert!(ws.active_editor().is_none());
        assert_eq!(
            ws.workspace_name(cx),
            empty_dir.file_name().unwrap().to_str().unwrap()
        );
    });

    // Now create a file in the folder and open it
    let doc_path = empty_dir.join("test.md");
    fs::write(&doc_path, "# Content").unwrap();

    cx.update(|_window, cx| {
        app.update(cx, |app, cx| {
            app.workspace().update(cx, |ws, cx| {
                ws.open_file(doc_path.clone(), cx);
            });
        });
    });
    cx.run_until_parked();

    cx.update(|_window, cx| {
        let ws = app.read(cx).workspace().read(cx);
        assert_eq!(ws.tab_count(), 1);
        assert_eq!(ws.active_file_path(cx), Some(doc_path.as_path()));
    });

    // Close that tab -> tabs returns to 0, workspace root remains
    cx.update(|_window, cx| {
        app.update(cx, |app, cx| {
            app.workspace().update(cx, |ws, cx| {
                ws.close_tab(0, cx);
            });
        });
    });
    cx.run_until_parked();

    cx.update(|_window, cx| {
        let ws = app.read(cx).workspace().read(cx);
        assert_eq!(ws.tab_count(), 0);
        assert!(ws.active_editor().is_none());
        assert_eq!(ws.root_dir(), Some(empty_dir.as_path()));
    });

    let _ = fs::remove_dir_all(&empty_dir);
}

#[test]
fn test_close_workspace_action_and_lifecycle() {
    let mut cx = TestAppContext::single();
    init_test_globals(&mut cx);

    let temp_dir = create_temp_workspace_dir("close_ws");
    let doc_path = temp_dir.join("README.md");
    fs::write(&doc_path, "# Hello Workspace").unwrap();

    let (app, cx) = cx.add_window_view(|_window, cx| InviscidWindow::new(cx));
    cx.run_until_parked();

    // Open folder in workspace
    cx.update(|_window, cx| {
        app.update(cx, |app, cx| {
            app.workspace().update(cx, |ws, cx| {
                ws.open_folder(temp_dir.clone(), cx);
            });
        });
    });
    cx.run_until_parked();

    // Verify workspace is opened with README.md in tab
    cx.update(|window, cx| {
        let ws = app.read(cx).workspace().read(cx);
        assert_eq!(ws.root_dir(), Some(temp_dir.as_path()));
        assert_eq!(ws.tab_count(), 1);
        assert_eq!(ws.active_file_path(cx), Some(doc_path.as_path()));
        assert_eq!(
            ws.project_panel().read(cx).root_dir(),
            Some(temp_dir.as_path())
        );

        ws.focus_active_editor(window, cx);
    });
    cx.run_until_parked();

    // Dispatch CloseWorkspace action
    cx.update(|window, cx| {
        window.dispatch_action(CloseWorkspace.boxed_clone(), cx);
    });
    cx.run_until_parked();

    // Verify workspace is completely closed: root_dir is None, tabs cleared, project panel empty
    cx.update(|_window, cx| {
        let ws = app.read(cx).workspace().read(cx);
        assert!(ws.root_dir().is_none());
        assert_eq!(ws.tab_count(), 0);
        assert!(ws.active_editor().is_none());
        assert!(ws.project_panel().read(cx).root_dir().is_none());
        assert_eq!(ws.workspace_name(cx), "Open Workspace");
        assert!(!ws.has_workspace(cx));

        // Verify config session state is cleared
        let config = cx.global::<AppConfig>();
        assert!(config.last_workspace().is_none());
    });

    let _ = fs::remove_dir_all(&temp_dir);
}

#[test]
fn test_clear_recent_workspaces_action() {
    let mut cx = TestAppContext::single();
    init_test_globals(&mut cx);

    // Pre-populate recent workspaces in config
    cx.update(|cx| {
        let mut config = cx.global::<AppConfig>().clone();
        config.session.recent_workspaces = vec![
            WorkspaceState {
                root_dir: Some(PathBuf::from("/project_1")),
                active_file: None,
                open_files: Vec::new(),
                sidebar_visible: true,
                sidebar_width: None,
            },
            WorkspaceState {
                root_dir: Some(PathBuf::from("/project_2")),
                active_file: None,
                open_files: Vec::new(),
                sidebar_visible: true,
                sidebar_width: None,
            },
        ];
        cx.set_global(config);
        assert_eq!(cx.global::<AppConfig>().recent_workspaces().len(), 2);
    });

    let (app, cx) = cx.add_window_view(|_window, cx| InviscidWindow::new(cx));
    cx.run_until_parked();

    // Focus workspace and dispatch ClearRecentWorkspaces action
    cx.update(|window, cx| {
        app.read(cx)
            .workspace()
            .read(cx)
            .focus_active_editor(window, cx);
        window.dispatch_action(ClearRecentWorkspaces.boxed_clone(), cx);
    });
    cx.run_until_parked();

    // Verify recent_workspaces is cleared
    cx.update(|_window, cx| {
        let config = cx.global::<AppConfig>();
        assert!(config.recent_workspaces().is_empty());
    });
}

#[test]
fn test_restore_recent_workspace_closes_dropdown_menu() {
    let mut cx = TestAppContext::single();
    init_test_globals(&mut cx);

    let temp_dir = create_temp_workspace_dir("recent_menu");
    let doc_path = temp_dir.join("README.md");
    fs::write(&doc_path, "# Recent Workspace Doc").unwrap();

    let target_state = WorkspaceState {
        root_dir: Some(temp_dir.clone()),
        active_file: Some(doc_path.clone()),
        open_files: vec![doc_path.clone()],
        sidebar_visible: true,
        sidebar_width: None,
    };

    cx.update(|cx| {
        let mut config = cx.global::<AppConfig>().clone();
        config.session.recent_workspaces = vec![target_state.clone()];
        cx.set_global(config);
    });

    let (app, cx) = cx.add_window_view(|_window, cx| InviscidWindow::new(cx));
    cx.run_until_parked();

    // 1. Open Workspace Menu
    cx.update(|_window, cx| {
        app.update(cx, |app, cx| {
            app.set_active_menu(Some(inviscid::app::ActiveMenu::Workspace), cx);
        });
    });
    cx.run_until_parked();

    assert_eq!(
        cx.update(|_window, cx| app.read(cx).active_menu()),
        Some(inviscid::app::ActiveMenu::Workspace)
    );

    // 2. Select recent workspace item through TitleBar dropdown handler:
    // closes titlebar menu, restores workspace state, and focuses active editor
    cx.update(|window, cx| {
        let titlebar = app.read(cx).titlebar().clone();
        let ws = app.read(cx).workspace().clone();

        titlebar.update(cx, |tb, cx| {
            tb.close_menu(cx);
        });
        ws.update(cx, |ws, cx| {
            ws.restore_from_state(&target_state, cx);
            ws.focus_active_editor(window, cx);
        });
    });
    cx.run_until_parked();

    // Verify active_menu MUST be collapsed (None)
    let active_menu = cx.update(|_window, cx| app.read(cx).active_menu());
    assert_eq!(
        active_menu, None,
        "Active menu should be closed after selecting a recent workspace"
    );

    // Verify workspace state was restored
    cx.update(|_window, cx| {
        let ws = app.read(cx).workspace().read(cx);
        assert_eq!(ws.root_dir(), Some(temp_dir.as_path()));
        assert_eq!(ws.tab_count(), 1);
        assert_eq!(ws.active_file_path(cx), Some(doc_path.as_path()));
    });

    let _ = fs::remove_dir_all(&temp_dir);
}

#[test]
fn test_project_panel_context_menu_open_and_close() {
    let mut cx = TestAppContext::single();
    init_test_globals(&mut cx);

    let temp_dir = create_temp_workspace_dir("cm_open_close");
    let (app, cx) = cx.add_window_view(|_window, cx| InviscidWindow::new(cx));
    cx.run_until_parked();

    let (panel_entity, _ws_entity) = cx.update(|_window, cx| {
        let ws = app.read(cx).workspace().clone();
        let panel = cx.new(|cx| ProjectPanel::new(Some(temp_dir.clone()), cx.focus_handle()));
        (panel, ws)
    });

    let target_file = temp_dir.join("README.md");

    // 1. Open context menu on target file
    cx.update(|_window, cx| {
        panel_entity.update(cx, |panel, cx| {
            panel.open_context_menu(
                gpui::point(gpui::px(100.0), gpui::px(150.0)),
                target_file.clone(),
                false,
                false,
                cx,
            );
        });
    });

    cx.update(|_window, cx| {
        let panel = panel_entity.read(cx);
        assert_eq!(panel.selected_path(), Some(target_file.as_path()));
        let cm = panel
            .active_context_menu()
            .expect("context menu should be active");
        assert_eq!(cm.target_path, target_file);
        assert!(!cm.is_dir);
        assert!(!cm.is_root);
        assert_eq!(cm.position, gpui::point(gpui::px(100.0), gpui::px(150.0)));
    });

    // 2. Close context menu
    cx.update(|_window, cx| {
        panel_entity.update(cx, |panel, cx| {
            panel.close_context_menu(cx);
        });
    });

    cx.update(|_window, cx| {
        let panel = panel_entity.read(cx);
        assert!(panel.active_context_menu().is_none());
    });

    // 3. Open on root/background
    cx.update(|_window, cx| {
        panel_entity.update(cx, |panel, cx| {
            panel.open_context_menu(
                gpui::point(gpui::px(50.0), gpui::px(50.0)),
                temp_dir.clone(),
                true,
                true,
                cx,
            );
        });
    });

    cx.update(|_window, cx| {
        let panel = panel_entity.read(cx);
        let cm = panel
            .active_context_menu()
            .expect("context menu should be active");
        assert_eq!(cm.target_path, temp_dir);
        assert!(cm.is_dir);
        assert!(cm.is_root);
    });

    let _ = fs::remove_dir_all(&temp_dir);
}

#[test]
fn test_project_panel_context_menu_positioning_alignment() {
    use gpui::{Bounds, Point, point, px, size};
    use inviscid::ui::{calculate_menu_height, compute_context_menu_coords};

    let titlebar_h = inviscid::app::TitleBar::HEIGHT;
    let statusbar_h = inviscid::app::status_bar::StatusBar::HEIGHT;
    let safe_bounds = Bounds {
        origin: point(px(0.0), titlebar_h),
        size: size(px(1000.0), px(800.0) - titlebar_h - statusbar_h),
    };
    let menu_h = calculate_menu_height(12, 4);
    assert_eq!(menu_h, px(374.0));

    // 1. Upper area click (y = 150px): top-left corner matches click
    let upper_click = Point {
        x: px(80.0),
        y: px(150.0),
    };
    let upper_coords = compute_context_menu_coords(upper_click, safe_bounds, px(0.0), menu_h);
    assert_eq!(upper_coords.x, px(80.0));
    // In ProjectPanel coords, top is click_y - titlebar_h
    assert_eq!(upper_coords.y, px(150.0) - titlebar_h);
    // In window coords, menu top is titlebar_h + upper_coords.y == 150px
    assert_eq!(titlebar_h + upper_coords.y, upper_click.y);

    // 2. Lower area click (y = 700px): bottom-left corner matches click!
    let lower_click = Point {
        x: px(80.0),
        y: px(700.0),
    };
    let lower_coords = compute_context_menu_coords(lower_click, safe_bounds, px(0.0), menu_h);
    assert_eq!(lower_coords.x, px(80.0));
    // In window coords, menu bottom is titlebar_h + lower_coords.y + menu_h == 700px!
    let menu_bottom_window = titlebar_h + lower_coords.y + menu_h;
    assert_eq!(menu_bottom_window, lower_click.y);

    // 3. Dynamic height test: arbitrary menu height also flips pixel-perfectly without jumping
    let custom_h = px(250.0);
    let custom_lower_coords =
        compute_context_menu_coords(lower_click, safe_bounds, px(0.0), custom_h);
    assert_eq!(titlebar_h + custom_lower_coords.y + custom_h, lower_click.y);
}

#[test]
fn test_project_panel_fs_ops_duplicate_and_naming() {
    let temp_dir = create_temp_workspace_dir("cm_dup");
    let file_path = temp_dir.join("README.md");

    // Duplicate 1: README copy.md
    let dup1 = inviscid::app::project_panel::fs_ops::duplicate_entry(&file_path).unwrap();
    assert_eq!(dup1, temp_dir.join("README copy.md"));
    assert!(dup1.exists());
    assert_eq!(
        fs::read_to_string(&dup1).unwrap(),
        fs::read_to_string(&file_path).unwrap()
    );

    // Duplicate 2: README copy 2.md
    let dup2 = inviscid::app::project_panel::fs_ops::duplicate_entry(&file_path).unwrap();
    assert_eq!(dup2, temp_dir.join("README copy 2.md"));
    assert!(dup2.exists());

    // Duplicate directory: docs copy
    let docs_dir = temp_dir.join("docs");
    let dup_dir = inviscid::app::project_panel::fs_ops::duplicate_entry(&docs_dir).unwrap();
    assert_eq!(dup_dir, temp_dir.join("docs copy"));
    assert!(dup_dir.is_dir());
    assert!(dup_dir.join("intro.md").exists());

    let _ = fs::remove_dir_all(&temp_dir);
}

#[test]
fn test_project_panel_fs_ops_clipboard_copy_cut_paste() {
    let temp_dir = create_temp_workspace_dir("cm_paste");
    let src_file = temp_dir.join("README.md");
    let target_dir = temp_dir.join("docs");

    // 1. Copy & Paste into docs/
    let pasted =
        inviscid::app::project_panel::fs_ops::perform_paste(&src_file, false, &target_dir).unwrap();
    assert_eq!(pasted, target_dir.join("README.md"));
    assert!(pasted.exists());
    assert!(src_file.exists()); // Original still exists on copy

    // 2. Cut & Paste z_notes.md into docs/
    let notes_file = temp_dir.join("z_notes.md");
    let pasted_cut =
        inviscid::app::project_panel::fs_ops::perform_paste(&notes_file, true, &target_dir)
            .unwrap();
    assert_eq!(pasted_cut, target_dir.join("z_notes.md"));
    assert!(pasted_cut.exists());
    assert!(!notes_file.exists()); // Original moved on cut

    let _ = fs::remove_dir_all(&temp_dir);
}

#[test]
fn test_project_panel_delete_and_workspace_tab_sync() {
    let mut cx = TestAppContext::single();
    init_test_globals(&mut cx);

    let temp_dir = create_temp_workspace_dir("cm_del");
    let doc_file = temp_dir.join("docs").join("intro.md");

    let (app, cx) = cx.add_window_view(|_window, cx| InviscidWindow::new(cx));
    cx.run_until_parked();

    let ws = cx.update(|_window, cx| app.read(cx).workspace().clone());

    // Open intro.md in workspace
    cx.update(|_window, cx| {
        ws.update(cx, |ws, cx| {
            ws.open_file(doc_file.clone(), cx);
        });
    });
    cx.run_until_parked();

    // Verify tab count is 1
    assert_eq!(cx.update(|_window, cx| ws.read(cx).tab_count()), 1);

    // Delete intro.md via fs_ops and notify workspace
    inviscid::app::project_panel::fs_ops::delete_entry(&doc_file, false).unwrap();
    assert!(!doc_file.exists());

    cx.update(|_window, cx| {
        ws.update(cx, |ws, cx| {
            ws.handle_path_deleted(&doc_file, cx);
        });
    });
    cx.run_until_parked();

    // Verify tab was automatically closed!
    assert_eq!(cx.update(|_window, cx| ws.read(cx).tab_count()), 0);

    let _ = fs::remove_dir_all(&temp_dir);
}

#[test]
fn test_project_panel_inline_input_new_file_and_rename() {
    let mut cx = TestAppContext::single();
    init_test_globals(&mut cx);

    let temp_dir = create_temp_workspace_dir("cm_inline");
    let (app, cx) = cx.add_window_view(|_window, cx| InviscidWindow::new(cx));
    cx.run_until_parked();

    let (panel_entity, ws_entity) = cx.update(|_window, cx| {
        let ws = app.read(cx).workspace().clone();
        ws.update(cx, |ws, cx| {
            ws.open_folder(temp_dir.clone(), cx);
        });
        let panel = ws.read(cx).project_panel().clone();
        (panel, ws)
    });
    cx.run_until_parked();

    // 1. Create a new file via inline input
    cx.update(|window, cx| {
        panel_entity.update(cx, |panel, cx| {
            panel.open_context_menu(
                gpui::point(gpui::px(0.0), gpui::px(0.0)),
                temp_dir.clone(),
                true,
                true,
                cx,
            );
            panel.execute_context_menu_action(
                inviscid::app::project_panel::context_menu::ContextMenuAction::NewFile,
                window,
                cx,
            );
        });
    });

    cx.update(|_window, cx| {
        let panel = panel_entity.read(cx);
        let input_entity = panel.inline_input().expect("inline input should be active");
        let input = input_entity.read(cx);
        assert_eq!(input.text(), "");
        assert_eq!(
            input.kind(),
            &inviscid::app::project_panel::InlineInputKind::NewFile {
                parent: temp_dir.clone()
            }
        );
    });

    // Type "changelog.md" and commit
    cx.update(|_window, cx| {
        panel_entity.update(cx, |panel, cx| {
            let input = panel
                .inline_input()
                .cloned()
                .expect("inline input should be open");
            input.update(cx, |inp, _cx| {
                inp.buffer_mut().insert_text("changelog.md");
            });
            // Press Enter. The box is deeper on the dispatch path than the panel and stops
            // propagation, so this is its key to handle — the panel never sees it.
            input.update(cx, |inp, cx| {
                inp.handle_key_down(
                    &gpui::KeyDownEvent {
                        keystroke: gpui::Keystroke {
                            modifiers: gpui::Modifiers::default(),
                            key: "enter".to_string(),
                            key_char: None,
                        },
                        is_held: false,
                    },
                    cx,
                );
            });
        });
    });
    cx.run_until_parked();

    // Verify file created on disk and opened in workspace
    let created_file = temp_dir.join("changelog.md");
    assert!(created_file.exists());
    assert_eq!(
        cx.update(|_window, cx| ws_entity
            .read(cx)
            .active_file_path(cx)
            .map(|p| p.to_path_buf())),
        Some(created_file.clone())
    );

    // 2. Rename changelog.md -> history.md
    cx.update(|window, cx| {
        panel_entity.update(cx, |panel, cx| {
            panel.open_context_menu(
                gpui::point(gpui::px(0.0), gpui::px(0.0)),
                created_file.clone(),
                false,
                false,
                cx,
            );
            panel.execute_context_menu_action(
                inviscid::app::project_panel::context_menu::ContextMenuAction::Rename,
                window,
                cx,
            );
        });
    });

    cx.update(|_window, cx| {
        panel_entity.update(cx, |panel, cx| {
            // Cancel with Escape, again through the box that owns the key.
            let input = panel
                .inline_input()
                .cloned()
                .expect("inline input should be open");
            input.update(cx, |inp, cx| {
                inp.handle_key_down(
                    &gpui::KeyDownEvent {
                        keystroke: gpui::Keystroke {
                            modifiers: gpui::Modifiers::default(),
                            key: "escape".to_string(),
                            key_char: None,
                        },
                        is_held: false,
                    },
                    cx,
                );
            });
        });
    });

    // Verify cancelled (still changelog.md)
    assert!(created_file.exists());
    assert!(panel_entity.read_with(cx, |p, _| p.inline_input().is_none()));

    let _ = fs::remove_dir_all(&temp_dir);
}

#[gpui::test]
fn test_inline_input_rich_navigation_and_ime(cx: &mut TestAppContext) {
    use gpui::EntityInputHandler;
    use inviscid::app::project_panel::{InlineInput, InlineInputKind};

    init_test_globals(cx);

    let (_, cx) = cx.add_window_view(|_window, cx| {
        let dummy_path = PathBuf::from("workspace/dummy");
        InlineInput::new(InlineInputKind::NewFolder { parent: dummy_path }, "", 1, cx)
    });

    // Test 1: Initial stem selection for Rename ("main.rs" -> selects "main" 0..4)
    cx.update(|window, cx| {
        let dummy_path = PathBuf::from("workspace/src/app/main.rs");
        let input_entity = cx.new(|cx| {
            InlineInput::new(
                InlineInputKind::Rename {
                    target: dummy_path,
                    is_dir: false,
                },
                "main.rs",
                1,
                cx,
            )
        });

        input_entity.update(cx, |this, cx| {
            assert_eq!(this.text(), "main.rs");
            let sel = this
                .selected_text_range(false, window, cx)
                .expect("selection expected");
            assert_eq!(sel.range, 0..4);
            let mut adjusted = None;
            assert_eq!(
                this.text_for_range(sel.range, &mut adjusted, window, cx),
                Some("main".to_string())
            );
        });
    });

    // Test 2: Folder rename selects whole name ("my_folder" -> 0..9)
    cx.update(|window, cx| {
        let dummy_path = PathBuf::from("workspace/my_folder");
        let input_entity = cx.new(|cx| {
            InlineInput::new(
                InlineInputKind::Rename {
                    target: dummy_path,
                    is_dir: true,
                },
                "my_folder",
                1,
                cx,
            )
        });

        input_entity.update(cx, |this, cx| {
            assert_eq!(this.text(), "my_folder");
            let sel = this
                .selected_text_range(false, window, cx)
                .expect("selection expected");
            assert_eq!(sel.range, 0..9);
        });
    });

    // Test 3: Dotfile rename selects whole name (".gitignore" -> 0..10)
    cx.update(|window, cx| {
        let dummy_path = PathBuf::from("workspace/.gitignore");
        let input_entity = cx.new(|cx| {
            InlineInput::new(
                InlineInputKind::Rename {
                    target: dummy_path,
                    is_dir: false,
                },
                ".gitignore",
                1,
                cx,
            )
        });

        input_entity.update(cx, |this, cx| {
            assert_eq!(this.text(), ".gitignore");
            let sel = this
                .selected_text_range(false, window, cx)
                .expect("selection expected");
            assert_eq!(sel.range, 0..10);
        });
    });

    // Test 4: IME Composition & Replacement workflow
    cx.update(|window, cx| {
        let parent = PathBuf::from("workspace");
        let input_entity =
            cx.new(|cx| InlineInput::new(InlineInputKind::NewFile { parent }, "", 1, cx));

        input_entity.update(cx, |this, cx| {
            // IME typing composition: "wenda"
            this.replace_and_mark_text_in_range(Some(0..0), "wenda", Some(0..5), window, cx);
            assert_eq!(this.text(), "wenda");
            assert_eq!(this.marked_text_range(window, cx), Some(0..5));

            // Candidate chosen: replace composition with "文档"
            this.replace_text_in_range(Some(0..5), "文档", window, cx);
            this.unmark_text(window, cx);
            assert_eq!(this.text(), "文档");
            assert_eq!(this.marked_text_range(window, cx), None);

            // Append ".md" via replace_text_in_range at end
            let end_utf16 = this
                .buffer()
                .pos_to_utf16_offset(this.buffer().cursor_pos());
            this.replace_text_in_range(Some(end_utf16..end_utf16), ".md", window, cx);
            assert_eq!(this.text(), "文档.md");

            // Verify text_for_range
            let mut adjusted = None;
            assert_eq!(
                this.text_for_range(0..2, &mut adjusted, window, cx),
                Some("文档".to_string())
            );
            let mut adjusted2 = None;
            assert_eq!(
                this.text_for_range(2..5, &mut adjusted2, window, cx),
                Some(".md".to_string())
            );
        });
    });

    // Test 5: Keyboard navigation & buffer operations (Left, Right, SelectAll, Delete, Backspace)
    cx.update(|window, cx| {
        let parent = PathBuf::from("workspace");
        let input_entity =
            cx.new(|cx| InlineInput::new(InlineInputKind::NewFile { parent }, "test.rs", 1, cx));

        input_entity.update(cx, |this, cx| {
            // Cursor is at end of text for NewFile
            assert_eq!(this.buffer().cursor_pos().col, 7);

            // Move left 3 times (over ".rs")
            this.buffer_mut().move_left_mode(false, false);
            this.buffer_mut().move_left_mode(false, false);
            this.buffer_mut().move_left_mode(false, false);
            assert_eq!(this.buffer().cursor_pos().col, 4);

            // Insert "_mod"
            this.buffer_mut().insert_text("_mod");
            assert_eq!(this.text(), "test_mod.rs");

            // Select all
            this.buffer_mut().select_all();
            let sel = this
                .selected_text_range(false, window, cx)
                .expect("selection expected");
            assert_eq!(sel.range, 0..11);

            // Type over selection
            this.replace_text_in_range(Some(0..11), "lib.rs", window, cx);
            assert_eq!(this.text(), "lib.rs");

            // Backspace deletes one character before cursor
            this.buffer_mut().backspace_mode(false);
            assert_eq!(this.text(), "lib.r");

            // Move to start, forward delete
            this.buffer_mut().move_to_line_start_mode(false, false);
            assert_eq!(this.buffer().cursor_pos().col, 0);
            this.buffer_mut().delete_forward_mode(false);
            assert_eq!(this.text(), "ib.r");

            // KeyDownEvent Backspace handling
            this.buffer_mut().move_to_line_end(false);
            this.handle_key_down(
                &gpui::KeyDownEvent {
                    keystroke: gpui::Keystroke::parse("backspace").unwrap(),
                    is_held: false,
                },
                cx,
            );
            assert_eq!(this.text(), "ib.");

            // KeyDownEvent Delete handling
            this.buffer_mut().move_to_line_start_mode(false, false);
            this.handle_key_down(
                &gpui::KeyDownEvent {
                    keystroke: gpui::Keystroke::parse("delete").unwrap(),
                    is_held: false,
                },
                cx,
            );
            assert_eq!(this.text(), "b.");
        });
    });
}

#[gpui::test]
fn test_inline_input_new_file_activates_inline_input(cx: &mut TestAppContext) {
    init_test_globals(cx);
    let temp_dir = create_temp_workspace_dir("new_file_pos");

    let (_app, cx) = cx.add_window_view(|_window, cx| InviscidWindow::new(cx));
    cx.run_until_parked();

    let panel_entity = cx.update(|_window, cx| {
        cx.new(|cx| ProjectPanel::new(Some(temp_dir.clone()), cx.focus_handle()))
    });

    // Expand "docs" directory
    cx.update(|_window, cx| {
        panel_entity.update(cx, |panel, cx| {
            panel.toggle_expand(&temp_dir.join("docs"), cx);
        });
    });
    cx.run_until_parked();

    // Trigger New File at workspace root
    cx.update(|window, cx| {
        panel_entity.update(cx, |panel, cx| {
            panel.open_context_menu(
                gpui::point(gpui::px(0.0), gpui::px(0.0)),
                temp_dir.clone(),
                true,
                true,
                cx,
            );
            panel.execute_context_menu_action(
                inviscid::app::project_panel::context_menu::ContextMenuAction::NewFile,
                window,
                cx,
            );
        });
    });

    // Verify inline_input is active
    cx.update(|_window, cx| {
        let panel = panel_entity.read(cx);
        assert!(panel.inline_input().is_some());
    });

    let _ = fs::remove_dir_all(&temp_dir);
}

#[gpui::test]
fn test_inline_input_blur_commit_and_cancel(cx: &mut TestAppContext) {
    init_test_globals(cx);
    let temp_dir = create_temp_workspace_dir("blur_test");

    let (app, cx) = cx.add_window_view(|_window, cx| InviscidWindow::new(cx));
    cx.run_until_parked();

    let (panel_entity, ws_entity) = cx.update(|_window, cx| {
        let ws = app.read(cx).workspace().clone();
        ws.update(cx, |ws, cx| {
            ws.open_folder(temp_dir.clone(), cx);
        });
        let panel = ws.read(cx).project_panel().clone();
        (panel, ws)
    });
    cx.run_until_parked();

    // 1. Trigger New File, blur without text -> should cancel
    cx.update(|window, cx| {
        panel_entity.update(cx, |panel, cx| {
            panel.open_context_menu(
                gpui::point(gpui::px(0.0), gpui::px(0.0)),
                temp_dir.clone(),
                true,
                true,
                cx,
            );
            panel.execute_context_menu_action(
                inviscid::app::project_panel::context_menu::ContextMenuAction::NewFile,
                window,
                cx,
            );
        });
    });

    // Simulate blur event on panel
    cx.update(|_window, cx| {
        panel_entity.update(cx, |panel, cx| {
            panel.commit_or_cancel_inline_input(cx);
        });
    });
    cx.run_until_parked();

    // Verify inline_input was removed and no file created
    cx.update(|_window, cx| {
        assert!(panel_entity.read(cx).inline_input().is_none());
    });

    // 2. Trigger New File, type "auto_commit.md", blur -> should commit and create file
    cx.update(|window, cx| {
        panel_entity.update(cx, |panel, cx| {
            panel.open_context_menu(
                gpui::point(gpui::px(0.0), gpui::px(0.0)),
                temp_dir.clone(),
                true,
                true,
                cx,
            );
            panel.execute_context_menu_action(
                inviscid::app::project_panel::context_menu::ContextMenuAction::NewFile,
                window,
                cx,
            );
            if let Some(input) = panel.inline_input() {
                input.update(cx, |inp, _cx| {
                    inp.buffer_mut().insert_text("auto_commit.md");
                });
            }
        });
    });

    // Simulate blur event on panel
    cx.update(|_window, cx| {
        panel_entity.update(cx, |panel, cx| {
            panel.commit_or_cancel_inline_input(cx);
        });
    });
    cx.run_until_parked();

    // Verify file created and input removed
    let created = temp_dir.join("auto_commit.md");
    assert!(created.exists());
    cx.update(|_window, cx| {
        assert!(panel_entity.read(cx).inline_input().is_none());
    });
    assert_eq!(
        cx.update(|_window, cx| ws_entity
            .read(cx)
            .active_file_path(cx)
            .map(|p| p.to_path_buf())),
        Some(created)
    );

    let _ = fs::remove_dir_all(&temp_dir);
}

#[test]
fn test_project_panel_trash_and_delete_confirm_modal() {
    let mut cx = TestAppContext::single();
    init_test_globals(&mut cx);

    let temp_dir = create_temp_workspace_dir("modal_test");
    let file_to_trash = temp_dir.join("docs").join("intro.md");
    let file_to_delete = temp_dir.join("docs").join("architecture.md");

    let (_app, cx) = cx.add_window_view(|_window, cx| InviscidWindow::new(cx));
    cx.run_until_parked();

    let panel_entity = cx.update(|_window, cx| {
        cx.new(|cx| ProjectPanel::new(Some(temp_dir.clone()), cx.focus_handle()))
    });

    // 1. Context Menu -> Trash action opens confirm modal
    cx.update(|window, cx| {
        panel_entity.update(cx, |panel, cx| {
            panel.open_context_menu(
                gpui::point(gpui::px(0.0), gpui::px(0.0)),
                file_to_trash.clone(),
                false,
                false,
                cx,
            );
            panel.execute_context_menu_action(
                inviscid::app::project_panel::context_menu::ContextMenuAction::Trash,
                window,
                cx,
            );
        });
    });

    cx.update(|_window, cx| {
        let panel = panel_entity.read(cx);
        let modal = panel
            .active_confirm_modal()
            .expect("Trash modal should be active");
        assert_eq!(
            modal.kind,
            inviscid::app::project_panel::ConfirmModalKind::Trash
        );
        assert_eq!(modal.confirm_button_label(), "Trash");
        assert_eq!(modal.target_path, file_to_trash);
        assert!(!modal.is_dir);
        assert_eq!(modal.message(), "Do you want to trash \"intro.md\"?");
        assert_eq!(modal.detail(), None);
    });

    // 2. Cancel modal via cancel_delete_modal -> file still exists
    cx.update(|_window, cx| {
        panel_entity.update(cx, |panel, cx| {
            panel.cancel_delete_modal(cx);
        });
    });

    assert!(file_to_trash.exists());
    cx.update(|_window, cx| {
        assert!(panel_entity.read(cx).active_confirm_modal().is_none());
    });

    // 3. Re-open the Trash modal with the keyboard command on the selected path.
    // `delete` is bound to `TrashEntry` in the keybinding table, so this is the entry point the
    // key's action handler calls.
    cx.update(|window, cx| {
        panel_entity.update(cx, |panel, cx| {
            panel.set_selected_path(Some(file_to_trash.clone()), cx);
            panel.handle_entry_action(
                inviscid::app::project_panel::context_menu::ContextMenuAction::Trash,
                window,
                cx,
            );
        });
    });

    cx.update(|_window, cx| {
        let panel = panel_entity.read(cx);
        let modal = panel
            .active_confirm_modal()
            .expect("Trash modal should be open via Delete key");
        assert_eq!(
            modal.kind,
            inviscid::app::project_panel::ConfirmModalKind::Trash
        );
    });

    // Confirm via handle_key_down Enter
    cx.update(|window, cx| {
        panel_entity.update(cx, |panel, cx| {
            let event = gpui::KeyDownEvent {
                keystroke: gpui::Keystroke {
                    modifiers: gpui::Modifiers::default(),
                    key: "Enter".to_string(),
                    key_char: None,
                },
                is_held: false,
            };
            panel.handle_key_down(&event, window, cx);
        });
    });

    cx.update(|_window, cx| {
        assert!(panel_entity.read(cx).active_confirm_modal().is_none());
    });

    // 4. Delete modal via the keyboard command bound to Ctrl+Delete (`DeleteEntry`).
    cx.update(|window, cx| {
        panel_entity.update(cx, |panel, cx| {
            panel.set_selected_path(Some(file_to_delete.clone()), cx);
            panel.handle_entry_action(
                inviscid::app::project_panel::context_menu::ContextMenuAction::Delete,
                window,
                cx,
            );
        });
    });

    cx.update(|_window, cx| {
        let panel = panel_entity.read(cx);
        let modal = panel
            .active_confirm_modal()
            .expect("Delete modal should be active");
        assert_eq!(
            modal.kind,
            inviscid::app::project_panel::ConfirmModalKind::Delete
        );
        assert_eq!(modal.confirm_button_label(), "Delete");
        assert_eq!(
            modal.message(),
            "Are you sure you want to permanently delete \"architecture.md\"?"
        );
        assert_eq!(modal.detail(), Some("This cannot be undone."));
    });

    // Cancel via handle_key_down Escape
    cx.update(|window, cx| {
        panel_entity.update(cx, |panel, cx| {
            let event = gpui::KeyDownEvent {
                keystroke: gpui::Keystroke {
                    modifiers: gpui::Modifiers::default(),
                    key: "Escape".to_string(),
                    key_char: None,
                },
                is_held: false,
            };
            panel.handle_key_down(&event, window, cx);
        });
    });

    assert!(file_to_delete.exists());
    cx.update(|_window, cx| {
        assert!(panel_entity.read(cx).active_confirm_modal().is_none());
    });

    // 5. Context menu Delete action -> confirm permanently deletes file
    cx.update(|window, cx| {
        panel_entity.update(cx, |panel, cx| {
            panel.open_context_menu(
                gpui::point(gpui::px(0.0), gpui::px(0.0)),
                file_to_delete.clone(),
                false,
                false,
                cx,
            );
            panel.execute_context_menu_action(
                inviscid::app::project_panel::context_menu::ContextMenuAction::Delete,
                window,
                cx,
            );
        });
    });

    cx.update(|_window, cx| {
        panel_entity.update(cx, |panel, cx| {
            panel.confirm_delete_modal(cx);
        });
    });

    assert!(!file_to_delete.exists());
    cx.update(|_window, cx| {
        assert!(panel_entity.read(cx).active_confirm_modal().is_none());
    });

    let _ = fs::remove_dir_all(&temp_dir);
}

#[test]
fn test_project_panel_file_item_blur_and_deselection() {
    let mut cx = TestAppContext::single();
    init_test_globals(&mut cx);

    let temp_dir = std::env::temp_dir().join(format!("inviscid_test_blur_{}", std::process::id()));
    let _ = fs::remove_dir_all(&temp_dir);
    fs::create_dir_all(&temp_dir).unwrap();
    let file_a = temp_dir.join("a.md");
    fs::write(&file_a, "# File A").unwrap();

    let (app, cx) =
        cx.add_window_view(|_window, cx| InviscidWindow::new_with_file(temp_dir.clone(), cx));
    cx.run_until_parked();

    let panel_entity =
        cx.update(|_window, cx| app.read(cx).workspace().read(cx).project_panel().clone());

    // 1. Initial selection
    cx.update(|_window, cx| {
        panel_entity.update(cx, |panel, cx| {
            panel.set_selected_path(Some(file_a.clone()), cx);
        });
    });
    cx.update(|_window, cx| {
        assert_eq!(
            panel_entity.read(cx).selected_path(),
            Some(file_a.as_path())
        );
    });

    // 2. Clear selection via clear_selection (simulates empty background click)
    cx.update(|_window, cx| {
        panel_entity.update(cx, |panel, cx| {
            panel.clear_selection(cx);
        });
    });
    cx.update(|_window, cx| {
        assert!(panel_entity.read(cx).selected_path().is_none());
    });

    // 3. Clear selection via Escape key
    cx.update(|window, cx| {
        panel_entity.update(cx, |panel, cx| {
            panel.set_selected_path(Some(file_a.clone()), cx);
            let event = gpui::KeyDownEvent {
                keystroke: gpui::Keystroke {
                    modifiers: gpui::Modifiers::default(),
                    key: "Escape".to_string(),
                    key_char: None,
                },
                is_held: false,
            };
            panel.handle_key_down(&event, window, cx);
        });
    });
    cx.update(|_window, cx| {
        assert!(panel_entity.read(cx).selected_path().is_none());
    });

    // 4. Escape with context menu open: 1st Escape closes menu, 2nd Escape clears selection
    cx.update(|window, cx| {
        panel_entity.update(cx, |panel, cx| {
            panel.set_selected_path(Some(file_a.clone()), cx);
            panel.open_context_menu(
                gpui::point(gpui::px(0.0), gpui::px(0.0)),
                file_a.clone(),
                false,
                false,
                cx,
            );
            assert!(panel.active_context_menu().is_some());

            let event = gpui::KeyDownEvent {
                keystroke: gpui::Keystroke {
                    modifiers: gpui::Modifiers::default(),
                    key: "Escape".to_string(),
                    key_char: None,
                },
                is_held: false,
            };
            // First Escape closes menu, preserves selection
            panel.handle_key_down(&event, window, cx);
            assert!(panel.active_context_menu().is_none());
            assert_eq!(panel.selected_path(), Some(file_a.as_path()));

            // Second Escape clears selection
            panel.handle_key_down(&event, window, cx);
            assert!(panel.selected_path().is_none());
        });
    });

    // 5. Blur event clears selection
    cx.update(|_window, cx| {
        panel_entity.update(cx, |panel, cx| {
            panel.set_selected_path(Some(file_a.clone()), cx);
            panel.handle_blur(cx);
            assert!(panel.selected_path().is_none());
        });
    });

    // 6. Blur event during inline input preserves selection
    cx.update(|window, cx| {
        panel_entity.update(cx, |panel, cx| {
            panel.open_context_menu(
                gpui::point(gpui::px(0.0), gpui::px(0.0)),
                temp_dir.clone(),
                true,
                true,
                cx,
            );
            panel.execute_context_menu_action(
                inviscid::app::project_panel::context_menu::ContextMenuAction::NewFile,
                window,
                cx,
            );
            assert!(panel.inline_input().is_some());

            panel.set_selected_path(Some(file_a.clone()), cx);
            // Calling handle_blur when inline input is open should NOT clear selection
            panel.handle_blur(cx);
            assert_eq!(panel.selected_path(), Some(file_a.as_path()));

            panel.handle_inline_cancel(cx);
        });
    });

    let _ = fs::remove_dir_all(&temp_dir);
}

#[test]
fn test_file_extension_whitelist_and_non_editable_guard() {
    use inviscid::editor::{WHITELISTED_EXTENSIONS, is_whitelisted_extension};
    use std::path::Path;

    // 1. Verify extension whitelist matching
    assert_eq!(WHITELISTED_EXTENSIONS, &["md", "markdown", "txt"]);
    assert!(is_whitelisted_extension(Path::new("document.md")));
    assert!(is_whitelisted_extension(Path::new("README.MD")));
    assert!(is_whitelisted_extension(Path::new("notes.markdown")));
    assert!(is_whitelisted_extension(Path::new("plain.txt")));
    assert!(!is_whitelisted_extension(Path::new("code.rs")));
    assert!(!is_whitelisted_extension(Path::new("config.toml")));
    assert!(!is_whitelisted_extension(Path::new("image.png")));
    assert!(!is_whitelisted_extension(Path::new("binary.exe")));
    assert!(!is_whitelisted_extension(Path::new("LICENSE")));

    let mut cx = TestAppContext::single();
    init_test_globals(&mut cx);

    let temp_dir = std::env::temp_dir().join(format!("inviscid_test_ext_{}", std::process::id()));
    let _ = fs::remove_dir_all(&temp_dir);
    fs::create_dir_all(&temp_dir).unwrap();

    let valid_md = temp_dir.join("valid.md");
    fs::write(&valid_md, "# Valid Markdown Content").unwrap();

    let invalid_rs = temp_dir.join("invalid.rs");
    fs::write(&invalid_rs, "fn main() { println!(\"hello\"); }").unwrap();

    let binary_png = temp_dir.join("binary.png");
    fs::write(
        &binary_png,
        &[0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A],
    )
    .unwrap();

    let no_ext_file = temp_dir.join("Makefile");
    fs::write(&no_ext_file, "all: build").unwrap();

    // 2. Open workspace with initial file
    let (app, cx) =
        cx.add_window_view(|_window, cx| InviscidWindow::new_with_file(temp_dir.clone(), cx));
    cx.run_until_parked();

    // Open valid.md in Workspace
    cx.update(|_window, cx| {
        let ws = app.read(cx).workspace().clone();
        ws.update(cx, |ws, cx| {
            ws.open_file(valid_md.clone(), cx);
        });
    });
    cx.run_until_parked();

    cx.update(|_window, cx| {
        let ws = app.read(cx).workspace().read(cx);
        let ed = ws.active_editor().unwrap().read(cx);
        assert!(ed.is_editable(), "valid.md must be editable");
        assert_eq!(ed.file_path(), Some(valid_md.as_path()));
    });

    // 3. Open non-whitelisted Rust file (invalid.rs)
    cx.update(|_window, cx| {
        let ws = app.read(cx).workspace().clone();
        ws.update(cx, |ws, cx| {
            ws.open_file(invalid_rs.clone(), cx);
        });
    });
    cx.run_until_parked();

    let rs_editor = cx.update(|_window, cx| {
        let ws = app.read(cx).workspace().read(cx);
        assert_eq!(ws.tab_count(), 2);
        let ed_entity = ws.active_editor().unwrap().clone();
        let ed = ed_entity.read(cx);
        assert!(!ed.is_editable(), "invalid.rs must NOT be editable");
        assert_eq!(ed.file_path(), Some(invalid_rs.as_path()));
        ed_entity
    });

    // 4. Attempt to edit non-whitelisted file via perform_edit
    cx.update(|_window, cx| {
        rs_editor.update(cx, |ed, cx| {
            ed.perform_edit(cx, |ed| {
                ed.buffer_mut().insert_text("MUTATION ATTEMPT");
            });
            assert!(
                ed.buffer().is_empty(),
                "Non-editable buffer must not be mutated"
            );
            assert!(
                !ed.buffer().is_dirty(),
                "Non-editable buffer must not become dirty"
            );
        });
    });

    // 5. Attempt to save non-whitelisted file via save_file_async
    cx.update(|_window, cx| {
        rs_editor.update(cx, |ed, cx| {
            let res = ed.save_file_async(cx);
            assert!(
                res.is_err(),
                "save_file_async must fail on non-editable file"
            );
        });
    });

    // Verify disk content was untouched
    assert_eq!(
        fs::read_to_string(&invalid_rs).unwrap(),
        "fn main() { println!(\"hello\"); }"
    );

    // 6. Open binary file (binary.png)
    cx.update(|_window, cx| {
        let ws = app.read(cx).workspace().clone();
        ws.update(cx, |ws, cx| {
            ws.open_file(binary_png.clone(), cx);
        });
    });
    cx.run_until_parked();

    cx.update(|_window, cx| {
        let ws = app.read(cx).workspace().read(cx);
        let ed = ws.active_editor().unwrap().read(cx);
        assert!(!ed.is_editable(), "binary.png must NOT be editable");
        assert!(
            ed.buffer().is_empty(),
            "Binary file must not be read into string buffer"
        );
    });

    // 7. Open file without extension (Makefile)
    cx.update(|_window, cx| {
        let ws = app.read(cx).workspace().clone();
        ws.update(cx, |ws, cx| {
            ws.open_file(no_ext_file.clone(), cx);
        });
    });
    cx.run_until_parked();

    cx.update(|_window, cx| {
        let ws = app.read(cx).workspace().read(cx);
        let ed = ws.active_editor().unwrap().read(cx);
        assert!(
            !ed.is_editable(),
            "Makefile without extension must NOT be editable"
        );
    });

    // 8. In-memory untitled buffer must remain editable
    cx.update(|_window, cx| {
        let ed = cx.new(|cx| inviscid::editor::Editor::new(cx));
        assert!(
            ed.read(cx).is_editable(),
            "Untitled in-memory editor must be editable"
        );
    });

    let _ = fs::remove_dir_all(&temp_dir);
}

#[gpui::test]
async fn test_tab_context_menu_bulk_close_and_pin_protection() {
    let mut cx = TestAppContext::single();
    init_test_globals(&mut cx);

    let temp_dir =
        std::env::temp_dir().join(format!("inviscid_test_tab_close_{}", std::process::id()));
    let _ = fs::remove_dir_all(&temp_dir);
    fs::create_dir_all(&temp_dir).unwrap();

    let f1 = temp_dir.join("f1.md");
    let f2 = temp_dir.join("f2.md");
    let f3 = temp_dir.join("f3.md");
    let f4 = temp_dir.join("f4.md");
    fs::write(&f1, "file 1").unwrap();
    fs::write(&f2, "file 2").unwrap();
    fs::write(&f3, "file 3").unwrap();
    fs::write(&f4, "file 4").unwrap();

    let (app, cx) =
        cx.add_window_view(|_window, cx| InviscidWindow::new_with_file(temp_dir.clone(), cx));
    cx.run_until_parked();

    // Open f1, f2, f3, f4
    cx.update(|_window, cx| {
        let ws = app.read(cx).workspace().clone();
        ws.update(cx, |ws, cx| {
            ws.open_file(f1.clone(), cx);
            ws.open_file(f2.clone(), cx);
            ws.open_file(f3.clone(), cx);
            ws.open_file(f4.clone(), cx);
        });
    });
    cx.run_until_parked();

    // Verify 4 tabs: [f1, f2, f3, f4]
    cx.update(|_window, cx| {
        let ws = app.read(cx).workspace().clone();
        assert_eq!(ws.read(cx).tab_count(), 4);

        // Pin tab 0 (f1)
        ws.update(cx, |ws, cx| {
            ws.toggle_tab_pin(0, cx);
        });
        assert!(ws.read(cx).tabs()[0].is_pinned);

        // Make tab 2 (f3) dirty
        let ed = ws.read(cx).tabs()[2].editor.clone();
        ed.update(cx, |ed, cx| {
            ed.perform_edit(cx, |ed| {
                ed.buffer_mut().insert_text(" dirty");
            });
        });
        assert!(ed.read(cx).is_dirty());

        // Test close_tabs_to_left on tab 3 (f4)
        // Tabs before: [f1 (pinned), f2 (clean), f3 (dirty), f4 (clean)]
        // Left of tab 3 are: f1 (pinned -> keep), f2 (unpinned -> close), f3 (unpinned -> close)
        // Tab 3 itself is kept.
        ws.update(cx, |ws, cx| {
            ws.close_tabs_to_left(3, cx);
        });
        assert_eq!(ws.read(cx).tab_count(), 2);
        assert_eq!(
            ws.read(cx).tabs()[0].editor.read(cx).file_path(),
            Some(f1.as_path())
        );
        assert_eq!(
            ws.read(cx).tabs()[1].editor.read(cx).file_path(),
            Some(f4.as_path())
        );

        // Re-open f2 and f3
        ws.update(cx, |ws, cx| {
            ws.open_file(f2.clone(), cx);
            ws.open_file(f3.clone(), cx);
        });
        assert_eq!(ws.read(cx).tab_count(), 4);
        // Tabs now: [f1 (pinned), f4, f2, f3]

        // Make f3 (index 3) dirty
        let ed3 = ws.read(cx).tabs()[3].editor.clone();
        ed3.update(cx, |ed, cx| {
            ed.perform_edit(cx, |ed| {
                ed.buffer_mut().insert_text(" dirty");
            });
        });
        assert!(ed3.read(cx).is_dirty());

        // Test close_clean_tabs
        // f1 is pinned (kept), f4 is clean (closed), f2 is clean (closed), f3 is dirty (kept)
        ws.update(cx, |ws, cx| {
            ws.close_clean_tabs(cx);
        });
        assert_eq!(ws.read(cx).tab_count(), 2);
        assert_eq!(
            ws.read(cx).tabs()[0].editor.read(cx).file_path(),
            Some(f1.as_path())
        );
        assert_eq!(
            ws.read(cx).tabs()[1].editor.read(cx).file_path(),
            Some(f3.as_path())
        );

        // Re-open f2 and f4
        ws.update(cx, |ws, cx| {
            ws.open_file(f2.clone(), cx);
            ws.open_file(f4.clone(), cx);
        });
        assert_eq!(ws.read(cx).tab_count(), 4);
        // Tabs: [f1 (pinned), f3, f2, f4]

        // Test close_tabs_to_right on tab 1 (f3)
        // Tabs to right: f2, f4 (both unpinned -> closed)
        ws.update(cx, |ws, cx| {
            ws.close_tabs_to_right(1, cx);
        });
        assert_eq!(ws.read(cx).tab_count(), 2);
        assert_eq!(
            ws.read(cx).tabs()[0].editor.read(cx).file_path(),
            Some(f1.as_path())
        );
        assert_eq!(
            ws.read(cx).tabs()[1].editor.read(cx).file_path(),
            Some(f3.as_path())
        );

        // Re-open f2
        ws.update(cx, |ws, cx| {
            ws.open_file(f2.clone(), cx);
        });
        assert_eq!(ws.read(cx).tab_count(), 3);
        // Tabs: [f1 (pinned), f3, f2]

        // Test close_other_tabs on tab 2 (f2)
        // Keeps f1 (pinned) and f2 (target), closes f3
        ws.update(cx, |ws, cx| {
            ws.close_other_tabs(2, cx);
        });
        assert_eq!(ws.read(cx).tab_count(), 2);
        assert_eq!(
            ws.read(cx).tabs()[0].editor.read(cx).file_path(),
            Some(f1.as_path())
        );
        assert_eq!(
            ws.read(cx).tabs()[1].editor.read(cx).file_path(),
            Some(f2.as_path())
        );

        // Test close_all_tabs
        // Closes all unpinned tabs (f2), keeps pinned (f1)
        ws.update(cx, |ws, cx| {
            ws.close_all_tabs(cx);
        });
        assert_eq!(ws.read(cx).tab_count(), 1);
        assert_eq!(
            ws.read(cx).tabs()[0].editor.read(cx).file_path(),
            Some(f1.as_path())
        );
        assert!(ws.read(cx).tabs()[0].is_pinned);
    });

    let _ = fs::remove_dir_all(&temp_dir);
}

#[gpui::test]
async fn test_tab_context_menu_pin_toggle_and_reorder() {
    let mut cx = TestAppContext::single();
    init_test_globals(&mut cx);

    let temp_dir =
        std::env::temp_dir().join(format!("inviscid_test_tab_pin_{}", std::process::id()));
    let _ = fs::remove_dir_all(&temp_dir);
    fs::create_dir_all(&temp_dir).unwrap();

    let a = temp_dir.join("a.md");
    let b = temp_dir.join("b.md");
    let c = temp_dir.join("c.md");
    fs::write(&a, "a").unwrap();
    fs::write(&b, "b").unwrap();
    fs::write(&c, "c").unwrap();

    let (app, cx) =
        cx.add_window_view(|_window, cx| InviscidWindow::new_with_file(temp_dir.clone(), cx));
    cx.run_until_parked();

    cx.update(|_window, cx| {
        let ws = app.read(cx).workspace().clone();
        ws.update(cx, |ws, cx| {
            ws.open_file(a.clone(), cx);
            ws.open_file(b.clone(), cx);
            ws.open_file(c.clone(), cx);
        });
    });
    cx.run_until_parked();

    cx.update(|_window, cx| {
        let ws = app.read(cx).workspace().clone();
        assert_eq!(ws.read(cx).tab_count(), 3);
        // Initially: [a, b, c], none pinned
        assert!(!ws.read(cx).tabs()[0].is_pinned);
        assert!(!ws.read(cx).tabs()[1].is_pinned);
        assert!(!ws.read(cx).tabs()[2].is_pinned);

        // Pin tab 2 ('c')
        ws.update(cx, |ws, cx| {
            ws.toggle_tab_pin(2, cx);
        });
        // 'c' moves to index 0: [c (pinned), a, b]
        assert!(ws.read(cx).tabs()[0].is_pinned);
        assert_eq!(
            ws.read(cx).tabs()[0].editor.read(cx).file_path(),
            Some(c.as_path())
        );
        assert_eq!(
            ws.read(cx).tabs()[1].editor.read(cx).file_path(),
            Some(a.as_path())
        );
        assert_eq!(
            ws.read(cx).tabs()[2].editor.read(cx).file_path(),
            Some(b.as_path())
        );

        // Pin tab 2 ('b')
        ws.update(cx, |ws, cx| {
            ws.toggle_tab_pin(2, cx);
        });
        // 'b' moves to index 1 (after 'c'): [c (pinned), b (pinned), a]
        assert!(ws.read(cx).tabs()[0].is_pinned);
        assert!(ws.read(cx).tabs()[1].is_pinned);
        assert_eq!(
            ws.read(cx).tabs()[0].editor.read(cx).file_path(),
            Some(c.as_path())
        );
        assert_eq!(
            ws.read(cx).tabs()[1].editor.read(cx).file_path(),
            Some(b.as_path())
        );
        assert_eq!(
            ws.read(cx).tabs()[2].editor.read(cx).file_path(),
            Some(a.as_path())
        );

        // Unpin tab 0 ('c')
        ws.update(cx, |ws, cx| {
            ws.toggle_tab_pin(0, cx);
        });
        // 'c' is unpinned and placed after remaining pinned tabs: [b (pinned), c, a]
        assert!(ws.read(cx).tabs()[0].is_pinned);
        assert!(!ws.read(cx).tabs()[1].is_pinned);
        assert_eq!(
            ws.read(cx).tabs()[0].editor.read(cx).file_path(),
            Some(b.as_path())
        );
        assert_eq!(
            ws.read(cx).tabs()[1].editor.read(cx).file_path(),
            Some(c.as_path())
        );
    });

    let _ = fs::remove_dir_all(&temp_dir);
}

#[gpui::test]
async fn test_tab_context_menu_reveal_in_project_panel_makes_sidebar_visible() {
    let mut cx = TestAppContext::single();
    init_test_globals(&mut cx);

    let temp_dir =
        std::env::temp_dir().join(format!("inviscid_test_tab_reveal_{}", std::process::id()));
    let _ = fs::remove_dir_all(&temp_dir);
    fs::create_dir_all(&temp_dir).unwrap();

    let doc = temp_dir.join("reveal_me.md");
    fs::write(&doc, "Reveal Me").unwrap();

    let (app, cx) =
        cx.add_window_view(|_window, cx| InviscidWindow::new_with_file(temp_dir.clone(), cx));
    cx.run_until_parked();

    let ws = cx.update(|_window, cx| app.read(cx).workspace().clone());
    cx.update(|_window, cx| {
        ws.update(cx, |ws, cx| {
            ws.open_file(doc.clone(), cx);
            // Hide sidebar initially
            ws.set_sidebar_visible(false, cx);
        });
    });
    cx.run_until_parked();
    cx.update(|_window, cx| assert!(!ws.read(cx).sidebar_visible()));

    // reveal_tab_in_project_panel must make the sidebar visible again
    cx.update(|_window, cx| {
        ws.update(cx, |ws, cx| {
            ws.reveal_tab_in_project_panel(0, cx);
        });
    });
    cx.run_until_parked();
    cx.update(|_window, cx| assert!(ws.read(cx).sidebar_visible()));

    let _ = fs::remove_dir_all(&temp_dir);
}

#[gpui::test]
async fn test_tab_context_menu_action_reentrancy_close() {
    let mut cx = TestAppContext::single();
    init_test_globals(&mut cx);

    let temp_dir = std::env::temp_dir().join(format!(
        "inviscid_test_tab_reentrancy_{}",
        std::process::id()
    ));
    let _ = fs::remove_dir_all(&temp_dir);
    fs::create_dir_all(&temp_dir).unwrap();

    let f1 = temp_dir.join("f1.md");
    fs::write(&f1, "file 1").unwrap();

    let (app, cx) =
        cx.add_window_view(|_window, cx| InviscidWindow::new_with_file(temp_dir.clone(), cx));
    cx.run_until_parked();

    cx.update(|_window, cx| {
        let ws = app.read(cx).workspace().clone();
        ws.update(cx, |ws, cx| {
            ws.open_file(f1.clone(), cx);
        });
    });
    cx.run_until_parked();

    cx.update(|window, cx| {
        let ws = app.read(cx).workspace().clone();
        let tab_bar = ws.read(cx).tab_bar().clone();

        // 1. Open context menu on tab 0
        tab_bar.update(cx, |tb, cx| {
            let state = inviscid::app::tab_bar::context_menu::TabContextMenuState {
                tab_idx: 0,
                position: gpui::point(gpui::px(100.0), gpui::px(10.0)),
                is_pinned: false,
                is_read_only: false,
                has_file_path: true,
                is_whitelisted: true,
                tab_count: 1,
                has_clean_tabs: true,
                has_workspace_root: true,
            };
            tb.open_context_menu(state, cx);
        });
        assert!(tab_bar.read(cx).active_context_menu().is_some());

        // 2. Execute Close action through execute_context_menu_action (simulating click callback)
        inviscid::app::tab_bar::TabBar::execute_context_menu_action(
            &tab_bar,
            inviscid::app::tab_bar::context_menu::TabContextMenuAction::Close,
            window,
            cx,
        );
    });

    cx.update(|_window, cx| {
        let ws = app.read(cx).workspace().clone();
        let tab_bar = ws.read(cx).tab_bar().clone();
        // 3. Verify context menu was dismissed and tab was closed without double lease panic
        assert!(tab_bar.read(cx).active_context_menu().is_none());
        assert_eq!(ws.read(cx).tab_count(), 0);
    });

    let _ = fs::remove_dir_all(&temp_dir);
}

#[gpui::test]
async fn test_all_tab_context_menu_actions_reentrancy_free() {
    let mut cx = TestAppContext::single();
    init_test_globals(&mut cx);

    let temp_dir =
        std::env::temp_dir().join(format!("inviscid_test_tab_actions_{}", std::process::id()));
    let _ = fs::remove_dir_all(&temp_dir);
    fs::create_dir_all(&temp_dir).unwrap();

    let a = temp_dir.join("a.md");
    let b = temp_dir.join("b.md");
    let c = temp_dir.join("c.md");
    fs::write(&a, "a").unwrap();
    fs::write(&b, "b").unwrap();
    fs::write(&c, "c").unwrap();

    let (app, cx) =
        cx.add_window_view(|_window, cx| InviscidWindow::new_with_file(temp_dir.clone(), cx));
    cx.run_until_parked();

    cx.update(|_window, cx| {
        let ws = app.read(cx).workspace().clone();
        ws.update(cx, |ws, cx| {
            ws.open_file(a.clone(), cx);
            ws.open_file(b.clone(), cx);
            ws.open_file(c.clone(), cx);
        });
    });
    cx.run_until_parked();

    // Test TogglePin via execute_context_menu_action
    cx.update(|window, cx| {
        let ws = app.read(cx).workspace().clone();
        let tab_bar = ws.read(cx).tab_bar().clone();

        tab_bar.update(cx, |tb, cx| {
            tb.open_context_menu(
                inviscid::app::tab_bar::context_menu::TabContextMenuState {
                    tab_idx: 1,
                    position: gpui::point(gpui::px(50.0), gpui::px(10.0)),
                    is_pinned: false,
                    is_read_only: false,
                    has_file_path: true,
                    is_whitelisted: true,
                    tab_count: 3,
                    has_clean_tabs: true,
                    has_workspace_root: true,
                },
                cx,
            );
        });
        inviscid::app::tab_bar::TabBar::execute_context_menu_action(
            &tab_bar,
            inviscid::app::tab_bar::context_menu::TabContextMenuAction::TogglePin,
            window,
            cx,
        );
    });
    cx.update(|_window, cx| {
        let ws = app.read(cx).workspace().clone();
        assert!(ws.read(cx).tabs()[0].is_pinned);
    });

    // Test ToggleReadOnly via execute_context_menu_action
    cx.update(|window, cx| {
        let ws = app.read(cx).workspace().clone();
        let tab_bar = ws.read(cx).tab_bar().clone();

        tab_bar.update(cx, |tb, cx| {
            tb.open_context_menu(
                inviscid::app::tab_bar::context_menu::TabContextMenuState {
                    tab_idx: 0,
                    position: gpui::point(gpui::px(50.0), gpui::px(10.0)),
                    is_pinned: true,
                    is_read_only: false,
                    has_file_path: true,
                    is_whitelisted: true,
                    tab_count: 3,
                    has_clean_tabs: true,
                    has_workspace_root: true,
                },
                cx,
            );
        });
        inviscid::app::tab_bar::TabBar::execute_context_menu_action(
            &tab_bar,
            inviscid::app::tab_bar::context_menu::TabContextMenuAction::ToggleReadOnly,
            window,
            cx,
        );
    });
    cx.update(|_window, cx| {
        let ws = app.read(cx).workspace().clone();
        assert!(ws.read(cx).tabs()[0].editor.read(cx).is_read_only());
    });

    // Test CloseOthers via execute_context_menu_action
    cx.update(|window, cx| {
        let ws = app.read(cx).workspace().clone();
        let tab_bar = ws.read(cx).tab_bar().clone();

        tab_bar.update(cx, |tb, cx| {
            tb.open_context_menu(
                inviscid::app::tab_bar::context_menu::TabContextMenuState {
                    tab_idx: 2,
                    position: gpui::point(gpui::px(50.0), gpui::px(10.0)),
                    is_pinned: false,
                    is_read_only: false,
                    has_file_path: true,
                    is_whitelisted: true,
                    tab_count: 3,
                    has_clean_tabs: true,
                    has_workspace_root: true,
                },
                cx,
            );
        });
        inviscid::app::tab_bar::TabBar::execute_context_menu_action(
            &tab_bar,
            inviscid::app::tab_bar::context_menu::TabContextMenuAction::CloseOthers,
            window,
            cx,
        );
    });
    cx.update(|_window, cx| {
        let ws = app.read(cx).workspace().clone();
        // Kept pinned tab (0) and target tab (2) -> 2 tabs remain
        assert_eq!(ws.read(cx).tab_count(), 2);
    });

    let _ = fs::remove_dir_all(&temp_dir);
}

#[test]
fn test_read_only_blocks_mutations_and_updates_tab_meta() {
    use gpui::EntityInputHandler;

    let mut cx = TestAppContext::single();
    init_test_globals(&mut cx);

    let temp_dir = create_temp_workspace_dir("readonly");
    let test_file = temp_dir.join("test.md");
    fs::write(&test_file, "# Header\n- [ ] Todo item\n").unwrap();

    let (app, cx) =
        cx.add_window_view(|_window, cx| InviscidWindow::new_with_file(test_file.clone(), cx));
    cx.run_until_parked();

    // 1. Initially editable
    cx.update(|_window, cx| {
        let ws = app.read(cx).workspace().clone();
        let ed_entity = ws.read(cx).active_editor().unwrap().clone();
        let ed = ed_entity.read(cx);
        assert!(!ed.is_read_only());
        assert!(ed.can_edit());
        assert_eq!(
            ed.buffer().to_string_content(),
            "# Header\n- [ ] Todo item\n"
        );
    });

    // 2. Toggle Read-Only on
    cx.update(|window, cx| {
        let ws = app.read(cx).workspace().clone();
        let tab_bar = ws.read(cx).tab_bar().clone();
        tab_bar.update(cx, |tb, cx| {
            tb.open_context_menu(
                inviscid::app::tab_bar::context_menu::TabContextMenuState {
                    tab_idx: 0,
                    position: gpui::point(gpui::px(50.0), gpui::px(10.0)),
                    is_pinned: false,
                    is_read_only: false,
                    has_file_path: true,
                    is_whitelisted: true,
                    tab_count: 1,
                    has_clean_tabs: true,
                    has_workspace_root: true,
                },
                cx,
            );
        });
        inviscid::app::tab_bar::TabBar::execute_context_menu_action(
            &tab_bar,
            inviscid::app::tab_bar::context_menu::TabContextMenuAction::ToggleReadOnly,
            window,
            cx,
        );
    });

    cx.update(|_window, cx| {
        let ws = app.read(cx).workspace().clone();
        let tab_bar = ws.read(cx).tab_bar().clone();
        let ed_entity = ws.read(cx).active_editor().unwrap().clone();
        let ed = ed_entity.read(cx);
        assert!(ed.is_read_only());
        assert!(!ed.can_edit());

        // Verify TabBar meta has is_read_only = true
        let tabs_meta = tab_bar.read(cx).tabs_meta();
        assert_eq!(tabs_meta.len(), 1);
        assert!(tabs_meta[0].is_read_only);
    });

    // 3. Attempt mutations while read-only: ALL MUST BE BLOCKED
    cx.update(|window, cx| {
        let ws = app.read(cx).workspace().clone();
        let ed_entity = ws.read(cx).active_editor().unwrap().clone();

        // 3a. Typing through EntityInputHandler::replace_text_in_range
        ed_entity.update(cx, |ed, cx| {
            ed.replace_text_in_range(None, "Blocked Typing", window, cx);
        });
        assert_eq!(
            ed_entity.read(cx).buffer().to_string_content(),
            "# Header\n- [ ] Todo item\n"
        );
        assert!(!ed_entity.read(cx).is_dirty());

        // 3b. IME input through EntityInputHandler::replace_and_mark_text_in_range
        ed_entity.update(cx, |ed, cx| {
            ed.replace_and_mark_text_in_range(None, "Blocked IME", None, window, cx);
        });
        assert_eq!(
            ed_entity.read(cx).buffer().to_string_content(),
            "# Header\n- [ ] Todo item\n"
        );
        assert!(!ed_entity.read(cx).is_dirty());

        // 3c. Keymap perform_edit
        ed_entity.update(cx, |ed, cx| {
            ed.perform_edit(cx, |ed| {
                ed.buffer_mut().insert_text("evil");
            });
        });
        assert_eq!(
            ed_entity.read(cx).buffer().to_string_content(),
            "# Header\n- [ ] Todo item\n"
        );
        assert!(!ed_entity.read(cx).is_dirty());

        // 3d. Paste
        ed_entity.update(cx, |ed, cx| {
            ed.paste(cx);
        });
        assert_eq!(
            ed_entity.read(cx).buffer().to_string_content(),
            "# Header\n- [ ] Todo item\n"
        );
        assert!(!ed_entity.read(cx).is_dirty());

        // 3e. Cut
        ed_entity.update(cx, |ed, cx| {
            ed.select_all(cx);
            ed.cut(cx);
        });
        assert_eq!(
            ed_entity.read(cx).buffer().to_string_content(),
            "# Header\n- [ ] Todo item\n"
        );
        assert!(!ed_entity.read(cx).is_dirty());

        // 3f. Task checkbox toggle
        ed_entity.update(cx, |ed, cx| {
            ed.toggle_task_checkbox(1, cx);
        });
        assert_eq!(
            ed_entity.read(cx).buffer().to_string_content(),
            "# Header\n- [ ] Todo item\n"
        );
        assert!(!ed_entity.read(cx).is_dirty());

        // 3g. Undo & Redo
        ed_entity.update(cx, |ed, cx| {
            ed.undo(cx);
            ed.redo(cx);
        });
        assert_eq!(
            ed_entity.read(cx).buffer().to_string_content(),
            "# Header\n- [ ] Todo item\n"
        );
        assert!(!ed_entity.read(cx).is_dirty());

        // 3h. Save file async returns error
        ed_entity.update(cx, |ed, cx| {
            let res = ed.save_file_async(cx);
            assert!(res.is_err());
        });
    });

    // 4. Toggle Read-Only back off -> editing works again
    cx.update(|window, cx| {
        let ws = app.read(cx).workspace().clone();
        let tab_bar = ws.read(cx).tab_bar().clone();
        tab_bar.update(cx, |tb, cx| {
            tb.open_context_menu(
                inviscid::app::tab_bar::context_menu::TabContextMenuState {
                    tab_idx: 0,
                    position: gpui::point(gpui::px(50.0), gpui::px(10.0)),
                    is_pinned: false,
                    is_read_only: true,
                    has_file_path: true,
                    is_whitelisted: true,
                    tab_count: 1,
                    has_clean_tabs: true,
                    has_workspace_root: true,
                },
                cx,
            );
        });
        inviscid::app::tab_bar::TabBar::execute_context_menu_action(
            &tab_bar,
            inviscid::app::tab_bar::context_menu::TabContextMenuAction::ToggleReadOnly,
            window,
            cx,
        );
    });

    cx.update(|window, cx| {
        let ws = app.read(cx).workspace().clone();
        let tab_bar = ws.read(cx).tab_bar().clone();
        let ed_entity = ws.read(cx).active_editor().unwrap().clone();
        let ed = ed_entity.read(cx);
        assert!(!ed.is_read_only());
        assert!(ed.can_edit());
        assert!(!tab_bar.read(cx).tabs_meta()[0].is_read_only);

        // Perform edit now (replaces the text selected earlier)
        ed_entity.update(cx, |ed, cx| {
            ed.replace_text_in_range(None, "New Text", window, cx);
        });
        assert!(ed_entity.read(cx).is_dirty());
        assert_eq!(
            ed_entity.read(cx).buffer().to_string_content(),
            "New Text\n"
        );

        // Undo edit
        ed_entity.update(cx, |ed, cx| {
            ed.undo(cx);
        });
        assert_eq!(
            ed_entity.read(cx).buffer().to_string_content(),
            "# Header\n- [ ] Todo item\n"
        );
    });

    let _ = fs::remove_dir_all(&temp_dir);
}

#[test]
fn test_project_panel_folder_expanded_collapsed_icons() {
    let mut cx = TestAppContext::single();
    init_test_globals(&mut cx);

    let temp_dir = create_temp_workspace_dir("folder_icons");
    let (_app, cx) = cx.add_window_view(|_window, cx| InviscidWindow::new(cx));
    cx.run_until_parked();

    let panel_entity = cx.update(|_window, cx| {
        cx.new(|cx| ProjectPanel::new(Some(temp_dir.clone()), cx.focus_handle()))
    });

    // 1. Initial state: 'docs' and 'src' are collapsed (is_expanded == false)
    cx.update(|_window, cx| {
        let panel = panel_entity.read(cx);
        let docs = panel.entries().iter().find(|e| e.name == "docs").unwrap();
        assert!(docs.is_dir());
        assert!(!docs.is_expanded());

        let src = panel.entries().iter().find(|e| e.name == "src").unwrap();
        assert!(src.is_dir());
        assert!(!src.is_expanded());
    });

    // 2. Expand 'docs'
    let docs_path = temp_dir.join("docs");
    cx.update(|_window, cx| {
        panel_entity.update(cx, |panel, cx| {
            panel.toggle_expand(&docs_path, cx);
        });
    });

    // Verify 'docs' is now expanded, while 'src' remains collapsed
    cx.update(|_window, cx| {
        let panel = panel_entity.read(cx);
        let docs = panel.entries().iter().find(|e| e.name == "docs").unwrap();
        assert!(docs.is_dir());
        assert!(
            docs.is_expanded(),
            "Expanded folder must have is_expanded = true"
        );

        let src = panel.entries().iter().find(|e| e.name == "src").unwrap();
        assert!(src.is_dir());
        assert!(
            !src.is_expanded(),
            "Collapsed folder must have is_expanded = false"
        );
    });

    // 3. Toggle 'docs' back to collapsed
    cx.update(|_window, cx| {
        panel_entity.update(cx, |panel, cx| {
            panel.toggle_expand(&docs_path, cx);
        });
    });

    cx.update(|_window, cx| {
        let panel = panel_entity.read(cx);
        let docs = panel.entries().iter().find(|e| e.name == "docs").unwrap();
        assert!(
            !docs.is_expanded(),
            "Toggled folder must be collapsed again"
        );
    });

    let _ = fs::remove_dir_all(&temp_dir);
}

#[test]
fn test_project_panel_renders_long_filenames_without_panic() {
    let mut cx = TestAppContext::single();
    init_test_globals(&mut cx);

    let temp_dir =
        std::env::temp_dir().join(format!("inviscid_test_long_name_{}", std::process::id()));
    let _ = fs::remove_dir_all(&temp_dir);
    fs::create_dir_all(&temp_dir).unwrap();

    let long_file_name = "超20万字,中国知名金融学者丁圣元倾情翻译。日本蜡烛图技术(K线之父史蒂夫.尼森全新修订精编版).pdf";
    let long_dir_name =
        "这是一个非常非常长的文件夹名称用于测试侧边栏目录不自动换行而溢出隐藏的表现";

    fs::create_dir_all(temp_dir.join(long_dir_name)).unwrap();
    fs::write(temp_dir.join(long_file_name), "pdf content").unwrap();

    let (app, cx) = cx.add_window_view(|_window, cx| InviscidWindow::new(cx));
    cx.run_until_parked();

    let ws = cx.update(|_window, cx| app.read(cx).workspace().clone());
    cx.update(|_window, cx| {
        ws.update(cx, |ws, cx| {
            ws.open_folder(temp_dir.clone(), cx);
        });
    });
    cx.run_until_parked();

    let panel_entity = cx.update(|_window, cx| ws.read(cx).project_panel().clone());
    cx.update(|_window, cx| {
        let panel = panel_entity.read(cx);
        let entries = panel.entries();
        assert!(
            entries
                .iter()
                .any(|e| e.name == long_dir_name && e.is_dir())
        );
        assert!(
            entries
                .iter()
                .any(|e| e.name == long_file_name && !e.is_dir())
        );
    });

    // Smoke check: rendering the panel with very long file/dir names must not panic.
    cx.run_until_parked();

    let _ = fs::remove_dir_all(&temp_dir);
}

#[test]
fn test_project_panel_renders_many_entries_without_panic() {
    let mut cx = TestAppContext::single();
    init_test_globals(&mut cx);

    let temp_dir =
        std::env::temp_dir().join(format!("inviscid_test_scroll_{}", std::process::id()));
    let _ = fs::remove_dir_all(&temp_dir);
    fs::create_dir_all(&temp_dir).unwrap();

    // Generate 50 files so that file list overflows the viewport height
    for i in 0..50 {
        fs::write(
            temp_dir.join(format!("file_{:03}.txt", i)),
            format!("content {}", i),
        )
        .unwrap();
    }

    let (app, cx) = cx.add_window_view(|_window, cx| InviscidWindow::new(cx));
    cx.run_until_parked();

    let ws = cx.update(|_window, cx| app.read(cx).workspace().clone());
    cx.update(|_window, cx| {
        ws.update(cx, |ws, cx| {
            ws.open_folder(temp_dir.clone(), cx);
        });
    });
    cx.run_until_parked();

    let panel_entity = cx.update(|_window, cx| ws.read(cx).project_panel().clone());
    cx.update(|_window, cx| {
        let panel = panel_entity.read(cx);
        assert_eq!(panel.entries().len(), 50);
    });

    // Render pass with enough entries to overflow the viewport
    cx.run_until_parked();

    let _ = fs::remove_dir_all(&temp_dir);
}

#[test]
fn test_workspace_sidebar_resize_clamping_and_drag() {
    let mut cx = TestAppContext::single();
    init_test_globals(&mut cx);

    let (app, cx) = cx.add_window_view(|_window, cx| InviscidWindow::new(cx));
    cx.run_until_parked();

    let ws = cx.update(|_window, cx| app.read(cx).workspace().clone());

    cx.update(|_window, cx| {
        ws.update(cx, |ws, cx| {
            // Default width is 240px
            assert_eq!(ws.sidebar_width(), px(DEFAULT_SIDEBAR_WIDTH));

            // Clamping below MIN_SIDEBAR_WIDTH (160px)
            ws.set_sidebar_width(px(100.0), cx);
            assert_eq!(ws.sidebar_width(), px(MIN_SIDEBAR_WIDTH));

            // Clamping above MAX_SIDEBAR_WIDTH (800px)
            ws.set_sidebar_width(px(1200.0), cx);
            assert_eq!(ws.sidebar_width(), px(MAX_SIDEBAR_WIDTH));

            // Setting valid intermediate width
            ws.set_sidebar_width(px(320.0), cx);
            assert_eq!(ws.sidebar_width(), px(320.0));

            // Drag resize interaction
            ws.start_sidebar_resize(px(320.0), cx);
        });
    });
    // Run paint and render while is_resizing_sidebar is true (must not panic!)
    cx.run_until_parked();

    cx.update(|_window, cx| {
        ws.update(cx, |ws, cx| {
            ws.handle_sidebar_resize_drag(px(370.0), cx);
            assert_eq!(ws.sidebar_width(), px(370.0));
        });
    });
    cx.run_until_parked();

    cx.update(|_window, cx| {
        ws.update(cx, |ws, cx| {
            ws.handle_sidebar_resize_drag(px(200.0), cx);
            assert_eq!(ws.sidebar_width(), px(200.0));

            ws.finish_sidebar_resize(cx);

            // Double click reset back to default
            ws.reset_sidebar_width(cx);
            assert_eq!(ws.sidebar_width(), px(DEFAULT_SIDEBAR_WIDTH));
        });
    });
    cx.run_until_parked();
}

#[test]
fn test_workspace_sidebar_width_persistence_and_restore() {
    let mut cx = TestAppContext::single();
    init_test_globals(&mut cx);

    let temp_dir = std::env::temp_dir().join(format!("inviscid_test_width_{}", std::process::id()));
    let _ = fs::remove_dir_all(&temp_dir);
    fs::create_dir_all(&temp_dir).unwrap();

    let (app, cx) = cx.add_window_view(|_window, cx| InviscidWindow::new(cx));
    cx.run_until_parked();

    let ws = cx.update(|_window, cx| app.read(cx).workspace().clone());
    cx.update(|_window, cx| {
        ws.update(cx, |ws, cx| {
            ws.open_folder(temp_dir.clone(), cx);
        });
    });
    cx.run_until_parked();

    // Modify sidebar width and sync state
    cx.update(|_window, cx| {
        ws.update(cx, |ws, cx| {
            ws.set_sidebar_width(px(380.0), cx);
            let mut config = AppConfig::default();
            ws.sync_workspace_state(&mut config, cx);

            let last_ws = config.last_workspace().unwrap();
            assert_eq!(last_ws.sidebar_width, Some(380.0));

            // Restore from state with 380px
            let restored = Workspace::from_state(last_ws, cx);
            assert_eq!(restored.sidebar_width(), px(380.0));

            // Restore from state with out of bounds value
            let mut oob_state = last_ws.clone();
            oob_state.sidebar_width = Some(50.0);
            let restored_oob = Workspace::from_state(&oob_state, cx);
            assert_eq!(restored_oob.sidebar_width(), px(MIN_SIDEBAR_WIDTH));
        });
    });

    let _ = fs::remove_dir_all(&temp_dir);
}

#[gpui::test]
async fn test_inline_input_drag_selection_forward_and_backward(cx: &mut TestAppContext) {
    use gpui::EntityInputHandler;
    use inviscid::app::project_panel::{InlineInput, InlineInputKind};
    use inviscid::buffer::{Position, Selection};

    init_test_globals(cx);

    let (_, cx) = cx.add_window_view(|_window, cx| {
        let dummy_path = PathBuf::from("workspace/component_spec.rs");
        InlineInput::new(
            InlineInputKind::Rename {
                target: dummy_path,
                is_dir: false,
            },
            "component_spec.rs",
            1,
            cx,
        )
    });

    cx.update(|window, cx| {
        let dummy_path = PathBuf::from("workspace/component_spec.rs");
        let input_entity = cx.new(|cx| {
            InlineInput::new(
                InlineInputKind::Rename {
                    target: dummy_path,
                    is_dir: false,
                },
                "component_spec.rs",
                1,
                cx,
            )
        });

        input_entity.update(cx, |this, cx| {
            // Initial Rename selection is stem: "component_spec" (0..14)
            assert_eq!(this.text(), "component_spec.rs");
            let initial_sel = this
                .selected_text_range(false, window, cx)
                .expect("stem selection expected");
            assert_eq!(initial_sel.range, 0..14);

            // 1. Begin mouse drag at column 2 ('m')
            this.start_drag(2, cx);
            assert!(this.is_dragging());
            assert_eq!(this.drag_anchor(), Some(2));
            assert_eq!(this.buffer().selection().anchor, Position::new(0, 2));
            assert_eq!(this.buffer().selection().head, Position::new(0, 2));

            // 2. Drag forward: anchor 2, head 7 ("mpone")
            this.buffer_mut().set_selection(Selection {
                anchor: Position::new(0, 2),
                head: Position::new(0, 7),
            });
            assert_eq!(this.buffer().selected_text(), Some("mpone".to_string()));
            let fwd_sel = this
                .selected_text_range(false, window, cx)
                .expect("forward selection expected");
            assert_eq!(fwd_sel.range, 2..7);
            assert!(!fwd_sel.reversed);

            // 3. Drag backward: anchor 7, head 0 ("compone")
            this.buffer_mut().set_selection(Selection {
                anchor: Position::new(0, 7),
                head: Position::new(0, 0),
            });
            assert_eq!(this.buffer().selected_text(), Some("compone".to_string()));
            let rev_sel = this
                .selected_text_range(false, window, cx)
                .expect("backward selection expected");
            assert_eq!(rev_sel.range, 0..7);
            assert!(rev_sel.reversed);

            // 4. Double-click selects word:
            this.buffer_mut().select_word_at(Position::new(0, 15)); // inside "rs"
            assert_eq!(this.buffer().selected_text(), Some("rs".to_string()));

            // 5. Triple-click selects all:
            this.buffer_mut().select_all();
            assert_eq!(
                this.buffer().selected_text(),
                Some("component_spec.rs".to_string())
            );

            // 6. Stop drag:
            this.stop_drag(cx);
            assert!(!this.is_dragging());
            assert_eq!(this.drag_anchor(), None);

            // 7. Verify col_for_point at offset 0
            assert_eq!(this.col_for_point(px(0.0), window), 0);
        });
    });
}

#[test]
fn test_inline_input_edit_actions_stay_in_input() {
    use inviscid::editor::actions::UndoAction;

    let mut cx = TestAppContext::single();
    init_test_globals(&mut cx);

    let temp_dir = create_temp_workspace_dir("inline_edit_actions");
    let doc = temp_dir.join("doc.md");
    fs::write(&doc, "# base").unwrap();

    let (app, cx) = cx.add_window_view(|_window, cx| InviscidWindow::new(cx));
    cx.run_until_parked();

    // Open folder + document, then give the editor an undo history entry worth protecting.
    let (ws_entity, editor_entity) = cx.update(|_window, cx| {
        let ws = app.read(cx).workspace().clone();
        ws.update(cx, |ws, cx| {
            ws.open_folder(temp_dir.clone(), cx);
            ws.open_file(doc.clone(), cx);
        });
        let editor = ws
            .read(cx)
            .active_editor()
            .cloned()
            .expect("doc.md should be open");
        editor.update(cx, |editor, _cx| {
            editor.buffer_mut().insert_text(" tail");
        });
        (ws, editor)
    });
    cx.run_until_parked();

    let editor_before =
        cx.update(|_window, cx| editor_entity.read(cx).buffer().to_string_content());

    // Rename the document: `attach_inline_input` focuses the inline input it creates.
    let input_entity = cx.update(|window, cx| {
        let panel = ws_entity.read(cx).project_panel().clone();
        panel.update(cx, |panel, cx| {
            panel.open_context_menu(gpui::point(px(0.0), px(0.0)), doc.clone(), false, false, cx);
            panel.execute_context_menu_action(
                inviscid::app::project_panel::context_menu::ContextMenuAction::Rename,
                window,
                cx,
            );
        });
        panel
            .read(cx)
            .inline_input()
            .cloned()
            .expect("rename inline input should be active")
    });
    cx.run_until_parked();

    let input_before = cx.update(|_window, cx| input_entity.read(cx).text());
    cx.update(|_window, cx| {
        input_entity.update(cx, |input, cx| {
            input.buffer_mut().insert_text("XYZ");
            cx.notify();
        });
    });

    cx.update(|window, cx| {
        let handle = input_entity.read(cx).focus_handle(cx);
        assert!(handle.is_focused(window), "inline input should hold focus");
    });

    // Undo is bound with context `Editor || InlineInput`; the focused input must consume it
    // instead of letting it bubble up to InviscidWindow, which would undo the editor instead.
    cx.dispatch_action(UndoAction);
    cx.run_until_parked();

    assert_eq!(
        cx.update(|_window, cx| editor_entity.read(cx).buffer().to_string_content()),
        editor_before,
        "the open editor must not be touched by edit actions aimed at the inline input"
    );
    assert_eq!(
        cx.update(|_window, cx| input_entity.read(cx).text()),
        input_before,
        "the inline input should undo its own edit"
    );

    let _ = fs::remove_dir_all(&temp_dir);
}

#[test]
fn test_set_render_mode_action_targets_active_editor() {
    use inviscid::app::actions::SetRenderMode;
    use inviscid::editor::RenderMode;

    let mut cx = TestAppContext::single();
    init_test_globals(&mut cx);

    let temp_dir = create_temp_workspace_dir("set_render_mode");
    let doc = temp_dir.join("doc.md");
    fs::write(&doc, "# base").unwrap();

    let (app, cx) =
        cx.add_window_view(|_window, cx| InviscidWindow::new_with_file(doc.clone(), cx));
    cx.run_until_parked();

    let editor_entity = cx.update(|_window, cx| {
        app.read(cx)
            .workspace()
            .read(cx)
            .active_editor()
            .cloned()
            .expect("doc.md should be open")
    });
    cx.update(|window, cx| {
        let handle = editor_entity.read(cx).focus_handle(cx);
        window.focus(&handle);
    });

    // The mode travels in the action parameter; the handler must forward it to the active editor.
    cx.dispatch_action(SetRenderMode(RenderMode::Source));
    cx.run_until_parked();
    assert_eq!(
        cx.update(|_window, cx| editor_entity.read(cx).render_mode()),
        RenderMode::Source
    );

    cx.dispatch_action(SetRenderMode(RenderMode::LivePreview));
    cx.run_until_parked();
    assert_eq!(
        cx.update(|_window, cx| editor_entity.read(cx).render_mode()),
        RenderMode::LivePreview
    );

    let _ = fs::remove_dir_all(&temp_dir);
}

#[gpui::test]
async fn test_inline_input_cursor_breathing_and_shared_engine(cx: &mut TestAppContext) {
    use inviscid::Editor;
    use inviscid::app::project_panel::{InlineInput, InlineInputKind};
    use inviscid::editor::shaping::calculate_caret_size;

    init_test_globals(cx);

    let (_, cx) = cx.add_window_view(|_window, cx| {
        let dummy_path = PathBuf::from("workspace/dummy.md");
        InlineInput::new(InlineInputKind::NewFile { parent: dummy_path }, "", 1, cx)
    });

    // 1. Verify caret dimensions calculated via calculate_caret_size
    let (caret_w, caret_h) = calculate_caret_size(px(12.0), px(16.0));
    assert_eq!(caret_w, px(2.0));
    assert_eq!(caret_h, px(15.0)); // (12.0 * 1.25).min(15.0).max(14.0) = 15.0

    // 2. Verify cursor lifecycle and task management on Editor
    cx.update(|_window, cx| {
        let editor_entity = cx.new(|cx| Editor::new(cx));
        editor_entity.update(cx, |editor, cx| {
            assert_eq!(editor.cursor_opacity(), 1.0);
            assert!(editor.has_blink_task());

            // on_blur clears task and resets opacity to 0.0
            editor.on_blur(cx);
            assert_eq!(editor.cursor_opacity(), 0.0);
            assert!(!editor.has_blink_task());

            // on_focus restores solid 1.0 opacity and restarts blink task
            editor.on_focus(cx);
            assert_eq!(editor.cursor_opacity(), 1.0);
            assert!(editor.has_blink_task());
        });
    });

    // 3. Verify cursor lifecycle and task management on InlineInput
    cx.update(|_window, cx| {
        let dummy_path = PathBuf::from("workspace/file.md");
        let input_entity = cx.new(|cx| {
            InlineInput::new(
                InlineInputKind::NewFile { parent: dummy_path },
                "hello",
                1,
                cx,
            )
        });

        input_entity.update(cx, |this, cx| {
            assert_eq!(this.cursor_opacity(), 1.0);
            assert!(this.has_blink_task());

            // on_blur clears task and resets opacity to 0.0
            this.on_blur(cx);
            assert_eq!(this.cursor_opacity(), 0.0);
            assert!(!this.has_blink_task());

            // Resetting cursor breathing restarts task and sets opacity to 1.0
            this.reset_cursor_blink(cx);
            assert_eq!(this.cursor_opacity(), 1.0);
            assert!(this.has_blink_task());
        });
    });
}

#[test]
fn test_cubic_ease_out_physics_curve() {
    use inviscid::ui::{SMOOTH_SCROLL_DURATION, cubic_ease_out};

    let start = px(0.0);
    let target = px(100.0);
    let duration = SMOOTH_SCROLL_DURATION; // 160ms

    // t = 0ms: Initial state
    let (val, active) = cubic_ease_out(start, target, std::time::Duration::ZERO, duration);
    assert_eq!(val, px(0.0));
    assert!(active);

    // t = 80ms (50% time): Rapid initial impulse covers 87.5% distance (1 - 0.5^3 = 0.875)
    let (val, active) = cubic_ease_out(
        start,
        target,
        std::time::Duration::from_millis(80),
        duration,
    );
    assert_eq!(val, px(87.5));
    assert!(active);

    // t = 120ms (75% time): 98.4375% distance covered
    let (val, active) = cubic_ease_out(
        start,
        target,
        std::time::Duration::from_millis(120),
        duration,
    );
    assert!((val - px(98.4375)).abs() < px(0.01));
    assert!(active);

    // t = 160ms (100% time): Crisp arrival at target with zero trailing creep
    let (val, active) = cubic_ease_out(
        start,
        target,
        std::time::Duration::from_millis(160),
        duration,
    );
    assert_eq!(val, px(100.0));
    assert!(!active);

    // t = 200ms (>100% time): Clamped at target and inactive
    let (val, active) = cubic_ease_out(
        start,
        target,
        std::time::Duration::from_millis(200),
        duration,
    );
    assert_eq!(val, px(100.0));
    assert!(!active);

    // Scrolling up: 200px to 100px
    let (val, active) = cubic_ease_out(
        px(200.0),
        px(100.0),
        std::time::Duration::from_millis(80),
        duration,
    );
    assert_eq!(val, px(112.5));
    assert!(active);
}

#[test]
fn test_editor_smooth_scrolling_physics() {
    use inviscid::Editor;

    let mut cx = TestAppContext::single();
    init_test_globals(&mut cx);

    let (_, cx) = cx.add_window_view(|_window, cx| InviscidWindow::new(cx));
    cx.run_until_parked();

    cx.update(|_window, cx| {
        let editor_entity = cx.new(|cx| {
            let mut ed = Editor::new(cx);
            ed.buffer_mut().insert_text(
                "line 1\nline 2\nline 3\nline 4\nline 5\nline 6\nline 7\nline 8\nline 9\nline 10\n\
                 line 11\nline 12\nline 13\nline 14\nline 15\nline 16\nline 17\nline 18\nline 19\nline 20\n\
                 line 21\nline 22\nline 23\nline 24\nline 25\nline 26\nline 27\nline 28\nline 29\nline 30\n\
                 line 31\nline 32\nline 33\nline 34\nline 35\nline 36\nline 37\nline 38\nline 39\nline 40\n\
                 line 41\nline 42\nline 43\nline 44\nline 45\nline 46\nline 47\nline 48\nline 49\nline 50\n"
            );
            ed
        });

        editor_entity.update(cx, |editor, cx| {
            let max_scroll = px(1000.0);

            // 1. Touchpad Precision Scrolling (ScrollDelta::Pixels):
            // Direct 1:1 displacement without animation delay
            editor.scroll_state_mut().scroll_direct(px(-45.0), max_scroll);
            assert_eq!(editor.scroll_state().current_scroll_top, px(45.0));
            assert_eq!(editor.scroll_state().target_scroll_top, px(45.0));
            assert!(!editor.scroll_state().is_animating());
            assert!(!editor.has_scroll_task());

            // 2. Mouse Wheel Notch (ScrollDelta::Lines):
            // Single notch downwards (Windows standard 3 lines = 100px) advances target by 100px (from 45 to 145)
            let wheel_event = gpui::ScrollWheelEvent {
                delta: gpui::ScrollDelta::Lines(gpui::Point::new(0.0, -3.0)),
                ..Default::default()
            };
            editor.handle_scroll_wheel(&wheel_event, cx);
            assert_eq!(editor.scroll_state().target_scroll_top, px(145.0));
            assert_eq!(editor.scroll_state().current_scroll_top, px(45.0));
            assert_eq!(editor.scroll_state().anim_start_scroll_top, px(45.0));
            assert!(editor.scroll_state().is_animating());
            assert!(editor.has_scroll_task());

            // 3. Step physics at halfway point (80ms):
            // Distance = 100px. At 80ms, 87.5% progress -> current = 45 + 87.5 = 132.5px
            let animating = editor.scroll_state_mut().step_interpolation_with_elapsed(std::time::Duration::from_millis(80));
            assert!(animating);
            assert_eq!(editor.scroll_state().current_scroll_top, px(132.5));

            // 4. Continuous Impulse Stacking:
            // Spin wheel again at 80ms (l.y = -1.0) -> target advances by 100px (145 -> 245)
            // anim_start_scroll_top re-anchors to current (132.5px) without stutter
            editor.handle_scroll_wheel(&wheel_event, cx);
            assert_eq!(editor.scroll_state().target_scroll_top, px(245.0));
            assert_eq!(editor.scroll_state().anim_start_scroll_top, px(132.5));
            assert_eq!(editor.scroll_state().current_scroll_top, px(132.5));

            // Complete the new animation (160ms) -> stops crisply at 245px
            let animating = editor.scroll_state_mut().step_interpolation_with_elapsed(std::time::Duration::from_millis(160));
            assert!(!animating);
            assert_eq!(editor.scroll_state().current_scroll_top, px(245.0));
            assert!(!editor.scroll_state().is_animating());

            // 5. Touchpad interruption:
            // While a wheel animation is in-flight, a touchpad event immediately cancels animation and sets 1:1 position
            editor.handle_scroll_wheel(&wheel_event, cx); // Starts animation to 345
            assert!(editor.scroll_state().is_animating());
            assert!(editor.has_scroll_task());

            let touchpad_event = gpui::ScrollWheelEvent {
                delta: gpui::ScrollDelta::Pixels(gpui::Point::new(px(0.0), px(-20.0))),
                ..Default::default()
            };
            editor.handle_scroll_wheel(&touchpad_event, cx);
            assert_eq!(editor.scroll_state().current_scroll_top, px(265.0)); // 245 + 20
            assert_eq!(editor.scroll_state().target_scroll_top, px(265.0));
            assert!(!editor.scroll_state().is_animating());
            assert!(!editor.has_scroll_task());
        });
    });
}

#[test]
fn test_project_panel_tree_scroll_uses_shared_physics() {
    let mut cx = TestAppContext::single();
    init_test_globals(&mut cx);

    let temp_dir = create_temp_workspace_dir("tree_scroll");

    let (app, cx) = cx.add_window_view(|_window, cx| InviscidWindow::new(cx));
    cx.run_until_parked();

    let ws = cx.update(|_window, cx| app.read(cx).workspace().clone());
    cx.update(|_window, cx| {
        ws.update(cx, |ws, cx| {
            ws.open_folder(temp_dir.clone(), cx);
        });
    });
    cx.run_until_parked();

    let panel_entity = cx.update(|_window, cx| ws.read(cx).project_panel().clone());
    cx.update(|_window, cx| {
        panel_entity.update(cx, |panel, _cx| {
            // The tree scroll state delegates to the shared smooth scroll physics:
            // a touchpad delta displaces 1:1 and leaves no animation running ...
            panel.tree_scroll_mut().scroll_direct(px(-60.0), px(800.0));
            assert_eq!(panel.tree_scroll().current_scroll_top, px(60.0));
            assert_eq!(panel.tree_scroll().target_scroll_top, px(60.0));
            assert!(!panel.tree_scroll().is_animating());

            // ... while a mouse wheel notch becomes an animated impulse.
            panel
                .tree_scroll_mut()
                .scroll_by_wheel(px(-100.0), px(800.0));
            assert_eq!(panel.tree_scroll().target_scroll_top, px(160.0));
            assert!(panel.tree_scroll().is_animating());
        });
    });

    let _ = fs::remove_dir_all(&temp_dir);
}

#[test]
fn test_tab_bar_scroll_to_tab_and_zero_bounds_wheel_events() {
    let mut cx = TestAppContext::single();
    init_test_globals(&mut cx);

    let (app, cx) = cx.add_window_view(|_window, cx| InviscidWindow::new(cx));
    cx.run_until_parked();

    let ws = cx.update(|_window, cx| app.read(cx).workspace().clone());

    // Open multiple tabs
    cx.update(|_window, cx| {
        ws.update(cx, |ws, cx| {
            for _ in 0..10 {
                ws.new_tab(cx);
            }
        });
    });
    cx.run_until_parked();

    let tab_bar = cx.update(|_window, cx| ws.read(cx).tab_bar().clone());

    cx.update(|_window, cx| {
        tab_bar.update(cx, |tb, cx| {
            assert_eq!(tb.tabs_meta().len(), 10);

            // scroll_to_tab drives the shared smooth scroll physics.
            tb.scroll_to_tab(0, cx);

            // handle_scroll_wheel must survive degenerate (unmeasured, zero) max_scroll bounds.
            let wheel_lines = gpui::ScrollWheelEvent {
                delta: gpui::ScrollDelta::Lines(gpui::Point::new(0.0, -3.0)),
                ..Default::default()
            };
            tb.handle_scroll_wheel(&wheel_lines, cx);

            let wheel_pixels = gpui::ScrollWheelEvent {
                delta: gpui::ScrollDelta::Pixels(gpui::Point::new(px(-20.0), px(0.0))),
                ..Default::default()
            };
            tb.handle_scroll_wheel(&wheel_pixels, cx);
        });
    });
}

/// Mounts the app on a throwaway workspace and hands back the project panel, focused.
///
/// Every test below drives the app through `simulate_keystrokes` rather than calling a handler
/// directly: the bug they guard against was a *binding* swallowing `escape` before any
/// `on_key_down` listener could run, which a direct method call cannot observe.
fn mount_focused_panel<'a>(
    tcx: &'a mut TestAppContext,
    prefix: &str,
) -> (
    PathBuf,
    gpui::Entity<InviscidWindow>,
    gpui::Entity<ProjectPanel>,
    &'a mut gpui::VisualTestContext,
) {
    init_test_globals(tcx);

    let temp_dir = create_temp_workspace_dir(prefix);
    let (app, cx) = tcx.add_window_view(|_window, cx| InviscidWindow::new(cx));
    cx.run_until_parked();

    let panel = cx.update(|_window, cx| {
        let ws = app.read(cx).workspace().clone();
        ws.update(cx, |ws, cx| ws.open_folder(temp_dir.clone(), cx));
        ws.read(cx).project_panel().clone()
    });
    cx.run_until_parked();

    refocus_panel(cx, &panel);
    assert!(
        cx.update(|window, cx| panel.read(cx).focus_handle(cx).is_focused(window)),
        "the project panel must hold focus for key dispatch to reach it"
    );

    (temp_dir, app, panel, cx)
}

/// `open_folder` hands focus to the editor of the file it opens; take it back for the panel.
fn refocus_panel(cx: &mut gpui::VisualTestContext, panel: &gpui::Entity<ProjectPanel>) {
    let focus_handle = cx.update(|_window, cx| panel.read(cx).focus_handle(cx));
    cx.update(|window, _cx| window.focus(&focus_handle));
    cx.run_until_parked();
}

#[test]
fn test_escape_closes_the_project_panel_context_menu() {
    let mut tcx = TestAppContext::single();
    let (temp_dir, _app, panel, cx) = mount_focused_panel(&mut tcx, "escape_context_menu");

    let target = temp_dir.join("README.md");
    cx.update(|_window, cx| {
        panel.update(cx, |panel, cx| {
            panel.open_context_menu(
                gpui::point(px(40.0), px(60.0)),
                target.clone(),
                false,
                false,
                cx,
            );
        });
    });
    cx.run_until_parked();
    assert!(
        cx.update(|_window, cx| panel.read(cx).active_context_menu().is_some()),
        "precondition: the context menu is open"
    );

    cx.simulate_keystrokes("escape");
    cx.run_until_parked();

    assert!(
        cx.update(|_window, cx| panel.read(cx).active_context_menu().is_none()),
        "escape must reach the panel and close its context menu"
    );

    let _ = fs::remove_dir_all(&temp_dir);
}

#[test]
fn test_escape_still_closes_the_titlebar_menu() {
    // The other half of the same coin: gating `escape` on an open menu must not cost the
    // title-bar dropdown its dismissal key.
    let mut tcx = TestAppContext::single();
    let (temp_dir, app, _panel, cx) = mount_focused_panel(&mut tcx, "escape_titlebar_menu");

    cx.update(|_window, cx| {
        app.update(cx, |app, cx| {
            app.set_active_menu(Some(inviscid::app::ActiveMenu::File), cx);
        });
    });
    cx.run_until_parked();
    assert!(cx.update(|_window, cx| app.read(cx).active_menu().is_some()));

    cx.simulate_keystrokes("escape");
    cx.run_until_parked();

    assert!(
        cx.update(|_window, cx| app.read(cx).active_menu().is_none()),
        "escape must still dismiss an open title-bar menu"
    );

    let _ = fs::remove_dir_all(&temp_dir);
}

#[test]
fn test_panel_shortcuts_dispatch_to_the_selection() {
    use inviscid::app::project_panel::context_menu::FileClipboard;

    let mut tcx = TestAppContext::single();
    let (temp_dir, _app, panel, cx) = mount_focused_panel(&mut tcx, "panel_shortcuts");
    let target = temp_dir.join("README.md");

    cx.update(|_window, cx| {
        panel.update(cx, |panel, cx| {
            panel.set_selected_path(Some(target.clone()), cx)
        });
    });

    // F2 opens the rename box instead of leaving the key unbound.
    cx.simulate_keystrokes("f2");
    cx.run_until_parked();
    assert!(
        cx.update(|_window, cx| panel.read(cx).inline_input().is_some()),
        "f2 must start an inline rename"
    );

    // The box owns the keyboard from here. `delete` and `secondary-delete` are the sharp edge:
    // the panel binds both, and a matched action listener consumes the keystroke before any
    // `on_key_down` listener runs — so unless the panel's commands exclude the box, they silently
    // break text editing. The box's own text is the observable.
    let type_into_box = |cx: &mut gpui::VisualTestContext, text: &str| {
        cx.update(|_window, cx| {
            let input = panel.read(cx).inline_input().unwrap().clone();
            input.update(cx, |input, cx| {
                input.buffer_mut().select_all();
                input.buffer_mut().insert_text(text);
                input
                    .buffer_mut()
                    .set_cursor(inviscid::buffer::Position::new(0, 0));
                cx.notify();
            });
        });
        cx.run_until_parked();
    };
    let box_text = |cx: &mut gpui::VisualTestContext| {
        cx.update(|_window, cx| {
            panel
                .read(cx)
                .inline_input()
                .map(|input| input.read(cx).text())
        })
    };

    type_into_box(cx, "renamed.md");
    assert_eq!(box_text(cx), Some("renamed.md".to_string()));

    // A second F2 must not restart the box and throw the typed name away.
    cx.simulate_keystrokes("f2");
    cx.run_until_parked();
    assert_eq!(
        box_text(cx),
        Some("renamed.md".to_string()),
        "f2 must be inert while the rename box owns the keyboard"
    );

    // Delete must delete text, not queue a Trash confirmation.
    cx.simulate_keystrokes("delete");
    cx.run_until_parked();
    assert_eq!(box_text(cx), Some("enamed.md".to_string()));
    assert!(
        cx.update(|_window, cx| panel.read(cx).active_confirm_modal().is_none()),
        "panel commands must stay inert while a rename is being typed"
    );

    // Ctrl+Delete must delete the next word for the same reason.
    cx.simulate_keystrokes("secondary-delete");
    cx.run_until_parked();
    assert_eq!(box_text(cx), Some(".md".to_string()));

    cx.update(|_window, cx| panel.update(cx, |panel, cx| panel.handle_inline_cancel(cx)));
    refocus_panel(cx, &panel);

    // Delete asks for confirmation before trashing the selected entry.
    cx.simulate_keystrokes("delete");
    cx.run_until_parked();
    let modal_kind = cx.update(|_window, cx| {
        panel
            .read(cx)
            .active_confirm_modal()
            .map(|modal| (modal.kind, modal.target_path.clone()))
    });
    assert_eq!(
        modal_kind,
        Some((
            inviscid::app::project_panel::ConfirmModalKind::Trash,
            target.clone()
        )),
        "delete must queue a Trash confirmation for the selected entry"
    );

    cx.update(|_window, cx| panel.update(cx, |panel, cx| panel.cancel_delete_modal(cx)));
    refocus_panel(cx, &panel);

    // Ctrl+C fills the panel's own clipboard rather than copying editor text.
    cx.simulate_keystrokes("secondary-c");
    cx.run_until_parked();
    assert_eq!(
        cx.update(|_window, cx| panel.read(cx).clipboard().clone()),
        FileClipboard::Copy(target),
        "ctrl+c must copy the selected entry"
    );

    let _ = fs::remove_dir_all(&temp_dir);
}

/// The remaining file-tree commands, end to end.
///
/// Every one of these needs an entry in four files (action, keybinding, menu enum + item, and both
/// halves of the panel dispatch). Forgetting the `on_action` listener in particular fails
/// *silently* — an unhandled action leaves `propagate_event` alone, the keystroke falls through to
/// `on_key_down`, and nothing happens. So each command gets an observable effect asserted here.
#[test]
fn test_panel_cut_paste_and_permanent_delete_commands() {
    use inviscid::app::project_panel::ConfirmModalKind;
    use inviscid::app::project_panel::context_menu::FileClipboard;

    let mut tcx = TestAppContext::single();
    let (temp_dir, _app, panel, cx) = mount_focused_panel(&mut tcx, "panel_fs_commands");

    let notes = temp_dir.join("z_notes.md");
    let docs = temp_dir.join("docs");
    let src = temp_dir.join("src");

    let select = |cx: &mut gpui::VisualTestContext, path: &PathBuf| {
        cx.update(|_window, cx| {
            panel.update(cx, |panel, cx| {
                panel.set_selected_path(Some(path.clone()), cx)
            });
        });
        refocus_panel(cx, &panel);
    };

    // Copy then paste: the duplicate lands in the selected folder, the original stays.
    select(cx, &notes);
    cx.simulate_keystrokes("secondary-c");
    cx.run_until_parked();
    assert_eq!(
        cx.update(|_window, cx| panel.read(cx).clipboard().clone()),
        FileClipboard::Copy(notes.clone())
    );

    select(cx, &docs);
    cx.simulate_keystrokes("secondary-v");
    cx.run_until_parked();
    assert!(
        docs.join("z_notes.md").exists(),
        "ctrl+v must copy into the selected folder"
    );
    assert!(notes.exists(), "a copy must leave the original in place");

    // Cut then paste: the file moves, and pasting consumes the clipboard.
    select(cx, &notes);
    cx.simulate_keystrokes("secondary-x");
    cx.run_until_parked();
    assert_eq!(
        cx.update(|_window, cx| panel.read(cx).clipboard().clone()),
        FileClipboard::Cut(notes.clone())
    );

    select(cx, &src);
    cx.simulate_keystrokes("secondary-v");
    cx.run_until_parked();
    assert!(
        src.join("z_notes.md").exists(),
        "ctrl+v after ctrl+x must move the file"
    );
    assert!(!notes.exists(), "a cut must remove the original");
    assert_eq!(
        cx.update(|_window, cx| panel.read(cx).clipboard().clone()),
        FileClipboard::None,
        "the clipboard must be cleared once the cut is pasted"
    );

    // Ctrl+Delete asks for permanent deletion instead of the trash.
    let moved = src.join("z_notes.md");
    select(cx, &moved);
    cx.simulate_keystrokes("secondary-delete");
    cx.run_until_parked();
    let modal = cx.update(|_window, cx| {
        panel
            .read(cx)
            .active_confirm_modal()
            .map(|modal| (modal.kind, modal.target_path.clone()))
    });
    assert_eq!(
        modal,
        Some((ConfirmModalKind::Delete, moved)),
        "ctrl+delete must queue a permanent-delete confirmation"
    );

    let _ = fs::remove_dir_all(&temp_dir);
}

#[test]
fn test_workspace_from_state_with_missing_files_active_idx() {
    let mut cx = TestAppContext::single();
    init_test_globals(&mut cx);

    let temp_dir = create_temp_workspace_dir("missing_files_active_idx");
    let file1 = temp_dir.join("README.md");
    let file2 = temp_dir.join("z_notes.md");
    let missing1 = temp_dir.join("non_existent_1.md");
    let missing2 = temp_dir.join("non_existent_2.md");

    let state = WorkspaceState {
        root_dir: Some(temp_dir.clone()),
        active_file: Some(file1.clone()),
        open_files: vec![missing1, missing2, file1.clone(), file2.clone()],
        sidebar_visible: true,
        sidebar_width: Some(240.0),
    };

    let (_app, cx) = cx.add_window_view(|_window, cx| InviscidWindow::new(cx));
    cx.run_until_parked();

    cx.update(|_window, cx| {
        let ws_entity = cx.new(|cx| Workspace::from_state(&state, cx));
        let ws = ws_entity.read(cx);
        assert_eq!(ws.tab_count(), 2);
        // Previously, active_idx was assigned enumerate index 2, resulting in index 1 (file2).
        // With the fix, active_idx tracks the real loaded tabs index 0 (file1).
        assert_eq!(ws.active_tab_idx(), 0);
        assert_eq!(ws.active_file_path(cx), Some(file1.as_path()));
    });

    let _ = fs::remove_dir_all(&temp_dir);
}

#[test]
fn test_workspace_restore_from_state_preserves_entities_and_focus() {
    let mut cx = TestAppContext::single();
    init_test_globals(&mut cx);

    let temp_dir1 = create_temp_workspace_dir("restore_preserve_1");
    let temp_dir2 = create_temp_workspace_dir("restore_preserve_2");

    let (app, cx) = cx.add_window_view(|_window, cx| InviscidWindow::new(cx));
    cx.run_until_parked();

    let (initial_focus_handle, tab_bar_id, project_panel_id, status_bar_id) =
        cx.update(|_window, cx| {
            let ws = app.read(cx).workspace().read(cx);
            (
                ws.focus_handle(cx),
                ws.tab_bar().entity_id(),
                ws.project_panel().entity_id(),
                ws.status_bar().entity_id(),
            )
        });

    let target_state = WorkspaceState {
        root_dir: Some(temp_dir2.clone()),
        active_file: Some(temp_dir2.join("README.md")),
        open_files: vec![temp_dir2.join("README.md")],
        sidebar_visible: false,
        sidebar_width: Some(280.0),
    };

    cx.update(|window, cx| {
        let ws = app.read(cx).workspace().clone();
        ws.update(cx, |ws, cx| {
            ws.restore_from_state(&target_state, cx);
            ws.focus_active_editor(window, cx);
        });
    });
    cx.run_until_parked();

    cx.update(|_window, cx| {
        let ws = app.read(cx).workspace().read(cx);
        // Entities and focus handle MUST be preserved in-place, NOT destroyed/re-instantiated!
        assert_eq!(ws.focus_handle(cx), initial_focus_handle);
        assert_eq!(ws.tab_bar().entity_id(), tab_bar_id);
        assert_eq!(ws.project_panel().entity_id(), project_panel_id);
        assert_eq!(ws.status_bar().entity_id(), status_bar_id);

        // State attributes must be updated properly
        assert_eq!(ws.root_dir(), Some(temp_dir2.as_path()));
        assert_eq!(ws.tab_count(), 1);
        assert_eq!(
            ws.active_file_path(cx),
            Some(temp_dir2.join("README.md").as_path())
        );
        assert!(!ws.sidebar_visible());
        assert_eq!(ws.sidebar_width(), px(280.0));
    });

    let _ = fs::remove_dir_all(&temp_dir1);
    let _ = fs::remove_dir_all(&temp_dir2);
}
