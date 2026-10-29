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

const REPLACE: &str = if cfg!(target_os = "macos") {
    "cmd-h"
} else {
    "ctrl-h"
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
            .search_highlights(inviscid::buffer::SourceLine::new(1))
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
        Position::new(inviscid::buffer::SourceLine::new(0), 4),
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
    assert!(cx.update(|_window, cx| {
        !editor
            .read(cx)
            .search_highlights(inviscid::buffer::SourceLine::new(0))
            .is_empty()
    }));

    cx.simulate_keystrokes("escape");
    cx.run_until_parked();

    cx.update(|window, cx| {
        let ed = editor.read(cx);
        assert!(!ed.is_find_bar_visible());
        assert!(
            ed.search_highlights(inviscid::buffer::SourceLine::new(0))
                .is_empty(),
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
            ed.buffer_mut().set_selection(Selection::new(
                Position::new(inviscid::buffer::SourceLine::new(0), 4),
                Position::new(inviscid::buffer::SourceLine::new(0), 7),
            ));
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
        assert_eq!(
            ed.buffer().line(inviscid::buffer::SourceLine::new(0)),
            Some("keep me")
        );
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
            .search_highlights(inviscid::buffer::SourceLine::new(0))
            .into_iter()
            .map(|h| h.kind)
            .collect::<Vec<_>>()
    });
    assert_eq!(
        kinds,
        vec![HighlightKind::SearchMatch, HighlightKind::ActiveSearchMatch]
    );
}

#[test]
fn test_replace_shortcut_opens_replace_mode() {
    let mut tcx = TestAppContext::single();
    let (editor, cx) = mount_editor(&mut tcx, "foo bar");

    cx.simulate_keystrokes(REPLACE);
    cx.run_until_parked();

    cx.update(|_window, cx| {
        let ed = editor.read(cx);
        assert!(ed.is_find_bar_visible());
    });

    cx.simulate_input("foo");
    cx.simulate_keystrokes("tab");
    cx.simulate_input("qux");
    cx.simulate_keystrokes("enter");
    cx.run_until_parked();

    cx.update(|_window, cx| {
        let ed = editor.read(cx);
        assert_eq!(
            ed.buffer().line(inviscid::buffer::SourceLine::new(0)),
            Some("qux bar")
        );
    });
}

#[test]
fn test_replace_next_replaces_and_advances() {
    let mut tcx = TestAppContext::single();
    let (editor, cx) = mount_editor(&mut tcx, "foo bar foo baz foo");

    cx.simulate_keystrokes(REPLACE);
    cx.simulate_input("foo");
    cx.run_until_parked();
    assert_eq!(current_match(&editor, cx), Some((1, 3)));

    cx.simulate_keystrokes("tab");
    cx.simulate_input("qux");
    cx.simulate_keystrokes("enter");
    cx.run_until_parked();

    cx.update(|_window, cx| {
        let ed = editor.read(cx);
        assert_eq!(
            ed.buffer().line(inviscid::buffer::SourceLine::new(0)),
            Some("qux bar foo baz foo")
        );
        assert_eq!(ed.find_state().match_count(), 2);
    });

    cx.simulate_keystrokes("enter");
    cx.run_until_parked();

    cx.update(|_window, cx| {
        let ed = editor.read(cx);
        assert_eq!(
            ed.buffer().line(inviscid::buffer::SourceLine::new(0)),
            Some("qux bar qux baz foo")
        );
        assert_eq!(ed.find_state().match_count(), 1);
    });
}

#[test]
fn test_replace_all_replaces_all_and_atomic_undo() {
    let mut tcx = TestAppContext::single();
    let (editor, cx) = mount_editor(&mut tcx, "apple banana apple cherry apple");

    cx.simulate_keystrokes(REPLACE);
    cx.simulate_input("apple");
    cx.run_until_parked();
    assert_eq!(current_match(&editor, cx), Some((1, 3)));

    cx.simulate_keystrokes("tab");
    cx.simulate_input("orange");
    let replace_all_key = if cfg!(target_os = "macos") {
        "cmd-alt-enter"
    } else {
        "ctrl-alt-enter"
    };
    cx.simulate_keystrokes(replace_all_key);
    cx.run_until_parked();

    cx.update(|_window, cx| {
        let ed = editor.read(cx);
        assert_eq!(
            ed.buffer().line(inviscid::buffer::SourceLine::new(0)),
            Some("orange banana orange cherry orange")
        );
        assert_eq!(ed.find_state().match_count(), 0);
    });

    // Close find bar via escape - editor should regain focus automatically
    cx.simulate_keystrokes("escape");
    cx.run_until_parked();

    cx.update(|window, cx| {
        let ed = editor.read(cx);
        assert!(!ed.is_find_bar_visible());
        assert!(ed.focus_handle(cx).is_focused(window));
    });

    let undo_key = if cfg!(target_os = "macos") {
        "cmd-z"
    } else {
        "ctrl-z"
    };
    cx.simulate_keystrokes(undo_key);
    cx.run_until_parked();

    cx.update(|_window, cx| {
        let ed = editor.read(cx);
        assert_eq!(
            ed.buffer().line(inviscid::buffer::SourceLine::new(0)),
            Some("apple banana apple cherry apple")
        );
    });
}
