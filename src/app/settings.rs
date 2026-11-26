//! Settings window entity, layout shell, and transient input state machine.
//!
//! Serves as the root module for application preferences, hosting the [`SettingsWindow`]
//! GPUI entity, coordinating category switching between sub-panels ([`general`], [`appearance`],
//! [`editor`], and [`keybindings`]), and managing mutually exclusive modal input modes via
//! [`ActiveInput`].

use gpui::prelude::*;
use gpui::*;

mod appearance;
mod bounds;
mod category;
mod editor;
mod general;
mod keybindings;
pub(crate) mod persistence;

pub use category::SettingsCategory;

use crate::config::{AppConfig, UserPreferences};
use crate::i18n::{ActiveLocale, TextKey};
use crate::syntax::{GrammarSource, normalize_grammar_base_url};
use crate::theme::ThemeManager;
use crate::ui::{
    ControlHeight, FontSize, Icon, IconName, IconSize, LineHeight, Radius, SettingTextField,
    SettingTextFieldEvent, Spacing, WindowTitleBar, keystroke_from_gpui,
};

/// Width of the left category navigation sidebar.
const SIDEBAR_WIDTH: Pixels = px(216.0);

/// Mutually exclusive transient input state within the settings window.
pub(crate) enum ActiveInput {
    RecordingKeybinding(&'static str),
}

pub struct SettingsWindow {
    pub(crate) focus_handle: FocusHandle,
    pub(crate) active_category: SettingsCategory,
    pub(crate) active_input: Option<ActiveInput>,
    pub(crate) grammar_url_field: Entity<SettingTextField>,
    pub(crate) asset_dir_field: Entity<SettingTextField>,
    _field_subscriptions: Vec<Subscription>,
    scroll_handle: ScrollHandle,
    content_scroll: crate::ui::SmoothScrollController,
}

impl Focusable for SettingsWindow {
    fn focus_handle(&self, _cx: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl SettingsWindow {
    pub fn new(cx: &mut Context<Self>) -> Self {
        let focus_handle = cx.focus_handle();
        let (initial_custom_grammar_url, initial_asset_dir) = cx
            .try_global::<AppConfig>()
            .map(|cfg| (cfg.custom_grammar_url.clone(), cfg.asset_dir.clone()))
            .unwrap_or_else(|| (String::new(), UserPreferences::DEFAULT_ASSET_DIR.to_string()));

        let grammar_url_field = cx.new(|cx| {
            SettingTextField::new("grammar_url_field", initial_custom_grammar_url, cx)
                .placeholder_key(TextKey::SettingsGrammarUrlPlaceholder, cx)
                .return_focus_to(focus_handle.clone())
                .on_commit(|draft, cx| {
                    let normalized = normalize_grammar_base_url(&draft);
                    persistence::update_config_app(cx, {
                        let normalized = normalized.clone();
                        move |cfg| cfg.custom_grammar_url = normalized
                    });
                    if let Some(cfg) = cx.try_global::<AppConfig>()
                        && cfg.grammar_source == GrammarSource::Custom
                    {
                        crate::syntax::set_global_grammar_base_url(
                            cfg.effective_grammar_base_url(),
                        );
                    }
                    normalized
                })
        });

        let asset_dir_field = cx.new(|cx| {
            SettingTextField::new("asset_dir_field", initial_asset_dir, cx)
                .return_focus_to(focus_handle.clone())
                .on_commit(|draft, cx| {
                    let normalized = crate::media::normalize_asset_dir(&draft);
                    persistence::update_config_app(cx, {
                        let normalized = normalized.clone();
                        move |cfg| cfg.asset_dir = normalized
                    });
                    normalized
                })
        });

        let grammar_sub = cx.subscribe(
            &grammar_url_field,
            |_this, _emitter, _event: &SettingTextFieldEvent, cx| {
                cx.notify();
            },
        );

        let asset_sub = cx.subscribe(
            &asset_dir_field,
            |_this, _emitter, _event: &SettingTextFieldEvent, cx| {
                cx.notify();
            },
        );

        Self {
            focus_handle,
            active_category: SettingsCategory::General,
            active_input: None,
            grammar_url_field,
            asset_dir_field,
            _field_subscriptions: vec![grammar_sub, asset_sub],
            scroll_handle: ScrollHandle::new(),
            content_scroll: crate::ui::SmoothScrollController::vertical(),
        }
    }

    pub(crate) fn flush_setting_inputs(&mut self, cx: &mut Context<Self>) {
        self.grammar_url_field
            .update(cx, |field, cx| field.commit_if_changed(cx));
        self.asset_dir_field
            .update(cx, |field, cx| field.commit_if_changed(cx));
    }

    #[inline]
    fn scroll_access(&mut self) -> (&mut crate::ui::SmoothScrollController, ScrollHandle) {
        let handle = self.scroll_handle.clone();
        (&mut self.content_scroll, handle)
    }

    pub fn focus(&self, window: &mut Window) {
        window.focus(&self.focus_handle);
    }

    pub fn set_category(&mut self, category: SettingsCategory, cx: &mut Context<Self>) {
        self.flush_setting_inputs(cx);
        self.active_category = category;
        self.active_input = None;
        self.scroll_handle.set_offset(Point::default());
        self.content_scroll.reset();
        cx.notify();
    }

    pub fn active_category(&self) -> SettingsCategory {
        self.active_category
    }

    fn render_titlebar(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.global::<ThemeManager>().theme();
        let title = cx.t(TextKey::SettingsTitle).to_string();

        WindowTitleBar::new().left(
            div()
                .flex()
                .flex_row()
                .items_center()
                .gap(Spacing::SM)
                .px(Spacing::SM)
                .child(
                    Icon::new(IconName::Settings)
                        .size(IconSize::Small)
                        .color(theme.text_accent),
                )
                .child(
                    div()
                        .text_size(FontSize::BODY)
                        .font_weight(FontWeight::BOLD)
                        .text_color(theme.text_primary)
                        .child(title),
                ),
        )
    }

    fn render_sidebar(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.global::<ThemeManager>().theme().clone();
        let active = self.active_category;

        div()
            .id("settings_sidebar")
            .w(SIDEBAR_WIDTH)
            .h_full()
            .bg(theme.bg_app)
            .p(Spacing::SM)
            .flex()
            .flex_col()
            .gap(Spacing::XS)
            .children(SettingsCategory::all().iter().map(|&cat| {
                let is_selected = cat == active;
                let label = cx.t(cat.label_key()).to_string();
                let bg_color = if is_selected {
                    theme.btn_active
                } else {
                    gpui::transparent_black()
                };
                let text_color = if is_selected {
                    theme.text_accent
                } else {
                    theme.text_secondary
                };

                div()
                    .id(ElementId::Name(cat.id().into()))
                    .w_full()
                    .h(ControlHeight::LG)
                    .px(Spacing::MDS)
                    .flex()
                    .flex_row()
                    .items_center()
                    .gap(Spacing::SM)
                    .rounded(Radius::LG)
                    .bg(bg_color)
                    .hover(|s| s.bg(theme.btn_hover))
                    .cursor_pointer()
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.set_category(cat, cx);
                    }))
                    .child(
                        Icon::new(cat.icon())
                            .size(IconSize::Small)
                            .color(if is_selected {
                                theme.text_accent
                            } else {
                                theme.text_muted
                            }),
                    )
                    .child(
                        div()
                            .min_w_0()
                            .truncate()
                            .text_size(FontSize::BODY)
                            .font_weight(if is_selected {
                                FontWeight::MEDIUM
                            } else {
                                FontWeight::NORMAL
                            })
                            .text_color(text_color)
                            .child(label),
                    )
            }))
    }
}

impl Render for SettingsWindow {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        crate::ui::SmoothScrollController::continue_on_window(
            self,
            window,
            cx,
            Self::scroll_access,
        );
        let theme = cx.global::<ThemeManager>().theme().clone();
        let config = cx.global::<AppConfig>();
        window.set_rem_size(px(config.ui_font_size));

        let panel = match self.active_category {
            SettingsCategory::General => self.render_general_panel(cx),
            SettingsCategory::Appearance => self.render_appearance_panel(cx),
            SettingsCategory::Editor => self.render_editor_panel(cx),
            SettingsCategory::Keybindings => self.render_keybindings_panel(cx),
        };

        let content_panel = crate::ui::SmoothScrollController::wrap(
            div()
                .id("settings_content_panel")
                .flex_1()
                .min_w_0()
                .h_full()
                .mr(Spacing::SM)
                .mb(Spacing::SM)
                .bg(theme.bg_editor)
                .rounded(Radius::CANVAS)
                .track_scroll(&self.scroll_handle)
                .overflow_y_scroll()
                .p(Spacing::XL)
                .child(panel),
            cx,
            Self::scroll_access,
        );

        div()
            .key_context(crate::ui::key_context::SETTINGS_WINDOW)
            .on_key_down(cx.listener(|this, event: &KeyDownEvent, window, cx| {
                match &this.active_input {
                    Some(ActiveInput::RecordingKeybinding(action_id)) => {
                        let action_id = *action_id;
                        if event.keystroke.key.eq_ignore_ascii_case("escape") {
                            this.active_input = None;
                            cx.notify();
                            return;
                        }
                        if let Some(keystroke) = keystroke_from_gpui(event) {
                            let mut cfg = cx.global::<AppConfig>().clone();
                            cfg.set_keybinding(action_id.to_string(), keystroke);
                            this.commit_keybindings(cfg, cx);
                        }
                    }
                    None => {
                        if event.keystroke.key.eq_ignore_ascii_case("escape") {
                            this.flush_setting_inputs(cx);
                            window.remove_window();
                        }
                    }
                }
            }))
            .size_full()
            .bg(theme.bg_app)
            .text_size(FontSize::BODY)
            .line_height(LineHeight::BODY)
            .flex()
            .flex_col()
            .child(self.render_titlebar(cx))
            .child(
                div()
                    .id("settings_body")
                    .track_focus(&self.focus_handle)
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(|this, _, window, _cx| {
                            window.focus(&this.focus_handle);
                        }),
                    )
                    .flex_1()
                    .min_h_0()
                    .w_full()
                    .flex()
                    .flex_row()
                    .child(self.render_sidebar(cx))
                    .child(content_panel),
            )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    // `gpui::*` exports its own `test` attribute, which would otherwise shadow the std one.
    use crate::config::AppConfig;
    use crate::theme::ThemeManager;
    use core::prelude::v1::test;

    struct ResetGlobalGrammarUrlOnDrop;
    impl Drop for ResetGlobalGrammarUrlOnDrop {
        fn drop(&mut self) {
            crate::syntax::set_global_grammar_base_url("");
        }
    }

    fn init_test_globals(cx: &mut gpui::TestAppContext) {
        cx.update(|cx| {
            let cfg = AppConfig::default();
            cx.set_global(ThemeManager::from_config(&cfg));
            cx.set_global(cfg);
        });
    }

    #[gpui::test]
    fn test_settings_active_input_mutual_exclusion_and_cleanup(cx: &mut gpui::TestAppContext) {
        let _guard = ResetGlobalGrammarUrlOnDrop;
        init_test_globals(cx);
        let (settings, cx) = cx.add_window_view(|window, cx| {
            window.activate_window();
            SettingsWindow::new(cx)
        });

        cx.update(|window, cx| {
            settings.update(cx, |s, cx| {
                // 1. Keybinding recording is active
                s.toggle_recording("SaveFile", cx);
                assert_eq!(s.recording_action_id(), Some("SaveFile"));

                // Entering Custom grammar URL mode interrupts and drops keybinding recording
                s.select_custom_grammar_mode(window, cx);
                assert_eq!(s.recording_action_id(), None);
                assert_eq!(cx.global::<AppConfig>().grammar_source, GrammarSource::Custom);

                // 2. Switching categories drops any active input
                s.set_category(SettingsCategory::Appearance, cx);
                assert_eq!(s.recording_action_id(), None);
            });
        });
    }

    #[gpui::test]
    fn test_settings_keybindings_smooth_scroll(cx: &mut gpui::TestAppContext) {
        init_test_globals(cx);
        let (settings, cx) = cx.add_window_view(|window, cx| {
            window.activate_window();
            let mut s = SettingsWindow::new(cx);
            s.active_category = SettingsCategory::Keybindings;
            s
        });
        cx.run_until_parked();

        settings.read_with(cx, |s, _| {
            let max_h = s.scroll_handle.max_offset().height;
            assert!(
                max_h > px(0.0),
                "keybindings list should overflow vertically"
            );
        });

        cx.simulate_event(ScrollWheelEvent {
            position: point(px(400.0), px(300.0)),
            delta: ScrollDelta::Lines(point(0.0, -3.0)),
            ..Default::default()
        });

        settings.read_with(cx, |s, _| {
            let offset_y = s.scroll_handle.offset().y;
            assert_eq!(
                offset_y,
                px(0.0),
                "native scroll listener must not teleport offset before smooth animation"
            );
            assert_eq!(s.content_scroll.target_scroll_top, px(100.0));
            assert!(s.content_scroll.is_animating());
        });
    }

    #[gpui::test]
    fn test_settings_asset_dir_auto_saves_on_blur_and_close(cx: &mut gpui::TestAppContext) {
        init_test_globals(cx);
        let (settings, cx) = cx.add_window_view(|window, cx| {
            window.activate_window();
            let mut s = SettingsWindow::new(cx);
            s.set_category(SettingsCategory::Editor, cx);
            s
        });
        cx.run_until_parked();

        // Focus field, type custom path, and blur -> auto-saves normalized path
        cx.update(|window, cx| {
            settings.update(cx, |s, cx| {
                assert_eq!(cx.global::<AppConfig>().asset_dir, "assets");
                s.asset_dir_field.update(cx, |f, cx| {
                    f.focus(window, cx);
                    f.input()
                        .update(cx, |ti, cx| ti.set_text("  D:\\media\\images\\  ", cx));
                });
            });
        });
        cx.run_until_parked();

        cx.update(|window, cx| {
            let focus = settings.read(cx).focus_handle.clone();
            window.focus(&focus);
        });
        cx.run_until_parked();

        assert_eq!(
            cx.read(|cx| cx.global::<AppConfig>().asset_dir.clone()),
            "D:\\media\\images"
        );
        assert_eq!(
            settings.read_with(cx, |s, cx| s.asset_dir_field.read(cx).text(cx)),
            "D:\\media\\images"
        );

        // Clearing field and closing window auto-saves empty string (same directory as md file)
        cx.update(|window, cx| {
            settings.update(cx, |s, cx| {
                s.asset_dir_field.update(cx, |f, cx| {
                    f.focus(window, cx);
                    f.input().update(cx, |ti, cx| ti.set_text("", cx));
                });
            });
        });
        cx.run_until_parked();

        cx.update(|window, _cx| window.remove_window());
        cx.run_until_parked();

        assert_eq!(
            cx.read(|cx| cx.global::<AppConfig>().asset_dir.clone()),
            ""
        );
    }
}
