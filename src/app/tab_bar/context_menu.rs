use gpui::*;
use std::rc::Rc;

use crate::app::actions::display_keystroke_for;
use crate::theme::ThemeManager;
use crate::ui::ContextMenuBuilder;

#[derive(Clone, Debug, PartialEq)]
pub struct TabContextMenuState {
    pub tab_idx: usize,
    pub position: Point<Pixels>,
    pub is_pinned: bool,
    pub is_read_only: bool,
    pub has_file_path: bool,
    pub is_whitelisted: bool,
    pub tab_count: usize,
    pub has_clean_tabs: bool,
    pub has_workspace_root: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TabContextMenuAction {
    Close,
    CloseOthers,
    CloseLeft,
    CloseRight,
    CloseClean,
    CloseAll,
    ToggleReadOnly,
    CopyPath,
    CopyRelativePath,
    RevealInFileExplorer,
    TogglePin,
    RevealInProjectPanel,
}

pub fn render_tab_context_menu(
    state: &TabContextMenuState,
    safe_bounds: Bounds<Pixels>,
    sidebar_width: Pixels,
    on_action: impl Fn(TabContextMenuAction, &mut Window, &mut App) + 'static,
    on_close: impl Fn(&mut Window, &mut App) + 'static,
    cx: &mut App,
) -> impl IntoElement {
    let theme = cx.global::<ThemeManager>().theme();
    let on_action: Rc<dyn Fn(TabContextMenuAction, &mut Window, &mut App)> = Rc::new(on_action);
    let border_subtle = theme.border_subtle;

    let (ro_label, ro_disabled) = if !state.is_whitelisted {
        ("Make Tab Read-Only", true)
    } else if state.is_read_only {
        ("Make Tab Writable", false)
    } else {
        ("Make Tab Read-Only", false)
    };

    let reveal_label = if cfg!(target_os = "macos") {
        "Reveal in Finder"
    } else {
        "Reveal in File Explorer"
    };

    let pin_label = if state.is_pinned {
        "Unpin Tab"
    } else {
        "Pin Tab"
    };

    ContextMenuBuilder::new()
        .action_item(
            "tab_cm_close",
            "Close",
            display_keystroke_for("CloseTab", cx),
            TabContextMenuAction::Close,
            false,
            &on_action,
        )
        .action_item(
            "tab_cm_close_others",
            "Close Others",
            String::new(),
            TabContextMenuAction::CloseOthers,
            state.tab_count <= 1,
            &on_action,
        )
        .separator(border_subtle)
        .action_item(
            "tab_cm_close_left",
            "Close Left",
            String::new(),
            TabContextMenuAction::CloseLeft,
            state.tab_idx == 0,
            &on_action,
        )
        .action_item(
            "tab_cm_close_right",
            "Close Right",
            String::new(),
            TabContextMenuAction::CloseRight,
            state.tab_idx >= state.tab_count.saturating_sub(1),
            &on_action,
        )
        .separator(border_subtle)
        .action_item(
            "tab_cm_close_clean",
            "Close Clean",
            String::new(),
            TabContextMenuAction::CloseClean,
            !state.has_clean_tabs,
            &on_action,
        )
        .action_item(
            "tab_cm_close_all",
            "Close All",
            String::new(),
            TabContextMenuAction::CloseAll,
            state.tab_count == 0,
            &on_action,
        )
        .separator(border_subtle)
        .action_item(
            "tab_cm_read_only",
            ro_label,
            String::new(),
            TabContextMenuAction::ToggleReadOnly,
            ro_disabled,
            &on_action,
        )
        .separator(border_subtle)
        .action_item(
            "tab_cm_copy_path",
            "Copy Path",
            String::new(),
            TabContextMenuAction::CopyPath,
            !state.has_file_path,
            &on_action,
        )
        .action_item(
            "tab_cm_copy_relative_path",
            "Copy Relative Path",
            String::new(),
            TabContextMenuAction::CopyRelativePath,
            !state.has_file_path,
            &on_action,
        )
        .separator(border_subtle)
        .action_item(
            "tab_cm_reveal_explorer",
            reveal_label,
            String::new(),
            TabContextMenuAction::RevealInFileExplorer,
            !state.has_file_path,
            &on_action,
        )
        .separator(border_subtle)
        .action_item(
            "tab_cm_pin_tab",
            pin_label,
            String::new(),
            TabContextMenuAction::TogglePin,
            false,
            &on_action,
        )
        .action_item(
            "tab_cm_reveal_project_panel",
            "Reveal In Panel",
            String::new(),
            TabContextMenuAction::RevealInProjectPanel,
            !state.has_file_path || !state.has_workspace_root,
            &on_action,
        )
        .into_element(
            "tab_context_menu",
            state.position,
            safe_bounds,
            sidebar_width,
            theme,
            on_close,
        )
}
