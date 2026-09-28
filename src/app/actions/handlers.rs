use crate::app::actions::defs::*;
use crate::app::{InviscidWindow, commands};
use gpui::prelude::*;
use gpui::{Context, Div};

/// Binds every app-level action on the root element.
///
/// They all stay on the root because the keymap binds them with the app-wide key context: an
/// action registered deeper (on the `Workspace` or `TitleBar` element) would silently stop
/// working whenever focus sits outside that subtree. Edit actions are the exception. They
/// belong to the focused editor and are bound there (`editor::keymap`) and on `InlineInput`.
pub fn attach_app_actions(div: Div, cx: &mut Context<InviscidWindow>) -> Div {
    div.map(|div| attach_window_actions(div, cx))
        .map(|div| attach_workspace_actions(div, cx))
        .map(|div| attach_menu_actions(div, cx))
        .map(|div| attach_view_actions(div, cx))
}

fn attach_window_actions(div: Div, cx: &mut Context<InviscidWindow>) -> Div {
    div.on_action(cx.listener(|this, _: &NewWindow, _, cx| {
        this.defer_after_menu_close(cx, |_this, cx| commands::new_window(cx));
    }))
    .on_action(cx.listener(|this, _: &OpenAbout, _, cx| {
        this.defer_after_menu_close(cx, |_this, cx| commands::open_about_window(cx));
    }))
    .on_action(cx.listener(|this, _: &OpenSettings, _, cx| {
        this.defer_after_menu_close(cx, |_this, cx| commands::open_settings_window(cx));
    }))
    .on_action(cx.listener(|_, _: &Quit, _, cx| commands::quit(cx)))
}

fn attach_workspace_actions(div: Div, cx: &mut Context<InviscidWindow>) -> Div {
    div.on_action(cx.listener(|this, _: &NewTab, window, cx| {
        commands::new_tab(this, window, cx);
    }))
    .on_action(cx.listener(|this, _: &CloseTab, window, cx| {
        commands::close_active_tab(this, window, cx);
    }))
    .on_action(cx.listener(|this, _: &NextTab, window, cx| {
        commands::next_tab(this, window, cx);
    }))
    .on_action(cx.listener(|this, _: &PrevTab, window, cx| {
        commands::prev_tab(this, window, cx);
    }))
    .on_action(cx.listener(|this, _: &CloseWorkspace, window, cx| {
        commands::close_workspace(this, window, cx);
    }))
    .on_action(cx.listener(|this, _: &OpenFile, _, cx| commands::open_file_prompt(this, cx)))
    .on_action(cx.listener(|this, _: &OpenFolder, _, cx| commands::open_folder_prompt(this, cx)))
    .on_action(cx.listener(|this, _: &SaveFile, _, cx| commands::save_file(this, cx)))
    .on_action(cx.listener(|this, _: &SaveFileAs, _, cx| commands::save_file_as_prompt(this, cx)))
    .on_action(cx.listener(|this, _: &ClearRecentWorkspaces, _, cx| {
        this.close_menu(cx);
        commands::clear_recent_workspaces(cx);
    }))
}

fn attach_menu_actions(div: Div, cx: &mut Context<InviscidWindow>) -> Div {
    div.on_action(
        cx.listener(|this, action: &ToggleMenu, _, cx| commands::toggle_menu(this, action.0, cx)),
    )
    .on_action(
        cx.listener(|this, action: &OpenMenu, _, cx| commands::open_menu(this, action.0, cx)),
    )
    .on_action(cx.listener(|this, _: &CloseMenu, _, cx| commands::close_menu(this, cx)))
    .on_action(cx.listener(|this, _: &DismissOverlay, _, cx| commands::close_menu(this, cx)))
}

fn attach_view_actions(div: Div, cx: &mut Context<InviscidWindow>) -> Div {
    div.on_action(
        cx.listener(|this, _: &ToggleRenderMode, _, cx| commands::toggle_render_mode(this, cx)),
    )
    .on_action(cx.listener(|this, _: &ToggleSidebar, _, cx| commands::toggle_sidebar(this, cx)))
    .on_action(cx.listener(|this, action: &SetRenderMode, _, cx| {
        commands::set_render_mode(this, action.0, cx);
    }))
}
