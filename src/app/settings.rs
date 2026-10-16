//! Settings window entity, layout shell, and transient input state machine.
//!
//! Serves as the root module for application preferences, hosting the [`SettingsWindow`]
//! GPUI entity, coordinating category switching between sub-panels ([`general`], [`appearance`],
//! [`editor`], and [`keybindings`]), and managing mutually exclusive modal input modes via
//! [`ActiveInput`].

use gpui::prelude::*;
use gpui::*;

pub mod appearance;
pub mod bounds;
pub mod category;
pub mod editor;
pub mod general;
pub mod keybindings;
pub(crate) mod persistence;

pub use bounds::*;
pub use category::SettingsCategory;

use crate::config::AppConfig;
use crate::theme::ThemeManager;
use crate::ui::{
    ControlHeight, FontSize, Icon, IconName, IconSize, LineHeight, Radius, Spacing, TextInput,
    WindowTitleBar, keystroke_from_gpui,
};

/// Width of the left category navigation sidebar.
const SIDEBAR_WIDTH: Pixels = px(190.0);

/// Mutually exclusive transient input state within the settings window.
pub(crate) enum ActiveInput {
    RecordingKeybinding(&'static str),
    EditingGrammarUrl {
        input: Entity<TextInput>,
        _subscriptions: [Subscription; 2],
    },
}

pub struct SettingsWindow {
    pub(crate) focus_handle: FocusHandle,
    pub(crate) active_category: SettingsCategory,
    pub(crate) active_input: Option<ActiveInput>,
}

impl Focusable for SettingsWindow {
    fn focus_handle(&self, _cx: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl SettingsWindow {
    pub fn new(cx: &mut Context<Self>) -> Self {
        Self {
            focus_handle: cx.focus_handle(),
            active_category: SettingsCategory::General,
            active_input: None,
        }
    }

    pub fn focus(&self, window: &mut Window) {
        window.focus(&self.focus_handle);
    }

    pub fn set_category(&mut self, category: SettingsCategory, cx: &mut Context<Self>) {
        self.active_category = category;
        self.active_input = None;
        cx.notify();
    }

    pub fn active_category(&self) -> SettingsCategory {
        self.active_category
    }

    fn render_titlebar(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.global::<ThemeManager>().theme();

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
                        .child("Settings"),
                ),
        )
    }

    fn render_sidebar(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.global::<ThemeManager>().theme();
        let active = self.active_category;

        div()
            .id("settings_sidebar")
            .w(SIDEBAR_WIDTH)
            .h_full()
            .bg(theme.bg_toolbar)
            .border_r_1()
            .border_color(theme.border)
            .p(Spacing::SM)
            .flex()
            .flex_col()
            .gap(Spacing::XS)
            .children(SettingsCategory::all().iter().map(|&cat| {
                let is_selected = cat == active;
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
                    .id(ElementId::Name(cat.label().into()))
                    .w_full()
                    .h(ControlHeight::LG)
                    .px(Spacing::MDS)
                    .flex()
                    .flex_row()
                    .items_center()
                    .gap(Spacing::SM)
                    .rounded(Radius::MD)
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
                            .text_size(FontSize::BODY)
                            .font_weight(if is_selected {
                                FontWeight::MEDIUM
                            } else {
                                FontWeight::NORMAL
                            })
                            .text_color(text_color)
                            .child(cat.label()),
                    )
            }))
    }
}

impl Render for SettingsWindow {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.global::<ThemeManager>().theme().clone();
        let config = cx.global::<AppConfig>();
        window.set_rem_size(px(config.ui_font_size));

        let panel = match self.active_category {
            SettingsCategory::General => self.render_general_panel(cx).into_any_element(),
            SettingsCategory::Appearance => self.render_appearance_panel(cx).into_any_element(),
            SettingsCategory::Editor => self.render_editor_panel(cx).into_any_element(),
            SettingsCategory::Keybindings => self.render_keybindings_panel(cx).into_any_element(),
        };

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
                    Some(ActiveInput::EditingGrammarUrl { .. }) => {
                        if event.keystroke.key.eq_ignore_ascii_case("escape") {
                            this.cancel_editing_grammar_url(cx);
                        }
                    }
                    None => {
                        if event.keystroke.key.eq_ignore_ascii_case("escape") {
                            window.remove_window();
                        }
                    }
                }
            }))
            .size_full()
            .bg(theme.bg_editor)
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
                    .child(
                        div()
                            .id("settings_content_panel")
                            .flex_1()
                            .min_w_0()
                            .h_full()
                            .overflow_y_scroll()
                            .p(Spacing::XL)
                            .child(panel),
                    ),
            )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    // `gpui::*` exports its own `test` attribute, which would otherwise shadow the std one.
    use core::prelude::v1::test;
    use crate::config::AppConfig;
    use crate::theme::ThemeManager;

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

                // Starting URL editing interrupts and drops keybinding recording
                s.start_editing_grammar_url(window, cx);
                assert_eq!(s.recording_action_id(), None);
                assert!(s.is_editing_grammar_url());

                // 2. Switching categories drops any active input
                s.set_category(SettingsCategory::Appearance, cx);
                assert!(!s.is_editing_grammar_url());
                assert_eq!(s.recording_action_id(), None);
            });
        });
    }
}
