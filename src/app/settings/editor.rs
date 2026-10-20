//! Editor typography, behavior, and formatting preferences.
//!
//! This module renders the "Editor" panel in the settings window, managing:
//! - Content font size and vertical line height multipliers.
//! - Debounced automatic file saving (`auto_save`).
//! - Soft line wrapping to visible editor width.
//! - Cursor blinking toggles and breathing vs. classic animation styles.

use gpui::prelude::*;
use gpui::*;

use super::SettingsWindow;
use super::bounds::{EDITOR_FONT_SIZE_BOUNDS, LINE_HEIGHT_BOUNDS};
use super::persistence::{update_config, update_config_all_windows};
use crate::config::AppConfig;
use crate::ui::{OptionPill, SettingRow, Spacing, Stepper, Switch};

impl SettingsWindow {
    pub(crate) fn render_editor_panel(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let cursor_blink = cx.global::<AppConfig>().cursor_blink;

        div().flex_1().flex().flex_col().gap(Spacing::LG).child(
            div()
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
                }),
        )
    }

    fn editor_font_size_row(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let value = cx.global::<AppConfig>().editor_font_size;

        SettingRow::new(
            "Font Size",
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
        .description(format!(
            "Font size in pixels for editor content and markdown documents ({}).",
            EDITOR_FONT_SIZE_BOUNDS.describe_px()
        ))
    }

    fn line_height_row(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let value = cx.global::<AppConfig>().line_height;

        SettingRow::new(
            "Line Height",
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
        .description(format!(
            "Vertical line height multiplier for editor text ({}).",
            LINE_HEIGHT_BOUNDS.describe_ratio()
        ))
    }

    fn auto_save_row(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let auto_save = cx.global::<AppConfig>().auto_save;

        SettingRow::new(
            "Auto Save",
            Switch::new("toggle_auto_save", auto_save).on_click(
                cx.listener(|_, _, _, cx| update_config(cx, |cfg| cfg.auto_save = !cfg.auto_save)),
            ),
        )
        .description("Automatically save modified files after editing with a debounce delay.")
    }

    fn soft_wrap_row(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let soft_wrap = cx.global::<AppConfig>().soft_wrap;

        SettingRow::new(
            "Soft Wrap",
            Switch::new("toggle_soft_wrap", soft_wrap).on_click(cx.listener(|_, _, _, cx| {
                update_config_all_windows(cx, |cfg| cfg.soft_wrap = !cfg.soft_wrap)
            })),
        )
        .description("Wrap long lines to fit the visible editor width.")
    }

    fn cursor_blink_row(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let cursor_blink = cx.global::<AppConfig>().cursor_blink;

        SettingRow::new(
            "Cursor Blink",
            Switch::new("toggle_cursor_blink", cursor_blink).on_click(cx.listener(
                |_, _, _, cx| {
                    update_config_all_windows(cx, |cfg| cfg.cursor_blink = !cfg.cursor_blink)
                },
            )),
        )
        .description("Animate editor cursor blinking when active.")
    }

    fn cursor_animation_row(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let cursor_breathing = cx.global::<AppConfig>().cursor_breathing;

        SettingRow::new(
            "Cursor Animation",
            div()
                .flex()
                .flex_row()
                .gap(Spacing::XS)
                .child(
                    OptionPill::new("cursor_style_breathing", "Breathing", cursor_breathing)
                        .on_click(cx.listener(|_, _, _, cx| {
                            update_config_all_windows(cx, |cfg| cfg.cursor_breathing = true)
                        })),
                )
                .child(
                    OptionPill::new("cursor_style_classic", "Classic Blink", !cursor_breathing)
                        .on_click(cx.listener(|_, _, _, cx| {
                            update_config_all_windows(cx, |cfg| cfg.cursor_breathing = false)
                        })),
                ),
        )
        .description("Choose between smooth cosine breathing and classic step blinking.")
    }
}
