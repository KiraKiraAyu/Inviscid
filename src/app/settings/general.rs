//! General application settings.
//!
//! This module renders the "General" panel in the settings window, managing:
//! - Default render mode (Live Preview vs. Source mode).
//! - Workspace restoration on launch.
//! - Tree-sitter WASM grammar download CDN sources (jsDelivr, unpkg, offline, or custom URL).

use gpui::prelude::*;
use gpui::*;

use super::persistence::{update_config, update_config_all_windows};
use super::{ActiveInput, SettingsWindow};
use crate::config::AppConfig;
use crate::editor::RenderMode;
use crate::syntax::{GrammarCdnPreset, normalize_grammar_base_url};
use crate::theme::ThemeManager;
use crate::ui::{
    ControlHeight, FontSize, OptionPill, Radius, SettingRow, Spacing, Switch, TextInput,
    TextInputEvent,
};

pub(crate) fn is_custom_grammar_url(url: &str) -> bool {
    let normalized = normalize_grammar_base_url(url);
    !normalized.is_empty() && GrammarCdnPreset::from_url(&normalized).is_none()
}

impl SettingsWindow {
    pub(crate) fn render_general_panel(&self, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .flex_1()
            .min_w_0()
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
                    .child(self.render_mode_row(cx))
                    .child(self.restore_workspace_row(cx))
                    .child(self.grammar_base_url_row(cx)),
            )
    }

    fn render_mode_row(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let is_live_preview =
            cx.global::<AppConfig>().default_render_mode == RenderMode::LivePreview;

        SettingRow::new(
            "Default Render Mode",
            div()
                .flex()
                .flex_row()
                .gap(Spacing::SMD)
                .child(
                    OptionPill::new(
                        "render_mode_live",
                        RenderMode::LivePreview.label(),
                        is_live_preview,
                    )
                    .on_click(cx.listener(|_, _, _, cx| {
                        update_config_all_windows(cx, |cfg| {
                            cfg.default_render_mode = RenderMode::LivePreview
                        })
                    })),
                )
                .child(
                    OptionPill::new(
                        "render_mode_source",
                        RenderMode::Source.label(),
                        !is_live_preview,
                    )
                    .on_click(cx.listener(|_, _, _, cx| {
                        update_config_all_windows(cx, |cfg| {
                            cfg.default_render_mode = RenderMode::Source
                        })
                    })),
                ),
        )
        .description("Preferred mode when opening or creating Markdown documents.")
    }

    fn restore_workspace_row(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let restore_ws = cx.global::<AppConfig>().restore_workspace;

        SettingRow::new(
            "Restore Workspace on Startup",
            Switch::new("toggle_restore_ws", restore_ws).on_click(cx.listener(|_, _, _, cx| {
                update_config(cx, |cfg| cfg.restore_workspace = !cfg.restore_workspace)
            })),
        )
        .description("Automatically restore previous open folder and documents on launch.")
    }

    fn grammar_base_url_row(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.global::<ThemeManager>().theme().clone();
        let current_url = cx.global::<AppConfig>().grammar_base_url.clone();
        let is_custom = self.is_custom_grammar_mode(cx);
        let preset = if is_custom {
            None
        } else {
            GrammarCdnPreset::from_url(&current_url)
        };
        let is_jsdelivr = preset == Some(GrammarCdnPreset::JsDelivr);
        let is_unpkg = preset == Some(GrammarCdnPreset::Unpkg);
        let is_offline = preset == Some(GrammarCdnPreset::Offline);

        let url_bar = if let Some(input) = self.grammar_url_input() {
            div()
                .id("grammar_url_editor")
                .flex()
                .flex_row()
                .items_center()
                .child(input.clone())
                .into_any_element()
        } else {
            let display_text = if current_url.is_empty() {
                "Offline — local grammars only".to_string()
            } else {
                current_url.clone()
            };
            let text_color = if current_url.is_empty() {
                theme.text_muted
            } else {
                theme.text_secondary
            };

            div()
                .id("grammar_url_display")
                .h(ControlHeight::MD)
                .flex()
                .flex_row()
                .items_center()
                .justify_between()
                .gap(Spacing::SM)
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .overflow_hidden()
                        .whitespace_nowrap()
                        .text_size(FontSize::CAPTION)
                        .text_color(text_color)
                        .child(display_text),
                )
                .when(is_custom, |this| {
                    this.child(
                        div()
                            .id("grammar_url_edit_btn")
                            .flex_none()
                            .px(Spacing::SM)
                            .py(Spacing::XXS)
                            .rounded(Radius::XS)
                            .bg(theme.bg_editor)
                            .border_1()
                            .border_color(theme.border_subtle)
                            .hover(|s| s.bg(theme.btn_hover).border_color(theme.border))
                            .cursor_pointer()
                            .text_size(FontSize::CAPTION)
                            .font_weight(FontWeight::MEDIUM)
                            .text_color(theme.text_accent)
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.start_editing_grammar_url(window, cx);
                            }))
                            .child("Edit"),
                    )
                })
                .into_any_element()
        };

        div()
            .w_full()
            .flex()
            .flex_col()
            .gap(Spacing::XS)
            .child(
                SettingRow::new(
                    "Grammar Download Source",
                    div()
                        .flex()
                        .flex_row()
                        .gap(Spacing::XS)
                        .child(
                            OptionPill::new("grammar_cdn_jsdelivr", "jsDelivr", is_jsdelivr)
                                .on_click(cx.listener(|this, _, _, cx| {
                                    this.select_grammar_preset(GrammarCdnPreset::JsDelivr, cx);
                                })),
                        )
                        .child(
                            OptionPill::new("grammar_cdn_unpkg", "unpkg", is_unpkg).on_click(
                                cx.listener(|this, _, _, cx| {
                                    this.select_grammar_preset(GrammarCdnPreset::Unpkg, cx);
                                }),
                            ),
                        )
                        .child(
                            OptionPill::new("grammar_cdn_offline", "Offline", is_offline).on_click(
                                cx.listener(|this, _, _, cx| {
                                    this.select_grammar_preset(GrammarCdnPreset::Offline, cx);
                                }),
                            ),
                        )
                        .child(
                            OptionPill::new("grammar_cdn_custom", "Custom", is_custom).on_click(
                                cx.listener(|this, _, window, cx| {
                                    this.start_editing_grammar_url(window, cx);
                                }),
                            ),
                        ),
                )
                .description(
                    "CDN base URL for on-demand Tree-sitter WASM syntax grammars, or Offline to use local grammars only.",
                ),
            )
            .child(url_bar)
    }

    pub(crate) fn is_custom_grammar_mode(&self, cx: &App) -> bool {
        self.is_editing_grammar_url()
            || is_custom_grammar_url(&cx.global::<AppConfig>().grammar_base_url)
    }

    pub(crate) fn select_grammar_preset(
        &mut self,
        preset: GrammarCdnPreset,
        cx: &mut Context<Self>,
    ) {
        self.set_grammar_base_url(preset.url(), cx);
    }

    pub(crate) fn is_editing_grammar_url(&self) -> bool {
        matches!(
            self.active_input,
            Some(ActiveInput::EditingGrammarUrl { .. })
        )
    }

    pub(crate) fn grammar_url_input(&self) -> Option<&Entity<TextInput>> {
        match &self.active_input {
            Some(ActiveInput::EditingGrammarUrl { input, .. }) => Some(input),
            _ => None,
        }
    }

    pub(crate) fn start_editing_grammar_url(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let current = cx.global::<AppConfig>().grammar_base_url.clone();
        let input = cx.new(|cx| {
            let mut input = TextInput::new(&current, cx)
                .placeholder("https://... (leave empty for offline)")
                .height(ControlHeight::MD)
                .radius(Radius::SM)
                .padding_x(Spacing::MDS);
            input.buffer_mut().select_all();
            input
        });
        let focus_handle = input.read(cx).focus_handle(cx);
        let event_sub = cx.subscribe_in(
            &input,
            window,
            |this, _emitter, event: &TextInputEvent, window, cx| match event {
                TextInputEvent::Commit(text) => {
                    this.set_grammar_base_url(text.clone(), cx);
                    window.focus(&this.focus_handle);
                }
                TextInputEvent::Cancel => {
                    this.cancel_editing_grammar_url(cx);
                    window.focus(&this.focus_handle);
                }
            },
        );
        let blur_sub = cx.on_blur(&focus_handle, window, |this, _window, cx| {
            this.commit_grammar_url(cx);
        });
        window.focus(&focus_handle);
        self.active_input = Some(ActiveInput::EditingGrammarUrl {
            input,
            _subscriptions: [event_sub, blur_sub],
        });
        cx.notify();
    }

    pub(crate) fn cancel_editing_grammar_url(&mut self, cx: &mut Context<Self>) {
        if self.is_editing_grammar_url() {
            self.active_input = None;
            cx.notify();
        }
    }

    pub(crate) fn set_grammar_base_url(
        &mut self,
        url: impl Into<String>,
        cx: &mut Context<Self>,
    ) {
        let normalized = normalize_grammar_base_url(&url.into());
        self.active_input = None;
        crate::syntax::set_global_grammar_base_url(normalized.clone());
        update_config(cx, move |cfg| {
            cfg.grammar_base_url = normalized;
        });
        cx.notify();
    }

    pub(crate) fn commit_grammar_url(&mut self, cx: &mut Context<Self>) {
        if let Some(input) = self.grammar_url_input() {
            let draft = input.read(cx).text();
            self.set_grammar_base_url(draft, cx);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use core::prelude::v1::test;

    #[test]
    fn test_render_mode_labels_match_persisted_form() {
        for mode in [RenderMode::LivePreview, RenderMode::Source] {
            let mut cfg = AppConfig::default();
            cfg.default_render_mode = mode;

            let serialized = toml::to_string(&cfg.preferences).unwrap();
            assert!(
                serialized.contains(&format!("default_render_mode = \"{}\"", mode.label())),
                "unexpected serialized form: {serialized}"
            );

            let parsed: crate::config::UserPreferences = toml::from_str(&serialized).unwrap();
            assert_eq!(parsed.default_render_mode, mode);
        }
    }

    #[test]
    fn test_is_custom_grammar_url_classification() {
        assert!(!is_custom_grammar_url(""));
        assert!(!is_custom_grammar_url("   "));
        assert!(!is_custom_grammar_url(GrammarCdnPreset::JsDelivr.url()));
        assert!(!is_custom_grammar_url("https://cdn.jsdelivr.net/npm/"));
        assert!(!is_custom_grammar_url(GrammarCdnPreset::Unpkg.url()));
        assert!(!is_custom_grammar_url("https://unpkg.com/"));
        assert!(is_custom_grammar_url("https://mirror.example.com/npm"));
    }

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
    fn test_settings_grammar_base_url_presets_and_custom_editing(cx: &mut gpui::TestAppContext) {
        let _guard = ResetGlobalGrammarUrlOnDrop;
        init_test_globals(cx);
        let (settings, cx) = cx.add_window_view(|window, cx| {
            window.activate_window();
            SettingsWindow::new(cx)
        });

        // Initial presets switching
        cx.update(|_window, cx| {
            settings.update(cx, |s, cx| {
                assert!(!s.is_editing_grammar_url());
                assert!(!s.is_custom_grammar_mode(cx));
                assert_eq!(
                    cx.global::<AppConfig>().grammar_base_url,
                    GrammarCdnPreset::DEFAULT.url()
                );

                // Switch to unpkg preset
                s.select_grammar_preset(GrammarCdnPreset::Unpkg, cx);
                assert!(!s.is_custom_grammar_mode(cx));
                assert_eq!(
                    cx.global::<AppConfig>().grammar_base_url,
                    GrammarCdnPreset::Unpkg.url()
                );

                // Switch to offline preset
                s.select_grammar_preset(GrammarCdnPreset::Offline, cx);
                assert!(!s.is_custom_grammar_mode(cx));
                assert_eq!(cx.global::<AppConfig>().grammar_base_url, "");
            });
        });

        // Custom mode starts editing URL directly
        let input = cx.update(|window, cx| {
            settings.update(cx, |s, cx| {
                s.start_editing_grammar_url(window, cx);
                assert!(s.is_editing_grammar_url());
                assert!(s.is_custom_grammar_mode(cx));
                s.grammar_url_input().unwrap().clone()
            })
        });

        // Typing custom URL and blurring/committing normalizes and saves
        cx.update(|_window, cx| {
            input.update(cx, |inp, _cx| {
                inp.buffer_mut()
                    .insert_text("https://mirror.example.com/npm/");
            });
        });

        // Blur event commits input
        cx.update(|_window, cx| {
            settings.update(cx, |s, cx| {
                s.commit_grammar_url(cx);
            });
        });
        cx.run_until_parked();

        cx.update(|_window, cx| {
            settings.update(cx, |s, cx| {
                assert!(!s.is_editing_grammar_url());
                assert!(s.is_custom_grammar_mode(cx));
                assert_eq!(
                    cx.global::<AppConfig>().grammar_base_url,
                    "https://mirror.example.com/npm"
                );
            });
        });
    }

    #[gpui::test]
    fn test_settings_grammar_url_editing_and_cancellation(cx: &mut gpui::TestAppContext) {
        let _guard = ResetGlobalGrammarUrlOnDrop;
        init_test_globals(cx);
        let (settings, cx) = cx.add_window_view(|window, cx| {
            window.activate_window();
            SettingsWindow::new(cx)
        });

        let input = cx.update(|window, cx| {
            settings.update(cx, |s, cx| {
                s.select_grammar_preset(GrammarCdnPreset::Offline, cx);
                s.start_editing_grammar_url(window, cx);
                assert!(s.is_editing_grammar_url());
                s.grammar_url_input().unwrap().clone()
            })
        });

        // Typing but cancelling discards draft without mutating config
        cx.update(|_window, cx| {
            assert_eq!(input.read(cx).text(), "");
            input.update(cx, |inp, cx| {
                inp.buffer_mut()
                    .insert_text("https://discarded.example.com");
                inp.cancel(cx);
            });
        });
        cx.run_until_parked();

        cx.update(|_window, cx| {
            settings.update(cx, |s, cx| {
                assert!(!s.is_editing_grammar_url());
                assert_eq!(cx.global::<AppConfig>().grammar_base_url, "");
            });
        });
    }
}
