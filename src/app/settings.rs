use gpui::prelude::*;
use gpui::*;

use crate::app::actions::{KeybindingMeta, apply_configured_keys, keybindings};
use crate::config::AppConfig;
use crate::editor::RenderMode;
use crate::theme::ThemeManager;
use crate::ui::{
    ControlHeight, FontSize, Icon, IconName, IconSize, LineHeight, OptionPill, Radius,
    SelectableRow, SettingRow, Spacing, Stepper, Switch, WindowTitleBar,
    format_keystroke_for_display, keystroke_from_gpui,
};

const SIDEBAR_WIDTH: Pixels = px(190.0);
const KEYBINDING_ROW_HEIGHT: Pixels = px(36.0);

const UI_FONT_SIZE_BOUNDS: NumericBounds = NumericBounds::new(11.0, 20.0, 1.0);
const EDITOR_FONT_SIZE_BOUNDS: NumericBounds = NumericBounds::new(12.0, 28.0, 1.0);
const LINE_HEIGHT_BOUNDS: NumericBounds = NumericBounds::new(1.2, 2.4, 0.1);

/// An inclusive range plus the granularity a stepper moves at.
///
/// Drives both the clamp and the row description, so the advertised range can't drift away from
/// the enforced one.
#[derive(Clone, Copy, Debug, PartialEq)]
struct NumericBounds {
    min: f32,
    max: f32,
    step: f32,
}

impl NumericBounds {
    const fn new(min: f32, max: f32, step: f32) -> Self {
        Self { min, max, step }
    }

    /// Moves one step up or down, snapping to step multiples so repeated float addition can't
    /// accumulate error, then clamps.
    fn step(&self, value: f32, up: bool) -> f32 {
        let units = (value / self.step).round() + if up { 1.0 } else { -1.0 };
        (units * self.step).clamp(self.min, self.max)
    }

    fn describe_px(&self) -> String {
        format!("{:.0}px to {:.0}px", self.min, self.max)
    }

    fn describe_ratio(&self) -> String {
        format!("{:.1} to {:.1}", self.min, self.max)
    }
}

/// Persists a preference change and repaints only the settings window.
///
/// Use for preferences other windows read while handling an event rather than while painting
/// (`tab_size`, `auto_save`, `restore_workspace`); repainting everything would be wasted work.
fn update_config(cx: &mut Context<SettingsWindow>, change: impl FnOnce(&mut AppConfig)) {
    write_config(cx, change, false)
}

/// Like [`update_config`], but repaints every window.
///
/// Required for preferences read while painting, such as theme colors, font metrics and the
/// default render mode.
fn update_config_all_windows(
    cx: &mut Context<SettingsWindow>,
    change: impl FnOnce(&mut AppConfig),
) {
    write_config(cx, change, true)
}

fn write_config(
    cx: &mut Context<SettingsWindow>,
    change: impl FnOnce(&mut AppConfig),
    repaint_all: bool,
) {
    let mut cfg = cx.global::<AppConfig>().clone();
    let before = cfg.clone();
    change(&mut cfg);
    // Re-picking the current value is a no-op. Skipping the write is what keeps a held stepper
    // button at its limit from rewriting config.toml on every repeat.
    if cfg == before {
        return;
    }
    cfg.save_preferences();
    cx.set_global(cfg);
    if repaint_all {
        cx.refresh_windows();
    } else {
        cx.notify();
    }
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum SettingsCategory {
    General,
    Appearance,
    Editor,
    Keybindings,
}

impl SettingsCategory {
    pub const fn all() -> &'static [SettingsCategory] {
        &[
            Self::General,
            Self::Appearance,
            Self::Editor,
            Self::Keybindings,
        ]
    }

    pub const fn label(self) -> &'static str {
        match self {
            Self::General => "General",
            Self::Appearance => "Appearance",
            Self::Editor => "Editor",
            Self::Keybindings => "Keybindings",
        }
    }

    pub const fn icon(self) -> IconName {
        match self {
            Self::General => IconName::Settings,
            Self::Appearance => IconName::Palette,
            Self::Editor => IconName::FileText,
            Self::Keybindings => IconName::Keyboard,
        }
    }
}

pub struct SettingsWindow {
    focus_handle: FocusHandle,
    active_category: SettingsCategory,
    recording_action_id: Option<String>,
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
            recording_action_id: None,
        }
    }

    pub fn focus(&self, window: &mut Window) {
        window.focus(&self.focus_handle);
    }

    pub fn set_category(&mut self, category: SettingsCategory, cx: &mut Context<Self>) {
        self.active_category = category;
        self.recording_action_id = None;
        cx.notify();
    }

    pub fn active_category(&self) -> SettingsCategory {
        self.active_category
    }

    pub fn recording_action_id(&self) -> Option<&str> {
        self.recording_action_id.as_deref()
    }

    pub fn toggle_recording(&mut self, action_id: &str, cx: &mut Context<Self>) {
        if self.recording_action_id.as_deref() == Some(action_id) {
            self.recording_action_id = None;
        } else {
            self.recording_action_id = Some(action_id.to_string());
        }
        cx.notify();
    }

    /// Publishes an already-mutated keybinding config and drops any in-progress recording.
    ///
    /// The `AppConfig` keybinding setters persist on their own, so this path deliberately bypasses
    /// [`update_config`] rather than writing `config.toml` a second time.
    fn commit_keybindings(&mut self, cfg: AppConfig, cx: &mut Context<Self>) {
        apply_configured_keys(&cfg, cx);
        cx.set_global(cfg);
        self.recording_action_id = None;
        // Menus in other windows render shortcut hints straight from the keymap.
        cx.refresh_windows();
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

    fn render_general_panel(&self, cx: &mut Context<Self>) -> impl IntoElement {
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
                    .child(self.restore_workspace_row(cx)),
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

    fn render_appearance_panel(&self, cx: &mut Context<Self>) -> impl IntoElement {
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
                    .child(self.ui_font_size_row(cx)),
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
                            .child("Color Theme"),
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

        SettingRow::new(
            "UI Font Size",
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
        .description(format!(
            "Scale application interface text and controls ({}).",
            UI_FONT_SIZE_BOUNDS.describe_px()
        ))
    }

    fn render_editor_panel(&self, cx: &mut Context<Self>) -> impl IntoElement {
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
                .child(self.tab_size_row(cx))
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

    fn tab_size_row(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let tab_size = cx.global::<AppConfig>().tab_size;

        SettingRow::new(
            "Tab Size",
            div()
                .flex()
                .flex_row()
                .gap(Spacing::XS)
                .child(
                    OptionPill::new("tab_size_2", "2 spaces", tab_size == 2).on_click(
                        cx.listener(|_, _, _, cx| update_config(cx, |cfg| cfg.tab_size = 2)),
                    ),
                )
                .child(
                    OptionPill::new("tab_size_4", "4 spaces", tab_size == 4).on_click(
                        cx.listener(|_, _, _, cx| update_config(cx, |cfg| cfg.tab_size = 4)),
                    ),
                ),
        )
        .description("Number of spaces inserted for tab indentation.")
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

    fn render_keybindings_panel(&self, cx: &mut Context<Self>) -> impl IntoElement {
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
    fn is_recording(&self, meta: &KeybindingMeta) -> bool {
        self.recording_action_id.as_deref() == Some(meta.id)
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
                let Some(action_id) = this.recording_action_id.as_deref() else {
                    if event.keystroke.key.eq_ignore_ascii_case("escape") {
                        window.remove_window();
                    }
                    return;
                };
                if event.keystroke.key.eq_ignore_ascii_case("escape") {
                    this.recording_action_id = None;
                    cx.notify();
                    return;
                }
                if let Some(keystroke) = keystroke_from_gpui(event) {
                    let mut cfg = cx.global::<AppConfig>().clone();
                    cfg.set_keybinding(action_id.to_string(), keystroke);
                    this.commit_keybindings(cfg, cx);
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
    use std::collections::HashSet;

    #[test]
    fn test_numeric_bounds_steps_within_range() {
        assert_eq!(UI_FONT_SIZE_BOUNDS.step(13.0, true), 14.0);
        assert_eq!(UI_FONT_SIZE_BOUNDS.step(13.0, false), 12.0);
        assert_eq!(EDITOR_FONT_SIZE_BOUNDS.step(15.0, true), 16.0);
    }

    #[test]
    fn test_numeric_bounds_clamps_at_limits() {
        assert_eq!(UI_FONT_SIZE_BOUNDS.step(20.0, true), 20.0);
        assert_eq!(UI_FONT_SIZE_BOUNDS.step(11.0, false), 11.0);
        assert_eq!(EDITOR_FONT_SIZE_BOUNDS.step(28.0, true), 28.0);
        assert_eq!(EDITOR_FONT_SIZE_BOUNDS.step(12.0, false), 12.0);
        assert_eq!(LINE_HEIGHT_BOUNDS.step(2.4, true), 2.4);
        assert_eq!(LINE_HEIGHT_BOUNDS.step(1.2, false), 1.2);
    }

    #[test]
    fn test_line_height_stepping_does_not_drift() {
        // 0.1 has no exact f32 representation, so naive repeated addition accumulates error.
        let mut value = 1.6;
        for _ in 0..8 {
            value = LINE_HEIGHT_BOUNDS.step(value, true);
        }
        assert_eq!(value, 2.4);

        for _ in 0..12 {
            value = LINE_HEIGHT_BOUNDS.step(value, false);
        }
        assert_eq!(value, 1.2);
    }


    #[test]
    fn test_settings_categories_have_unique_labels() {
        let all = SettingsCategory::all();
        let labels: HashSet<_> = all.iter().map(|c| c.label()).collect();
        assert_eq!(labels.len(), all.len());
        assert!(all.iter().all(|c| !c.label().is_empty()));
    }

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
}
