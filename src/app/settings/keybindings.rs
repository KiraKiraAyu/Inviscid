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
use crate::theme::ThemeManager;
use crate::ui::{
    ControlHeight, FontSize, Radius, SelectableRow, Spacing, format_keystroke_for_display,
};

/// Fixed row height for keybinding entries in the list table.
const KEYBINDING_ROW_HEIGHT: Pixels = px(36.0);

impl SettingsWindow {
    pub(crate) fn render_keybindings_panel(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let has_custom = !cx.global::<AppConfig>().custom_keybindings.is_empty();

        div()
            .flex_1()
            .flex()
            .flex_col()
            .gap(Spacing::LG)
            .child(
                div()
                    .flex()
                    .flex_row()
                    .items_center()
                    .justify_end()
                    .when(has_custom, |this| this.child(self.reset_all_button(cx))),
            )
            .child(
                div()
                    .flex_1()
                    .flex()
                    .flex_col()
                    .gap(Spacing::SMD)
                    .children(keybindings().map(|meta| self.keybinding_row(meta, cx))),
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

    /// Publishes an already-mutated keybinding config and drops any in-progress recording.
    ///
    /// The `AppConfig` keybinding setters persist on their own, so this path deliberately bypasses
    /// [`update_config`] rather than writing `config.toml` a second time.
    pub(crate) fn commit_keybindings(&mut self, cfg: AppConfig, cx: &mut Context<Self>) {
        apply_configured_keys(&cfg, cx);
        cx.set_global(cfg);
        self.active_input = None;
        // Menus in other windows render shortcut hints straight from the keymap.
        cx.refresh_windows();
    }

    fn reset_all_button(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.global::<ThemeManager>().theme().clone();

        div()
            .id("reset_all_keybindings_btn")
            .h(ControlHeight::SM)
            .px(Spacing::MDS)
            .rounded(Radius::SM)
            .bg(theme.bg_toolbar)
            .border_1()
            .border_color(theme.border_subtle)
            .hover(|s| s.bg(theme.btn_hover))
            .cursor_pointer()
            .flex()
            .items_center()
            .justify_center()
            .on_click(cx.listener(|this, _, _, cx| {
                let mut cfg = cx.global::<AppConfig>().clone();
                cfg.reset_all_keybindings();
                this.commit_keybindings(cfg, cx);
            }))
            .child(
                div()
                    .text_size(FontSize::CAPTION)
                    .text_color(theme.text_secondary)
                    .child("Reset All"),
            )
    }

    fn keybinding_row(&self, meta: &'static KeybindingMeta, cx: &mut Context<Self>) -> AnyElement {
        let theme = cx.global::<ThemeManager>().theme().clone();
        let is_recording = self.is_recording(meta);
        let is_modified = cx
            .global::<AppConfig>()
            .custom_keybindings
            .contains_key(meta.id);
        let badge = self.key_badge(meta, cx);
        let reset = is_modified.then(|| self.reset_button(meta, cx));

        SelectableRow::new(ElementId::Name(format!("keybind_row_{}", meta.id).into()))
            .height(KEYBINDING_ROW_HEIGHT)
            .selected(is_recording)
            .leading(
                div()
                    .flex()
                    .flex_row()
                    .items_center()
                    .gap(Spacing::MDS)
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
                    .child(
                        div()
                            .text_size(FontSize::BODY)
                            .text_color(theme.text_primary)
                            .child(meta.name),
                    ),
            )
            .trailing(
                div()
                    .flex()
                    .flex_row()
                    .items_center()
                    .gap(Spacing::SM)
                    .child(badge)
                    .when_some(reset, |this, button| this.child(button)),
            )
            .into_any_element()
    }

    fn key_badge(&self, meta: &'static KeybindingMeta, cx: &mut Context<Self>) -> AnyElement {
        let theme = cx.global::<ThemeManager>().theme().clone();
        let is_recording = self.is_recording(meta);
        let config = cx.global::<AppConfig>();
        let is_modified = config.custom_keybindings.contains_key(meta.id);
        let display_key =
            format_keystroke_for_display(&config.get_keybinding(meta.id, meta.primary_keystroke()));

        div()
            .id(ElementId::Name(format!("keybind_badge_{}", meta.id).into()))
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
            .items_center()
            .justify_center()
            .on_click(cx.listener(move |this, _, window, cx| {
                this.toggle_recording(meta.id, cx);
                window.focus(&this.focus_handle);
            }))
            .child(if is_recording {
                div()
                    .text_size(FontSize::CAPTION)
                    .font_weight(FontWeight::MEDIUM)
                    .text_color(theme.text_accent)
                    .child("Recording...")
            } else {
                div()
                    .flex()
                    .flex_row()
                    .items_center()
                    .gap(Spacing::XS)
                    .child(
                        div()
                            .text_size(FontSize::CAPTION)
                            .font_weight(FontWeight::MEDIUM)
                            .text_color(if is_modified {
                                theme.text_accent
                            } else {
                                theme.text_secondary
                            })
                            .child(display_key),
                    )
                    .when(is_modified, |this| {
                        this.child(
                            div()
                                .text_size(FontSize::CAPTION)
                                .text_color(theme.text_accent)
                                .child("●"),
                        )
                    })
            })
            .into_any_element()
    }

    fn reset_button(&self, meta: &'static KeybindingMeta, cx: &mut Context<Self>) -> AnyElement {
        let theme = cx.global::<ThemeManager>().theme().clone();

        div()
            .id(ElementId::Name(format!("reset_btn_{}", meta.id).into()))
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
            .into_any_element()
    }
}
