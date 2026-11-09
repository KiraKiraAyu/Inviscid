use super::{Position, SourceLine, Workspace};
use gpui::TestAppContext;

fn init(cx: &mut TestAppContext) {
    cx.update(|cx| {
        let config = crate::config::AppConfig::default();
        cx.set_global(crate::theme::ThemeManager::from_config(&config));
        cx.set_global(config);
    });
}

#[gpui::test]
fn opens_immediately_merges_pending_duplicates_and_installs_requested_position(
    cx: &mut TestAppContext,
) {
    init(cx);
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("document.md");
    std::fs::write(&path, "first\nsecond target\nthird").unwrap();
    let (ws, cx) = cx.add_window_view(|_, cx| Workspace::new(None, cx));
    ws.update(cx, |ws, cx| {
        ws.new_tab(cx);
        ws.open_file(path.clone(), cx);
        assert_eq!(ws.tab_count(), 1);
        let editor = ws.active_editor().unwrap().read(cx);
        assert!(editor.is_loading());
        assert!(!editor.can_edit());
        assert!(editor.buffer().is_empty());
        ws.open_file_at(path.clone(), Position::new(SourceLine::new(1), 7), cx);
        assert_eq!(ws.tab_count(), 1);
        assert!(ws.get_tabs_meta(cx)[0].is_loading);
        // A second UI command runs while the file's background work is pending.
        ws.new_tab(cx);
        assert_eq!(ws.tab_count(), 2);
        ws.switch_tab(0, cx);
    });
    cx.run_until_parked();
    ws.read_with(cx, |ws, cx| {
        let editor = ws.active_editor().unwrap().read(cx);
        assert!(!editor.is_loading());
        assert!(editor.can_edit());
        assert_eq!(
            editor.buffer().to_string_content(),
            "first\nsecond target\nthird"
        );
        assert_eq!(editor.cursor_pos(), Position::new(SourceLine::new(1), 7));
        assert!(!ws.get_tabs_meta(cx)[0].is_loading);
        assert!(ws.tabs[0]._load_task.is_none());
    });
}

#[gpui::test]
fn closing_loading_tab_cancels_install_and_releases_editor(cx: &mut TestAppContext) {
    init(cx);
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("document.md");
    std::fs::write(&path, "loaded text").unwrap();
    let (ws, cx) = cx.add_window_view(|_, cx| Workspace::new(None, cx));
    let retained = ws.update(cx, |ws, cx| {
        ws.open_file(path, cx);
        let editor = ws.active_editor().unwrap().clone();
        assert!(editor.read(cx).is_loading());
        ws.close_tab(0, cx);
        editor
    });
    cx.run_until_parked();
    retained.read_with(cx, |editor, _| assert!(editor.is_loading()));
    assert_eq!(ws.read_with(cx, |ws, _| ws.tab_count()), 0);
    let weak = retained.downgrade();
    drop(retained);
    cx.run_until_parked();
    assert!(weak.upgrade().is_none());
}

#[gpui::test]
fn asynchronous_load_failure_stays_read_only_and_reports_status(cx: &mut TestAppContext) {
    init(cx);
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("invalid.md");
    std::fs::write(&path, [0xff, 0xfe]).unwrap();
    let (ws, cx) = cx.add_window_view(|_, cx| Workspace::new(Some(path.clone()), cx));
    cx.run_until_parked();
    ws.read_with(cx, |ws, cx| {
        let editor = ws.active_editor().unwrap().read(cx);
        assert!(!editor.is_loading());
        assert!(!editor.can_edit());
        assert!(editor.status_meta().unwrap().load_error.is_some());
        assert!(ws.status_bar.read(cx).meta().unwrap().load_error.is_some());
    });
    assert_eq!(std::fs::read(path).unwrap(), [0xff, 0xfe]);
}

#[gpui::test]
fn renaming_loading_file_retries_new_path_without_restoring_old_path(cx: &mut TestAppContext) {
    init(cx);
    let dir = tempfile::tempdir().unwrap();
    let old = dir.path().join("old.md");
    let new = dir.path().join("new.md");
    std::fs::write(&old, "renamed content").unwrap();
    let (ws, cx) = cx.add_window_view(|_, cx| Workspace::new(None, cx));
    ws.update(cx, |ws, cx| {
        ws.open_file(old.clone(), cx);
        std::fs::rename(&old, &new).unwrap();
        ws.handle_path_renamed(&old, &new, cx);
        assert!(ws.active_editor().unwrap().read(cx).is_loading());
    });
    cx.run_until_parked();
    ws.read_with(cx, |ws, cx| {
        let editor = ws.active_editor().unwrap().read(cx);
        assert_eq!(editor.file_path(), Some(new.as_path()));
        assert!(editor.can_edit());
        assert_eq!(editor.buffer().to_string_content(), "renamed content");
    });
}
