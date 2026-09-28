use std::rc::Rc;

use crate::app::actions::{OpenMenu, display_keystroke_for};
use crate::config::{AppConfig, WorkspaceState};
use crate::editor::RenderMode;
use crate::editor::actions::{
    CopyAction, CutAction, PasteAction, RedoAction, SelectAll, UndoAction,
};
use crate::theme::ThemeManager;
use crate::ui::{ControlHeight, FontSize, Icon, IconName, IconSize, MenuItem, Radius, Spacing};
use gpui::prelude::FluentBuilder;
use gpui::*;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ActiveMenu {
    Inviscid,
    File,
    Edit,
    View,
    Theme,
    Workspace,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TextMenu {
    Inviscid,
    File,
    Edit,
    View,
    Theme,
}

impl From<TextMenu> for ActiveMenu {
    fn from(m: TextMenu) -> Self {
        match m {
            TextMenu::Inviscid => ActiveMenu::Inviscid,
            TextMenu::File => ActiveMenu::File,
            TextMenu::Edit => ActiveMenu::Edit,
            TextMenu::View => ActiveMenu::View,
            TextMenu::Theme => ActiveMenu::Theme,
        }
    }
}

impl ActiveMenu {
    /// Returns true if this menu belongs to the horizontal text menu bar
    pub fn is_text_menu(self) -> bool {
        matches!(
            self,
            ActiveMenu::Inviscid
                | ActiveMenu::File
                | ActiveMenu::Edit
                | ActiveMenu::View
                | ActiveMenu::Theme
        )
    }

    /// Hook executed when the menu is closed or transitioned away from
    pub fn on_close(self, cx: &mut App) {
        if self == ActiveMenu::Theme {
            crate::app::commands::cancel_theme_preview_global(cx);
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ViewMenuState {
    pub cur_render_mode: RenderMode,
    pub is_sidebar_visible: bool,
}

#[derive(IntoElement)]
pub struct MenuTrigger {
    category: TextMenu,
    label: &'static str,
    active_menu: Option<ActiveMenu>,
    view_state: Option<ViewMenuState>,
    edit_target: Option<FocusHandle>,
}

impl MenuTrigger {
    pub fn new(category: TextMenu, label: &'static str, active_menu: Option<ActiveMenu>) -> Self {
        Self {
            category,
            label,
            active_menu,
            view_state: None,
            edit_target: None,
        }
    }

    pub fn view_state(mut self, state: ViewMenuState) -> Self {
        self.view_state = Some(state);
        self
    }

    /// The editor the Edit menu should dispatch its actions to, if a document is open.
    pub fn edit_target(mut self, focus: Option<FocusHandle>) -> Self {
        self.edit_target = focus;
        self
    }
}

impl RenderOnce for MenuTrigger {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = cx.global::<ThemeManager>().theme();
        let target_menu: ActiveMenu = self.category.into();
        let is_active = self.active_menu == Some(target_menu);
        let category = self.category;
        let active_menu = self.active_menu;
        let view_state = self.view_state;
        let edit_target = self.edit_target;

        div()
            .relative()
            .id(ElementId::NamedInteger(
                "menu_trigger".into(),
                category as u64,
            ))
            .h(ControlHeight::SM)
            .px(px(7.0))
            .flex()
            .flex_row()
            .items_center()
            .justify_center()
            .rounded(Radius::SM)
            .bg(if is_active {
                theme.btn_hover
            } else {
                transparent_black()
            })
            .hover(|s| s.bg(theme.btn_hover))
            .active(|s| s.bg(theme.btn_active))
            .cursor_pointer()
            .on_click(move |_, window, cx| {
                if is_active {
                    window.dispatch_action(crate::app::actions::CloseMenu.boxed_clone(), cx);
                } else {
                    window.dispatch_action(OpenMenu(target_menu).boxed_clone(), cx);
                }
            })
            .on_mouse_move(move |_, window, cx| {
                if active_menu.is_some_and(|m| m.is_text_menu() && m != target_menu) {
                    window.dispatch_action(OpenMenu(target_menu).boxed_clone(), cx);
                }
            })
            .child(
                div()
                    .text_size(FontSize::SM)
                    .text_color(if is_active {
                        theme.text_primary
                    } else {
                        theme.text_secondary
                    })
                    .child(self.label),
            )
            .when(is_active, |this| {
                let dropdown_element = match category {
                    TextMenu::Inviscid => render_inviscid_dropdown(cx).into_any_element(),
                    TextMenu::File => render_file_dropdown(cx).into_any_element(),
                    TextMenu::Edit => render_edit_dropdown(edit_target, cx).into_any_element(),
                    TextMenu::View => render_view_dropdown(view_state, cx).into_any_element(),
                    TextMenu::Theme => render_theme_dropdown(cx).into_any_element(),
                };
                this.child(deferred(dropdown_element))
            })
    }
}

fn render_menu_item(
    label: &'static str,
    shortcut: impl Into<SharedString>,
    on_click: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
) -> impl IntoElement {
    MenuItem::from_name(label)
        .shortcut(shortcut)
        .on_click(on_click)
}

fn render_menu_selectable_item(
    label: &'static str,
    shortcut: impl Into<SharedString>,
    is_selected: bool,
    on_click: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
) -> impl IntoElement {
    MenuItem::from_name(label)
        .shortcut(shortcut)
        .selected(is_selected)
        .on_click(on_click)
}

fn render_theme_item(
    name: &str,
    is_selected: bool,
    on_hover: impl Fn(&bool, &mut Window, &mut App) + 'static,
    on_click: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
) -> impl IntoElement {
    MenuItem::from_name(name.to_string())
        .selected(is_selected)
        .on_hover(on_hover)
        .on_click(on_click)
}

fn render_menu_separator(cx: &mut App) -> impl IntoElement {
    let theme = cx.global::<ThemeManager>().theme();
    div().w_full().h(px(1.0)).my(Spacing::XS).bg(theme.border)
}

pub fn render_menu_dropdown(
    width: f32,
    children: Vec<AnyElement>,
    cx: &mut App,
) -> impl IntoElement {
    let theme = cx.global::<ThemeManager>().theme();
    div()
        .id("menu_dropdown_card")
        .occlude()
        .absolute()
        .top(px(30.0))
        .left_0()
        .w(px(width))
        .p(Spacing::XS)
        .bg(theme.bg_editor)
        .border_1()
        .border_color(theme.border)
        .rounded(Radius::MD)
        .shadow_md()
        .flex()
        .flex_col()
        .on_mouse_down_out(|_, window, cx| {
            window.dispatch_action(crate::app::actions::CloseMenu.boxed_clone(), cx);
        })
        .children(children)
}

fn render_inviscid_dropdown(cx: &mut App) -> impl IntoElement {
    render_menu_dropdown(
        180.0,
        vec![
            render_menu_item("About Inviscid", "", |_, window, cx| {
                window.dispatch_action(crate::app::actions::OpenAbout.boxed_clone(), cx);
            })
            .into_any_element(),
            render_menu_separator(cx).into_any_element(),
            render_menu_item(
                "Settings",
                display_keystroke_for("OpenSettings", cx),
                |_, window, cx| {
                    window.dispatch_action(crate::app::actions::OpenSettings.boxed_clone(), cx);
                },
            )
            .into_any_element(),
        ],
        cx,
    )
}

fn render_file_dropdown(cx: &mut App) -> impl IntoElement {
    render_menu_dropdown(
        220.0,
        vec![
            render_menu_item(
                "New Tab",
                display_keystroke_for("NewTab", cx),
                |_, window, cx| {
                    window.dispatch_action(crate::app::actions::NewTab.boxed_clone(), cx);
                },
            )
            .into_any_element(),
            render_menu_item(
                "New Window",
                display_keystroke_for("NewWindow", cx),
                |_, window, cx| {
                    window.dispatch_action(crate::app::actions::NewWindow.boxed_clone(), cx);
                },
            )
            .into_any_element(),
            render_menu_item(
                "Open File...",
                display_keystroke_for("OpenFile", cx),
                |_, window, cx| {
                    window.dispatch_action(crate::app::actions::OpenFile.boxed_clone(), cx);
                },
            )
            .into_any_element(),
            render_menu_item(
                "Open Folder...",
                display_keystroke_for("OpenFolder", cx),
                |_, window, cx| {
                    window.dispatch_action(crate::app::actions::OpenFolder.boxed_clone(), cx);
                },
            )
            .into_any_element(),
            render_menu_item(
                "Save",
                display_keystroke_for("SaveFile", cx),
                |_, window, cx| {
                    window.dispatch_action(crate::app::actions::SaveFile.boxed_clone(), cx);
                },
            )
            .into_any_element(),
            render_menu_item(
                "Save As...",
                display_keystroke_for("SaveFileAs", cx),
                |_, window, cx| {
                    window.dispatch_action(crate::app::actions::SaveFileAs.boxed_clone(), cx);
                },
            )
            .into_any_element(),
            render_menu_separator(cx).into_any_element(),
            render_menu_item(
                "Close Tab",
                display_keystroke_for("CloseTab", cx),
                |_, window, cx| {
                    window.dispatch_action(crate::app::actions::CloseTab.boxed_clone(), cx);
                },
            )
            .into_any_element(),
            render_menu_item(
                "Close Workspace",
                display_keystroke_for("CloseWorkspace", cx),
                |_, window, cx| {
                    window.dispatch_action(crate::app::actions::CloseWorkspace.boxed_clone(), cx);
                },
            )
            .into_any_element(),
            render_menu_separator(cx).into_any_element(),
            render_menu_item(
                "Exit",
                display_keystroke_for("Quit", cx),
                |_, window, cx| {
                    window.dispatch_action(crate::app::actions::Quit.boxed_clone(), cx);
                },
            )
            .into_any_element(),
        ],
        cx,
    )
}

fn render_edit_dropdown(editor: Option<FocusHandle>, cx: &mut App) -> impl IntoElement {
    // Edit actions belong to the focused editor (see `editor::keymap`), so they are dispatched at
    // its handle instead of going through an app-level handler that guesses the active tab.
    let item =
        |label: &'static str, action_id: &'static str, action: Box<dyn Action>, cx: &mut App| {
            let editor = editor.clone();
            render_menu_item(
                label,
                display_keystroke_for(action_id, cx),
                move |_, window, cx| {
                    if let Some(editor) = &editor {
                        editor.dispatch_action(action.as_ref(), window, cx);
                    }
                },
            )
            .into_any_element()
        };

    render_menu_dropdown(
        180.0,
        vec![
            item("Undo", "UndoAction", UndoAction.boxed_clone(), cx),
            item("Redo", "RedoAction", RedoAction.boxed_clone(), cx),
            render_menu_separator(cx).into_any_element(),
            item("Cut", "CutAction", CutAction.boxed_clone(), cx),
            item("Copy", "CopyAction", CopyAction.boxed_clone(), cx),
            item("Paste", "PasteAction", PasteAction.boxed_clone(), cx),
            render_menu_separator(cx).into_any_element(),
            item("Select All", "SelectAll", SelectAll.boxed_clone(), cx),
        ],
        cx,
    )
}

fn render_view_dropdown(view_state: Option<ViewMenuState>, cx: &mut App) -> impl IntoElement {
    let cur_render_mode = view_state
        .map(|s| s.cur_render_mode)
        .unwrap_or(RenderMode::LivePreview);
    let is_sidebar_visible = view_state.map(|s| s.is_sidebar_visible).unwrap_or(true);

    render_menu_dropdown(
        200.0,
        vec![
            render_menu_selectable_item(
                "Sidebar",
                display_keystroke_for("ToggleSidebar", cx),
                is_sidebar_visible,
                |_, window, cx| {
                    window.dispatch_action(crate::app::actions::ToggleSidebar.boxed_clone(), cx);
                },
            )
            .into_any_element(),
            render_menu_separator(cx).into_any_element(),
            render_menu_selectable_item(
                "Live Preview",
                "",
                cur_render_mode == RenderMode::LivePreview,
                |_, window, cx| {
                    window.dispatch_action(
                        crate::app::actions::SetRenderMode(RenderMode::LivePreview).boxed_clone(),
                        cx,
                    );
                },
            )
            .into_any_element(),
            render_menu_selectable_item(
                "Source Mode",
                "",
                cur_render_mode == RenderMode::Source,
                |_, window, cx| {
                    window.dispatch_action(
                        crate::app::actions::SetRenderMode(RenderMode::Source).boxed_clone(),
                        cx,
                    );
                },
            )
            .into_any_element(),
            render_menu_separator(cx).into_any_element(),
            render_menu_item(
                "Next Tab",
                display_keystroke_for("NextTab", cx),
                |_, window, cx| {
                    window.dispatch_action(crate::app::actions::NextTab.boxed_clone(), cx);
                },
            )
            .into_any_element(),
            render_menu_item(
                "Previous Tab",
                display_keystroke_for("PrevTab", cx),
                |_, window, cx| {
                    window.dispatch_action(crate::app::actions::PrevTab.boxed_clone(), cx);
                },
            )
            .into_any_element(),
        ],
        cx,
    )
}

fn render_theme_dropdown(cx: &mut App) -> impl IntoElement {
    let theme_names = cx.global::<ThemeManager>().list_themes().to_vec();
    let saved_theme_name = cx.global::<AppConfig>().theme.clone();

    let items = theme_names
        .iter()
        .map(|name| {
            let is_selected = name == &saved_theme_name;
            let name_for_hover = name.clone();
            let name_for_click = name.clone();
            render_theme_item(
                name,
                is_selected,
                move |is_hovered, _, cx| {
                    if *is_hovered {
                        crate::app::commands::preview_theme_global(&name_for_hover, cx);
                    }
                },
                move |_, window, cx| {
                    crate::app::commands::switch_theme_global(&name_for_click, cx);
                    window.dispatch_action(crate::app::actions::CloseMenu.boxed_clone(), cx);
                },
            )
            .into_any_element()
        })
        .collect();

    render_menu_dropdown(180.0, items, cx)
}

pub fn render_workspace_dropdown(
    current_ws_name: &str,
    on_select: impl Fn(&WorkspaceState, &mut Window, &mut App) + 'static,
    cx: &mut App,
) -> impl IntoElement {
    let on_select = Rc::new(on_select);
    let theme = cx.global::<ThemeManager>().theme();
    let config = cx.global::<AppConfig>();
    let recent_workspaces = config.session.recent_workspaces.clone();

    let mut items = Vec::new();

    items.push(
        div()
            .px(Spacing::SM)
            .py(Spacing::XS)
            .text_size(FontSize::CAPTION)
            .font_weight(FontWeight::BOLD)
            .text_color(theme.text_muted)
            .child("RECENT WORKSPACES")
            .into_any_element(),
    );

    let valid_recent: Vec<_> = recent_workspaces
        .into_iter()
        .filter(|ws_state| {
            if let Some(root) = &ws_state.root_dir {
                root.exists()
            } else if let Some(active) = &ws_state.active_file {
                active.exists()
            } else if let Some(first) = ws_state.open_files.first() {
                first.exists()
            } else {
                false
            }
        })
        .collect();

    if valid_recent.is_empty() {
        items.push(
            div()
                .px(Spacing::MDS)
                .py(Spacing::SMD)
                .text_size(FontSize::SM)
                .text_color(theme.text_muted)
                .child("No recent workspaces")
                .into_any_element(),
        );
    } else {
        for (i, ws_state) in valid_recent.iter().enumerate() {
            let ws_name = if let Some(root) = &ws_state.root_dir
                && let Some(name) = root.file_name().and_then(|n| n.to_str())
                && !name.is_empty()
            {
                name.to_string()
            } else {
                let name = crate::app::workspace::extract_workspace_name_from_paths([
                    ws_state.active_file.as_deref(),
                    ws_state.open_files.first().map(|p| p.as_path()),
                ]);
                if name == "Open Workspace" {
                    format!("Workspace #{}", i + 1)
                } else {
                    name
                }
            };

            let subtitle = ws_state
                .root_dir
                .as_ref()
                .or(ws_state.active_file.as_ref())
                .or(ws_state.open_files.first())
                .and_then(|p| p.to_str());

            let is_active = ws_name == current_ws_name;
            let ws_clone = ws_state.clone();
            let on_select = on_select.clone();

            items.push(
                render_workspace_item(
                    &format!("recent_ws_{}", i),
                    &ws_name,
                    subtitle,
                    is_active,
                    move |_, window, cx| {
                        on_select(&ws_clone, window, cx);
                    },
                    cx,
                )
                .into_any_element(),
            );
        }

        items.push(render_menu_separator(cx).into_any_element());
        items.push(
            render_menu_item("Clear Recent Workspaces", "", |_, window, cx| {
                window
                    .dispatch_action(crate::app::actions::ClearRecentWorkspaces.boxed_clone(), cx);
            })
            .into_any_element(),
        );
    }

    items.push(render_menu_separator(cx).into_any_element());

    items.push(
        render_menu_item(
            "Open Folder...",
            display_keystroke_for("OpenFolder", cx),
            |_, window, cx| {
                window.dispatch_action(crate::app::actions::OpenFolder.boxed_clone(), cx);
            },
        )
        .into_any_element(),
    );

    items.push(
        render_menu_item(
            "Open File...",
            display_keystroke_for("OpenFile", cx),
            |_, window, cx| {
                window.dispatch_action(crate::app::actions::OpenFile.boxed_clone(), cx);
            },
        )
        .into_any_element(),
    );

    render_menu_dropdown(260.0, items, cx)
}

fn render_workspace_item(
    id_str: &str,
    title: &str,
    subtitle: Option<&str>,
    is_active: bool,
    on_click: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    cx: &mut App,
) -> impl IntoElement {
    let theme = cx.global::<ThemeManager>().theme();
    let title_str = title.to_string();
    let subtitle_str = subtitle.map(|s| s.to_string());

    div()
        .id(ElementId::Name(id_str.to_string().into()))
        .w_full()
        .px(Spacing::MDS)
        .py(px(5.0))
        .flex()
        .flex_row()
        .items_center()
        .justify_between()
        .rounded(Radius::SM)
        .cursor_pointer()
        .hover(|s| s.bg(theme.btn_hover))
        .active(|s| s.bg(theme.btn_active))
        .on_click(on_click)
        .child(
            div()
                .flex()
                .flex_row()
                .items_center()
                .gap(Spacing::SM)
                .child(
                    Icon::new(IconName::Folder)
                        .size(IconSize::Small)
                        .color(if is_active {
                            theme.text_accent
                        } else {
                            theme.text_muted
                        }),
                )
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .child(
                            div()
                                .text_size(FontSize::SM)
                                .text_color(if is_active {
                                    theme.text_accent
                                } else {
                                    theme.text_primary
                                })
                                .font_weight(if is_active {
                                    FontWeight::SEMIBOLD
                                } else {
                                    FontWeight::NORMAL
                                })
                                .child(title_str),
                        )
                        .children(subtitle_str.map(|sub| {
                            div()
                                .text_size(FontSize::CAPTION)
                                .text_color(theme.text_muted)
                                .child(sub)
                        })),
                ),
        )
        .child(
            div()
                .text_color(theme.text_accent)
                .text_size(FontSize::SM)
                .child(if is_active { "✓" } else { "" }),
        )
}
