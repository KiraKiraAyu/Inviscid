use crate::app::SidebarView;
use crate::app::actions::{ToggleSearchSidebar, ToggleSidebar};
use crate::editor::EditorStatusMeta;
use crate::theme::{DEFAULT_THEME, Theme, ThemeManager};
use crate::ui::tokens::{ControlHeight, FontSize, Radius, Spacing};
use crate::ui::{Icon, IconName, IconSize};
use gpui::prelude::*;
use gpui::*;

pub struct StatusBar {
    meta: Option<EditorStatusMeta>,
    /// The sidebar panel on screen; `None` while the sidebar is hidden.
    active_sidebar: Option<SidebarView>,
}

impl StatusBar {
    pub const HEIGHT: Pixels = ControlHeight::LG;

    pub fn new(meta: Option<EditorStatusMeta>, active_sidebar: Option<SidebarView>) -> Self {
        Self {
            meta,
            active_sidebar,
        }
    }

    pub fn active_sidebar(&self) -> Option<SidebarView> {
        self.active_sidebar
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
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx
            .try_global::<ThemeManager>()
            .map(|m| m.theme())
            .unwrap_or(&DEFAULT_THEME);
        let active_sidebar = self.active_sidebar;

        div()
            .w_full()
            .h(Self::HEIGHT)
            .flex_none()
            .bg(theme.bg_statusbar)
            .border_t_1()
            .border_color(theme.border)
            .pl(Spacing::XS)
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
                            meta.cursor_pos.line.index() + 1,
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

/// One button of the sidebar switcher; highlighted while its panel is on screen.
fn render_sidebar_switch(
    id: &'static str,
    icon: IconName,
    is_active: bool,
    action: Box<dyn Action>,
    theme: &Theme,
) -> impl IntoElement {
    div()
        .id(id)
        .size(ControlHeight::SM)
        .flex()
        .items_center()
        .justify_center()
        .rounded(Radius::SM)
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
            is_dirty: false,
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
            assert_eq!(sb.meta().map(|m| m.cursor_pos.line.index()), Some(20));
        });
    }
}
