use gpui::prelude::*;
use gpui::*;
use std::path::Path;

use crate::app::project_panel::inline_input::{InlineInput, InlineInputKind};
use crate::app::project_panel::{FileTreeEntry, ProjectPanel};
use crate::theme::Theme;
use crate::ui::{FontSize, Icon, IconName, IconSize, Radius, Spacing};

/// Calculates the index in `entries` where a new inline input row should be rendered.
pub fn calculate_new_entry_insert_index(
    entries: &[FileTreeEntry],
    root_dir: Option<&Path>,
    input: &InlineInput,
) -> Option<usize> {
    match input.kind() {
        InlineInputKind::NewFolder { parent } => {
            if root_dir == Some(parent) {
                Some(0)
            } else if let Some(pos) = entries.iter().position(|e| e.path == *parent) {
                Some(pos + 1)
            } else {
                Some(0)
            }
        }
        InlineInputKind::NewFile { parent } => {
            // Find the last direct directory child of parent in entries
            let last_dir = entries
                .iter()
                .enumerate()
                .filter(|(_, e)| e.is_dir() && e.path.parent() == Some(parent.as_path()))
                .next_back();

            if let Some((dir_idx, dir_entry)) = last_dir {
                // Advance past the directory and all of its expanded subtree
                let mut after_idx = dir_idx + 1;
                while after_idx < entries.len()
                    && entries[after_idx].path.starts_with(&dir_entry.path)
                {
                    after_idx += 1;
                }
                Some(after_idx)
            } else {
                // If parent has no directory children, insert at top of parent
                if root_dir == Some(parent) {
                    Some(0)
                } else if let Some(pos) = entries.iter().position(|e| e.path == *parent) {
                    Some(pos + 1)
                } else {
                    Some(0)
                }
            }
        }
        InlineInputKind::Rename { .. } => None,
    }
}

/// Renders an inline text input row for file/folder creation or renaming.
pub fn render_inline_row(
    input_entity: &Entity<InlineInput>,
    is_dir: bool,
    is_expanded: bool,
    depth: usize,
    theme: &Theme,
) -> AnyElement {
    let indent = px((depth * 14 + 10) as f32);
    div()
        .id("inline_tree_input_row")
        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
        .on_mouse_down(MouseButton::Right, |_, _, cx| cx.stop_propagation())
        .h(px(22.0))
        .w_full()
        .occlude()
        .pl(indent)
        .pr(Spacing::SM)
        .flex()
        .flex_row()
        .items_center()
        .gap(px(5.0))
        .rounded(Radius::XS)
        .bg(theme.btn_active)
        .child(if is_dir {
            let folder_icon = if is_expanded {
                IconName::FolderOpen
            } else {
                IconName::Folder
            };
            Icon::new(folder_icon)
                .size(IconSize::XSmall)
                .color(theme.text_accent)
        } else {
            Icon::new(IconName::FileText)
                .size(IconSize::XSmall)
                .color(theme.text_muted)
        })
        .child(input_entity.clone())
        .into_any_element()
}

/// Everything `build_tree_items` needs to read off the panel.
///
/// Bundled because the call site would otherwise thread six borrowed fields through, and every
/// new one would change the signature again. Every row still talks back to the panel through
/// `panel`, since an element cannot borrow the entity it is rendered from.
pub struct TreeItemList<'a> {
    pub entries: &'a [FileTreeEntry],
    pub inline_input: Option<&'a Entity<InlineInput>>,
    pub selected_path: Option<&'a Path>,
    pub active_file: Option<&'a Path>,
    pub root_dir: Option<&'a Path>,
    pub panel: &'a WeakEntity<ProjectPanel>,
}

/// Builds the visual elements for file tree items, including active inline inputs.
pub fn build_tree_items(list: TreeItemList<'_>, theme: &Theme, cx: &App) -> Vec<AnyElement> {
    let TreeItemList {
        entries,
        inline_input,
        selected_path,
        active_file,
        root_dir,
        panel: panel_weak,
    } = list;

    let new_entry_insert_index = inline_input
        .and_then(|input| calculate_new_entry_insert_index(entries, root_dir, input.read(cx)));

    let mut item_elements = Vec::with_capacity(entries.len() + 1);

    for (entry_idx, entry) in entries.iter().enumerate() {
        if new_entry_insert_index == Some(entry_idx)
            && let Some(input) = inline_input
        {
            let input_ref = input.read(cx);
            let is_dir = matches!(input_ref.kind(), InlineInputKind::NewFolder { .. });
            item_elements.push(render_inline_row(
                input,
                is_dir,
                false,
                input_ref.depth(),
                theme,
            ));
        }

        if let Some(input) = inline_input {
            let input_ref = input.read(cx);
            if let InlineInputKind::Rename { target, is_dir } = input_ref.kind()
                && target == &entry.path
            {
                item_elements.push(render_inline_row(
                    input,
                    *is_dir,
                    entry.is_expanded(),
                    input_ref.depth(),
                    theme,
                ));
                continue;
            }
        }

        let is_active = !entry.is_dir() && active_file == Some(&entry.path);
        let is_selected = selected_path == Some(&entry.path);
        let panel_click = panel_weak.clone();
        let panel_right_click = panel_weak.clone();
        let entry_path = entry.path.clone();
        let is_dir = entry.is_dir();
        let indent = px((entry.depth * 14 + 10) as f32);

        let bg_color = if is_selected {
            theme.btn_active
        } else {
            gpui::transparent_black()
        };

        let text_color = if is_selected {
            theme.text_primary
        } else if is_active {
            theme.text_accent
        } else if is_dir {
            theme.text_secondary
        } else {
            theme.text_muted
        };

        let row_path = entry_path.clone();
        let row =
            div()
                .id(ElementId::NamedInteger(
                    "file_tree_entry".into(),
                    entry_idx as u64,
                ))
                .h(px(22.0))
                .w_full()
                .pl(indent)
                .pr(Spacing::SM)
                .flex()
                .flex_row()
                .items_center()
                .gap(px(5.0))
                .overflow_hidden()
                .bg(bg_color)
                .occlude()
                .hover(|s| s.bg(theme.btn_hover))
                .cursor_pointer()
                .on_click(move |_, window, cx| {
                    if let Some(panel) = panel_click.upgrade() {
                        panel.update(cx, |this, cx| {
                            this.select_and_activate_entry(entry_path.clone(), is_dir, window, cx);
                        });
                    }
                })
                .on_mouse_down(MouseButton::Right, {
                    let entry_path = row_path.clone();
                    move |event, window, cx| {
                        cx.stop_propagation();
                        if let Some(panel) = panel_right_click.upgrade() {
                            panel.update(cx, |this, cx| {
                                this.open_entry_context_menu(
                                    entry_path.clone(),
                                    is_dir,
                                    event.position,
                                    window,
                                    cx,
                                );
                            });
                        }
                    }
                })
                .when(is_dir, |this| {
                    let folder_icon = if entry.is_expanded() {
                        IconName::FolderOpen
                    } else {
                        IconName::Folder
                    };
                    this.child(Icon::new(folder_icon).size(IconSize::XSmall).color(
                        if is_selected {
                            theme.text_primary
                        } else {
                            theme.text_accent
                        },
                    ))
                })
                .when(!is_dir, |this| {
                    this.child(Icon::new(IconName::FileText).size(IconSize::XSmall).color(
                        if is_selected {
                            theme.text_primary
                        } else if is_active {
                            theme.text_accent
                        } else {
                            theme.text_muted
                        },
                    ))
                })
                .child(
                    div()
                        .text_size(FontSize::SM)
                        .text_color(text_color)
                        .whitespace_nowrap()
                        .flex_1()
                        .overflow_hidden()
                        .child(entry.name.clone()),
                );

        item_elements.push(row.into_any_element());
    }

    if new_entry_insert_index == Some(entries.len())
        && let Some(input) = inline_input
    {
        let input_ref = input.read(cx);
        let is_dir = matches!(input_ref.kind(), InlineInputKind::NewFolder { .. });
        item_elements.push(render_inline_row(
            input,
            is_dir,
            false,
            input_ref.depth(),
            theme,
        ));
    }

    item_elements
}

/// Renders empty state placeholders ("No folder opened", "Empty folder", or initial creation row).
pub fn render_empty_state(
    has_root: bool,
    inline_input: Option<&Entity<InlineInput>>,
    theme: &Theme,
    cx: &App,
) -> AnyElement {
    if !has_root {
        div()
            .w_full()
            .h_full()
            .flex()
            .flex_col()
            .items_center()
            .justify_center()
            .child(
                div()
                    .text_size(FontSize::SM)
                    .text_color(theme.text_muted)
                    .child("No folder opened"),
            )
            .into_any_element()
    } else if let Some(input) = inline_input {
        let input_ref = input.read(cx);
        let is_dir = matches!(input_ref.kind(), InlineInputKind::NewFolder { .. });
        let depth = input_ref.depth();
        div()
            .flex_1()
            .w_full()
            .py(Spacing::XS)
            .child(render_inline_row(input, is_dir, false, depth, theme))
            .into_any_element()
    } else {
        div()
            .w_full()
            .h_full()
            .flex()
            .flex_col()
            .items_center()
            .justify_center()
            .child(
                div()
                    .text_size(FontSize::SM)
                    .text_color(theme.text_muted)
                    .child("Empty folder"),
            )
            .into_any_element()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    // Load-bearing: `use super::*` also brings in `gpui::test`, so this explicit import
    // shadows it and keeps bare `#[test]` resolving to the built-in test attribute.
    use core::prelude::v1::test;
    use gpui::{AppContext, TestAppContext};
    use std::path::PathBuf;

    fn entry(
        root: &Path,
        rel: &str,
        is_dir: bool,
        depth: usize,
        is_expanded: bool,
    ) -> FileTreeEntry {
        let path = root.join(rel);
        let name = path.file_name().unwrap().to_string_lossy().into_owned();
        if is_dir {
            FileTreeEntry::new_dir(path, name, depth, is_expanded)
        } else {
            FileTreeEntry::new_file(path, name, depth)
        }
    }

    #[gpui::test]
    fn test_new_entry_insert_index(cx: &mut TestAppContext) {
        let root = PathBuf::from("/ws");
        // /ws
        // ├── src/            (expanded)
        // │   ├── app.rs
        // │   └── lib.rs
        // ├── docs/           (collapsed)
        // │   └── intro.md
        // └── README.md
        let entries = vec![
            entry(&root, "src", true, 0, true),
            entry(&root, "src/app.rs", false, 1, false),
            entry(&root, "src/lib.rs", false, 1, false),
            entry(&root, "docs", true, 0, false),
            entry(&root, "docs/intro.md", false, 1, false),
            entry(&root, "README.md", false, 0, false),
        ];

        cx.update(|cx| {
            let root_dir = Some(root.as_path());

            // NewFolder at the workspace root renders above everything else.
            let folder_at_root = cx.new(|cx| {
                InlineInput::new(
                    InlineInputKind::NewFolder {
                        parent: root.clone(),
                    },
                    "",
                    0,
                    cx,
                )
            });
            assert_eq!(
                calculate_new_entry_insert_index(&entries, root_dir, folder_at_root.read(cx)),
                Some(0)
            );

            // NewFolder inside "src" renders directly below that directory row.
            let folder_in_src = cx.new(|cx| {
                InlineInput::new(
                    InlineInputKind::NewFolder {
                        parent: root.join("src"),
                    },
                    "",
                    1,
                    cx,
                )
            });
            assert_eq!(
                calculate_new_entry_insert_index(&entries, root_dir, folder_in_src.read(cx)),
                Some(1)
            );

            // NewFile at the workspace root renders below the whole last directory subtree
            // ("docs" + "docs/intro.md"), i.e. directly above README.md.
            let file_at_root = cx.new(|cx| {
                InlineInput::new(
                    InlineInputKind::NewFile {
                        parent: root.clone(),
                    },
                    "",
                    0,
                    cx,
                )
            });
            assert_eq!(
                calculate_new_entry_insert_index(&entries, root_dir, file_at_root.read(cx)),
                Some(5)
            );

            // NewFile inside a directory that has no sub-directories follows that directory row.
            let file_in_src = cx.new(|cx| {
                InlineInput::new(
                    InlineInputKind::NewFile {
                        parent: root.join("src"),
                    },
                    "",
                    1,
                    cx,
                )
            });
            assert_eq!(
                calculate_new_entry_insert_index(&entries, root_dir, file_in_src.read(cx)),
                Some(1)
            );

            // Rename never renders an extra inline row.
            let rename = cx.new(|cx| {
                InlineInput::new(
                    InlineInputKind::Rename {
                        target: root.join("README.md"),
                        is_dir: false,
                    },
                    "README.md",
                    0,
                    cx,
                )
            });
            assert_eq!(
                calculate_new_entry_insert_index(&entries, root_dir, rename.read(cx)),
                None
            );
        });
    }
}
