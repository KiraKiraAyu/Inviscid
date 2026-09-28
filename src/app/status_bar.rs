use crate::editor::EditorStatusMeta;
use crate::theme::{DEFAULT_THEME, ThemeManager};
use crate::ui::tokens::{ControlHeight, FontSize, Radius, Spacing};
use crate::ui::{Icon, IconName, IconSize};
use gpui::prelude::*;
use gpui::*;

pub struct StatusBar {
    meta: Option<EditorStatusMeta>,
    sidebar_visible: bool,
}

impl StatusBar {
    pub const HEIGHT: Pixels = ControlHeight::LG;

    pub fn new(meta: Option<EditorStatusMeta>, sidebar_visible: bool) -> Self {
        Self {
            meta,
            sidebar_visible,
        }
    }

    pub fn sidebar_visible(&self) -> bool {
        self.sidebar_visible
    }

    pub fn meta(&self) -> Option<&EditorStatusMeta> {
        self.meta.as_ref()
    }

    pub fn set_sidebar_visible(&mut self, visible: bool, cx: &mut Context<Self>) {
        if self.sidebar_visible != visible {
            self.sidebar_visible = visible;
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
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx
            .try_global::<ThemeManager>()
            .map(|m| m.theme())
            .unwrap_or(&DEFAULT_THEME);
        let sidebar_visible = self.sidebar_visible;

        div()
            .w_full()
            .h(Self::HEIGHT)
            .flex_none()
            .bg(theme.bg_statusbar)
            .border_t_1()
            .border_color(theme.border)
            .pl(Spacing::SM)
            .pr(Spacing::LG)
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
                    // Sidebar Toggle Button at bottom-left corner
                    .child(
                        div()
                            .id("sidebar_toggle_btn")
                            .h(ControlHeight::SM)
                            .px(Spacing::SMD)
                            .flex()
                            .items_center()
                            .justify_center()
                            .rounded(Radius::SM)
                            .cursor_pointer()
                            .hover(|s| s.bg(theme.btn_hover))
                            .active(|s| s.bg(theme.btn_active))
                            .on_click(|_, window, cx| {
                                window.dispatch_action(
                                    crate::app::actions::ToggleSidebar.boxed_clone(),
                                    cx,
                                );
                            })
                            .child(Icon::new(IconName::PanelLeft).size(IconSize::Small).color(
                                if sidebar_visible {
                                    theme.text_accent
                                } else {
                                    theme.text_muted
                                },
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
            .child(
                div()
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
                                .child(if is_dirty { "Unsaved" } else { "Saved" }),
                        )
                    })
                    .child(
                        div()
                            .flex()
                            .flex_row()
                            .items_center()
                            .gap(Spacing::XS)
                            .child(Icon::new(IconName::Palette).size(IconSize::XSmall))
                            .child(theme.name.clone()),
                    ),
            )
    }
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
        let bar = StatusBar::new(None, true);
        assert!(bar.sidebar_visible());
        assert!(bar.meta().is_none());

        let bar_hidden = StatusBar::new(None, false);
        assert!(!bar_hidden.sidebar_visible());
    }

    #[gpui::test]
    fn test_status_bar_state_updates(cx: &mut gpui::TestAppContext) {
        let status_bar = cx.new(|_cx| StatusBar::new(None, true));

        // Test set_sidebar_visible
        status_bar.update(cx, |sb, cx| {
            sb.set_sidebar_visible(false, cx);
            assert!(!sb.sidebar_visible());
        });

        // Test set_meta
        let meta = EditorStatusMeta {
            is_dirty: false,
            cursor_pos: crate::buffer::Position { line: 10, col: 5 },
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
            sb.set_cursor_pos(crate::buffer::Position { line: 20, col: 1 }, cx);
            assert_eq!(sb.meta().map(|m| m.cursor_pos.line), Some(20));
        });
    }
}
