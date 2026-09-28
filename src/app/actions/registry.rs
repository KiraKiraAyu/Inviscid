use crate::app::actions::defs::*;
use crate::config::AppConfig;
use crate::editor::actions::*;
use crate::ui::format_keystroke_for_display;
use crate::ui::key_context;
use gpui::App;

/// Category for editor-internal keys. They are bound from this table like everything else,
/// but are not offered for rebinding in the settings UI.
pub const EDITOR_CATEGORY: &str = "Editor";

/// Category for the project panel's file-tree commands. Unlike [`EDITOR_CATEGORY`] these are
/// offered for rebinding in the settings UI.
pub const PROJECT_PANEL_CATEGORY: &str = "Project Panel";

/// Presentation metadata for one action's keybindings.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct KeybindingMeta {
    pub id: &'static str,
    pub name: &'static str,
    pub category: &'static str,
    /// Every keystroke bound by default. The first one is what the UI advertises; the rest are
    /// platform habits that would otherwise be lost, such as `secondary-y` for redo.
    pub default_keystrokes: &'static [&'static str],
}

impl KeybindingMeta {
    /// The keystroke shown in menus and in the settings list.
    pub fn primary_keystroke(&self) -> &str {
        self.default_keystrokes.first().copied().unwrap_or("")
    }
}

/// Which platform a binding belongs to.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Platform {
    All,
    Macos,
    NonMacos,
}

impl Platform {
    fn applies(self) -> bool {
        match self {
            Platform::All => true,
            Platform::Macos => cfg!(target_os = "macos"),
            Platform::NonMacos => !cfg!(target_os = "macos"),
        }
    }
}

/// The single source of truth for every key in the app.
///
/// Both the live keymap, the menu labels and the settings UI are generated from `KEYBINDINGS`.
/// Nothing else is allowed to name a keystroke.
pub struct KeybindingSpec {
    pub meta: KeybindingMeta,
    /// A gpui context predicate such as `"Editor || InlineInput"`. `None` binds globally.
    pub context: Option<&'static str>,
    pub platform: Platform,
    bind: fn(&[&str], Option<&'static str>, &mut App),
}

macro_rules! define_keybindings {
    ($( ($id:ident, $name:expr, $cat:expr, $strokes:expr, $ctx:expr, $platform:expr) ),* $(,)?) => {
        pub static KEYBINDINGS: &[KeybindingSpec] = &[
            $(
                KeybindingSpec {
                    meta: KeybindingMeta {
                        id: stringify!($id),
                        name: $name,
                        category: $cat,
                        default_keystrokes: $strokes,
                    },
                    context: $ctx,
                    platform: $platform,
                    bind: |keystrokes, context, cx| {
                        for keystroke in keystrokes {
                            cx.bind_keys([gpui::KeyBinding::new(keystroke, $id, context)]);
                        }
                    },
                },
            )*
        ];
    };
}

const APP: Option<&str> = Some(key_context::APP);
const EDITOR: Option<&str> = Some(key_context::EDITOR);
const EDITOR_OR_INLINE: Option<&str> = Some(key_context::EDITOR_OR_INLINE);
const PROJECT_PANEL_TREE: Option<&str> = Some(key_context::PROJECT_PANEL_TREE);
const MENU_OPEN: Option<&str> = Some(key_context::MENU_OPEN);

use Platform::{All, Macos, NonMacos};

// Application commands (rebindable)

#[rustfmt::skip]
define_keybindings!(
    (NewTab, "New Tab", "File", &["secondary-n"], APP, All),
    (NewWindow, "New Window", "File", &["secondary-shift-n"], APP, All),
    (OpenFile, "Open File...", "File", &["secondary-o"], APP, All),
    (OpenFolder, "Open Folder...", "File", &["secondary-shift-o"], APP, All),
    (SaveFile, "Save", "File", &["secondary-s"], APP, All),
    (SaveFileAs, "Save As...", "File", &["secondary-shift-s"], APP, All),
    (CloseTab, "Close Tab", "File", &["secondary-w"], APP, All),
    (CloseWorkspace, "Close Workspace", "File", &["secondary-shift-w"], APP, All),
    (NextTab, "Next Tab", "View", &["secondary-tab"], APP, All),
    (PrevTab, "Previous Tab", "View", &["secondary-shift-tab"], APP, All),
    (ToggleSidebar, "Toggle Sidebar", "View", &["secondary-b"], APP, All),
    (ToggleRenderMode, "Toggle Render Mode", "View", &["secondary-e"], APP, All),
    (OpenSettings, "Settings", "General", &["secondary-,"], APP, All),
    (Quit, "Exit", "General", &["secondary-q"], APP, All),
    (DismissOverlay, "Dismiss Overlay", "General", &["escape"], MENU_OPEN, All),
    (UndoAction, "Undo", "Edit", &["secondary-z"], EDITOR_OR_INLINE, All),
    (RedoAction, "Redo", "Edit", &["secondary-shift-z", "secondary-y"], EDITOR_OR_INLINE, All),
    (CutAction, "Cut", "Edit", &["secondary-x"], EDITOR_OR_INLINE, All),
    (CopyAction, "Copy", "Edit", &["secondary-c"], EDITOR_OR_INLINE, All),
    (PasteAction, "Paste", "Edit", &["secondary-v"], EDITOR_OR_INLINE, All),
    (SelectAll, "Select All", "Edit", &["secondary-a"], EDITOR_OR_INLINE, All),

    // Project panel: scoped to the file tree (`PROJECT_PANEL_TREE` also excludes the rename box),
    // so the same keystrokes keep their text meaning in the editor and in the inline input.

    (CopyEntry, "Copy File", PROJECT_PANEL_CATEGORY, &["secondary-c"], PROJECT_PANEL_TREE, All),
    (CutEntry, "Cut File", PROJECT_PANEL_CATEGORY, &["secondary-x"], PROJECT_PANEL_TREE, All),
    (PasteEntry, "Paste File", PROJECT_PANEL_CATEGORY, &["secondary-v"], PROJECT_PANEL_TREE, All),
    (RenameEntry, "Rename File", PROJECT_PANEL_CATEGORY, &["f2"], PROJECT_PANEL_TREE, All),
    (TrashEntry, "Move File to Trash", PROJECT_PANEL_CATEGORY, &["delete"], PROJECT_PANEL_TREE, All),
    (DeleteEntry, "Delete File Permanently", PROJECT_PANEL_CATEGORY, &["secondary-delete"], PROJECT_PANEL_TREE, All),

    // Editor commands: bound from this same table, hidden from the settings UI

    // Navigation & selection
    (MoveLeft, "Move Left", EDITOR_CATEGORY, &["left"], EDITOR_OR_INLINE, All),
    (SelectLeft, "Select Left", EDITOR_CATEGORY, &["shift-left"], EDITOR_OR_INLINE, All),
    (MoveRight, "Move Right", EDITOR_CATEGORY, &["right"], EDITOR_OR_INLINE, All),
    (SelectRight, "Select Right", EDITOR_CATEGORY, &["shift-right"], EDITOR_OR_INLINE, All),
    (MoveUp, "Move Up", EDITOR_CATEGORY, &["up"], EDITOR, All),
    (SelectUp, "Select Up", EDITOR_CATEGORY, &["shift-up"], EDITOR, All),
    (MoveDown, "Move Down", EDITOR_CATEGORY, &["down"], EDITOR, All),
    (SelectDown, "Select Down", EDITOR_CATEGORY, &["shift-down"], EDITOR, All),
    (PageUp, "Page Up", EDITOR_CATEGORY, &["pageup"], EDITOR, All),
    (SelectPageUp, "Select Page Up", EDITOR_CATEGORY, &["shift-pageup"], EDITOR, All),
    (PageDown, "Page Down", EDITOR_CATEGORY, &["pagedown"], EDITOR, All),
    (SelectPageDown, "Select Page Down", EDITOR_CATEGORY, &["shift-pagedown"], EDITOR, All),

    // Line manipulation
    (MoveLineUp, "Move Line Up", EDITOR_CATEGORY, &["alt-up"], EDITOR, All),
    (MoveLineDown, "Move Line Down", EDITOR_CATEGORY, &["alt-down"], EDITOR, All),
    (DuplicateLineUp, "Duplicate Line Up", EDITOR_CATEGORY, &["alt-shift-up"], EDITOR, All),
    (DuplicateLineDown, "Duplicate Line Down", EDITOR_CATEGORY, &["alt-shift-down"], EDITOR, All),
    (DeleteLine, "Delete Line", EDITOR_CATEGORY, &["secondary-shift-k", "secondary-d"], EDITOR, All),
    (ToggleComment, "Toggle Comment", EDITOR_CATEGORY, &["secondary-/"], EDITOR, All),

    // Indent & newline
    (Indent, "Indent", EDITOR_CATEGORY, &["tab"], EDITOR, All),
    (Outdent, "Outdent", EDITOR_CATEGORY, &["shift-tab"], EDITOR, All),
    (Newline, "Newline", EDITOR_CATEGORY, &["enter"], EDITOR, All),
    (NewlineBelow, "Newline Below", EDITOR_CATEGORY, &["secondary-enter"], EDITOR, All),
    (NewlineBelowBlock, "Newline Below Block", EDITOR_CATEGORY, &["shift-enter"], EDITOR, All),
    (NewlineAbove, "Newline Above", EDITOR_CATEGORY, &["secondary-shift-enter"], EDITOR, All),

    // Deletion
    (Backspace, "Backspace", EDITOR_CATEGORY, &["backspace"], EDITOR_OR_INLINE, All),
    (Delete, "Delete", EDITOR_CATEGORY, &["delete"], EDITOR_OR_INLINE, All),

    // Word navigation
    (MoveToPreviousWord, "Previous Word", EDITOR_CATEGORY, &["alt-left"], EDITOR_OR_INLINE, Macos),
    (SelectToPreviousWord, "Select Previous Word", EDITOR_CATEGORY, &["alt-shift-left"], EDITOR_OR_INLINE, Macos),
    (MoveToNextWord, "Next Word", EDITOR_CATEGORY, &["alt-right"], EDITOR_OR_INLINE, Macos),
    (SelectToNextWord, "Select Next Word", EDITOR_CATEGORY, &["alt-shift-right"], EDITOR_OR_INLINE, Macos),
    (DeleteToPreviousWord, "Delete Previous Word", EDITOR_CATEGORY, &["alt-backspace"], EDITOR_OR_INLINE, Macos),
    (DeleteToNextWord, "Delete Next Word", EDITOR_CATEGORY, &["alt-delete"], EDITOR_OR_INLINE, Macos),

    // Line & document navigation
    (MoveToBeginningOfLine, "Line Start", EDITOR_CATEGORY, &["cmd-left", "home"], EDITOR_OR_INLINE, Macos),
    (SelectToBeginningOfLine, "Select to Line Start", EDITOR_CATEGORY, &["cmd-shift-left", "shift-home"], EDITOR_OR_INLINE, Macos),
    (MoveToEndOfLine, "Line End", EDITOR_CATEGORY, &["cmd-right", "end"], EDITOR_OR_INLINE, Macos),
    (SelectToEndOfLine, "Select to Line End", EDITOR_CATEGORY, &["cmd-shift-right", "shift-end"], EDITOR_OR_INLINE, Macos),
    (MoveToBeginningOfDocument, "Document Start", EDITOR_CATEGORY, &["cmd-up", "cmd-home"], EDITOR, Macos),
    (SelectToBeginningOfDocument, "Select to Document Start", EDITOR_CATEGORY, &["cmd-shift-up", "cmd-shift-home"], EDITOR, Macos),
    (MoveToEndOfDocument, "Document End", EDITOR_CATEGORY, &["cmd-down", "cmd-end"], EDITOR, Macos),
    (SelectToEndOfDocument, "Select to Document End", EDITOR_CATEGORY, &["cmd-shift-down", "cmd-shift-end"], EDITOR, Macos),
    (DeleteLine, "Delete Line", EDITOR_CATEGORY, &["cmd-backspace"], EDITOR, Macos),

    // Word navigation
    (MoveToPreviousWord, "Previous Word", EDITOR_CATEGORY, &["ctrl-left", "alt-left"], EDITOR_OR_INLINE, NonMacos),
    (SelectToPreviousWord, "Select Previous Word", EDITOR_CATEGORY, &["ctrl-shift-left", "alt-shift-left"], EDITOR_OR_INLINE, NonMacos),
    (MoveToNextWord, "Next Word", EDITOR_CATEGORY, &["ctrl-right", "alt-right"], EDITOR_OR_INLINE, NonMacos),
    (SelectToNextWord, "Select Next Word", EDITOR_CATEGORY, &["ctrl-shift-right", "alt-shift-right"], EDITOR_OR_INLINE, NonMacos),
    (DeleteToPreviousWord, "Delete Previous Word", EDITOR_CATEGORY, &["ctrl-backspace"], EDITOR_OR_INLINE, NonMacos),
    (DeleteToNextWord, "Delete Next Word", EDITOR_CATEGORY, &["ctrl-delete"], EDITOR_OR_INLINE, NonMacos),
    (MoveToBeginningOfLine, "Line Start", EDITOR_CATEGORY, &["home"], EDITOR_OR_INLINE, NonMacos),
    (SelectToBeginningOfLine, "Select to Line Start", EDITOR_CATEGORY, &["shift-home"], EDITOR_OR_INLINE, NonMacos),
    (MoveToEndOfLine, "Line End", EDITOR_CATEGORY, &["end"], EDITOR_OR_INLINE, NonMacos),
    (SelectToEndOfLine, "Select to Line End", EDITOR_CATEGORY, &["shift-end"], EDITOR_OR_INLINE, NonMacos),
    (MoveToBeginningOfDocument, "Document Start", EDITOR_CATEGORY, &["ctrl-home"], EDITOR, NonMacos),
    (SelectToBeginningOfDocument, "Select to Document Start", EDITOR_CATEGORY, &["ctrl-shift-home"], EDITOR, NonMacos),
    (MoveToEndOfDocument, "Document End", EDITOR_CATEGORY, &["ctrl-end"], EDITOR, NonMacos),
    (SelectToEndOfDocument, "Select to Document End", EDITOR_CATEGORY, &["ctrl-shift-end"], EDITOR, NonMacos),
);

/// Metadata for the settings UI: everything the user is allowed to rebind.
pub fn keybindings() -> impl Iterator<Item = &'static KeybindingMeta> {
    KEYBINDINGS
        .iter()
        .filter(|spec| spec.meta.category != EDITOR_CATEGORY)
        .map(|spec| &spec.meta)
}

/// Every entry that applies to the current platform. Single source of the platform filter, so
/// binding and lookup can never disagree about which entry is live.
fn specs_for_platform() -> impl Iterator<Item = &'static KeybindingSpec> {
    KEYBINDINGS.iter().filter(|spec| spec.platform.applies())
}

/// Binds every action to each of its default keystrokes, for the current platform.
pub fn bind_default_bindings(cx: &mut App) {
    for spec in specs_for_platform() {
        (spec.bind)(spec.meta.default_keystrokes, spec.context, cx);
    }
}

/// The entry for `action_id` on the current platform.
///
/// An action may have one entry per platform (macOS word jumps vs. ctrl ones), so the platform
/// filter is not optional here.
fn spec_for(action_id: &str) -> Option<&'static KeybindingSpec> {
    specs_for_platform().find(|spec| spec.meta.id == action_id)
}

/// Rebinds a single action to one keystroke.
pub fn bind_keys_for_action(action_id: &str, keystroke: &str, cx: &mut App) {
    if let Some(spec) = spec_for(action_id) {
        (spec.bind)(std::slice::from_ref(&keystroke), spec.context, cx);
    }
}

/// Rebuilds the whole keymap from scratch: defaults first, then the user's overrides.
///
/// The clear is required — gpui's `bind_keys` only appends, so without it every call would pile
/// another full copy of the default table on top of the previous one. For the same reason an
/// overridden action's *defaults are skipped*: gpui can clear everything but never drop a single
/// binding, so binding them and then adding the custom keystroke would leave both firing the
/// action.
pub fn apply_configured_keys(config: &AppConfig, cx: &mut App) {
    cx.clear_key_bindings();
    for spec in
        specs_for_platform().filter(|spec| !config.custom_keybindings.contains_key(spec.meta.id))
    {
        (spec.bind)(spec.meta.default_keystrokes, spec.context, cx);
    }
    for (action_id, keystroke) in &config.custom_keybindings {
        bind_keys_for_action(action_id, keystroke, cx);
    }
}

/// The keystroke to render next to a menu item: the user's override when set, else the default.
///
/// Returns an empty string for actions that carry no binding.
pub fn display_keystroke_for(action_id: &str, cx: &App) -> String {
    let Some(spec) = spec_for(action_id) else {
        return String::new();
    };
    let fallback = spec.meta.primary_keystroke();
    let keystroke = cx
        .try_global::<AppConfig>()
        .map(|config| config.get_keybinding(spec.meta.id, fallback))
        .unwrap_or_else(|| fallback.to_string());
    format_keystroke_for_display(&keystroke)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::{HashMap, HashSet};

    #[test]
    fn keybindings_are_unique_per_platform() {
        let mut seen = HashSet::new();
        for spec in KEYBINDINGS {
            assert!(
                seen.insert((spec.meta.id, spec.platform)),
                "duplicate entry for {} on {:?}",
                spec.meta.id,
                spec.platform
            );
        }
    }

    #[test]
    fn every_keybinding_declares_a_keystroke_and_context() {
        for spec in KEYBINDINGS {
            assert!(
                !spec.meta.default_keystrokes.is_empty(),
                "{} has no default keystroke",
                spec.meta.id
            );
            assert!(
                spec.context.is_some(),
                "{} would be bound globally; scope it to a context",
                spec.meta.id
            );
        }
    }

    #[test]
    fn escape_is_only_claimed_while_a_menu_is_open() {
        // A matched binding is consumed before element `on_key_down` listeners run, so binding
        // `escape` to an always-present context would make it unreachable for the focused
        // component — the project panel's context menu and rename box both want it.
        let spec = spec_for("DismissOverlay").expect("DismissOverlay missing");
        assert_eq!(spec.context, Some(key_context::MENU_OPEN));
    }

    #[test]
    fn project_panel_commands_are_scoped_to_the_file_tree() {
        // Without the ProjectPanel predicate these keystrokes would collide with the editor's
        // own copy/cut/paste and with `delete`, and the context menu would have to keep
        // advertising shortcuts it does not actually own.
        for id in [
            "CopyEntry",
            "CutEntry",
            "PasteEntry",
            "RenameEntry",
            "TrashEntry",
            "DeleteEntry",
        ] {
            let spec =
                spec_for(id).unwrap_or_else(|| panic!("{id} missing from the keybinding table"));
            assert_eq!(
                spec.context,
                Some(key_context::PROJECT_PANEL_TREE),
                "{id} must be scoped to the file tree and stay out of the rename box"
            );
        }
    }

    #[test]
    fn shared_editor_commands_cover_the_inline_input() {
        for id in [
            "UndoAction",
            "RedoAction",
            "CutAction",
            "CopyAction",
            "PasteAction",
            "SelectAll",
        ] {
            let spec = KEYBINDINGS
                .iter()
                .find(|spec| spec.meta.id == id)
                .unwrap_or_else(|| panic!("{id} missing from the keybinding table"));
            assert_eq!(
                spec.context,
                Some(key_context::EDITOR_OR_INLINE),
                "{id} must be bound in both the editor and the inline input"
            );
        }
    }

    #[test]
    fn redo_keeps_the_platform_secondary_keystroke() {
        let redo = KEYBINDINGS
            .iter()
            .find(|spec| spec.meta.id == "RedoAction")
            .unwrap();
        assert!(redo.meta.default_keystrokes.contains(&"secondary-y"));
        assert_eq!(redo.meta.primary_keystroke(), "secondary-shift-z");
    }

    #[test]
    fn platform_specific_commands_have_one_entry_per_platform() {
        for id in [
            "MoveToPreviousWord",
            "MoveToBeginningOfLine",
            "MoveToBeginningOfDocument",
            "DeleteToPreviousWord",
        ] {
            let platforms: HashSet<Platform> = KEYBINDINGS
                .iter()
                .filter(|spec| spec.meta.id == id)
                .map(|spec| spec.platform)
                .collect();
            assert!(
                platforms.contains(&Platform::Macos) && platforms.contains(&Platform::NonMacos),
                "{id} is missing a platform-specific entry"
            );
        }
    }

    #[test]
    fn default_bindings_are_installed() {
        // Guards the context predicates: gpui parses them at bind time and panics on bad syntax,
        // which would otherwise only surface when the app starts.
        let cx = gpui::TestAppContext::single();
        cx.update(|cx| {
            bind_default_bindings(cx);
            let bindings = cx.key_bindings();
            let count = bindings.borrow().bindings().count();
            assert!(count > 0, "no default bindings were installed");
        });
    }

    #[test]
    fn every_editor_action_is_bound() {
        // Guard against a new editor action being added without a keybinding.
        let bound: HashSet<&str> = KEYBINDINGS.iter().map(|spec| spec.meta.id).collect();
        for id in [
            "MoveLeft",
            "MoveUp",
            "SelectAll",
            "Newline",
            "Backspace",
            "DeleteLine",
            "ToggleComment",
            "Indent",
            "Outdent",
            "PageUp",
            "MoveToEndOfDocument",
            "DeleteToNextWord",
        ] {
            assert!(bound.contains(id), "{id} has no keybinding");
        }
    }

    #[test]
    fn lookups_resolve_the_entry_for_this_platform() {
        // This id has a macOS entry and a non-macOS entry; a bare `find` would always take the
        // macOS one because it is listed first.
        let spec = spec_for("MoveToPreviousWord").expect("no entry for the current platform");
        let expected = if cfg!(target_os = "macos") {
            "alt-left"
        } else {
            "ctrl-left"
        };
        assert_eq!(spec.meta.primary_keystroke(), expected);
    }

    #[test]
    fn a_custom_keybinding_replaces_the_default() {
        let cx = gpui::TestAppContext::single();
        cx.update(|cx| {
            let mut config = AppConfig::default();
            config
                .custom_keybindings
                .insert("SaveFile".to_string(), "alt-s".to_string());
            apply_configured_keys(&config, cx);

            let bound = cx.key_bindings();
            let keystrokes: Vec<(bool, String)> = bound
                .borrow()
                .bindings_for_action(&crate::app::actions::SaveFile)
                .flat_map(|binding| {
                    binding
                        .keystrokes()
                        .iter()
                        .map(|k| (k.inner().modifiers.alt, k.key().to_string()))
                        .collect::<Vec<_>>()
                })
                .collect();
            assert_eq!(
                keystrokes,
                vec![(true, "s".to_string())],
                "the default secondary-s must not stay bound alongside the override"
            );
        });
    }

    #[test]
    fn an_action_has_a_single_display_name() {
        let mut names: HashMap<&str, &str> = HashMap::new();
        for spec in KEYBINDINGS {
            if let Some(name) = names.insert(spec.meta.id, spec.meta.name) {
                assert_eq!(
                    name, spec.meta.name,
                    "{} is listed with two different names",
                    spec.meta.id
                );
            }
        }
    }
}
