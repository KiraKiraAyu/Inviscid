//! Editor typography, behavior, and formatting preferences.
//!
//! This module renders the "Editor" panel in the settings window, managing:
//! - Content font size and vertical line height multipliers.
//! - Debounced automatic file saving (`auto_save`).
//! - Soft line wrapping to visible editor width.
//! - Cursor blinking toggles and breathing vs. classic animation styles.

use gpui::prelude::*;
use gpui::*;

use super::bounds::{EDITOR_FONT_SIZE_BOUNDS, LINE_HEIGHT_BOUNDS};
use super::persistence::{update_config, update_config_all_windows};
use super::SettingsWindow;
use crate::config::AppConfig;
use crate::i18n::{ActiveLocale, TextKey};
use crate::theme::ThemeManager;
use crate::ui::{FontSize, LineHeight, OptionPill, SettingRow, Spacing, Stepper, Switch};

impl SettingsWindow {
    pub(crate) fn render_editor_panel(&self, cx: &mut Context<Self>) -> Div {
        let cursor_blink = cx.global::<AppConfig>().cursor_blink;

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
                    .child(self.editor_font_size_row(cx))
                    .child(self.line_height_row(cx))
                    .child(self.auto_save_row(cx))
                    .child(self.soft_wrap_row(cx))
                    .child(self.cursor_blink_row(cx))
                    .when(cursor_blink, |this| {
                        this.child(self.cursor_animation_row(cx))
                    })
                    .child(self.asset_dir_row(cx)),
            )
    }

    fn editor_font_size_row(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let value = cx.global::<AppConfig>().editor_font_size;
        let bounds = EDITOR_FONT_SIZE_BOUNDS.describe_px();

        SettingRow::new(
            cx.t(TextKey::SettingsEditorFontSize).to_string(),
            Stepper::new("editor_font_size", format!("{:.0} px", value))
                .on_decrement(cx.listener(|_, _, _, cx| {
                    update_config_all_windows(cx, |cfg| {
                        cfg.editor_font_size =
                            EDITOR_FONT_SIZE_BOUNDS.step(cfg.editor_font_size, false)
                    })
                }))
                .on_increment(cx.listener(|_, _, _, cx| {
                    update_config_all_windows(cx, |cfg| {
                        cfg.editor_font_size =
                            EDITOR_FONT_SIZE_BOUNDS.step(cfg.editor_font_size, true)
                    })
                })),
        )
        .description(cx.tf(TextKey::SettingsEditorFontSizeDesc, &[("bounds", &bounds)]))
    }

    fn line_height_row(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let value = cx.global::<AppConfig>().line_height;
        let bounds = LINE_HEIGHT_BOUNDS.describe_ratio();

        SettingRow::new(
            cx.t(TextKey::SettingsLineHeight).to_string(),
            Stepper::new("editor_line_height", format!("{:.1}", value))
                .on_decrement(cx.listener(|_, _, _, cx| {
                    update_config_all_windows(cx, |cfg| {
                        cfg.line_height = LINE_HEIGHT_BOUNDS.step(cfg.line_height, false)
                    })
                }))
                .on_increment(cx.listener(|_, _, _, cx| {
                    update_config_all_windows(cx, |cfg| {
                        cfg.line_height = LINE_HEIGHT_BOUNDS.step(cfg.line_height, true)
                    })
                })),
        )
        .description(cx.tf(TextKey::SettingsLineHeightDesc, &[("bounds", &bounds)]))
    }

    fn auto_save_row(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let auto_save = cx.global::<AppConfig>().auto_save;

        SettingRow::new(
            cx.t(TextKey::SettingsAutoSave).to_string(),
            Switch::new("toggle_auto_save", auto_save).on_click(
                cx.listener(|_, _, _, cx| update_config(cx, |cfg| cfg.auto_save = !cfg.auto_save)),
            ),
        )
        .description(cx.t(TextKey::SettingsAutoSaveDesc).to_string())
    }

    fn soft_wrap_row(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let soft_wrap = cx.global::<AppConfig>().soft_wrap;

        SettingRow::new(
            cx.t(TextKey::SettingsSoftWrap).to_string(),
            Switch::new("toggle_soft_wrap", soft_wrap).on_click(cx.listener(|_, _, _, cx| {
                update_config_all_windows(cx, |cfg| cfg.soft_wrap = !cfg.soft_wrap)
            })),
        )
        .description(cx.t(TextKey::SettingsSoftWrapDesc).to_string())
    }

    fn cursor_blink_row(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let cursor_blink = cx.global::<AppConfig>().cursor_blink;

        SettingRow::new(
            cx.t(TextKey::SettingsCursorBlink).to_string(),
            Switch::new("toggle_cursor_blink", cursor_blink).on_click(cx.listener(
                |_, _, _, cx| {
                    update_config_all_windows(cx, |cfg| cfg.cursor_blink = !cfg.cursor_blink)
                },
            )),
        )
        .description(cx.t(TextKey::SettingsCursorBlinkDesc).to_string())
    }

    fn cursor_animation_row(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let cursor_breathing = cx.global::<AppConfig>().cursor_breathing;

        SettingRow::new(
            cx.t(TextKey::SettingsCursorAnimation).to_string(),
            div()
                .flex()
                .flex_row()
                .gap(Spacing::XS)
                .child(
                    OptionPill::new(
                        "cursor_style_breathing",
                        cx.t(TextKey::SettingsCursorBreathing).to_string(),
                        cursor_breathing,
                    )
                    .on_click(cx.listener(|_, _, _, cx| {
                        update_config_all_windows(cx, |cfg| cfg.cursor_breathing = true)
                    })),
                )
                .child(
                    OptionPill::new(
                        "cursor_style_classic",
                        cx.t(TextKey::SettingsCursorClassicBlink).to_string(),
                        !cursor_breathing,
                    )
                    .on_click(cx.listener(|_, _, _, cx| {
                        update_config_all_windows(cx, |cfg| cfg.cursor_breathing = false)
                    })),
                ),
        )
        .description(cx.t(TextKey::SettingsCursorAnimationDesc).to_string())
    }

    fn asset_dir_row(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.global::<ThemeManager>().theme();
        let title = cx.t(TextKey::SettingsAssetDir).to_string();
        let desc = cx.t(TextKey::SettingsAssetDirDesc).to_string();

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
            .child(self.asset_dir_field.clone())
    }
}
