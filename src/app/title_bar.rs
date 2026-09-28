use crate::app::actions::{OpenMenu, ToggleMenu};
use crate::app::menu::{ActiveMenu, MenuTrigger, TextMenu, ViewMenuState};
use crate::app::workspace::Workspace;
use crate::theme::ThemeManager;
use crate::ui::tokens::{ControlHeight, FontSize, Radius, Spacing};
use crate::ui::{Icon, IconName, IconSize, WindowTitleBar};
use gpui::prelude::FluentBuilder;
use gpui::*;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TitleBarEvent {
    MenuChanged(Option<ActiveMenu>),
}

impl EventEmitter<TitleBarEvent> for TitleBar {}

pub struct TitleBar {
    active_menu: Option<ActiveMenu>,
    workspace: Entity<Workspace>,
}

impl TitleBar {
    pub const HEIGHT: Pixels = WindowTitleBar::HEIGHT;

    pub fn new(workspace: Entity<Workspace>) -> Self {
        Self {
            active_menu: None,
            workspace,
        }
    }

    pub fn set_active_menu(&mut self, menu: Option<ActiveMenu>, cx: &mut Context<Self>) {
        if self.active_menu != menu {
            if let Some(prev) = self.active_menu {
                prev.on_close(cx);
            }
            self.active_menu = menu;
            cx.emit(TitleBarEvent::MenuChanged(menu));
            cx.notify();
        }
    }

    pub fn close_menu(&mut self, cx: &mut Context<Self>) {
        self.set_active_menu(None, cx);
    }

    pub fn notify_workspace_updated(&mut self, cx: &mut Context<Self>) {
        cx.notify();
    }
}

impl Render for TitleBar {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.global::<ThemeManager>().theme().clone();
        let has_workspace = self.workspace.read(cx).has_workspace(cx);
        let workspace_name = self.workspace.read(cx).workspace_name(cx);
        let is_menu_expanded = self.active_menu.is_some_and(|m| m.is_text_menu());
        let is_workspace_menu_active = self.active_menu == Some(ActiveMenu::Workspace);

        let view_menu_state = ViewMenuState {
            cur_render_mode: self
                .workspace
                .read(cx)
                .active_editor()
                .map(|ed| ed.read(cx).render_mode())
                .unwrap_or(crate::editor::RenderMode::LivePreview),
            is_sidebar_visible: self.workspace.read(cx).sidebar_visible(),
        };

        // Edit menu actions are dispatched at the editor itself; nothing to target without a document.
        let edit_target = self
            .workspace
            .read(cx)
            .active_editor()
            .map(|editor| editor.read(cx).focus_handle(cx));

        let left_group = div()
            .flex()
            .flex_row()
            .items_center()
            .gap(Spacing::SMD)
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .child(if is_menu_expanded {
                div()
                    .flex()
                    .flex_row()
                    .items_center()
                    .gap(Spacing::XXS)
                    .child(MenuTrigger::new(
                        TextMenu::Inviscid,
                        "Inviscid",
                        self.active_menu,
                    ))
                    .child(MenuTrigger::new(TextMenu::File, "File", self.active_menu))
                    .child(
                        MenuTrigger::new(TextMenu::Edit, "Edit", self.active_menu)
                            .edit_target(edit_target),
                    )
                    .child(
                        MenuTrigger::new(TextMenu::View, "View", self.active_menu)
                            .view_state(view_menu_state),
                    )
                    .child(MenuTrigger::new(TextMenu::Theme, "Theme", self.active_menu))
                    .into_any_element()
            } else {
                div()
                    .flex()
                    .flex_row()
                    .items_center()
                    .gap(Spacing::XXS)
                    .child(
                        div()
                            .id("app_menu_hamburger_btn")
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
                                    ToggleMenu(ActiveMenu::Inviscid).boxed_clone(),
                                    cx,
                                );
                            })
                            .child(
                                Icon::new(IconName::Menu)
                                    .size(IconSize::Small)
                                    .color(theme.text_secondary),
                            ),
                    )
                    .child(
                        div()
                            .relative()
                            .id("workspace_selector_btn")
                            .h(ControlHeight::SM)
                            .px(Spacing::SM)
                            .rounded(Radius::SM)
                            .cursor_pointer()
                            .bg(if is_workspace_menu_active {
                                theme.btn_hover
                            } else {
                                transparent_black()
                            })
                            .hover(|s| s.bg(theme.btn_hover))
                            .active(|s| s.bg(theme.btn_active))
                            .on_click(move |_, window, cx| {
                                if is_workspace_menu_active {
                                    window.dispatch_action(
                                        crate::app::actions::CloseMenu.boxed_clone(),
                                        cx,
                                    );
                                } else {
                                    window.dispatch_action(
                                        OpenMenu(ActiveMenu::Workspace).boxed_clone(),
                                        cx,
                                    );
                                }
                            })
                            .flex()
                            .flex_row()
                            .items_center()
                            .gap(Spacing::XS)
                            .when(has_workspace, |this| {
                                this.child(
                                    Icon::new(IconName::Folder)
                                        .size(IconSize::Small)
                                        .color(theme.text_secondary),
                                )
                            })
                            .child(
                                div()
                                    .text_size(FontSize::SM)
                                    .font_weight(FontWeight::MEDIUM)
                                    .text_color(theme.text_primary)
                                    .child(workspace_name.clone()),
                            )
                            .child(
                                Icon::new(IconName::ChevronDown)
                                    .size(IconSize::Indicator)
                                    .color(theme.text_secondary),
                            )
                            .when(is_workspace_menu_active, |this| {
                                let titlebar = cx.entity().clone();
                                let workspace = self.workspace.clone();
                                let ws_name = workspace_name.clone();
                                this.child(deferred(crate::app::menu::render_workspace_dropdown(
                                    &ws_name,
                                    move |ws_state, window, cx| {
                                        titlebar.update(cx, |tb, cx| tb.close_menu(cx));
                                        workspace.update(cx, |ws, cx| {
                                            ws.restore_from_state(ws_state, cx);
                                            ws.focus_active_editor(window, cx);
                                        });
                                    },
                                    cx,
                                )))
                            }),
                    )
                    .into_any_element()
            });

        WindowTitleBar::new().left(left_group)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use core::prelude::v1::test;

    #[test]
    fn test_active_menu_text_menu_classification() {
        assert!(ActiveMenu::File.is_text_menu());
        assert!(ActiveMenu::Edit.is_text_menu());
        assert!(!ActiveMenu::Workspace.is_text_menu());
    }
}
