use gpui::*;
use std::rc::Rc;

use crate::app::actions::display_keystroke_for;
use crate::i18n::{ActiveLocale, TextKey};
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
        (cx.t(TextKey::TabCmMakeReadOnly).to_string(), true)
    } else if state.is_read_only {
        (cx.t(TextKey::TabCmMakeWritable).to_string(), false)
    } else {
        (cx.t(TextKey::TabCmMakeReadOnly).to_string(), false)
    };

    let reveal_label = if cfg!(target_os = "macos") {
        cx.t(TextKey::TabCmRevealFinder).to_string()
    } else {
        cx.t(TextKey::TabCmRevealExplorer).to_string()
    };

    let pin_label = if state.is_pinned {
        cx.t(TextKey::TabCmUnpin).to_string()
    } else {
        cx.t(TextKey::TabCmPin).to_string()
    };

    ContextMenuBuilder::new()
        .action_item(
            "tab_cm_close",
            cx.t(TextKey::TabCmClose).to_string(),
            display_keystroke_for("CloseTab", cx),
            TabContextMenuAction::Close,
            false,
            &on_action,
        )
        .action_item(
            "tab_cm_close_others",
            cx.t(TextKey::TabCmCloseOthers).to_string(),
            String::new(),
            TabContextMenuAction::CloseOthers,
            state.tab_count <= 1,
            &on_action,
        )
        .separator(border_subtle)
        .action_item(
            "tab_cm_close_left",
            cx.t(TextKey::TabCmCloseLeft).to_string(),
            String::new(),
            TabContextMenuAction::CloseLeft,
            state.tab_idx == 0,
            &on_action,
        )
        .action_item(
            "tab_cm_close_right",
            cx.t(TextKey::TabCmCloseRight).to_string(),
            String::new(),
            TabContextMenuAction::CloseRight,
            state.tab_idx >= state.tab_count.saturating_sub(1),
            &on_action,
        )
        .separator(border_subtle)
        .action_item(
            "tab_cm_close_clean",
            cx.t(TextKey::TabCmCloseClean).to_string(),
            String::new(),
            TabContextMenuAction::CloseClean,
            !state.has_clean_tabs,
            &on_action,
        )
        .action_item(
            "tab_cm_close_all",
            cx.t(TextKey::TabCmCloseAll).to_string(),
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
            cx.t(TextKey::TabCmCopyPath).to_string(),
            String::new(),
            TabContextMenuAction::CopyPath,
            !state.has_file_path,
            &on_action,
        )
        .action_item(
            "tab_cm_copy_relative_path",
            cx.t(TextKey::TabCmCopyRelativePath).to_string(),
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
            cx.t(TextKey::TabCmRevealInPanel).to_string(),
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
