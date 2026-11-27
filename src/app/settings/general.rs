//! General application settings.
//!
//! This module renders the "General" panel in the settings window, managing:
//! - Default render mode (Live Preview vs. Source mode).
//! - Workspace restoration on launch.
//! - Tree-sitter WASM grammar download CDN sources (jsDelivr, unpkg, offline, or custom URL).

use gpui::prelude::*;
use gpui::*;

use super::persistence::{update_config, update_config_all_windows};
use super::SettingsWindow;
use crate::config::AppConfig;
use crate::editor::RenderMode;
use crate::i18n::{ActiveLocale, LanguagePreference, LocaleId, LocaleManager, TextKey};
use crate::syntax::GrammarSource;
use crate::theme::ThemeManager;
use crate::ui::{
    ControlHeight, FontSize, LineHeight, OptionPill, Radius, SettingRow, Spacing, Switch,
};
use crate::updater::{UpdateManager, UpdateSource, UpdateStatus};

impl SettingsWindow {
    pub(crate) fn render_general_panel(&self, cx: &mut Context<Self>) -> Div {
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
                    .child(self.language_row(cx))
                    .child(self.render_mode_row(cx))
                    .child(self.restore_workspace_row(cx))
                    .child(self.collocated_mode_row(cx))
                    .child(self.grammar_base_url_row(cx))
                    .child(self.auto_check_updates_row(cx))
                    .child(self.update_source_row(cx))
                    .child(self.check_updates_now_row(cx)),
            )
    }

    fn language_row(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.global::<ThemeManager>().theme().clone();
        let current_pref = cx.global::<AppConfig>().language;
        let system_locale = cx
            .try_global::<LocaleManager>()
            .map(|m| m.system_locale())
            .unwrap_or(LocaleId::En);

        let system_label = cx.tf(
            TextKey::LanguageSystemDefault,
            &[("language", system_locale.native_name())],
        );
        let title = cx.t(TextKey::SettingsLanguage).to_string();
        let desc = cx.t(TextKey::SettingsLanguageDesc).to_string();

        div()
            .w_full()
            .py(Spacing::SMD)
            .flex()
            .flex_col()
            .gap(Spacing::SM)
            .child(
                div()
                    .w_full()
                    .flex()
                    .flex_col()
                    .gap(Spacing::XXS)
                    .child(
                        div()
                            .text_size(FontSize::BODY)
                            .font_weight(FontWeight::MEDIUM)
                            .text_color(theme.text_primary)
                            .child(title),
                    )
                    .child(
                        div()
                            .text_size(FontSize::SM)
                            .text_color(theme.text_muted)
                            .line_height(LineHeight::SM)
                            .child(desc),
                    ),
            )
            .child(
                div()
                    .w_full()
                    .flex()
                    .flex_row()
                    .flex_wrap()
                    .gap(Spacing::XS)
                    .child(
                        OptionPill::new(
                            "lang_system",
                            system_label,
                            current_pref == LanguagePreference::System,
                        )
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.select_language(LanguagePreference::System, cx);
                        })),
                    )
                    .children(LocaleId::ALL.iter().copied().map(|locale| {
                        let id = SharedString::from(format!("lang_{}", locale.code()));
                        OptionPill::new(
                            id,
                            locale.native_name(),
                            current_pref == LanguagePreference::Locale(locale),
                        )
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.select_language(LanguagePreference::Locale(locale), cx);
                        }))
                    })),
            )
    }

    pub(crate) fn select_language(
        &mut self,
        preference: LanguagePreference,
        cx: &mut Context<Self>,
    ) {
        if let Some(lm) = cx.try_global::<LocaleManager>() {
            let mut next = lm.clone();
            next.set_preference(preference);
            cx.set_global(next);
        } else {
            let detected = crate::platform::detect_system_locale();
            cx.set_global(LocaleManager::new(preference, detected.as_deref()));
        }
        update_config_all_windows(cx, |cfg| cfg.language = preference);
    }

    fn render_mode_row(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let is_live_preview =
            cx.global::<AppConfig>().default_render_mode == RenderMode::LivePreview;

        SettingRow::new(
            cx.t(TextKey::SettingsRenderMode).to_string(),
            div()
                .flex()
                .flex_row()
                .gap(Spacing::SMD)
                .child(
                    OptionPill::new(
                        "render_mode_live",
                        cx.t(TextKey::SettingsRenderModeLivePreview).to_string(),
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
                        cx.t(TextKey::SettingsRenderModeSource).to_string(),
                        !is_live_preview,
                    )
                    .on_click(cx.listener(|_, _, _, cx| {
                        update_config_all_windows(cx, |cfg| {
                            cfg.default_render_mode = RenderMode::Source
                        })
                    })),
                ),
        )
        .description(cx.t(TextKey::SettingsRenderModeDesc).to_string())
    }

    fn restore_workspace_row(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let restore_ws = cx.global::<AppConfig>().restore_workspace;

        SettingRow::new(
            cx.t(TextKey::SettingsRestoreWorkspace).to_string(),
            Switch::new("toggle_restore_ws", restore_ws).on_click(cx.listener(|_, _, _, cx| {
                update_config(cx, |cfg| cfg.restore_workspace = !cfg.restore_workspace)
            })),
        )
        .description(cx.t(TextKey::SettingsRestoreWorkspaceDesc).to_string())
    }

    fn collocated_mode_row(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let collocated = cx.global::<AppConfig>().collocated_mode;

        SettingRow::new(
            cx.t(TextKey::SettingsCollocatedMode).to_string(),
            Switch::new("toggle_collocated_mode", collocated).on_click(cx.listener(
                |this, _, _, cx| {
                    this.toggle_collocated_mode(cx);
                },
            )),
        )
        .description(cx.t(TextKey::SettingsCollocatedModeDesc).to_string())
    }

    pub(crate) fn toggle_collocated_mode(&mut self, cx: &mut Context<Self>) {
        update_config_all_windows(cx, |cfg| cfg.collocated_mode = !cfg.collocated_mode);
    }

    fn grammar_base_url_row(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.global::<ThemeManager>().theme().clone();
        let cfg = cx.global::<AppConfig>();
        let source = cfg.grammar_source;
        let effective_url = cfg.effective_grammar_base_url().to_string();
        let is_jsdelivr = source == GrammarSource::JsDelivr;
        let is_unpkg = source == GrammarSource::Unpkg;
        let is_offline = source == GrammarSource::Offline;
        let is_custom = source == GrammarSource::Custom;

        let url_bar = if is_custom {
            self.grammar_url_field.clone().into_any_element()
        } else {
            let display_text = if effective_url.is_empty() {
                cx.t(TextKey::SettingsGrammarOfflineStatus).to_string()
            } else {
                effective_url
            };
            let text_color = if is_offline {
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
                .into_any_element()
        };

        div()
            .w_full()
            .flex()
            .flex_col()
            .gap(Spacing::XS)
            .child(
                SettingRow::new(
                    cx.t(TextKey::SettingsGrammarSource).to_string(),
                    div()
                        .flex()
                        .flex_row()
                        .gap(Spacing::XS)
                        .child(
                            OptionPill::new("grammar_cdn_jsdelivr", "jsDelivr", is_jsdelivr)
                                .on_click(cx.listener(|this, _, window, cx| {
                                    this.select_grammar_source(GrammarSource::JsDelivr, cx);
                                    this.focus(window);
                                })),
                        )
                        .child(
                            OptionPill::new("grammar_cdn_unpkg", "unpkg", is_unpkg).on_click(
                                cx.listener(|this, _, window, cx| {
                                    this.select_grammar_source(GrammarSource::Unpkg, cx);
                                    this.focus(window);
                                }),
                            ),
                        )
                        .child(
                            OptionPill::new(
                                "grammar_cdn_offline",
                                cx.t(TextKey::SettingsGrammarOffline).to_string(),
                                is_offline,
                            )
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.select_grammar_source(GrammarSource::Offline, cx);
                                this.focus(window);
                            })),
                        )
                        .child(
                            OptionPill::new(
                                "grammar_cdn_custom",
                                cx.t(TextKey::SettingsGrammarCustom).to_string(),
                                is_custom,
                            )
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.select_custom_grammar_mode(window, cx);
                            })),
                        ),
                )
                .description(cx.t(TextKey::SettingsGrammarSourceDesc).to_string()),
            )
            .child(url_bar)
    }

    pub(crate) fn select_grammar_source(
        &mut self,
        source: GrammarSource,
        cx: &mut Context<Self>,
    ) {
        self.active_input = None;
        update_config(cx, move |cfg| {
            cfg.grammar_source = source;
        });
        self.grammar_url_field
            .update(cx, |field, cx| field.commit_if_changed(cx));
        let effective = cx
            .global::<AppConfig>()
            .effective_grammar_base_url()
            .to_string();
        crate::syntax::set_global_grammar_base_url(effective);
        cx.notify();
    }

    pub(crate) fn select_custom_grammar_mode(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.select_grammar_source(GrammarSource::Custom, cx);
        self.grammar_url_field.update(cx, |field, cx| {
            field.focus_and_select_all(window, cx);
        });
    }

    fn auto_check_updates_row(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let auto_check = cx.global::<AppConfig>().auto_check_updates;

        SettingRow::new(
            cx.t(TextKey::SettingsAutoCheckUpdates).to_string(),
            Switch::new("toggle_auto_check_updates", auto_check).on_click(cx.listener(
                |_, _, _, cx| {
                    update_config(cx, |cfg| cfg.auto_check_updates = !cfg.auto_check_updates);
                },
            )),
        )
        .description(cx.t(TextKey::SettingsAutoCheckUpdatesDesc).to_string())
    }

    fn update_source_row(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.global::<ThemeManager>().theme().clone();
        let cfg = cx.global::<AppConfig>();
        let source = cfg.update_source;
        let effective_url = cfg.effective_update_base_url().to_string();
        let is_github = source == UpdateSource::GitHub;
        let is_disabled = source == UpdateSource::Disabled;
        let is_custom = source == UpdateSource::Custom;

        let url_bar = if is_custom {
            self.update_url_field.clone().into_any_element()
        } else {
            let display_text = if effective_url.is_empty() {
                cx.t(TextKey::SettingsUpdateDisabledStatus).to_string()
            } else {
                effective_url
            };
            let text_color = if is_disabled {
                theme.text_muted
            } else {
                theme.text_secondary
            };

            div()
                .id("update_url_display")
                .h(ControlHeight::MD)
                .flex()
                .flex_row()
                .items_center()
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
                .into_any_element()
        };

        div()
            .w_full()
            .flex()
            .flex_col()
            .gap(Spacing::XS)
            .child(
                SettingRow::new(
                    cx.t(TextKey::SettingsUpdateSource).to_string(),
                    div()
                        .flex()
                        .flex_row()
                        .gap(Spacing::XS)
                        .child(
                            OptionPill::new(
                                "update_source_github",
                                cx.t(TextKey::SettingsUpdateSourceGitHub).to_string(),
                                is_github,
                            )
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.select_update_source(UpdateSource::GitHub, cx);
                                this.focus(window);
                            })),
                        )
                        .child(
                            OptionPill::new(
                                "update_source_disabled",
                                cx.t(TextKey::SettingsUpdateSourceDisabled).to_string(),
                                is_disabled,
                            )
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.select_update_source(UpdateSource::Disabled, cx);
                                this.focus(window);
                            })),
                        )
                        .child(
                            OptionPill::new(
                                "update_source_custom",
                                cx.t(TextKey::SettingsUpdateSourceCustom).to_string(),
                                is_custom,
                            )
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.select_custom_update_mode(window, cx);
                            })),
                        ),
                )
                .description(cx.t(TextKey::SettingsUpdateSourceDesc).to_string()),
            )
            .child(url_bar)
    }

    pub(crate) fn select_update_source(
        &mut self,
        source: UpdateSource,
        cx: &mut Context<Self>,
    ) {
        self.active_input = None;
        update_config(cx, move |cfg| cfg.update_source = source);
        self.update_url_field
            .update(cx, |field, cx| field.commit_if_changed(cx));
        cx.notify();
    }

    pub(crate) fn select_custom_update_mode(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.select_update_source(UpdateSource::Custom, cx);
        self.update_url_field.update(cx, |field, cx| {
            field.focus_and_select_all(window, cx);
        });
    }

    fn check_updates_now_row(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.global::<ThemeManager>().theme().clone();
        let disabled = cx.global::<AppConfig>().update_source == UpdateSource::Disabled;
        let status = cx
            .try_global::<UpdateManager>()
            .map(UpdateManager::status)
            .unwrap_or(UpdateStatus::Idle);
        let is_busy = matches!(
            status,
            UpdateStatus::Checking { .. }
                | UpdateStatus::Downloading { .. }
                | UpdateStatus::Verifying { .. }
                | UpdateStatus::Extracting { .. }
                | UpdateStatus::Applying { .. }
        );
        let can_click = !disabled && !is_busy;

        let desc = match &status {
            UpdateStatus::Checking { .. } => cx.t(TextKey::UpdateChecking).to_string(),
            UpdateStatus::UpToDate { .. } => cx.t(TextKey::UpdateUpToDate).to_string(),
            UpdateStatus::Available(rel) => cx.tf(
                TextKey::UpdateAvailableTitle,
                &[("version", rel.version.as_str())],
            ),
            UpdateStatus::ManualUpdateOnly { release, .. } => cx.tf(
                TextKey::UpdateManualTitle,
                &[("version", release.version.as_str())],
            ),
            UpdateStatus::ReadyToInstall(staged) => cx.tf(
                TextKey::UpdateReadyTitle,
                &[("version", staged.release.version.as_str())],
            ),
            UpdateStatus::Failed { message, .. } => {
                cx.tf(TextKey::UpdateFailed, &[("error", message.as_str())])
            }
            _ => format!("v{}", env!("CARGO_PKG_VERSION")),
        };

        let btn_label = cx.t(TextKey::SettingsCheckUpdatesNow).to_string();

        SettingRow::new(
            cx.t(TextKey::MenuCheckForUpdates).to_string(),
            div()
                .id("settings_check_updates_btn")
                .debug_selector(|| "settings_check_updates_btn".to_string())
                .h(ControlHeight::MD)
                .px(Spacing::MD)
                .rounded(Radius::MD)
                .bg(if can_click {
                    theme.btn_bg
                } else {
                    theme.bg_surface
                })
                .when(can_click, |s| {
                    s.hover(|h| h.bg(theme.btn_hover))
                        .active(|a| a.bg(theme.btn_active))
                        .cursor_pointer()
                })
                .text_color(if can_click {
                    theme.btn_text
                } else {
                    theme.text_muted
                })
                .text_size(FontSize::SM)
                .font_weight(FontWeight::MEDIUM)
                .flex()
                .items_center()
                .justify_center()
                .child(btn_label)
                .on_click(move |_, _, cx| {
                    if can_click {
                        crate::app::commands::check_for_updates(cx);
                    }
                }),
        )
        .description(desc)
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
    fn test_grammar_source_from_legacy_url() {
        assert_eq!(GrammarSource::from_legacy_url(""), GrammarSource::Offline);
        assert_eq!(
            GrammarSource::from_legacy_url("   "),
            GrammarSource::Offline
        );
        assert_eq!(
            GrammarSource::from_legacy_url(GrammarSource::JSDELIVR_URL),
            GrammarSource::JsDelivr
        );
        assert_eq!(
            GrammarSource::from_legacy_url("https://cdn.jsdelivr.net/npm/"),
            GrammarSource::JsDelivr
        );
        assert_eq!(
            GrammarSource::from_legacy_url(GrammarSource::UNPKG_URL),
            GrammarSource::Unpkg
        );
        assert_eq!(
            GrammarSource::from_legacy_url("https://unpkg.com/"),
            GrammarSource::Unpkg
        );
        assert_eq!(
            GrammarSource::from_legacy_url("https://mirror.example.com/npm"),
            GrammarSource::Custom
        );
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
        cx.run_until_parked();

        // Initial source switching
        cx.update(|_window, cx| {
            settings.update(cx, |s, cx| {
                assert_eq!(cx.global::<AppConfig>().grammar_source, GrammarSource::JsDelivr);
                assert_eq!(
                    cx.global::<AppConfig>().effective_grammar_base_url(),
                    GrammarSource::JSDELIVR_URL
                );

                // Switch to Unpkg
                s.select_grammar_source(GrammarSource::Unpkg, cx);
                assert_eq!(cx.global::<AppConfig>().grammar_source, GrammarSource::Unpkg);
                assert_eq!(
                    cx.global::<AppConfig>().effective_grammar_base_url(),
                    GrammarSource::UNPKG_URL
                );

                // Switch to Offline
                s.select_grammar_source(GrammarSource::Offline, cx);
                assert_eq!(cx.global::<AppConfig>().grammar_source, GrammarSource::Offline);
                assert_eq!(cx.global::<AppConfig>().effective_grammar_base_url(), "");
            });
        });

        // Custom mode reveals and focuses the standalone SettingTextField (initially empty, NOT polluted by preset)
        let field = cx.update(|window, cx| {
            settings.update(cx, |s, cx| {
                s.select_custom_grammar_mode(window, cx);
                assert_eq!(cx.global::<AppConfig>().grammar_source, GrammarSource::Custom);
                assert_eq!(s.grammar_url_field.read(cx).text(cx), "");
                s.grammar_url_field.clone()
            })
        });
        cx.run_until_parked();

        // Typing custom URL and blurring auto-saves normalized URL to custom_grammar_url
        cx.update(|_window, cx| {
            let input = field.read(cx).input().clone();
            input.update(cx, |inp, cx| {
                inp.set_text("https://mirror.example.com/npm/", cx);
            });
        });

        cx.update(|window, cx| {
            settings.read(cx).focus(window);
        });
        cx.run_until_parked();

        cx.update(|_window, cx| {
            settings.update(cx, |s, cx| {
                let cfg = cx.global::<AppConfig>();
                assert_eq!(cfg.grammar_source, GrammarSource::Custom);
                assert_eq!(cfg.custom_grammar_url, "https://mirror.example.com/npm");
                assert_eq!(
                    cfg.effective_grammar_base_url(),
                    "https://mirror.example.com/npm"
                );
                assert_eq!(
                    s.grammar_url_field.read(cx).text(cx),
                    "https://mirror.example.com/npm"
                );
            });
        });

        // Switching across all other presets must NEVER overwrite/pollute custom_grammar_url
        cx.update(|window, cx| {
            settings.update(cx, |s, cx| {
                for (source, expected_url) in [
                    (GrammarSource::JsDelivr, GrammarSource::JSDELIVR_URL),
                    (GrammarSource::Unpkg, GrammarSource::UNPKG_URL),
                    (GrammarSource::Offline, ""),
                ] {
                    s.select_grammar_source(source, cx);
                    let cfg = cx.global::<AppConfig>();
                    assert_eq!(cfg.grammar_source, source);
                    assert_eq!(cfg.effective_grammar_base_url(), expected_url);
                    assert_eq!(cfg.custom_grammar_url, "https://mirror.example.com/npm");
                    assert_eq!(
                        s.grammar_url_field.read(cx).text(cx),
                        "https://mirror.example.com/npm"
                    );
                }

                // Switching back to Custom restores saved custom URL cleanly
                s.select_custom_grammar_mode(window, cx);
                let cfg = cx.global::<AppConfig>();
                assert_eq!(cfg.grammar_source, GrammarSource::Custom);
                assert_eq!(
                    cfg.effective_grammar_base_url(),
                    "https://mirror.example.com/npm"
                );
            });
        });

        // Editing draft in Custom then switching directly to a preset preserves custom draft without overwriting preset
        cx.update(|_window, cx| {
            let input = field.read(cx).input().clone();
            input.update(cx, |inp, cx| {
                inp.set_text("https://edited.example.com/npm/", cx);
            });
            settings.update(cx, |s, cx| {
                s.select_grammar_source(GrammarSource::Unpkg, cx);
                let cfg = cx.global::<AppConfig>();
                assert_eq!(cfg.grammar_source, GrammarSource::Unpkg);
                assert_eq!(cfg.effective_grammar_base_url(), GrammarSource::UNPKG_URL);
                assert_eq!(cfg.custom_grammar_url, "https://edited.example.com/npm");
            });
        });

        // Closing settings window must not flush Custom field back over active Unpkg preset
        cx.update(|window, _cx| window.remove_window());
        cx.run_until_parked();

        assert_eq!(
            cx.read(|cx| cx.global::<AppConfig>().grammar_source),
            GrammarSource::Unpkg
        );
        assert_eq!(
            cx.read(|cx| cx.global::<AppConfig>().effective_grammar_base_url().to_string()),
            GrammarSource::UNPKG_URL
        );
        assert_eq!(
            cx.read(|cx| cx.global::<AppConfig>().custom_grammar_url.clone()),
            "https://edited.example.com/npm"
        );
    }

    #[gpui::test]
    fn test_settings_grammar_url_editing_and_cancellation(cx: &mut gpui::TestAppContext) {
        let _guard = ResetGlobalGrammarUrlOnDrop;
        init_test_globals(cx);
        let (settings, cx) = cx.add_window_view(|window, cx| {
            window.activate_window();
            SettingsWindow::new(cx)
        });
        cx.run_until_parked();

        let field = cx.update(|window, cx| {
            settings.update(cx, |s, cx| {
                s.select_grammar_source(GrammarSource::Offline, cx);
                s.select_custom_grammar_mode(window, cx);
                s.grammar_url_field.clone()
            })
        });
        cx.run_until_parked();

        // Typing then cancelling reverts draft while keeping GrammarSource::Custom distinct from Offline
        cx.update(|_window, cx| {
            assert_eq!(field.read(cx).text(cx), "");
            let input = field.read(cx).input().clone();
            input.update(cx, |inp, cx| {
                inp.set_text("https://discarded.example.com", cx);
                inp.cancel(cx);
            });
        });
        cx.run_until_parked();

        cx.update(|_window, cx| {
            let cfg = cx.global::<AppConfig>();
            assert_eq!(cfg.grammar_source, GrammarSource::Custom);
            assert_eq!(cfg.custom_grammar_url, "");
            assert_eq!(cfg.effective_grammar_base_url(), "");
            assert_eq!(field.read(cx).text(cx), "");
        });
    }

    #[gpui::test]
    fn test_settings_collocated_mode_toggle_and_toml_roundtrip(cx: &mut gpui::TestAppContext) {
        let temp_dir = tempfile::tempdir().unwrap();
        let cfg_path = temp_dir.path().join("config.toml");
        let state_path = temp_dir.path().join("state.toml");
        cx.update(|cx| {
            let cfg = AppConfig {
                config_file_path: Some(cfg_path.clone()),
                state_file_path: Some(state_path.clone()),
                ..AppConfig::default()
            };
            cx.set_global(ThemeManager::from_config(&cfg));
            cx.set_global(cfg);
        });

        let (settings, cx) = cx.add_window_view(|window, cx| {
            window.activate_window();
            SettingsWindow::new(cx)
        });
        cx.run_until_parked();

        cx.update(|_window, cx| {
            assert!(!cx.global::<AppConfig>().collocated_mode);
            settings.update(cx, |s, cx| {
                s.toggle_collocated_mode(cx);
            });
            assert!(cx.global::<AppConfig>().collocated_mode);
        });
        cx.run_until_parked();

        let reloaded = AppConfig::load_from_paths(cfg_path.clone(), state_path.clone());
        assert!(reloaded.collocated_mode);

        cx.update(|_window, cx| {
            settings.update(cx, |s, cx| {
                s.toggle_collocated_mode(cx);
            });
            assert!(!cx.global::<AppConfig>().collocated_mode);
        });
        cx.run_until_parked();

        let reloaded_off = AppConfig::load_from_paths(cfg_path, state_path);
        assert!(!reloaded_off.collocated_mode);
    }

    #[gpui::test]
    fn test_settings_language_selection_updates_config_and_live_locale(
        cx: &mut gpui::TestAppContext,
    ) {
        let temp_dir = tempfile::tempdir().unwrap();
        let cfg_path = temp_dir.path().join("config.toml");
        let state_path = temp_dir.path().join("state.toml");
        cx.update(|cx| {
            let cfg = AppConfig {
                config_file_path: Some(cfg_path.clone()),
                state_file_path: Some(state_path.clone()),
                ..AppConfig::default()
            };
            cx.set_global(LocaleManager::new(cfg.language, Some("en-US")));
            cx.set_global(ThemeManager::from_config(&cfg));
            cx.set_global(cfg);
        });

        let (settings, cx) = cx.add_window_view(|window, cx| {
            window.activate_window();
            SettingsWindow::new(cx)
        });
        cx.run_until_parked();

        cx.update(|_window, cx| {
            assert_eq!(cx.locale(), LocaleId::En);
            assert_eq!(cx.t(TextKey::SettingsTitle), "Settings");

            settings.update(cx, |s, cx| {
                s.select_language(LanguagePreference::Locale(LocaleId::ZhHans), cx);
            });

            assert_eq!(
                cx.global::<AppConfig>().language,
                LanguagePreference::Locale(LocaleId::ZhHans)
            );
            assert_eq!(cx.locale(), LocaleId::ZhHans);
            assert_eq!(cx.t(TextKey::SettingsTitle), "设置");

            settings.update(cx, |s, cx| {
                s.select_language(LanguagePreference::Locale(LocaleId::Ja), cx);
            });
            assert_eq!(cx.locale(), LocaleId::Ja);
            assert_eq!(cx.t(TextKey::SettingsTitle), "設定");

            settings.update(cx, |s, cx| {
                s.select_language(LanguagePreference::System, cx);
            });
            assert_eq!(cx.locale(), LocaleId::En);
        });
        cx.run_until_parked();
    }

    #[gpui::test]
    fn test_settings_update_source_isolation(cx: &mut gpui::TestAppContext) {
        init_test_globals(cx);
        let (settings, cx) = cx.add_window_view(|window, cx| {
            window.activate_window();
            SettingsWindow::new(cx)
        });
        cx.run_until_parked();

        let field = cx.update(|window, cx| {
            settings.update(cx, |s, cx| {
                assert_eq!(cx.global::<AppConfig>().update_source, UpdateSource::GitHub);

                s.select_custom_update_mode(window, cx);
                assert_eq!(cx.global::<AppConfig>().update_source, UpdateSource::Custom);
                s.update_url_field.clone()
            })
        });
        cx.run_until_parked();

        cx.update(|_window, cx| {
            let input = field.read(cx).input().clone();
            input.update(cx, |inp, cx| {
                inp.set_text("https://mirror.example.com/inviscid/", cx);
            });
        });
        cx.update(|window, cx| {
            settings.read(cx).focus(window);
        });
        cx.run_until_parked();

        cx.update(|_window, cx| {
            settings.update(cx, |s, cx| {
                assert_eq!(
                    cx.global::<AppConfig>().custom_update_url,
                    "https://mirror.example.com/inviscid"
                );
                s.select_update_source(UpdateSource::GitHub, cx);
                assert_eq!(cx.global::<AppConfig>().update_source, UpdateSource::GitHub);
                assert_eq!(
                    cx.global::<AppConfig>().effective_update_base_url(),
                    UpdateSource::GITHUB_BASE_URL
                );
                assert_eq!(
                    cx.global::<AppConfig>().custom_update_url,
                    "https://mirror.example.com/inviscid"
                );
                s.select_update_source(UpdateSource::Disabled, cx);
                assert_eq!(cx.global::<AppConfig>().effective_update_base_url(), "");
                assert_eq!(
                    cx.global::<AppConfig>().custom_update_url,
                    "https://mirror.example.com/inviscid"
                );
            });
        });
    }
}
