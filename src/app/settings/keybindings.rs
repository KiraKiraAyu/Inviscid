//! Keybinding customization, inspection, and reset interface.
//!
//! This module renders the "Keybindings" panel in the settings window, allowing
//! users to:
//! - View default and customized application keybindings grouped by action metadata.
//! - Rebind shortcuts via interactive recording state.
//! - Reset individual custom keybindings or restore all defaults.

use gpui::prelude::*;
use gpui::*;

use super::{ActiveInput, SettingsWindow};
use crate::app::actions::{KeybindingMeta, apply_configured_keys, keybindings};
use crate::config::AppConfig;
use crate::theme::{Theme, ThemeManager};
use crate::ui::{ControlHeight, FontSize, Radius, Spacing, format_keystroke_for_display};

/// Fixed row height for keybinding entries in the list table.
const KEYBINDING_ROW_HEIGHT: Pixels = px(36.0);

impl SettingsWindow {
    pub(crate) fn render_keybindings_panel(&self, cx: &mut Context<Self>) -> Div {
        let theme = cx.global::<ThemeManager>().theme().clone();
        let has_custom = !cx.global::<AppConfig>().custom_keybindings.is_empty();

        div()
            .flex_1()
            .flex()
            .flex_col()
            .gap(Spacing::LG)
            .when(has_custom, |this| {
                this.child(
                    div()
                        .flex()
                        .flex_row()
                        .items_center()
                        .justify_end()
                        .child(self.reset_all_button(&theme, cx)),
                )
            })
            .child(
                div().flex_1().flex().flex_col().gap(Spacing::SMD).children(
                    keybindings()
                        .enumerate()
                        .map(|(index, meta)| self.keybinding_row(index, meta, &theme, cx)),
                ),
            )
    }

    /// Whether this action's binding is currently being re-recorded.
    pub(crate) fn is_recording(&self, meta: &KeybindingMeta) -> bool {
        self.recording_action_id() == Some(meta.id)
    }

    pub fn recording_action_id(&self) -> Option<&'static str> {
        match self.active_input {
            Some(ActiveInput::RecordingKeybinding(id)) => Some(id),
            _ => None,
        }
    }

    pub fn toggle_recording(&mut self, action_id: &'static str, cx: &mut Context<Self>) {
        if self.recording_action_id() == Some(action_id) {
            self.active_input = None;
        } else {
            self.active_input = Some(ActiveInput::RecordingKeybinding(action_id));
        }
        cx.notify();
    }

    /// Publishes an already-mutated keybinding config, persists it asynchronously, and drops any
    /// in-progress recording.
    pub(crate) fn commit_keybindings(&mut self, cfg: AppConfig, cx: &mut Context<Self>) {
        apply_configured_keys(&cfg, cx);
        cfg.persist_preferences_async(cx.background_executor());
        cx.set_global(cfg);
        self.active_input = None;
        // Menus in other windows render shortcut hints straight from the keymap.
        cx.refresh_windows();
    }

    fn reset_all_button(&self, theme: &Theme, cx: &mut Context<Self>) -> Stateful<Div> {
        div()
            .id("reset_all_keybindings_btn")
            .h(ControlHeight::SM)
            .px(Spacing::MDS)
            .rounded(Radius::SM)
            .bg(theme.bg_surface)
            .border_1()
            .border_color(theme.border_subtle)
            .hover(|s| s.bg(theme.btn_hover))
            .cursor_pointer()
            .flex()
            .items_center()
            .justify_center()
            .text_size(FontSize::CAPTION)
            .text_color(theme.text_secondary)
            .on_click(cx.listener(|this, _, _, cx| {
                let mut cfg = cx.global::<AppConfig>().clone();
                cfg.reset_all_keybindings();
                this.commit_keybindings(cfg, cx);
            }))
            .child("Reset All")
    }

    fn keybinding_row(
        &self,
        index: usize,
        meta: &'static KeybindingMeta,
        theme: &Theme,
        cx: &mut Context<Self>,
    ) -> Div {
        let is_recording = self.is_recording(meta);
        let custom_binding = cx
            .global::<AppConfig>()
            .custom_keybindings
            .get(meta.id)
            .cloned();
        let is_modified = custom_binding.is_some();
        let badge = self.key_badge(index, meta, custom_binding.as_deref(), theme, cx);
        let reset = is_modified.then(|| self.reset_button(index, meta, theme, cx));

        let row = div()
            .h(KEYBINDING_ROW_HEIGHT)
            .w_full()
            .px(Spacing::MD)
            .rounded(Radius::MD)
            .flex()
            .flex_row()
            .items_center()
            .justify_between()
            .border_1()
            .border_color(if is_recording {
                theme.text_accent
            } else {
                theme.border_subtle
            })
            .child(
                div()
                    .flex()
                    .flex_row()
                    .items_center()
                    .gap(Spacing::MDS)
                    .text_size(FontSize::BODY)
                    .text_color(theme.text_primary)
                    .child(
                        div()
                            .px(Spacing::SMD)
                            .py(Spacing::XXS)
                            .rounded(Radius::XS)
                            .bg(theme.btn_bg)
                            .text_size(FontSize::CAPTION)
                            .text_color(theme.text_muted)
                            .child(meta.category),
                    )
                    .child(meta.name),
            );

        if let Some(reset) = reset {
            row.child(
                div()
                    .flex()
                    .flex_row()
                    .items_center()
                    .gap(Spacing::SM)
                    .child(badge)
                    .child(reset),
            )
        } else {
            row.child(badge)
        }
    }

    fn key_badge(
        &self,
        index: usize,
        meta: &'static KeybindingMeta,
        custom_binding: Option<&str>,
        theme: &Theme,
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        let is_recording = self.is_recording(meta);
        let is_modified = custom_binding.is_some();
        let display_key = if !is_recording {
            Some(format_keystroke_for_display(
                custom_binding.unwrap_or_else(|| meta.primary_keystroke()),
            ))
        } else {
            None
        };

        let badge = div()
            .id(("keybind_badge", index))
            .h(ControlHeight::SM)
            .px(Spacing::MDS)
            .rounded(Radius::SM)
            .border_1()
            .border_color(if is_recording {
                theme.text_accent
            } else {
                theme.border_subtle
            })
            .bg(if is_recording {
                theme.btn_active
            } else {
                theme.bg_editor
            })
            .hover(|s| {
                if is_recording {
                    s
                } else {
                    s.bg(theme.btn_hover).border_color(theme.border)
                }
            })
            .cursor_pointer()
            .flex()
            .flex_row()
            .items_center()
            .justify_center()
            .gap(Spacing::XS)
            .text_size(FontSize::CAPTION)
            .font_weight(FontWeight::MEDIUM)
            .text_color(if is_recording || is_modified {
                theme.text_accent
            } else {
                theme.text_secondary
            })
            .on_click(cx.listener(move |this, _, window, cx| {
                this.toggle_recording(meta.id, cx);
                window.focus(&this.focus_handle);
            }));

        if let Some(display_key) = display_key {
            if is_modified {
                badge.child(display_key).child("●")
            } else {
                badge.child(display_key)
            }
        } else {
            badge.child("Recording...")
        }
    }

    fn reset_button(
        &self,
        index: usize,
        meta: &'static KeybindingMeta,
        theme: &Theme,
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        div()
            .id(("reset_btn", index))
            .h(ControlHeight::SM)
            .px(Spacing::SM)
            .rounded(Radius::SM)
            .bg(theme.btn_hover)
            .hover(|s| s.bg(theme.btn_active).text_color(theme.text_primary))
            .text_size(FontSize::CAPTION)
            .text_color(theme.text_muted)
            .cursor_pointer()
            .flex()
            .items_center()
            .justify_center()
            .on_click(cx.listener(move |this, _, _, cx| {
                let mut cfg = cx.global::<AppConfig>().clone();
                cfg.reset_keybinding(meta.id);
                this.commit_keybindings(cfg, cx);
            }))
            .child("Reset")
    }
}
