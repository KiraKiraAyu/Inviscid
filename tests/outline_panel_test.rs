use std::fs;

use gpui::{TestAppContext, px};
use inviscid::app::Workspace;
use inviscid::app::workspace::{DEFAULT_SIDEBAR_WIDTH, MAX_SIDEBAR_WIDTH, MIN_SIDEBAR_WIDTH};
use inviscid::buffer::{Position, SourceLine};
use inviscid::config::AppConfig;
use inviscid::theme::ThemeManager;

#[test]
fn outline_tracks_active_document_and_jumps_to_heading() {
    let mut cx = TestAppContext::single();
    let config = AppConfig::default().with_theme("Dracula");
    let theme_manager = ThemeManager::from_config(&config);
    cx.update(|cx| {
        cx.set_global(theme_manager);
        cx.set_global(config);
    });

    let path =
        std::env::temp_dir().join(format!("inviscid_outline_test_{}.md", std::process::id()));
    fs::write(&path, "# First\n\n## Second\nBody text").unwrap();

    let (workspace, cx) = cx.add_window_view(|_window, cx| Workspace::new(Some(path.clone()), cx));
    workspace.read_with(cx, |workspace, _| {
        assert_eq!(
            workspace.active_sidebar(),
            Some(inviscid::app::SidebarView::Files)
        );
        assert!(!workspace.outline_visible());
    });

    cx.update(|window, cx| {
        workspace.update(cx, |workspace, cx| {
            workspace.toggle_outline_panel(window, cx)
        });
    });
    workspace.read_with(cx, |workspace, cx| {
        assert_eq!(
            workspace.active_sidebar(),
            Some(inviscid::app::SidebarView::Files)
        );
        assert!(workspace.outline_visible());
        assert!(workspace.status_bar().read(cx).outline_visible());
    });

    workspace.read_with(cx, |workspace, cx| {
        let outline = workspace.outline_panel().read(cx);
        assert_eq!(
            outline
                .entries()
                .iter()
                .map(|entry| (entry.title.as_str(), entry.source_line.index()))
                .collect::<Vec<_>>(),
            [("First", 0), ("Second", 2)]
        );
    });

    cx.update(|window, cx| {
        workspace.update(cx, |workspace, cx| {
            workspace.toggle_outline_panel(window, cx)
        });
    });
    workspace.read_with(cx, |workspace, cx| {
        assert!(!workspace.outline_visible());
        assert!(!workspace.status_bar().read(cx).outline_visible());
        assert_eq!(
            workspace.active_sidebar(),
            Some(inviscid::app::SidebarView::Files)
        );
    });

    workspace.update(cx, |workspace, cx| {
        workspace.jump_to_outline(SourceLine::new(2), cx);
    });
    workspace.read_with(cx, |workspace, cx| {
        let editor = workspace.active_editor().unwrap().read(cx);
        assert_eq!(editor.cursor_pos(), Position::new(SourceLine::new(2), 0));
        assert_eq!(
            workspace.outline_panel().read(cx).active_line(),
            Some(SourceLine::new(2))
        );
    });

    let _ = fs::remove_file(path);
}

#[test]
fn outline_panel_width_resizes_from_its_left_edge_and_resets() {
    let mut cx = TestAppContext::single();
    let config = AppConfig::default().with_theme("Dracula");
    let theme_manager = ThemeManager::from_config(&config);
    cx.update(|cx| {
        cx.set_global(theme_manager);
        cx.set_global(config);
    });

    let (workspace, cx) = cx.add_window_view(|_window, cx| Workspace::new(None, cx));
    workspace.read_with(cx, |workspace, _| {
        assert_eq!(workspace.outline_width(), px(DEFAULT_SIDEBAR_WIDTH));
        assert_eq!(workspace.sidebar_width(), px(DEFAULT_SIDEBAR_WIDTH));
    });

    workspace.update(cx, |workspace, cx| {
        workspace.start_outline_resize(px(600.0), cx);
        assert!(workspace.is_resizing_outline());
        assert!(!workspace.is_resizing_sidebar());
    });
    workspace.update(cx, |workspace, cx| {
        workspace.handle_outline_resize_drag(px(560.0), cx);
        assert_eq!(workspace.outline_width(), px(DEFAULT_SIDEBAR_WIDTH + 40.0));

        workspace.handle_outline_resize_drag(px(1000.0), cx);
        assert_eq!(workspace.outline_width(), px(MIN_SIDEBAR_WIDTH));
        workspace.finish_panel_resize(cx);

        workspace.set_outline_width(px(1200.0), cx);
        assert_eq!(workspace.outline_width(), px(MAX_SIDEBAR_WIDTH));
        workspace.reset_outline_width(cx);
        assert_eq!(workspace.outline_width(), px(DEFAULT_SIDEBAR_WIDTH));

        assert_eq!(workspace.sidebar_width(), px(DEFAULT_SIDEBAR_WIDTH));
    });
}
