use gpui::{Focusable, TestAppContext};
use inviscid::app::InviscidWindow;
use inviscid::app::about::AboutWindow;
use inviscid::app::commands;
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

#[test]
fn test_about_window_initialization_and_focus() {
    let mut cx = TestAppContext::single();
    init_test_globals(&mut cx);

    let (about, cx) = cx.add_window_view(|_window, cx| AboutWindow::new(cx));
    cx.run_until_parked();

    cx.update(|window, cx| {
        about.read(cx).focus(window);
        window.activate_window();
        let handle = about.read(cx).focus_handle(cx);
        assert!(handle.is_focused(window));
    });
}

#[test]
fn test_open_about_window_single_instance_command() {
    let mut cx = TestAppContext::single();
    init_test_globals(&mut cx);

    let (app, cx) = cx.add_window_view(|_window, cx| InviscidWindow::new(cx));
    cx.run_until_parked();

    // Trigger open_about_window first time
    app.update(cx, |_app, cx| {
        commands::open_about_window(cx);
    });
    cx.run_until_parked();

    let handle_1 = cx
        .update(|_window, cx| {
            assert!(cx.has_global::<commands::AboutWindowState>());
            cx.global::<commands::AboutWindowState>().handle
        })
        .expect("About window handle must be registered in GPUI Global");

    // Trigger open_about_window second time -> activates existing window without duplicate creation
    app.update(cx, |_app, cx| {
        commands::open_about_window(cx);
    });
    cx.run_until_parked();

    let handle_2 = cx
        .update(|_window, cx| cx.global::<commands::AboutWindowState>().handle)
        .expect("About window handle must remain valid");

    assert_eq!(
        handle_1, handle_2,
        "Expected singleton about window handle to be reused"
    );
}
