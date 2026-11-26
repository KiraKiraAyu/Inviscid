#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
#![forbid(dead_code)]
use gpui::prelude::*;
use gpui::{App, Application};
use inviscid::InviscidWindow;
use inviscid::app;
use inviscid::assets::Assets;
use inviscid::config::AppConfig;
use inviscid::http::UreqClient;
use inviscid::i18n::LocaleManager;
use inviscid::theme::ThemeManager;
use std::path::PathBuf;
use std::sync::Arc;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let initial_file = args.get(1).map(PathBuf::from);

    Application::new()
        .with_assets(Assets)
        .run(move |cx: &mut App| {
            cx.set_http_client(Arc::new(UreqClient::new()));

            // Load embedded fonts into text system
            if let Err(e) = Assets.load_fonts(cx) {
                eprintln!("Failed to load embedded fonts: {e}");
            }

            // Initialize process-level Globals
            let config = AppConfig::load();
            inviscid::syntax::set_global_grammar_base_url(config.effective_grammar_base_url());
            let theme_manager = ThemeManager::from_config(&config);
            cx.set_global(theme_manager);
            let locale_manager = LocaleManager::from_config(&config);
            cx.set_global(locale_manager);

            // Bind default and configured keybindings
            app::actions::apply_configured_keys(&config, cx);
            let (display_id, window_bounds) =
                config.resolve_initial_window_bounds(initial_file.is_some(), cx);
            cx.set_global(config);

            cx.on_window_closed(|cx| {
                if let Some(config) = cx.try_global::<AppConfig>() {
                    config.save_session();
                }
            })
            .detach();

            cx.on_app_quit(|cx| {
                inviscid::app::lifecycle::persist_all_windows_state_sync(cx);
                async {}
            })
            .detach();

            // Open initial window
            cx.open_window(
                app::commands::main_window_options(display_id, window_bounds),
                |window, cx| {
                    let app = cx.new(|cx| InviscidWindow::open_initial(initial_file, cx));
                    window.activate_window();
                    app
                },
            )
            .unwrap();
        });
}
