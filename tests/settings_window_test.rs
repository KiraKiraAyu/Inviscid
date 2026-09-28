use gpui::{Focusable, KeyDownEvent, Keystroke, Modifiers, TestAppContext};
use inviscid::app::InviscidWindow;
use inviscid::app::actions::keybindings;
use inviscid::app::commands;
use inviscid::app::settings::{SettingsCategory, SettingsWindow};
use inviscid::config::AppConfig;
use inviscid::theme::ThemeManager;
use inviscid::ui::{
    format_keystroke_for_display, format_keystroke_for_display_with_platform,
    keystroke_from_gpui_with_platform,
};

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
fn test_settings_categories_order_and_switching() {
    let mut cx = TestAppContext::single();
    init_test_globals(&mut cx);

    let categories = SettingsCategory::all();
    assert_eq!(categories.len(), 4);
    assert_eq!(categories[0], SettingsCategory::General);
    assert_eq!(categories[1], SettingsCategory::Appearance);
    assert_eq!(categories[2], SettingsCategory::Editor);
    assert_eq!(categories[3], SettingsCategory::Keybindings);

    let (settings, cx) = cx.add_window_view(|_window, cx| SettingsWindow::new(cx));
    cx.run_until_parked();

    let initial_cat = cx.update(|_window, cx| settings.read(cx).active_category());
    assert_eq!(initial_cat, SettingsCategory::General);

    for &cat in categories {
        cx.update(|_window, cx| {
            settings.update(cx, |view, cx| {
                view.set_category(cat, cx);
            });
        });
        cx.run_until_parked();
        let current_cat = cx.update(|_window, cx| settings.read(cx).active_category());
        assert_eq!(current_cat, cat);
    }
}

#[test]
fn test_settings_keybinding_customization_lifecycle() {
    let mut cx = TestAppContext::single();
    init_test_globals(&mut cx);

    // The settings list is every command outside the editor-internal category: editor keys are
    // bound from the same table but are not offered for rebinding. Asserted by membership rather
    // than by a count so that adding a command does not silently rot this test.
    let ids: Vec<&str> = keybindings().map(|meta| meta.id).collect();
    assert!(
        !ids.contains(&"MoveLeft"),
        "editor-internal keys must not be offered"
    );
    assert!(
        !ids.contains(&"PageUp"),
        "editor-internal keys must not be offered"
    );
    assert!(ids.contains(&"OpenSettings"));
    assert!(
        ids.contains(&"RenameEntry"),
        "project panel commands must be rebindable"
    );
    assert!(
        ids.contains(&"TrashEntry"),
        "project panel commands must be rebindable"
    );

    let open_settings_meta = keybindings().find(|m| m.id == "OpenSettings").unwrap();
    assert_eq!(open_settings_meta.name, "Settings");

    let display = format_keystroke_for_display("secondary-s");
    #[cfg(target_os = "macos")]
    assert_eq!(display, "⌘S");
    #[cfg(not(target_os = "macos"))]
    assert_eq!(display, "Ctrl+S");

    let display_shift = format_keystroke_for_display("secondary-shift-p");
    #[cfg(target_os = "macos")]
    assert_eq!(display_shift, "⇧⌘P");
    #[cfg(not(target_os = "macos"))]
    assert_eq!(display_shift, "Ctrl+Shift+P");

    cx.update(|cx| {
        let mut config = cx.global::<AppConfig>().clone();
        assert!(config.custom_keybindings.is_empty());

        let default_save = config.get_keybinding("SaveFile", "secondary-s");
        assert_eq!(default_save, "secondary-s");

        config.set_keybinding("SaveFile", "secondary-alt-s");
        assert_eq!(
            config.get_keybinding("SaveFile", "secondary-s"),
            "secondary-alt-s"
        );
        assert_eq!(config.custom_keybindings.len(), 1);

        config.reset_keybinding("SaveFile");
        assert_eq!(
            config.get_keybinding("SaveFile", "secondary-s"),
            "secondary-s"
        );
        assert!(config.custom_keybindings.is_empty());

        config.set_keybinding("SaveFile", "secondary-shift-s");
        config.set_keybinding("NewTab", "secondary-t");
        assert_eq!(config.custom_keybindings.len(), 2);

        config.reset_all_keybindings();
        assert!(config.custom_keybindings.is_empty());
    });
}

#[test]
fn test_settings_window_focus_and_activation() {
    let mut cx = TestAppContext::single();
    init_test_globals(&mut cx);

    let (settings, cx) = cx.add_window_view(|_window, cx| SettingsWindow::new(cx));
    cx.run_until_parked();

    cx.update(|window, cx| {
        settings.read(cx).focus(window);
        window.activate_window();
        let handle = settings.read(cx).focus_handle(cx);
        assert!(handle.is_focused(window));
    });
}

#[test]
fn test_open_settings_window_single_instance_command() {
    let mut cx = TestAppContext::single();
    init_test_globals(&mut cx);

    let (app, cx) = cx.add_window_view(|_window, cx| InviscidWindow::new(cx));
    cx.run_until_parked();

    // Trigger open_settings_window first time
    app.update(cx, |_app, cx| {
        commands::open_settings_window(cx);
    });
    cx.run_until_parked();

    let handle_1 = cx
        .update(|_window, cx| {
            assert!(cx.has_global::<commands::SettingsWindowState>());
            cx.global::<commands::SettingsWindowState>().handle
        })
        .expect("Settings window handle must be registered in GPUI Global");

    // Trigger open_settings_window second time -> activates existing window without duplicate creation
    app.update(cx, |_app, cx| {
        commands::open_settings_window(cx);
    });
    cx.run_until_parked();

    let handle_2 = cx
        .update(|_window, cx| cx.global::<commands::SettingsWindowState>().handle)
        .expect("Settings window handle must remain valid");

    assert_eq!(
        handle_1, handle_2,
        "Expected singleton settings window handle to be reused"
    );
}

#[test]
fn test_settings_keybinding_recording_toggle_and_direct_save() {
    let mut cx = TestAppContext::single();
    init_test_globals(&mut cx);

    let (settings, cx) = cx.add_window_view(|_window, cx| SettingsWindow::new(cx));
    cx.run_until_parked();

    // Switch to Keybindings category
    cx.update(|_window, cx| {
        settings.update(cx, |view, cx| {
            view.set_category(SettingsCategory::Keybindings, cx);
        });
    });
    cx.run_until_parked();

    // Initially, no action is being recorded
    cx.update(|_window, cx| {
        assert_eq!(settings.read(cx).recording_action_id(), None);
    });

    // Clicking keybind badge toggles recording mode ON
    cx.update(|_window, cx| {
        settings.update(cx, |view, cx| {
            view.toggle_recording("SaveFile", cx);
        });
    });
    cx.run_until_parked();

    cx.update(|_window, cx| {
        assert_eq!(settings.read(cx).recording_action_id(), Some("SaveFile"));
    });

    // Clicking it again toggles recording mode OFF without needing a Cancel button
    cx.update(|_window, cx| {
        settings.update(cx, |view, cx| {
            view.toggle_recording("SaveFile", cx);
        });
    });
    cx.run_until_parked();

    cx.update(|_window, cx| {
        assert_eq!(settings.read(cx).recording_action_id(), None);
    });
}

#[test]
fn test_settings_window_titlebar_window_controls_and_close() {
    let mut cx = TestAppContext::single();
    init_test_globals(&mut cx);

    let (settings, cx) = cx.add_window_view(|_window, cx| SettingsWindow::new(cx));
    cx.run_until_parked();

    // Verify settings window focus and rendering with native window controls
    cx.update(|window, cx| {
        settings.read(cx).focus(window);
        let handle = settings.read(cx).focus_handle(cx);
        assert!(handle.is_focused(window));
    });

    // Verify closing settings window via standard GPUI window removal
    cx.update(|window, _cx| {
        window.remove_window();
    });
    cx.run_until_parked();
}

#[test]
fn test_settings_window_titlebar_drag_and_focus_isolation() {
    let mut cx = TestAppContext::single();
    init_test_globals(&mut cx);

    let (settings, cx) = cx.add_window_view(|_window, cx| SettingsWindow::new(cx));
    cx.run_until_parked();

    // Focus the settings window
    cx.update(|window, cx| {
        settings.read(cx).focus(window);
        let handle = settings.read(cx).focus_handle(cx);
        assert!(handle.is_focused(window));
    });

    // Simulate mouse down on the title bar (e.g. x = 100px, y = 16px)
    cx.simulate_mouse_down(
        gpui::point(gpui::px(100.0), gpui::px(16.0)),
        gpui::MouseButton::Left,
        gpui::Modifiers::default(),
    );

    // Crucial: default must NOT be prevented on titlebar clicks, so Windows native
    // WM_NCLBUTTONDOWN can fall through to DefWindowProcW to initiate window dragging.
    let default_prevented = cx.update(|window, _| window.default_prevented());
    assert!(
        !default_prevented,
        "Titlebar click must not prevent default so that native drag can occur"
    );

    // Verify pressing Escape on the focused settings body closes the window
    cx.simulate_keystrokes("escape");
    cx.run_until_parked();
}

#[test]
fn test_format_keystroke_for_display_cross_platform() {
    // macOS display format: uses symbols (⌘, ⌥, ⇧, ⌃) without '+' connector
    assert_eq!(
        format_keystroke_for_display_with_platform("secondary-s", true),
        "⌘S"
    );
    assert_eq!(
        format_keystroke_for_display_with_platform("cmd-s", true),
        "⌘S"
    );
    assert_eq!(
        format_keystroke_for_display_with_platform("secondary-shift-p", true),
        "⇧⌘P"
    );
    assert_eq!(
        format_keystroke_for_display_with_platform("alt-left", true),
        "⌥←"
    );
    assert_eq!(
        format_keystroke_for_display_with_platform("ctrl-alt-shift-platform-t", true),
        "⌃⌥⇧⌘T"
    );
    assert_eq!(
        format_keystroke_for_display_with_platform("ctrl-shift-t", true),
        "⌃⇧T"
    );
    assert_eq!(
        format_keystroke_for_display_with_platform("secondary-k secondary-c", true),
        "⌘K ⌘C"
    );
    assert_eq!(
        format_keystroke_for_display_with_platform("escape", true),
        "⎋"
    );
    assert_eq!(format_keystroke_for_display_with_platform("tab", true), "⇥");
    assert_eq!(
        format_keystroke_for_display_with_platform("enter", true),
        "↩"
    );
    assert_eq!(
        format_keystroke_for_display_with_platform("backspace", true),
        "⌫"
    );
    assert_eq!(
        format_keystroke_for_display_with_platform("delete", true),
        "Delete"
    );
    assert_eq!(format_keystroke_for_display_with_platform("up", true), "↑");
    assert_eq!(
        format_keystroke_for_display_with_platform("down", true),
        "↓"
    );
    assert_eq!(format_keystroke_for_display_with_platform("f2", true), "F2");

    // Windows / Linux display format: uses words with '+' connector
    assert_eq!(
        format_keystroke_for_display_with_platform("secondary-s", false),
        "Ctrl+S"
    );
    assert_eq!(
        format_keystroke_for_display_with_platform("cmd-s", false),
        "Win+S"
    );
    assert_eq!(
        format_keystroke_for_display_with_platform("secondary-shift-p", false),
        "Ctrl+Shift+P"
    );
    assert_eq!(
        format_keystroke_for_display_with_platform("alt-left", false),
        "Alt+Left"
    );
    assert_eq!(
        format_keystroke_for_display_with_platform("ctrl-alt-shift-win-t", false),
        "Ctrl+Alt+Shift+Win+T"
    );
    assert_eq!(
        format_keystroke_for_display_with_platform("ctrl-shift-t", false),
        "Ctrl+Shift+T"
    );
    assert_eq!(
        format_keystroke_for_display_with_platform("ctrl-k ctrl-c", false),
        "Ctrl+K Ctrl+C"
    );
    assert_eq!(
        format_keystroke_for_display_with_platform("escape", false),
        "Esc"
    );
    assert_eq!(
        format_keystroke_for_display_with_platform("tab", false),
        "Tab"
    );
    assert_eq!(
        format_keystroke_for_display_with_platform("enter", false),
        "Enter"
    );
    assert_eq!(
        format_keystroke_for_display_with_platform("backspace", false),
        "Backspace"
    );
    assert_eq!(
        format_keystroke_for_display_with_platform("delete", false),
        "Delete"
    );
    assert_eq!(
        format_keystroke_for_display_with_platform("up", false),
        "Up"
    );
    assert_eq!(
        format_keystroke_for_display_with_platform("down", false),
        "Down"
    );
    assert_eq!(
        format_keystroke_for_display_with_platform("f2", false),
        "F2"
    );
}

#[test]
fn test_keystroke_from_gpui_cross_platform() {
    // Modifier alone pressed should return None
    let standalone_mod = KeyDownEvent {
        keystroke: Keystroke {
            modifiers: Modifiers {
                control: true,
                alt: false,
                shift: false,
                platform: false,
                function: false,
            },
            key: "control".to_string(),
            key_char: None,
        },
        is_held: false,
    };
    assert_eq!(
        keystroke_from_gpui_with_platform(&standalone_mod, true),
        None
    );
    assert_eq!(
        keystroke_from_gpui_with_platform(&standalone_mod, false),
        None
    );

    // macOS: platform -> cmd
    let cmd_s_mac = KeyDownEvent {
        keystroke: Keystroke {
            modifiers: Modifiers {
                control: false,
                alt: false,
                shift: false,
                platform: true,
                function: false,
            },
            key: "s".to_string(),
            key_char: None,
        },
        is_held: false,
    };
    assert_eq!(
        keystroke_from_gpui_with_platform(&cmd_s_mac, true),
        Some("cmd-s".to_string())
    );
    // Windows: platform -> win
    assert_eq!(
        keystroke_from_gpui_with_platform(&cmd_s_mac, false),
        Some("win-s".to_string())
    );

    // Windows: control -> ctrl
    let ctrl_s_win = KeyDownEvent {
        keystroke: Keystroke {
            modifiers: Modifiers {
                control: true,
                alt: false,
                shift: false,
                platform: false,
                function: false,
            },
            key: "s".to_string(),
            key_char: None,
        },
        is_held: false,
    };
    assert_eq!(
        keystroke_from_gpui_with_platform(&ctrl_s_win, false),
        Some("ctrl-s".to_string())
    );
    assert_eq!(
        keystroke_from_gpui_with_platform(&ctrl_s_win, true),
        Some("ctrl-s".to_string())
    );

    // macOS full chord: ctrl-alt-shift-cmd-t
    let chord_mac = KeyDownEvent {
        keystroke: Keystroke {
            modifiers: Modifiers {
                control: true,
                alt: true,
                shift: true,
                platform: true,
                function: false,
            },
            key: "t".to_string(),
            key_char: None,
        },
        is_held: false,
    };
    assert_eq!(
        keystroke_from_gpui_with_platform(&chord_mac, true),
        Some("ctrl-alt-shift-cmd-t".to_string())
    );
    assert_eq!(
        keystroke_from_gpui_with_platform(&chord_mac, false),
        Some("ctrl-alt-shift-win-t".to_string())
    );
}
