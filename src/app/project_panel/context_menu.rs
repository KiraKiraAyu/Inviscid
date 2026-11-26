use gpui::*;
use std::path::PathBuf;
use std::rc::Rc;

use crate::app::actions::display_keystroke_for;
use crate::i18n::{ActiveLocale, TextKey};
use crate::theme::ThemeManager;
use crate::ui::ContextMenuBuilder;

#[derive(Clone, Debug, PartialEq)]
pub struct ContextMenuState {
    pub position: Point<Pixels>,
    pub target_path: PathBuf,
    pub is_dir: bool,
    pub is_root: bool,
    pub collocated_mode: bool,
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
    let (new_file_sel, new_folder_sel, _) =
        contextual_action_labels(state.is_dir, state.collocated_mode);
    let (new_file_key, new_folder_key, paste_key) =
        contextual_action_keys(state.is_dir, state.collocated_mode);
    let border_subtle = theme.border_subtle;
    let shortcut = |action_id: &str| display_keystroke_for(action_id, cx);

    let reveal_label = if cfg!(target_os = "macos") {
        cx.t(TextKey::TabCmRevealFinder).to_string()
    } else {
        cx.t(TextKey::TabCmRevealExplorer).to_string()
    };

    ContextMenuBuilder::new()
        .action_item_with_selector(
            "cm_new_file",
            cx.t(new_file_key).to_string(),
            format!("menu_item_{new_file_sel}"),
            String::new(),
            ContextMenuAction::NewFile,
            false,
            &on_action,
        )
        .action_item_with_selector(
            "cm_new_folder",
            cx.t(new_folder_key).to_string(),
            format!("menu_item_{new_folder_sel}"),
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
            cx.t(TextKey::ProjectCmCut).to_string(),
            shortcut("CutEntry"),
            ContextMenuAction::Cut,
            is_root,
            &on_action,
        )
        .action_item(
            "cm_copy",
            cx.t(TextKey::ProjectCmCopy).to_string(),
            shortcut("CopyEntry"),
            ContextMenuAction::Copy,
            is_root,
            &on_action,
        )
        .action_item(
            "cm_duplicate",
            cx.t(TextKey::ProjectCmDuplicate).to_string(),
            String::new(),
            ContextMenuAction::Duplicate,
            is_root,
            &on_action,
        )
        .action_item(
            "cm_paste",
            cx.t(paste_key).to_string(),
            shortcut("PasteEntry"),
            ContextMenuAction::Paste,
            !can_paste,
            &on_action,
        )
        .separator(border_subtle)
        .action_item(
            "cm_copy_path",
            cx.t(TextKey::TabCmCopyPath).to_string(),
            String::new(),
            ContextMenuAction::CopyPath,
            false,
            &on_action,
        )
        .action_item(
            "cm_copy_rel_path",
            cx.t(TextKey::TabCmCopyRelativePath).to_string(),
            String::new(),
            ContextMenuAction::CopyRelativePath,
            false,
            &on_action,
        )
        .separator(border_subtle)
        .action_item(
            "cm_rename",
            cx.t(TextKey::ProjectCmRename).to_string(),
            shortcut("RenameEntry"),
            ContextMenuAction::Rename,
            is_root,
            &on_action,
        )
        .action_item(
            "cm_trash",
            cx.t(TextKey::ProjectCmTrash).to_string(),
            shortcut("TrashEntry"),
            ContextMenuAction::Trash,
            is_root,
            &on_action,
        )
        .action_item(
            "cm_delete",
            cx.t(TextKey::ProjectCmDelete).to_string(),
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

pub fn contextual_action_keys(
    is_dir: bool,
    collocated_mode: bool,
) -> (TextKey, TextKey, TextKey) {
    match (is_dir, collocated_mode) {
        (true, false) => (
            TextKey::ProjectCmNewFile,
            TextKey::ProjectCmNewFolder,
            TextKey::ProjectCmPaste,
        ),
        (false, false) => (
            TextKey::ProjectCmNewFileInFolder,
            TextKey::ProjectCmNewFolderInFolder,
            TextKey::ProjectCmPasteIntoFolder,
        ),
        (true, true) => (
            TextKey::ProjectCmNewDocument,
            TextKey::ProjectCmNewFolder,
            TextKey::ProjectCmPaste,
        ),
        (false, true) => (
            TextKey::ProjectCmNewDocumentInFolder,
            TextKey::ProjectCmNewFolderInFolder,
            TextKey::ProjectCmPasteIntoFolder,
        ),
    }
}

/// Returns contextual labels for (New File/Document, New Folder, Paste) based on target type
/// (`is_dir`) and whether collocated mode is active.
pub fn contextual_action_labels(
    is_dir: bool,
    collocated_mode: bool,
) -> (&'static str, &'static str, &'static str) {
    match (is_dir, collocated_mode) {
        (true, false) => ("New File", "New Folder", "Paste"),
        (false, false) => (
            "New File in Folder",
            "New Folder in Folder",
            "Paste into Folder",
        ),
        (true, true) => ("New Document", "New Folder", "Paste"),
        (false, true) => (
            "New Document in Folder",
            "New Folder in Folder",
            "Paste into Folder",
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use core::prelude::v1::test;

    #[test]
    fn test_contextual_action_labels_distinguish_file_and_folder() {
        let (file_nf, file_nfolder, file_paste) = contextual_action_labels(false, false);
        assert_eq!(file_nf, "New File in Folder");
        assert_eq!(file_nfolder, "New Folder in Folder");
        assert_eq!(file_paste, "Paste into Folder");

        let (dir_nf, dir_nfolder, dir_paste) = contextual_action_labels(true, false);
        assert_eq!(dir_nf, "New File");
        assert_eq!(dir_nfolder, "New Folder");
        assert_eq!(dir_paste, "Paste");

        let (colloc_doc_nf, colloc_doc_nfolder, colloc_doc_paste) =
            contextual_action_labels(false, true);
        assert_eq!(colloc_doc_nf, "New Document in Folder");
        assert_eq!(colloc_doc_nfolder, "New Folder in Folder");
        assert_eq!(colloc_doc_paste, "Paste into Folder");

        let (colloc_dir_nf, colloc_dir_nfolder, colloc_dir_paste) =
            contextual_action_labels(true, true);
        assert_eq!(colloc_dir_nf, "New Document");
        assert_eq!(colloc_dir_nfolder, "New Folder");
        assert_eq!(colloc_dir_paste, "Paste");
    }
}
