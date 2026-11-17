use crate::app::SidebarView;
use crate::app::actions::{ToggleOutlineSidebar, ToggleSearchSidebar, ToggleSidebar};
use crate::editor::{EditorStatusMeta, ExternalChangeState};
use crate::theme::{DEFAULT_THEME, Theme, ThemeManager};
use crate::ui::tokens::{ControlHeight, FontSize, Radius, Spacing};
use crate::ui::{Icon, IconName, IconSize};
use gpui::prelude::*;
use gpui::*;

pub struct StatusBar {
    meta: Option<EditorStatusMeta>,
    /// The sidebar panel on screen; `None` while the sidebar is hidden.
    active_sidebar: Option<SidebarView>,
    outline_visible: bool,
    #[cfg(debug_assertions)]
    fps_visible: bool,
    #[cfg(debug_assertions)]
    fps_frame_scheduled: bool,
    #[cfg(debug_assertions)]
    fps_meter: FpsMeter,
}

#[cfg(debug_assertions)]
const FPS_ENV_VAR: &str = "INVISCID_FPS";
#[cfg(debug_assertions)]
const FPS_SAMPLE_WINDOW: std::time::Duration = std::time::Duration::from_secs(1);
#[cfg(debug_assertions)]
const FPS_DISPLAY_REFRESH: std::time::Duration = std::time::Duration::from_millis(200);

#[cfg(debug_assertions)]
fn parse_fps_env_flag(value: &str) -> Option<bool> {
    match value.trim().to_ascii_lowercase().as_str() {
        "1" | "true" | "on" | "yes" => Some(true),
        "0" | "false" | "off" | "no" => Some(false),
        _ => None,
    }
}

#[cfg(debug_assertions)]
fn default_fps_visible() -> bool {
    std::env::var(FPS_ENV_VAR)
        .ok()
        .and_then(|val| parse_fps_env_flag(&val))
        .unwrap_or(true)
}

#[cfg(debug_assertions)]
#[derive(Default)]
struct FpsMeter {
    timestamps: std::collections::VecDeque<std::time::Instant>,
    last_display_refresh: Option<std::time::Instant>,
    displayed_fps: Option<u32>,
    displayed_frame_ms: Option<f32>,
}

#[cfg(debug_assertions)]
impl FpsMeter {
    fn reset(&mut self) {
        self.timestamps.clear();
        self.last_display_refresh = None;
        self.displayed_fps = None;
        self.displayed_frame_ms = None;
    }

    fn record_frame(&mut self, now: std::time::Instant) {
        while let Some(&front) = self.timestamps.front() {
            if now.saturating_duration_since(front) > FPS_SAMPLE_WINDOW {
                self.timestamps.pop_front();
            } else {
                break;
            }
        }
        self.timestamps.push_back(now);

        let should_refresh = self
            .last_display_refresh
            .is_none_or(|last| now.saturating_duration_since(last) >= FPS_DISPLAY_REFRESH)
            || self.displayed_fps.is_none();

        if should_refresh && self.timestamps.len() >= 2 {
            let span = now.saturating_duration_since(*self.timestamps.front().unwrap());
            if !span.is_zero() {
                let intervals = (self.timestamps.len() - 1) as f64;
                let secs = span.as_secs_f64();
                self.displayed_fps = Some((intervals / secs).round() as u32);
                self.displayed_frame_ms = Some((secs * 1000.0 / intervals) as f32);
                self.last_display_refresh = Some(now);
            }
        }
    }

    fn format_label(&self) -> String {
        match (self.displayed_fps, self.displayed_frame_ms) {
            (Some(fps), Some(ms)) => format!("{fps:>3} FPS {ms:>4.1}ms"),
            _ => " -- FPS".to_string(),
        }
    }
}

impl StatusBar {
    pub const HEIGHT: Pixels = ControlHeight::XL;

    pub fn new(meta: Option<EditorStatusMeta>, active_sidebar: Option<SidebarView>) -> Self {
        Self {
            meta,
            active_sidebar,
            outline_visible: false,
            #[cfg(debug_assertions)]
            fps_visible: default_fps_visible(),
            #[cfg(debug_assertions)]
            fps_frame_scheduled: false,
            #[cfg(debug_assertions)]
            fps_meter: FpsMeter::default(),
        }
    }

    #[cfg(debug_assertions)]
    pub fn fps_visible(&self) -> bool {
        self.fps_visible
    }

    #[cfg(debug_assertions)]
    pub fn set_fps_visible(&mut self, visible: bool, cx: &mut Context<Self>) {
        if self.fps_visible != visible {
            self.fps_visible = visible;
            self.fps_frame_scheduled = false;
            self.fps_meter.reset();
            cx.notify();
        }
    }

    #[cfg(debug_assertions)]
    pub fn toggle_fps_visible(&mut self, cx: &mut Context<Self>) {
        self.set_fps_visible(!self.fps_visible, cx);
    }

    pub fn active_sidebar(&self) -> Option<SidebarView> {
        self.active_sidebar
    }

    pub fn outline_visible(&self) -> bool {
        self.outline_visible
    }

    pub fn set_outline_visible(&mut self, visible: bool, cx: &mut Context<Self>) {
        if self.outline_visible != visible {
            self.outline_visible = visible;
            cx.notify();
        }
    }

    pub fn meta(&self) -> Option<&EditorStatusMeta> {
        self.meta.as_ref()
    }

    pub fn set_active_sidebar(&mut self, active: Option<SidebarView>, cx: &mut Context<Self>) {
        if self.active_sidebar != active {
            self.active_sidebar = active;
            cx.notify();
        }
    }

    pub fn set_meta(&mut self, meta: Option<EditorStatusMeta>, cx: &mut Context<Self>) {
        if self.meta != meta {
            self.meta = meta;
            cx.notify();
        }
    }

    pub fn set_dirty(&mut self, is_dirty: bool, cx: &mut Context<Self>) {
        if let Some(meta) = self.meta.as_mut()
            && meta.is_dirty != is_dirty
        {
            meta.is_dirty = is_dirty;
            cx.notify();
        }
    }

    pub fn set_cursor_pos(&mut self, pos: crate::buffer::Position, cx: &mut Context<Self>) {
        if let Some(meta) = self.meta.as_mut()
            && meta.cursor_pos != pos
        {
            meta.cursor_pos = pos;
            cx.notify();
        }
    }
}

impl Render for StatusBar {
    fn render(
        &mut self,
        #[cfg_attr(not(debug_assertions), allow(unused_variables))] window: &mut Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        #[cfg(debug_assertions)]
        let fps_label = if self.fps_visible {
            self.fps_meter.record_frame(std::time::Instant::now());
            if !self.fps_frame_scheduled {
                self.fps_frame_scheduled = true;
                let status_bar = cx.entity().downgrade();
                window.on_next_frame(move |_, cx| {
                    let _ = status_bar.update(cx, |sb, cx| {
                        sb.fps_frame_scheduled = false;
                        if sb.fps_visible {
                            cx.notify();
                        }
                    });
                });
            }
            Some(self.fps_meter.format_label())
        } else {
            None
        };

        let theme = cx
            .try_global::<ThemeManager>()
            .map(|m| m.theme())
            .unwrap_or(&DEFAULT_THEME);
        let active_sidebar = self.active_sidebar;
        let outline_visible = self.outline_visible;

        let right_group = div()
            .flex()
            .flex_row()
            .items_center()
            .gap(Spacing::LG)
            .when_some(self.meta.as_ref(), |this, meta| {
                let is_dirty = meta.is_dirty;
                this.child(
                    div()
                        .flex()
                        .flex_row()
                        .items_center()
                        .gap(Spacing::XS)
                        .child(if is_dirty {
                            div()
                                .w(Spacing::SMD)
                                .h(Spacing::SMD)
                                .rounded_full()
                                .bg(theme.inline_code_fg)
                                .into_any_element()
                        } else {
                            Icon::new(IconName::Check)
                                .size(IconSize::Indicator)
                                .color(theme.text_accent)
                                .into_any_element()
                        })
                        .child(if meta.is_loading {
                            "Loading..."
                        } else if is_dirty {
                            "Unsaved"
                        } else {
                            "Saved"
                        }),
                )
                .when_some(meta.load_error.as_ref(), |this, error| {
                    this.child(div().child(error.clone()))
                })
                .when(meta.external_change != ExternalChangeState::None, |this| {
                    let message = match meta.external_change {
                        ExternalChangeState::Deleted => "File deleted",
                        ExternalChangeState::Pending => "Changed on disk",
                        ExternalChangeState::None => "",
                    };
                    this.child(div().child(message))
                        .when(
                            meta.external_change == ExternalChangeState::Pending,
                            |this| {
                                this.child(external_change_button(
                                    "reload_external_file",
                                    "Reload",
                                    crate::app::actions::ReloadFile.boxed_clone(),
                                    theme,
                                ))
                            },
                        )
                        .child(external_change_button(
                            "keep_local_external_file",
                            "Keep Local",
                            crate::app::actions::KeepLocalChanges.boxed_clone(),
                            theme,
                        ))
                })
            });

        #[cfg(debug_assertions)]
        let right_group = right_group.when_some(fps_label, |this, label| {
            this.child(
                div()
                    .id("status_bar_fps")
                    .font_family(crate::platform::platform_monospace_font())
                    .text_color(theme.text_muted)
                    .child(label),
            )
        });

        let right_group = right_group
            .child(
                div()
                    .flex()
                    .flex_row()
                    .items_center()
                    .gap(Spacing::XS)
                    .child(Icon::new(IconName::Palette).size(IconSize::XSmall))
                    .child(theme.name.clone()),
            )
            .child(render_sidebar_switch(
                "outline_panel_btn",
                IconName::PanelRight,
                outline_visible,
                Box::new(ToggleOutlineSidebar),
                theme,
            ));

        div()
            .w_full()
            .h(Self::HEIGHT)
            .flex_none()
            .bg(theme.bg_app)
            .px(px(6.0))
            .flex()
            .flex_row()
            .items_center()
            .justify_between()
            .text_size(FontSize::SM)
            .text_color(theme.text_muted)
            .child(
                div()
                    .flex()
                    .flex_row()
                    .items_center()
                    .gap(Spacing::MD)
                    .child(
                        div()
                            .flex()
                            .flex_row()
                            .items_center()
                            .gap(Spacing::XS)
                            .child(render_sidebar_switch(
                                "sidebar_files_btn",
                                IconName::FileText,
                                active_sidebar == Some(SidebarView::Files),
                                Box::new(ToggleSidebar),
                                theme,
                            ))
                            .child(render_sidebar_switch(
                                "sidebar_search_btn",
                                IconName::Search,
                                active_sidebar == Some(SidebarView::Search),
                                Box::new(ToggleSearchSidebar),
                                theme,
                            )),
                    )
                    .when_some(self.meta.as_ref(), |this, meta| {
                        this.child(format!(
                            "Ln {}, Col {}",
                            meta.cursor_pos.line + 1,
                            meta.cursor_pos.col + 1
                        ))
                        .child(format!("{} words", meta.word_count))
                        .child(format!("{} chars", meta.char_count))
                    }),
            )
            .child(right_group)
    }
}

fn external_change_button(
    id: &'static str,
    label: &'static str,
    action: Box<dyn Action>,
    theme: &Theme,
) -> impl IntoElement {
    div()
        .id(id)
        .px(Spacing::XS)
        .cursor_pointer()
        .text_color(theme.text_accent)
        .hover(|style| style.text_color(theme.text_primary))
        .on_click(move |_, window, cx| window.dispatch_action(action.boxed_clone(), cx))
        .child(label)
}

/// One panel toggle; highlighted while its panel is on screen.
fn render_sidebar_switch(
    id: &'static str,
    icon: IconName,
    is_active: bool,
    action: Box<dyn Action>,
    theme: &Theme,
) -> impl IntoElement {
    div()
        .id(id)
        .size(ControlHeight::MD)
        .flex()
        .items_center()
        .justify_center()
        .rounded(Radius::LG)
        .cursor_pointer()
        .when(is_active, |this| this.bg(theme.btn_active))
        .hover(|s| s.bg(theme.btn_hover))
        .active(|s| s.bg(theme.btn_active))
        .on_click(move |_, window, cx| window.dispatch_action(action.boxed_clone(), cx))
        .child(Icon::new(icon).size(IconSize::Small).color(if is_active {
            theme.text_accent
        } else {
            theme.text_muted
        }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::TitleBar;
    // Load-bearing: the parent module's glob also brings in `gpui::test`, so this explicit
    // import shadows it and keeps bare `#[test]` resolving to the built-in test attribute.
    use core::prelude::v1::test;

    #[test]
    fn test_status_bar_height_matches_title_bar() {
        // Design invariant: the status bar must align with the title bar.
        assert_eq!(StatusBar::HEIGHT, TitleBar::HEIGHT);
    }

    #[test]
    fn test_status_bar_initial_state() {
        let bar = StatusBar::new(None, Some(SidebarView::Files));
        assert_eq!(bar.active_sidebar(), Some(SidebarView::Files));
        assert!(bar.meta().is_none());

        let bar_hidden = StatusBar::new(None, None);
        assert_eq!(bar_hidden.active_sidebar(), None);
    }

    #[gpui::test]
    fn test_status_bar_state_updates(cx: &mut gpui::TestAppContext) {
        let status_bar = cx.new(|_cx| StatusBar::new(None, Some(SidebarView::Files)));

        // Test set_active_sidebar
        status_bar.update(cx, |sb, cx| {
            sb.set_active_sidebar(Some(SidebarView::Search), cx);
            assert_eq!(sb.active_sidebar(), Some(SidebarView::Search));
            sb.set_active_sidebar(None, cx);
            assert_eq!(sb.active_sidebar(), None);
        });

        // Test set_meta
        let meta = EditorStatusMeta {
            is_loading: false,
            load_error: None,
            is_dirty: false,
            external_change: ExternalChangeState::None,
            cursor_pos: crate::buffer::Position {
                line: crate::buffer::SourceLine::new(10),
                col: 5,
            },
            word_count: 100,
            char_count: 500,
            render_mode: crate::editor::RenderMode::Source,
        };
        status_bar.update(cx, |sb, cx| {
            sb.set_meta(Some(meta), cx);
            assert_eq!(sb.meta().map(|m| m.word_count), Some(100));
        });

        // Test set_dirty
        status_bar.update(cx, |sb, cx| {
            sb.set_dirty(true, cx);
            assert_eq!(sb.meta().map(|m| m.is_dirty), Some(true));
        });

        // Test set_cursor_pos
        status_bar.update(cx, |sb, cx| {
            sb.set_cursor_pos(
                crate::buffer::Position {
                    line: crate::buffer::SourceLine::new(20),
                    col: 1,
                },
                cx,
            );
            assert_eq!(
                sb.meta().map(|m| m.cursor_pos.line),
                Some(crate::buffer::SourceLine::new(20))
            );
        });
    }

    #[cfg(debug_assertions)]
    #[test]
    fn test_parse_fps_env_flag() {
        assert_eq!(parse_fps_env_flag("1"), Some(true));
        assert_eq!(parse_fps_env_flag(" true "), Some(true));
        assert_eq!(parse_fps_env_flag("ON"), Some(true));
        assert_eq!(parse_fps_env_flag("0"), Some(false));
        assert_eq!(parse_fps_env_flag("false"), Some(false));
        assert_eq!(parse_fps_env_flag("off"), Some(false));
        assert_eq!(parse_fps_env_flag("invalid"), None);
    }

    #[cfg(debug_assertions)]
    #[test]
    fn test_fps_meter_rolling_window_and_reset() {
        use std::time::{Duration, Instant};

        let mut meter = FpsMeter::default();
        assert_eq!(meter.format_label(), " -- FPS");

        let t0 = Instant::now();
        // Simulate 60Hz frames over 1.2s (16.6667ms per frame).
        for i in 0..=72 {
            let now = t0 + Duration::from_secs_f64(i as f64 / 60.0);
            meter.record_frame(now);
        }
        assert_eq!(meter.displayed_fps, Some(60));
        let ms = meter.displayed_frame_ms.unwrap();
        assert!((ms - 16.6667).abs() < 0.2, "unexpected frame ms: {ms}");
        assert_eq!(meter.format_label(), " 60 FPS · 16.7ms");

        meter.reset();
        assert_eq!(meter.format_label(), " -- FPS");
    }

    #[cfg(debug_assertions)]
    #[gpui::test]
    fn test_status_bar_toggle_fps_visible(cx: &mut gpui::TestAppContext) {
        let status_bar = cx.new(|_cx| StatusBar::new(None, None));
        status_bar.update(cx, |sb, cx| {
            let initial = sb.fps_visible();
            sb.toggle_fps_visible(cx);
            assert_eq!(sb.fps_visible(), !initial);
            sb.set_fps_visible(true, cx);
            assert!(sb.fps_visible());
            sb.set_fps_visible(false, cx);
            assert!(!sb.fps_visible());
        });
    }
}
