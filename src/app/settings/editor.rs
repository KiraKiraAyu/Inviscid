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
use super::{ActiveInput, SettingsWindow};
use crate::config::{AppConfig, UserPreferences};
use crate::theme::ThemeManager;
use crate::ui::{
    ControlHeight, FontSize, OptionPill, Radius, SettingRow, Spacing, Stepper, Switch, TextInput,
    TextInputEvent,
};

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

    fn asset_dir_row(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.global::<ThemeManager>().theme().clone();
        let current_dir = cx.global::<AppConfig>().asset_dir.clone();
        let is_default = current_dir == UserPreferences::DEFAULT_ASSET_DIR;
        let is_editing = self.is_editing_asset_dir();

        let action_btn = |id: &'static str, label: &'static str, accent: bool| {
            div()
                .id(id)
                .px(Spacing::SM)
                .py(Spacing::XXS)
                .rounded(Radius::XS)
                .bg(theme.bg_surface)
                .border_1()
                .border_color(theme.border_subtle)
                .hover(|s| s.bg(theme.btn_hover).border_color(theme.border))
                .cursor_pointer()
                .text_size(FontSize::CAPTION)
                .font_weight(FontWeight::MEDIUM)
                .text_color(if accent {
                    theme.text_accent
                } else {
                    theme.text_secondary
                })
                .child(label)
        };

        let controls = div()
            .flex()
            .flex_row()
            .items_center()
            .gap(Spacing::XS)
            .when(!is_editing, |this| {
                this.child(action_btn("asset_dir_edit_btn", "Edit", true).on_click(
                    cx.listener(|this, _, window, cx| {
                        this.start_editing_asset_dir(window, cx);
                    }),
                ))
            })
            .child(
                action_btn("asset_dir_browse_btn", "Browse...", false).on_click(cx.listener(
                    |_, _, _, cx| {
                        cx.spawn(async move |this, cx| {
                            if let Some(folder) = rfd::AsyncFileDialog::new().pick_folder().await {
                                let picked = folder.path().to_string_lossy().into_owned();
                                let _ = this.update(cx, |settings, cx| {
                                    settings.set_asset_dir(picked, cx);
                                });
                            }
                        })
                        .detach();
                    },
                )),
            )
            .when(!is_default, |this| {
                this.child(
                    action_btn("asset_dir_reset_btn", "Reset", false).on_click(cx.listener(
                        |this, _, _, cx| {
                            this.reset_asset_dir(cx);
                        },
                    )),
                )
            });

        let path_bar = if let Some(input) = self.asset_dir_input() {
            div()
                .id("asset_dir_editor")
                .flex()
                .flex_row()
                .items_center()
                .child(input.clone())
                .into_any_element()
        } else {
            div()
                .id("asset_dir_display")
                .h(ControlHeight::MD)
                .px(Spacing::MDS)
                .rounded(Radius::MD)
                .bg(theme.bg_surface)
                .border_1()
                .border_color(theme.border_subtle)
                .hover(|s| s.border_color(theme.border))
                .cursor_pointer()
                .flex()
                .flex_row()
                .items_center()
                .overflow_hidden()
                .whitespace_nowrap()
                .text_size(FontSize::CAPTION)
                .text_color(theme.text_secondary)
                .on_click(cx.listener(|this, _, window, cx| {
                    this.start_editing_asset_dir(window, cx);
                }))
                .child(current_dir)
                .into_any_element()
        };

        div()
            .w_full()
            .flex()
            .flex_col()
            .gap(Spacing::XS)
            .child(
                SettingRow::new("Image Assets Directory", controls).description(
                    "Where pasted or dropped images are saved. Relative paths (default: assets) resolve next to the document; paths starting with / or a drive letter (e.g. D:\\assets) are absolute.",
                ),
            )
            .child(path_bar)
    }

    pub(crate) fn is_editing_asset_dir(&self) -> bool {
        matches!(self.active_input, Some(ActiveInput::EditingAssetDir { .. }))
    }

    pub(crate) fn asset_dir_input(&self) -> Option<&Entity<TextInput>> {
        match &self.active_input {
            Some(ActiveInput::EditingAssetDir { input, .. }) => Some(input),
            _ => None,
        }
    }

    pub(crate) fn start_editing_asset_dir(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let current = cx.global::<AppConfig>().asset_dir.clone();
        let input = cx.new(|cx| {
            let mut input = TextInput::new(&current, cx)
                .placeholder(UserPreferences::DEFAULT_ASSET_DIR)
                .height(ControlHeight::MD)
                .radius(Radius::MD)
                .padding_x(Spacing::MDS)
                .bg(crate::ui::TextInputBg::Surface);
            input.buffer_mut().select_all();
            input
        });
        let focus_handle = input.read(cx).focus_handle(cx);
        let event_sub = cx.subscribe_in(
            &input,
            window,
            |this, _emitter, event: &TextInputEvent, window, cx| match event {
                TextInputEvent::Commit(text) => {
                    this.set_asset_dir(text.clone(), cx);
                    window.focus(&this.focus_handle);
                }
                TextInputEvent::Cancel => {
                    this.cancel_editing_asset_dir(cx);
                    window.focus(&this.focus_handle);
                }
                TextInputEvent::Changed(_) => {}
            },
        );
        let blur_sub = cx.on_blur(&focus_handle, window, |this, _window, cx| {
            this.commit_asset_dir(cx);
        });
        window.focus(&focus_handle);
        self.active_input = Some(ActiveInput::EditingAssetDir {
            input,
            _subscriptions: [event_sub, blur_sub],
        });
        cx.notify();
    }

    pub(crate) fn cancel_editing_asset_dir(&mut self, cx: &mut Context<Self>) {
        if self.is_editing_asset_dir() {
            self.active_input = None;
            cx.notify();
        }
    }

    pub(crate) fn set_asset_dir(&mut self, path: impl Into<String>, cx: &mut Context<Self>) {
        let normalized = crate::media::normalize_asset_dir(&path.into());
        self.active_input = None;
        update_config(cx, move |cfg| {
            cfg.asset_dir = normalized;
        });
        cx.notify();
    }

    pub(crate) fn commit_asset_dir(&mut self, cx: &mut Context<Self>) {
        if let Some(input) = self.asset_dir_input() {
            let draft = input.read(cx).text();
            self.set_asset_dir(draft, cx);
        }
    }

    pub(crate) fn reset_asset_dir(&mut self, cx: &mut Context<Self>) {
        self.set_asset_dir(UserPreferences::DEFAULT_ASSET_DIR, cx);
    }
}
