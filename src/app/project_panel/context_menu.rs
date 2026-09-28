use gpui::*;
use std::path::PathBuf;
use std::rc::Rc;

use crate::app::actions::display_keystroke_for;
use crate::theme::ThemeManager;
use crate::ui::ContextMenuBuilder;

#[derive(Clone, Debug, PartialEq)]
pub struct ContextMenuState {
    pub position: Point<Pixels>,
    pub target_path: PathBuf,
    pub is_dir: bool,
    pub is_root: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub enum FileClipboard {
    #[default]
    None,
    Cut(PathBuf),
    Copy(PathBuf),
}

impl FileClipboard {
    pub fn is_none(&self) -> bool {
        matches!(self, FileClipboard::None)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ContextMenuAction {
    NewFile,
    NewFolder,
    RevealInFileManager,
    Cut,
    Copy,
    Duplicate,
    Paste,
    CopyPath,
    CopyRelativePath,
    Rename,
    Trash,
    Delete,
}

pub fn render_context_menu(
    state: &ContextMenuState,
    clipboard: &FileClipboard,
    safe_bounds: Bounds<Pixels>,
    on_action: impl Fn(ContextMenuAction, &mut Window, &mut App) + 'static,
    on_close: impl Fn(&mut Window, &mut App) + 'static,
    cx: &App,
) -> impl IntoElement {
    let can_paste = !clipboard.is_none();
    let theme = cx.global::<ThemeManager>().theme();
    let on_action: Rc<dyn Fn(ContextMenuAction, &mut Window, &mut App)> = Rc::new(on_action);

    let is_root = state.is_root;
    let (new_file_label, new_folder_label, paste_label) = contextual_action_labels(state.is_dir);
    let border_subtle = theme.border_subtle;
    let shortcut = |action_id: &str| display_keystroke_for(action_id, cx);

    let reveal_label = if cfg!(target_os = "macos") {
        "Reveal in Finder"
    } else {
        "Reveal in File Explorer"
    };

    ContextMenuBuilder::new()
        .action_item(
            "cm_new_file",
            new_file_label,
            String::new(),
            ContextMenuAction::NewFile,
            false,
            &on_action,
        )
        .action_item(
            "cm_new_folder",
            new_folder_label,
            String::new(),
            ContextMenuAction::NewFolder,
            false,
            &on_action,
        )
        .separator(border_subtle)
        .action_item(
            "cm_reveal",
            reveal_label,
            String::new(),
            ContextMenuAction::RevealInFileManager,
            false,
            &on_action,
        )
        .separator(border_subtle)
        .action_item(
            "cm_cut",
            "Cut",
            shortcut("CutEntry"),
            ContextMenuAction::Cut,
            is_root,
            &on_action,
        )
        .action_item(
            "cm_copy",
            "Copy",
            shortcut("CopyEntry"),
            ContextMenuAction::Copy,
            is_root,
            &on_action,
        )
        .action_item(
            "cm_duplicate",
            "Duplicate",
            String::new(),
            ContextMenuAction::Duplicate,
            is_root,
            &on_action,
        )
        .action_item(
            "cm_paste",
            paste_label,
            shortcut("PasteEntry"),
            ContextMenuAction::Paste,
            !can_paste,
            &on_action,
        )
        .separator(border_subtle)
        .action_item(
            "cm_copy_path",
            "Copy Path",
            String::new(),
            ContextMenuAction::CopyPath,
            false,
            &on_action,
        )
        .action_item(
            "cm_copy_rel_path",
            "Copy Relative Path",
            String::new(),
            ContextMenuAction::CopyRelativePath,
            false,
            &on_action,
        )
        .separator(border_subtle)
        .action_item(
            "cm_rename",
            "Rename",
            shortcut("RenameEntry"),
            ContextMenuAction::Rename,
            is_root,
            &on_action,
        )
        .action_item(
            "cm_trash",
            "Trash",
            shortcut("TrashEntry"),
            ContextMenuAction::Trash,
            is_root,
            &on_action,
        )
        .action_item(
            "cm_delete",
            "Delete",
            shortcut("DeleteEntry"),
            ContextMenuAction::Delete,
            is_root,
            &on_action,
        )
        .into_element(
            "project_panel_context_menu",
            state.position,
            safe_bounds,
            px(0.0),
            theme,
            on_close,
        )
}

/// Returns contextual labels for (New File, New Folder, Paste) based on target type (`is_dir`).
pub fn contextual_action_labels(is_dir: bool) -> (&'static str, &'static str, &'static str) {
    if is_dir {
        ("New File", "New Folder", "Paste")
    } else {
        (
            "New File in Folder",
            "New Folder in Folder",
            "Paste into Folder",
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use core::prelude::v1::test;

    #[test]
    fn test_contextual_action_labels_distinguish_file_and_folder() {
        let (file_nf, file_nfolder, file_paste) = contextual_action_labels(false);
        assert_eq!(file_nf, "New File in Folder");
        assert_eq!(file_nfolder, "New Folder in Folder");
        assert_eq!(file_paste, "Paste into Folder");

        let (dir_nf, dir_nfolder, dir_paste) = contextual_action_labels(true);
        assert_eq!(dir_nf, "New File");
        assert_eq!(dir_nfolder, "New Folder");
        assert_eq!(dir_paste, "Paste");
    }
}
