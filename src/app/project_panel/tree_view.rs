use gpui::prelude::*;
use gpui::*;
use std::{ops::Range, path::Path};

use crate::app::project_panel::inline_input::{InlineInput, InlineInputKind};
use crate::app::project_panel::{DraggedProjectEntry, FileTreeEntry, ProjectPanel};
use crate::theme::Theme;
use crate::ui::dock_panel;
use crate::ui::{FontSize, Icon, IconName, IconSize, Radius, Spacing};

/// Calculates the index in `entries` where a new inline input row should be rendered.
pub fn calculate_new_entry_insert_index(
    entries: &[FileTreeEntry],
    root_dir: Option<&Path>,
    input: &InlineInput,
) -> Option<usize> {
    calculate_new_entry_insert_index_for_list(entries, None, root_dir, input)
}

fn calculate_new_entry_insert_index_for_list(
    entries: &[FileTreeEntry],
    visible_indices: Option<&[usize]>,
    root_dir: Option<&Path>,
    input: &InlineInput,
) -> Option<usize> {
    let entry_count = visible_indices.map_or(entries.len(), <[usize]>::len);
    let entry_at = |display_index: usize| {
        let entry_index = visible_indices.map_or(display_index, |indices| indices[display_index]);
        &entries[entry_index]
    };

    match input.kind() {
        InlineInputKind::NewFolder { parent } => {
            if root_dir == Some(parent) {
                Some(0)
            } else if let Some(pos) =
                (0..entry_count).find(|&index| entry_at(index).path == *parent)
            {
                Some(pos + 1)
            } else {
                Some(0)
            }
        }
        InlineInputKind::NewFile { parent } => {
            // Find the last direct directory child of parent in entries
            let last_dir = (0..entry_count)
                .rev()
                .find(|&index| {
                    let entry = entry_at(index);
                    entry.is_dir() && entry.path.parent() == Some(parent.as_path())
                })
                .map(|index| (index, entry_at(index)));

            if let Some((dir_idx, dir_entry)) = last_dir {
                // Advance past the directory and all of its expanded subtree
                let mut after_idx = dir_idx + 1;
                while after_idx < entry_count
                    && entry_at(after_idx).path.starts_with(&dir_entry.path)
                {
                    after_idx += 1;
                }
                Some(after_idx)
            } else {
                // If parent has no directory children, insert at top of parent
                if root_dir == Some(parent) {
                    Some(0)
                } else if let Some(pos) =
                    (0..entry_count).find(|&index| entry_at(index).path == *parent)
                {
                    Some(pos + 1)
                } else {
                    Some(0)
                }
            }
        }
        InlineInputKind::Rename { .. } => None,
    }
}

/// Renders the leading folder/document icon in standard mode (hidden in collocated mode).
pub(super) fn render_entry_leading_icon(
    is_dir: bool,
    is_expanded: bool,
    collocated_mode: bool,
    color: Hsla,
) -> Option<Icon> {
    if collocated_mode {
        return None;
    }
    let icon = if is_dir {
        if is_expanded {
            IconName::FolderOpen
        } else {
            IconName::Folder
        }
    } else {
        IconName::FileText
    };
    Some(Icon::new(icon).size(IconSize::XSmall).color(color))
}

/// Renders the trailing collapse/expand chevron for directories in collocated mode.
pub(super) fn render_entry_trailing_chevron(
    is_dir: bool,
    is_expanded: bool,
    collocated_mode: bool,
    color: Hsla,
) -> Option<Icon> {
    if !collocated_mode || !is_dir {
        return None;
    }
    let chevron = if is_expanded {
        IconName::ChevronDown
    } else {
        IconName::ChevronLeft
    };
    Some(Icon::new(chevron).size(IconSize::XSmall).color(color))
}

/// Renders an inline text input row for file/folder creation or renaming.
pub fn render_inline_row(
    input_entity: &Entity<InlineInput>,
    is_expanded: bool,
    collocated_mode: bool,
    theme: &Theme,
    cx: &App,
) -> AnyElement {
    let input_ref = input_entity.read(cx);
    let is_dir = matches!(
        input_ref.kind(),
        InlineInputKind::NewFolder { .. } | InlineInputKind::Rename { is_dir: true, .. }
    );
    let indent = px((input_ref.depth() * 14 + 10) as f32);
    let icon_color = if is_dir {
        theme.text_accent
    } else {
        theme.text_muted
    };
    div()
        .id("inline_tree_input_row")
        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
        .on_mouse_down(MouseButton::Right, |_, _, cx| cx.stop_propagation())
        .h(px(26.0))
        .w_full()
        .block_mouse_except_scroll()
        .pl(indent)
        .pr(Spacing::SM)
        .flex()
        .flex_row()
        .items_center()
        .gap(px(5.0))
        .rounded(Radius::LG)
        .bg(theme.btn_active)
        .children(render_entry_leading_icon(
            is_dir,
            is_expanded,
            collocated_mode,
            icon_color,
        ))
        .child(
            div()
                .flex_1()
                .min_w_0()
                .flex()
                .items_center()
                .child(input_entity.clone()),
        )
        .children(render_entry_trailing_chevron(
            is_dir,
            is_expanded,
            collocated_mode,
            theme.text_muted,
        ))
        .into_any_element()
}

/// Everything `build_tree_items` needs to read off the panel.
///
/// Bundled because the call site would otherwise thread six borrowed fields through, and every
/// new one would change the signature again. Every row still talks back to the panel through
/// `panel`, since an element cannot borrow the entity it is rendered from.
pub struct TreeItemList<'a> {
    pub entries: &'a [FileTreeEntry],
    pub visible_indices: Option<&'a [usize]>,
    pub inline_input: Option<&'a Entity<InlineInput>>,
    pub selected_path: Option<&'a Path>,
    pub active_file: Option<&'a Path>,
    pub root_dir: Option<&'a Path>,
    pub collocated_mode: bool,
    pub panel: &'a WeakEntity<ProjectPanel>,
}

/// Builds the visual elements for file tree items, including active inline inputs.
pub fn build_tree_items(
    list: TreeItemList<'_>,
    range: Range<usize>,
    theme: &Theme,
    cx: &App,
) -> Vec<AnyElement> {
    let TreeItemList {
        entries,
        visible_indices,
        inline_input,
        selected_path,
        active_file,
        root_dir,
        collocated_mode,
        panel: panel_weak,
    } = list;

    let new_entry_insert_index = inline_input.and_then(|input| {
        calculate_new_entry_insert_index_for_list(
            entries,
            visible_indices,
            root_dir,
            input.read(cx),
        )
    });
    let entry_count = visible_indices.map_or(entries.len(), <[usize]>::len);
    let item_count = entry_count + usize::from(new_entry_insert_index.is_some());

    let mut item_elements = Vec::with_capacity(range.len());

    for row_idx in range {
        if row_idx >= item_count {
            break;
        }

        if new_entry_insert_index == Some(row_idx)
            && let Some(input) = inline_input
        {
            item_elements.push(render_inline_row(input, false, collocated_mode, theme, cx));
            continue;
        }

        let display_idx = if new_entry_insert_index.is_some_and(|insert_at| row_idx > insert_at) {
            row_idx - 1
        } else {
            row_idx
        };
        let entry_idx = visible_indices.map_or(display_idx, |indices| indices[display_idx]);
        let entry = &entries[entry_idx];

        if let Some(input) = inline_input {
            let input_ref = input.read(cx);
            if let InlineInputKind::Rename { target, .. } = input_ref.kind()
                && target == &entry.path
            {
                item_elements.push(render_inline_row(
                    input,
                    entry.is_expanded(),
                    collocated_mode,
                    theme,
                    cx,
                ));
                continue;
            }
        }

        let is_active = entry.matches_active_file(active_file);
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
        let target_dir = super::directory_of(&row_path, is_dir);
        let drag_entry = DraggedProjectEntry {
            path: row_path.clone(),
            name: entry.name.clone(),
            is_dir,
            collocated_mode,
        };
        let row = dock_panel::row(indent, theme)
            .id(ElementId::NamedInteger(
                "file_tree_entry".into(),
                entry_idx as u64,
            ))
            .bg(bg_color)
            .block_mouse_except_scroll()
            .on_drag(drag_entry, |dragged, _offset, _window, cx| {
                cx.new(|_cx| dragged.clone())
            })
            .drag_over::<DraggedProjectEntry>({
                let target_dir = target_dir.clone();
                let drag_bg = theme.selection;
                move |style, dragged, _window, _cx| {
                    if super::fs_ops::can_move_entry(&dragged.path, &target_dir) {
                        style.bg(drag_bg)
                    } else {
                        style
                    }
                }
            })
            .when(is_dir, |this| {
                let panel_drag_move = panel_weak.clone();
                let dir_path = row_path.clone();
                this.on_drag_move::<DraggedProjectEntry>(move |event, _window, cx| {
                    let hovered = event.bounds.contains(&event.event.position);
                    let dragged = event.drag(cx);
                    let can_expand =
                        hovered && dragged.path != dir_path && !dir_path.starts_with(&dragged.path);
                    if let Some(panel) = panel_drag_move.upgrade() {
                        panel.update(cx, |this, cx| {
                            let viewport = this.scroll_handle().bounds();
                            let in_viewport = viewport.size.height <= px(0.0)
                                || viewport.contains(&event.event.position);
                            if can_expand && in_viewport {
                                this.update_drag_hover_dir(Some(dir_path.clone()), cx);
                            } else if this.drag_hover_dir.as_deref() == Some(dir_path.as_path()) {
                                this.update_drag_hover_dir(None, cx);
                            }
                        });
                    }
                })
            })
            .on_drop({
                let panel_drop = panel_weak.clone();
                let target_dir = target_dir.clone();
                move |dragged: &DraggedProjectEntry, _window, cx| {
                    if let Some(panel) = panel_drop.upgrade() {
                        panel.update(cx, |this, cx| {
                            this.move_entry_to(&dragged.path, &target_dir, cx);
                        });
                    }
                }
            })
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
            .children(render_entry_leading_icon(
                is_dir,
                entry.is_expanded(),
                collocated_mode,
                if is_selected {
                    theme.text_primary
                } else if is_dir || is_active {
                    theme.text_accent
                } else {
                    theme.text_muted
                },
            ))
            .child(
                div()
                    .text_size(FontSize::SM)
                    .text_color(text_color)
                    .whitespace_nowrap()
                    .flex_1()
                    .min_w_0()
                    .overflow_hidden()
                    .child(entry.name.clone()),
            )
            .children(render_entry_trailing_chevron(
                is_dir,
                entry.is_expanded(),
                collocated_mode,
                if is_selected {
                    theme.text_primary
                } else {
                    theme.text_muted
                },
            ));

        item_elements.push(row.into_any_element());
    }

    item_elements
}

/// Renders empty state placeholders ("No folder opened", "Empty folder"/"No documents", or initial creation row).
pub fn render_empty_state(
    has_root: bool,
    inline_input: Option<&Entity<InlineInput>>,
    collocated_mode: bool,
    theme: &Theme,
    cx: &App,
) -> AnyElement {
    if !has_root {
        dock_panel::empty_state("No folder opened", theme).into_any_element()
    } else if let Some(input) = inline_input {
        div()
            .flex_1()
            .w_full()
            .py(Spacing::XS)
            .child(render_inline_row(input, false, collocated_mode, theme, cx))
            .into_any_element()
    } else {
        let message = if collocated_mode {
            "No documents"
        } else {
            "Empty folder"
        };
        dock_panel::empty_state(message, theme).into_any_element()
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
