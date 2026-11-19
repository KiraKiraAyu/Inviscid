use gpui::*;
use std::path::{Path, PathBuf};

use crate::fs::{COLLOCATED_INDEX_FILE, find_collocated_index_file, resolve_document_parent_dir};

/// Locates an initial markdown entry file in a directory when opening it as a workspace.
///
/// In standard mode, prefers `README.md`, then `index.md`, then the first sorted `.md`/`.markdown` file.
/// In collocated mode, loose files are ignored; prefers a `readme/` bundle's `index.md`, then the
/// first sorted direct subdirectory containing `index.md`.
pub fn find_folder_target_file(folder: &Path, collocated_mode: bool) -> Option<PathBuf> {
    if !folder.is_dir() {
        return None;
    }

    let entries = std::fs::read_dir(folder).ok()?;

    if collocated_mode {
        let mut bundles: Vec<(PathBuf, PathBuf)> = Vec::new();
        for entry in entries.flatten() {
            let p = entry.path();
            let is_hidden = p
                .file_name()
                .and_then(|n| n.to_str())
                .is_some_and(|n| n.starts_with('.'));
            if is_hidden || !p.is_dir() {
                continue;
            }
            if let Some(index_path) = find_collocated_index_file(&p) {
                if p.file_name()
                    .and_then(|n| n.to_str())
                    .is_some_and(|n| n.eq_ignore_ascii_case("readme"))
                {
                    return Some(index_path);
                }
                bundles.push((p, index_path));
            }
        }
        bundles.sort_by(|a, b| a.0.cmp(&b.0));
        return bundles.into_iter().next().map(|(_, index)| index);
    }

    let mut md_files = Vec::new();
    for entry in entries.flatten() {
        let p = entry.path();
        if p.is_file()
            && let Some(ext) = p.extension().and_then(|e| e.to_str())
            && (ext.eq_ignore_ascii_case("md") || ext.eq_ignore_ascii_case("markdown"))
        {
            if let Some(name) = p.file_name().and_then(|n| n.to_str())
                && name.eq_ignore_ascii_case("readme.md")
            {
                return Some(p);
            }
            md_files.push(p);
        }
    }
    if !md_files.is_empty() {
        if let Some(idx) = md_files.iter().find(|p| {
            p.file_name()
                .and_then(|n| n.to_str())
                .is_some_and(|n| n.eq_ignore_ascii_case(COLLOCATED_INDEX_FILE))
        }) {
            return Some(idx.clone());
        }

        md_files.sort();
        return Some(md_files[0].clone());
    }
    None
}

/// Returns the safe workspace viewport bounds inside the window,
/// excluding the TitleBar (top) and StatusBar (bottom).
pub fn workspace_safe_bounds(window_bounds: Bounds<Pixels>) -> Bounds<Pixels> {
    let titlebar_h = crate::app::TitleBar::HEIGHT;
    let statusbar_h = crate::app::status_bar::StatusBar::HEIGHT;
    let available_h = (window_bounds.size.height - titlebar_h - statusbar_h).max(px(0.0));
    Bounds {
        origin: point(px(0.0), titlebar_h),
        size: size(window_bounds.size.width, available_h),
    }
}

pub fn extract_workspace_name_from_paths<'a>(
    paths: impl IntoIterator<Item = Option<&'a Path>>,
    collocated_mode: bool,
) -> String {
    for path in paths.into_iter().flatten() {
        if let Some(parent) = resolve_document_parent_dir(path, collocated_mode, None)
            && let Some(name) = parent.file_name().and_then(|n| n.to_str())
            && !name.is_empty()
        {
            return name.to_string();
        }
    }
    "Open Workspace".to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    // `super::*` also brings in `gpui::test`; shadow it so `#[test]` resolves to the built-in attribute.
    use core::prelude::v1::test;
    use std::path::Path;

    #[test]
    fn test_extract_workspace_name_from_paths() {
        assert_eq!(
            extract_workspace_name_from_paths([None], false),
            "Open Workspace"
        );

        let p1 = Path::new("d:/my_project/docs/readme.md");
        assert_eq!(extract_workspace_name_from_paths([Some(p1)], false), "docs");

        let p2 = Path::new("c:/workspace/file.md");
        assert_eq!(
            extract_workspace_name_from_paths([None, Some(p2)], false),
            "workspace"
        );
    }

    #[test]
    fn test_find_folder_target_file_and_workspace_name_for_mode() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();

        std::fs::write(root.join("README.md"), "# Loose Readme").unwrap();
        std::fs::create_dir_all(root.join("category")).unwrap();
        std::fs::create_dir_all(root.join("beta")).unwrap();
        std::fs::write(root.join("beta").join("index.md"), "# Beta").unwrap();
        std::fs::create_dir_all(root.join("README")).unwrap();
        std::fs::write(root.join("README").join("INDEX.MD"), "# Bundle Readme").unwrap();

        // Standard mode prefers loose README.md; collocated mode skips loose files and picks README bundle
        assert_eq!(
            find_folder_target_file(root, false),
            Some(root.join("README.md"))
        );
        let collocated_target = find_folder_target_file(root, true).unwrap();
        assert_eq!(
            collocated_target.parent(),
            Some(root.join("README").as_path())
        );
        assert!(crate::fs::is_collocated_index_path(&collocated_target));

        // Workspace name extraction in collocated mode skips the bundle dir itself
        let nested_idx = Path::new("d:/projects/docs/my-note/index.md");
        assert_eq!(
            extract_workspace_name_from_paths([Some(nested_idx)], false),
            "my-note"
        );
        assert_eq!(
            extract_workspace_name_from_paths([Some(nested_idx)], true),
            "docs"
        );
    }
}
