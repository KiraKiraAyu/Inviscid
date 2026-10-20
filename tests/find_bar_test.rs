use gpui::{Entity, Focusable, TestAppContext, VisualTestContext};
use inviscid::buffer::{Position, Selection, TextBuffer};
use inviscid::config::AppConfig;
use inviscid::editor::{Editor, HighlightKind};
use inviscid::theme::ThemeManager;

const FIND: &str = if cfg!(target_os = "macos") {
    "cmd-f"
} else {
    "ctrl-f"
};

fn mount_editor<'a>(
    tcx: &'a mut TestAppContext,
    text: &str,
) -> (Entity<Editor>, &'a mut VisualTestContext) {
    let config = AppConfig::default().with_theme("Dracula");
    let theme_manager = ThemeManager::from_config(&config);
    tcx.update(|cx| {
        cx.set_global(theme_manager);
        cx.set_global(config);
        inviscid::app::actions::bind_default_bindings(cx);
    });

    let buffer = TextBuffer::from_str(text, None);
    let (editor, cx) = tcx.add_window_view(|_window, cx| Editor::new_with_buffer(buffer, cx));
    let focus = cx.update(|_window, cx| editor.read(cx).focus_handle(cx));
    cx.update(|window, _| window.focus(&focus));
    cx.run_until_parked();
    (editor, cx)
}

fn query_text(editor: &Entity<Editor>, cx: &mut VisualTestContext) -> Option<String> {
    cx.update(|_window, cx| {
        editor
            .read(cx)
            .find_bar()
            .map(|bar| bar.read(cx).query_text(cx))
    })
}

fn current_match(editor: &Entity<Editor>, cx: &mut VisualTestContext) -> Option<(usize, usize)> {
    cx.update(|_window, cx| {
        let state = editor.read(cx).find_state();
        state
            .current_match_number()
            .map(|n| (n, state.match_count()))
    })
}

#[test]
fn test_find_shortcut_opens_the_bar_and_typing_searches() {
    let mut tcx = TestAppContext::single();
    let (editor, cx) = mount_editor(&mut tcx, "alpha beta\nbeta gamma beta");

    cx.simulate_keystrokes(FIND);
    assert_eq!(query_text(&editor, cx).as_deref(), Some(""));

    cx.simulate_input("beta");
    cx.run_until_parked();
    assert_eq!(current_match(&editor, cx), Some((1, 3)));

    let highlighted = cx.update(|_window, cx| {
        editor
            .read(cx)
            .search_highlights(1)
            .into_iter()
            .map(|h| h.cols)
            .collect::<Vec<_>>()
    });
    assert_eq!(highlighted, vec![0..4, 11..15]);
}

#[test]
fn test_enter_and_shift_enter_step_through_matches_from_the_bar() {
    let mut tcx = TestAppContext::single();
    let (editor, cx) = mount_editor(&mut tcx, "x x x");

    cx.simulate_keystrokes(FIND);
    cx.simulate_input("x");
    cx.run_until_parked();
    assert_eq!(current_match(&editor, cx), Some((1, 3)));

    cx.simulate_keystrokes("enter");
    cx.simulate_keystrokes("enter");
    assert_eq!(current_match(&editor, cx), Some((3, 3)));
    assert_eq!(
        cx.update(|_window, cx| editor.read(cx).cursor_pos()),
        Position::new(0, 4),
        "the caret follows the current match"
    );

    cx.simulate_keystrokes("shift-enter");
    assert_eq!(current_match(&editor, cx), Some((2, 3)));

    cx.simulate_keystrokes("f3");
    assert_eq!(current_match(&editor, cx), Some((3, 3)));
}

#[test]
fn test_escape_closes_the_bar_and_returns_focus_to_the_document() {
    let mut tcx = TestAppContext::single();
    let (editor, cx) = mount_editor(&mut tcx, "find me");

    cx.simulate_keystrokes(FIND);
    cx.simulate_input("me");
    cx.run_until_parked();
    assert!(cx.update(|_window, cx| !editor.read(cx).search_highlights(0).is_empty()));

    cx.simulate_keystrokes("escape");
    cx.run_until_parked();

    cx.update(|window, cx| {
        let ed = editor.read(cx);
        assert!(!ed.is_find_bar_visible());
        assert!(
            ed.search_highlights(0).is_empty(),
            "closing drops the highlights"
        );
        assert!(ed.focus_handle(cx).is_focused(window));
    });
}

#[test]
fn test_find_seeds_the_query_from_a_single_line_selection() {
    let mut tcx = TestAppContext::single();
    let (editor, cx) = mount_editor(&mut tcx, "one two\nthree two");
    cx.update(|_window, cx| {
        editor.update(cx, |ed, _| {
            ed.buffer_mut()
                .set_selection(Selection::new(Position::new(0, 4), Position::new(0, 7)));
        })
    });

    cx.simulate_keystrokes(FIND);
    cx.run_until_parked();

    assert_eq!(query_text(&editor, cx).as_deref(), Some("two"));
    assert_eq!(current_match(&editor, cx), Some((1, 2)));
}

#[test]
fn test_typing_in_the_bar_does_not_edit_the_document() {
    let mut tcx = TestAppContext::single();
    let (editor, cx) = mount_editor(&mut tcx, "keep me");

    cx.simulate_keystrokes(FIND);
    cx.simulate_input("zz");
    cx.simulate_keystrokes("backspace enter");
    cx.run_until_parked();

    cx.update(|_window, cx| {
        let ed = editor.read(cx);
        assert_eq!(ed.buffer().line(0), Some("keep me"));
        assert!(!ed.is_dirty());
    });
}

#[test]
fn test_match_highlight_tracks_the_active_match() {
    let mut tcx = TestAppContext::single();
    let (editor, cx) = mount_editor(&mut tcx, "ab ab");

    cx.simulate_keystrokes(FIND);
    cx.simulate_input("ab");
    cx.simulate_keystrokes("enter");
    cx.run_until_parked();

    let kinds = cx.update(|_window, cx| {
        editor
            .read(cx)
            .search_highlights(0)
            .into_iter()
            .map(|h| h.kind)
            .collect::<Vec<_>>()
    });
    assert_eq!(
        kinds,
        vec![HighlightKind::SearchMatch, HighlightKind::ActiveSearchMatch]
    );
}
