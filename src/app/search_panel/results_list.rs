use std::path::Path;

use gpui::prelude::*;
use gpui::*;

use super::{ResultRow, SearchPanel};
use crate::app::actions::OpenWorkspaceMatch;
use crate::buffer::Position;
use crate::search::{FileSearchResult, WorkspaceMatch};
use crate::theme::{Theme, ThemeManager};
use crate::ui::icon::{Icon, IconName, IconSize};
use crate::ui::tokens::{FontSize, Radius, Spacing};

const ROW_HEIGHT: Pixels = px(22.0);
const MATCH_INDENT: Pixels = px(20.0);
const LINE_NUMBER_WIDTH: Pixels = px(32.0);

impl SearchPanel {
    pub(super) fn render_results_list(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let panel = cx.entity().downgrade();
        uniform_list(
            "search_results",
            self.rows.len(),
            move |range, _window, cx| {
                let Some(panel) = panel.upgrade() else {
                    return Vec::new();
                };
                let theme = cx.global::<ThemeManager>().theme();
                let this = panel.read(cx);
                range
                    .filter_map(|row_idx| {
                        let element = match *this.rows.get(row_idx)? {
                            ResultRow::FileHeader(file_idx) => {
                                let file = this.results.files().get(file_idx)?;
                                let is_collapsed = this.collapsed_files.contains(&file.path);
                                render_file_header(row_idx, file, is_collapsed, &panel, theme)
                                    .into_any_element()
                            }
                            ResultRow::Match {
                                file_idx,
                                match_idx,
                            } => {
                                let file = this.results.files().get(file_idx)?;
                                let m = file.matches.get(match_idx)?;
                                render_match_row(
                                    row_idx,
                                    &file.path,
                                    m,
                                    this.replacing,
                                    this.show_replace,
                                    &panel,
                                    theme,
                                )
                                .into_any_element()
                            }
                        };
                        Some(element)
                    })
                    .collect()
            },
        )
        .track_scroll(self.scroll_handle.clone())
        .flex_1()
        .w_full()
        .py(Spacing::XS)
    }
}

pub(super) fn render_empty_state(message: &'static str, theme: &Theme) -> impl IntoElement {
    div()
        .flex_1()
        .w_full()
        .flex()
        .items_center()
        .justify_center()
        .p(Spacing::LG)
        .child(
            div()
                .text_size(FontSize::CAPTION)
                .text_color(theme.text_muted)
                .child(message),
        )
}

fn render_file_header(
    row_idx: usize,
    file: &FileSearchResult,
    is_collapsed: bool,
    panel: &Entity<SearchPanel>,
    theme: &Theme,
) -> impl IntoElement {
    let file_name = file
        .path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| file.relative_path.clone());
    let dir_path = Path::new(&file.relative_path)
        .parent()
        .and_then(|p| p.to_str())
        .filter(|s| !s.is_empty())
        .map(|s| format!("{s}/"));
    let match_count = file.matches.len();
    let (hover, active) = (theme.btn_hover, theme.btn_active);
    let panel = panel.downgrade();
    let path = file.path.clone();

    div()
        .id(("search_file", row_idx))
        .w_full()
        .h(ROW_HEIGHT)
        .px(Spacing::SM)
        .flex()
        .items_center()
        .justify_between()
        .cursor_pointer()
        .rounded(Radius::XS)
        .hover(move |s| s.bg(hover))
        .active(move |s| s.bg(active))
        .on_click(move |_, _, cx| {
            if let Some(panel) = panel.upgrade() {
                panel.update(cx, |this, cx| this.toggle_file_collapsed(&path, cx));
            }
        })
        .child(
            div()
                .flex()
                .items_center()
                .gap(px(4.0))
                .min_w_0()
                .flex_1()
                .child(
                    Icon::new(if is_collapsed {
                        IconName::ChevronRight
                    } else {
                        IconName::ChevronDown
                    })
                    .size(IconSize::XSmall)
                    .color(theme.text_secondary),
                )
                .child(
                    Icon::new(IconName::FileText)
                        .size(IconSize::XSmall)
                        .color(theme.text_secondary),
                )
                .child(
                    div()
                        .text_size(FontSize::CAPTION)
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(theme.text_primary)
                        .child(file_name),
                )
                .when_some(dir_path, |this, dir| {
                    this.child(
                        div()
                            .text_size(FontSize::CAPTION)
                            .text_color(theme.text_muted)
                            .overflow_hidden()
                            .whitespace_nowrap()
                            .child(dir),
                    )
                }),
        )
        .child(
            div()
                .text_size(FontSize::CAPTION)
                .text_color(theme.text_muted)
                .child(match_count.to_string()),
        )
}

fn render_match_row(
    row_idx: usize,
    path: &Path,
    m: &WorkspaceMatch,
    replacing: bool,
    show_replace: bool,
    panel: &Entity<SearchPanel>,
    theme: &Theme,
) -> impl IntoElement {
    let (pre, matched, post) = split_preview(m);
    let action = OpenWorkspaceMatch {
        path: path.to_path_buf(),
        position: workspace_match_position(m),
    };
    let (hover, active) = (theme.btn_hover, theme.btn_active);
    let panel = panel.downgrade();
    let replace_path = path.to_path_buf();
    let replace_match = m.clone();

    div()
        .id(("search_match", row_idx))
        .w_full()
        .h(ROW_HEIGHT)
        .pl(MATCH_INDENT)
        .pr(Spacing::SM)
        .flex()
        .items_center()
        .cursor_pointer()
        .rounded(Radius::XS)
        .hover(move |s| s.bg(hover))
        .active(move |s| s.bg(active))
        .on_click(move |_, window, cx| window.dispatch_action(action.boxed_clone(), cx))
        .child(
            div()
                .w(LINE_NUMBER_WIDTH)
                .flex_none()
                .text_size(FontSize::CAPTION)
                .text_color(theme.text_muted)
                .child(format!("{}:", m.line_number)),
        )
        .child(
            div()
                .flex_1()
                .min_w_0()
                .overflow_hidden()
                .whitespace_nowrap()
                .text_size(FontSize::CAPTION)
                .flex()
                .items_center()
                .child(
                    div()
                        .text_color(theme.text_secondary)
                        .child(pre.to_string()),
                )
                .child(
                    div()
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(theme.text_accent)
                        .bg(theme.search_match)
                        .child(matched.to_string()),
                )
                .child(
                    div()
                        .text_color(theme.text_secondary)
                        .child(post.to_string()),
                ),
        )
        .when(show_replace, |this| {
            this.child(
                div()
                    .id(("search_replace_match_btn", row_idx))
                    .h(ROW_HEIGHT)
                    .px(Spacing::XS)
                    .flex_none()
                    .flex()
                    .items_center()
                    .text_size(FontSize::CAPTION)
                    .text_color(if replacing {
                        theme.text_muted.opacity(0.5)
                    } else {
                        theme.text_secondary
                    })
                    .when(!replacing, |this| {
                        this.cursor_pointer().hover(move |s| s.bg(hover)).on_click(
                            move |_, _, cx| {
                                cx.stop_propagation();
                                if let Some(panel) = panel.upgrade() {
                                    panel.update(cx, |this, cx| {
                                        this.replace_match(
                                            replace_path.clone(),
                                            replace_match.clone(),
                                            cx,
                                        );
                                    });
                                }
                            },
                        )
                    })
                    .child("Replace"),
            )
        })
}

/// Returns the cursor position immediately after a workspace-search match.
fn workspace_match_position(m: &WorkspaceMatch) -> Position {
    Position::new(m.line_number.saturating_sub(1), m.char_range.1)
}

/// Splits the matched line around the hit, dropping the leading indentation.
fn split_preview(m: &WorkspaceMatch) -> (&str, &str, &str) {
    let line = &*m.line_content;
    let trimmed = line.trim_start();
    let indent = line.len() - trimmed.len();
    let start = m.byte_range.0.saturating_sub(indent);
    let end = m.byte_range.1.saturating_sub(indent);
    match (
        trimmed.get(..start),
        trimmed.get(start..end),
        trimmed.get(end..),
    ) {
        (Some(pre), Some(matched), Some(post)) => (pre, matched, post),
        _ => (trimmed, "", ""),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    // Load-bearing: the parent module's glob also brings in `gpui::test`, so this explicit
    // import shadows it and keeps bare `#[test]` resolving to the built-in test attribute.
    use core::prelude::v1::test;
    use std::sync::Arc;

    fn hit(line: &str, byte_range: (usize, usize)) -> WorkspaceMatch {
        WorkspaceMatch {
            line_number: 1,
            line_content: Arc::from(line),
            byte_range,
            char_range: byte_range,
        }
    }

    #[test]
    fn workspace_match_position_places_cursor_at_match_end() {
        let m = hit("prefix needle suffix", (7, 13));
        assert_eq!(workspace_match_position(&m), Position::new(0, 13));
    }

    #[test]
    fn preview_drops_indentation_around_the_match() {
        assert_eq!(
            split_preview(&hit("    let apple = 42;", (8, 13))),
            ("let ", "apple", " = 42;")
        );
    }

    #[test]
    fn preview_keeps_a_match_that_starts_in_the_indentation() {
        assert_eq!(split_preview(&hit("  ab", (0, 3))), ("", "a", "b"));
    }
}
