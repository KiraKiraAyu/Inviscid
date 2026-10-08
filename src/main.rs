#![forbid(dead_code)]
use gpui::prelude::*;
use gpui::{
    App, Application, Bounds, TitlebarOptions, WindowBounds, WindowDecorations, WindowOptions, px,
    size,
};
use inviscid::InviscidWindow;
use inviscid::app;
use inviscid::assets::Assets;
use inviscid::config::AppConfig;
use inviscid::http::UreqClient;
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
            inviscid::syntax::set_global_grammar_base_url(config.grammar_base_url.clone());
            let theme_manager = ThemeManager::from_config(&config);
            cx.set_global(theme_manager);

            // Bind default and configured keybindings
            app::actions::apply_configured_keys(&config, cx);
            cx.set_global(config);

            // Open initial window
            let bounds = Bounds::centered(None, size(px(1080.0), px(780.0)), cx);
            cx.open_window(
                WindowOptions {
                    window_bounds: Some(WindowBounds::Windowed(bounds)),
                    titlebar: Some(TitlebarOptions {
                        title: Some("Inviscid".into()),
                        appears_transparent: true,
                        traffic_light_position: None,
                    }),
                    window_decorations: Some(WindowDecorations::Client),
                    window_min_size: Some(size(px(360.0), px(240.0))),
                    is_movable: true,
                    focus: false,
                    show: false,
                    ..Default::default()
                },
                |window, cx| {
                    let app = cx.new(|cx| InviscidWindow::open_initial(initial_file, cx));
                    app.read(cx)
                        .workspace()
                        .read(cx)
                        .focus_active_editor(window, cx);
                    window.activate_window();
                    app
                },
            )
            .unwrap();
        });
}
