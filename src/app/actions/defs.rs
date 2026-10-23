use std::path::PathBuf;

use crate::app::menu::ActiveMenu;
use crate::buffer::Position;
use crate::editor::RenderMode;
use gpui::Action;
use gpui::actions;

actions!(
    inviscid,
    [
        NewTab,
        CloseTab,
        CloseWorkspace,
        ClearRecentWorkspaces,
        NextTab,
        PrevTab,
        OpenFile,
        OpenFolder,
        SaveFile,
        SaveFileAs,
        NewWindow,
        ToggleRenderMode,
        ToggleSidebar,
        ToggleSearchSidebar,
        DismissOverlay,
        OpenAbout,
        OpenSettings,
        Quit,
        CloseMenu,
        RenameEntry,
        TrashEntry,
        DeleteEntry,
        CutEntry,
        CopyEntry,
        PasteEntry,
        FindInDocument,
        FindNext,
        FindPrevious,
        CloseFindBar,
        ReplaceInDocument,
        FindInWorkspace,
        CloseSearchPanel,
        ToggleCaseSensitive,
        ToggleWholeWord,
        ToggleRegex,
    ]
);

/// Toggles the given menu: closes it when it is already open, otherwise opens it.
#[derive(Clone, PartialEq, Eq, Debug, Action)]
#[action(namespace = inviscid, no_json)]
pub struct ToggleMenu(pub ActiveMenu);

/// Opens the given menu, leaving it untouched when it is already the active one.
#[derive(Clone, PartialEq, Eq, Debug, Action)]
#[action(namespace = inviscid, no_json)]
pub struct OpenMenu(pub ActiveMenu);

/// Switches the active editor to a specific render mode.
///
/// The concrete mode lives in the parameter, so callers decide it and the handler stays a plain
/// forward. `no_json` because `RenderMode` has no serde impl: the action can only be bound as a
/// value (`KeyBinding::new(keystroke, SetRenderMode(mode), ctx)`), never built from a keymap file.
/// If two modes ever need their own default key, the registry's id column has to be split from
/// the action column — `stringify!(SetRenderMode)` alone cannot distinguish them in
/// `AppConfig::custom_keybindings`.
#[derive(Clone, PartialEq, Eq, Debug, Action)]
#[action(namespace = inviscid, no_json)]
pub struct SetRenderMode(pub RenderMode);

/// Opens a workspace search hit and moves the cursor to it.
///
/// Dispatched by the search results list rather than emitted as an event, so the handler runs
/// with the window and can focus the editor. Carries runtime data and is never bound to a key.
#[derive(Clone, PartialEq, Eq, Debug, Action)]
#[action(namespace = inviscid, no_json)]
pub struct OpenWorkspaceMatch {
    pub path: PathBuf,
    pub position: Position,
}
