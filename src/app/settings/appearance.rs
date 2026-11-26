//! Appearance-related settings: theme selection and UI font size.
//!
//! This module renders the "Appearance" panel in the settings window, allowing
//! users to pick active color themes and scale the interface font size.

use gpui::prelude::*;
use gpui::*;

use super::SettingsWindow;
use super::bounds::UI_FONT_SIZE_BOUNDS;
use super::persistence::update_config_all_windows;
use crate::config::AppConfig;
use crate::i18n::{ActiveLocale, TextKey};
use crate::theme::ThemeManager;
use crate::ui::{
    FontSize, Icon, IconName, IconSize, SelectableRow, SettingRow, Spacing, Stepper, Switch,
};

impl SettingsWindow {
    pub(crate) fn render_appearance_panel(&self, cx: &mut Context<Self>) -> Div {
        let theme_manager = cx.global::<ThemeManager>();
        let theme = theme_manager.theme().clone();
        let themes = theme_manager.list_themes().to_vec();

        div()
            .flex_1()
            .flex()
            .flex_col()
            .gap(Spacing::LG)
            .child(
                div()
                    .w_full()
                    .min_w_0()
                    .flex()
                    .flex_col()
                    .gap(Spacing::MDS)
                    .child(self.ui_font_size_row(cx))
                    .child(self.fps_counter_row(cx)),
            )
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(Spacing::SM)
                    .child(
                        div()
                            .text_size(FontSize::BODY)
                            .font_weight(FontWeight::MEDIUM)
                            .text_color(theme.text_primary)
                            .child(cx.t(TextKey::SettingsColorTheme).to_string()),
                    )
                    .child(
                        div()
                            .w_full()
                            .flex()
                            .flex_col()
                            .gap(Spacing::XS)
                            .children(themes.into_iter().map(|name| self.theme_row(name, cx))),
                    ),
            )
    }

    fn theme_row(&self, name: String, cx: &mut Context<Self>) -> AnyElement {
        let theme = cx.global::<ThemeManager>().theme().clone();
        let is_active = cx.global::<AppConfig>().theme == name;
        let chosen = name.clone();

        SelectableRow::new(ElementId::Name(format!("theme_choice_{}", name).into()))
            .selected(is_active)
            .on_click(cx.listener(move |_, _, _, cx| {
                cx.update_global::<ThemeManager, _>(|tm, _| {
                    tm.switch_to(&chosen);
                });
                let picked = chosen.clone();
                update_config_all_windows(cx, move |cfg| cfg.theme = picked);
            }))
            .leading(
                div()
                    .text_size(FontSize::SM)
                    .font_weight(if is_active {
                        FontWeight::BOLD
                    } else {
                        FontWeight::NORMAL
                    })
                    .text_color(if is_active {
                        theme.text_accent
                    } else {
                        theme.text_primary
                    })
                    .child(name),
            )
            .when(is_active, |this| {
                this.trailing(
                    Icon::new(IconName::Check)
                        .size(IconSize::Small)
                        .color(theme.text_accent),
                )
            })
            .into_any_element()
    }

    fn ui_font_size_row(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let value = cx.global::<AppConfig>().ui_font_size;
        let bounds = UI_FONT_SIZE_BOUNDS.describe_px();

        SettingRow::new(
            cx.t(TextKey::SettingsUiFontSize).to_string(),
            Stepper::new("ui_font_size", format!("{:.0} px", value))
                .on_decrement(cx.listener(|_, _, _, cx| {
                    update_config_all_windows(cx, |cfg| {
                        cfg.ui_font_size = UI_FONT_SIZE_BOUNDS.step(cfg.ui_font_size, false)
                    })
                }))
                .on_increment(cx.listener(|_, _, _, cx| {
                    update_config_all_windows(cx, |cfg| {
                        cfg.ui_font_size = UI_FONT_SIZE_BOUNDS.step(cfg.ui_font_size, true)
                    })
                })),
        )
        .description(cx.tf(TextKey::SettingsUiFontSizeDesc, &[("bounds", &bounds)]))
    }

    fn fps_counter_row(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let show_fps = cx.global::<AppConfig>().show_fps;

        SettingRow::new(
            cx.t(TextKey::SettingsFpsCounter).to_string(),
            div()
                .debug_selector(|| "toggle_show_fps".to_string())
                .child(
                    Switch::new("toggle_show_fps", show_fps).on_click(cx.listener(
                        |_, _, _, cx| {
                            update_config_all_windows(cx, |cfg| cfg.show_fps = !cfg.show_fps)
                        },
                    )),
                ),
        )
        .description(cx.t(TextKey::SettingsFpsCounterDesc).to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::settings::SettingsCategory;
    use core::prelude::v1::test;

    #[gpui::test]
    fn test_settings_fps_counter_defaults_off_and_toggles_via_switch(
        cx: &mut gpui::TestAppContext,
    ) {
        cx.update(|cx| {
            let cfg = AppConfig::default();
            assert!(!cfg.show_fps, "FPS counter must default to disabled");
            cx.set_global(ThemeManager::from_config(&cfg));
            cx.set_global(cfg);
        });

        let (settings, cx) = cx.add_window_view(|window, cx| {
            window.activate_window();
            let mut s = SettingsWindow::new(cx);
            s.set_category(SettingsCategory::Appearance, cx);
            s
        });
        cx.run_until_parked();

        let switch_pos = cx.debug_bounds("toggle_show_fps").unwrap().center();
        cx.simulate_click(switch_pos, Modifiers::default());
        cx.run_until_parked();
        assert!(cx.read(|cx| cx.global::<AppConfig>().show_fps));

        cx.simulate_click(switch_pos, Modifiers::default());
        cx.run_until_parked();
        assert!(!cx.read(|cx| cx.global::<AppConfig>().show_fps));
        let _ = settings;
    }
}
