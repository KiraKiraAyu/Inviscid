use gpui::*;
use std::path::{Path, PathBuf};

/// Locates an initial markdown entry file (preferring README.md, then index.md) in a directory
pub fn find_folder_target_file(folder: &Path) -> Option<PathBuf> {
    if !folder.is_dir() {
        return None;
    }

    if let Ok(entries) = std::fs::read_dir(folder) {
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
                    .map(|n| n.eq_ignore_ascii_case("index.md"))
                    .unwrap_or(false)
            }) {
                return Some(idx.clone());
            }

            md_files.sort();
            return Some(md_files[0].clone());
        }
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
) -> String {
    for path in paths.into_iter().flatten() {
        if let Some(parent) = path.parent()
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
    use super::extract_workspace_name_from_paths;
    use std::path::Path;

    #[test]
    fn test_extract_workspace_name_from_paths() {
        assert_eq!(extract_workspace_name_from_paths([None]), "Open Workspace");

        let p1 = Path::new("d:/my_project/docs/readme.md");
        assert_eq!(extract_workspace_name_from_paths([Some(p1)]), "docs");

        let p2 = Path::new("c:/workspace/file.md");
        assert_eq!(
            extract_workspace_name_from_paths([None, Some(p2)]),
            "workspace"
        );
    }
}
