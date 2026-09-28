use gpui::prelude::FluentBuilder;
use gpui::*;

use crate::markdown::{BlockKind, ParsedLine, is_syntactic_separator, table};
use crate::theme::Theme;
use crate::ui::{Icon, IconName, IconSize};

use super::Editor;
use super::LineRenderContext;
use super::shaping::render_shaped_line;

/// Renders a line in Live Preview mode.
pub fn render_live_line_content(
    line_idx: usize,
    parsed: &ParsedLine,
    lines: &[String],
    lrc: &LineRenderContext,
    editor: Option<&Entity<Editor>>,
) -> AnyElement {
    let theme = lrc.theme;
    let is_active = lrc.is_active_line;
    let is_focused = lrc.is_focused;
    let selection = lrc.selection;
    let content_width = lrc.content_width;

    match &parsed.kind {
        BlockKind::Heading { .. } => {
            let typo = crate::editor::layout::get_block_typography_with_metrics(
                &parsed.kind,
                lrc.font_size,
                lrc.line_height,
            );
            let color = crate::editor::layout::get_block_color_override(&parsed.kind, theme);

            let text_canvas = render_shaped_line(
                line_idx,
                parsed,
                lrc,
                typo.font_size,
                typo.line_height,
                color,
                false,
            );

            div()
                .w_full()
                .pt(px(14.0))
                .pb(px(4.0))
                .child(text_canvas)
                .into_any_element()
        }

        BlockKind::CodeBlock { is_fence_start } => {
            let typo = crate::editor::layout::get_block_typography_with_metrics(
                &parsed.kind,
                lrc.font_size,
                lrc.line_height,
            );
            if *is_fence_start {
                let lang_canvas = render_shaped_line(
                    line_idx,
                    parsed,
                    lrc,
                    typo.font_size,
                    typo.line_height,
                    Some(theme.code_block_header_fg),
                    false,
                );

                let raw_lang = if parsed.raw_text.len() >= 3 {
                    parsed.raw_text[3..].trim()
                } else {
                    ""
                };

                div()
                    .w_full()
                    .mt(px(6.0))
                    .bg(theme.code_block_header_bg)
                    .border_t_1()
                    .border_l_1()
                    .border_r_1()
                    .border_color(theme.code_block_border)
                    .rounded_t(px(6.0))
                    .px(px(14.0))
                    .py(px(4.0))
                    .flex()
                    .justify_between()
                    .items_center()
                    .child(div().flex().items_center().gap(px(6.0)).child(
                        if raw_lang.is_empty() && !(is_active && is_focused) {
                            div()
                                .text_size(px((lrc.font_size * (12.0 / 15.0)).round()))
                                .font_family(crate::platform::platform_monospace_font())
                                .text_color(theme.text_muted)
                                .child("plaintext")
                                .into_any_element()
                        } else {
                            div().min_w(px(50.0)).child(lang_canvas).into_any_element()
                        },
                    ))
                    .into_any_element()
            } else {
                div()
                    .w_full()
                    .h(px(6.0))
                    .mb(px(6.0))
                    .bg(theme.code_block_bg)
                    .border_b_1()
                    .border_l_1()
                    .border_r_1()
                    .border_color(theme.code_block_border)
                    .rounded_b(px(6.0))
                    .into_any_element()
            }
        }

        BlockKind::CodeBlockContent => {
            let typo = crate::editor::layout::get_block_typography_with_metrics(
                &parsed.kind,
                lrc.font_size,
                lrc.line_height,
            );
            let text_canvas = render_shaped_line(
                line_idx,
                parsed,
                lrc,
                typo.font_size,
                typo.line_height,
                Some(theme.code_block_text),
                false,
            );

            div()
                .w_full()
                .bg(theme.code_block_bg)
                .border_l_1()
                .border_r_1()
                .border_color(theme.code_block_border)
                .px(px(14.0))
                .py(px(1.0))
                .child(text_canvas)
                .into_any_element()
        }

        BlockKind::ThematicBreak => div()
            .w_full()
            .h(px(32.0))
            .flex()
            .flex_col()
            .justify_center()
            .child(
                div()
                    .w_full()
                    .h(px(1.5))
                    .rounded(px(1.0))
                    .bg(if is_active && is_focused {
                        theme.text_accent
                    } else {
                        theme.thematic_break
                    }),
            )
            .into_any_element(),

        BlockKind::TaskList { checked } => {
            let is_checked = *checked;
            let typo = crate::editor::layout::get_block_typography_with_metrics(
                &parsed.kind,
                lrc.font_size,
                lrc.line_height,
            );
            let text_canvas = render_shaped_line(
                line_idx,
                parsed,
                lrc,
                typo.font_size,
                typo.line_height,
                if is_checked {
                    Some(theme.text_muted)
                } else {
                    None
                },
                false,
            );

            div()
                .w_full()
                .min_h(typo.line_height)
                .py(px(4.0))
                .flex()
                .flex_row()
                .items_start()
                .child(
                    div()
                        .w(px(24.0))
                        .h(typo.line_height)
                        .flex_none()
                        .flex()
                        .items_center()
                        .justify_start()
                        .child(
                            div()
                                .id(ElementId::NamedInteger(
                                    "task_check".into(),
                                    line_idx as u64,
                                ))
                                .w(px(16.0))
                                .h(px(16.0))
                                .rounded(px(4.0))
                                .border_1()
                                .border_color(if is_checked {
                                    theme.task_box_checked_bg
                                } else {
                                    theme.task_box_border
                                })
                                .bg(if is_checked {
                                    theme.task_box_checked_bg
                                } else {
                                    transparent_black()
                                })
                                .flex()
                                .items_center()
                                .justify_center()
                                .cursor_pointer()
                                .when_some(editor, |this, ed| {
                                    let ed = ed.clone();
                                    this.on_mouse_down(MouseButton::Left, move |_, _window, cx| {
                                        ed.update(cx, |this, cx| {
                                            this.toggle_task_checkbox(line_idx, cx);
                                        });
                                    })
                                })
                                .child(if is_checked {
                                    Icon::new(IconName::Check)
                                        .size(IconSize::Indicator)
                                        .color(theme.task_box_checked_fg)
                                        .into_any_element()
                                } else {
                                    div().into_any_element()
                                }),
                        ),
                )
                .child(div().flex_grow().child(text_canvas))
                .into_any_element()
        }

        BlockKind::BulletList => {
            let typo = crate::editor::layout::get_block_typography_with_metrics(
                &parsed.kind,
                lrc.font_size,
                lrc.line_height,
            );
            let text_canvas = render_shaped_line(
                line_idx,
                parsed,
                lrc,
                typo.font_size,
                typo.line_height,
                None,
                false,
            );

            div()
                .w_full()
                .min_h(typo.line_height)
                .py(px(4.0))
                .flex()
                .flex_row()
                .items_start()
                .child(
                    div()
                        .w(px(22.0))
                        .h(typo.line_height)
                        .flex_none()
                        .flex()
                        .items_center()
                        .justify_start()
                        .child(
                            div()
                                .w(px(5.5))
                                .h(px(5.5))
                                .rounded(px(3.0))
                                .bg(theme.list_marker),
                        ),
                )
                .child(div().flex_grow().child(text_canvas))
                .into_any_element()
        }

        BlockKind::OrderedList { num } => {
            let num_label = format!("{}.", num);
            let typo = crate::editor::layout::get_block_typography_with_metrics(
                &parsed.kind,
                lrc.font_size,
                lrc.line_height,
            );
            let text_canvas = render_shaped_line(
                line_idx,
                parsed,
                lrc,
                typo.font_size,
                typo.line_height,
                None,
                false,
            );

            div()
                .w_full()
                .min_h(typo.line_height)
                .py(px(4.0))
                .flex()
                .flex_row()
                .items_start()
                .child(
                    div()
                        .w(px(24.0))
                        .h(typo.line_height)
                        .flex_none()
                        .flex()
                        .items_center()
                        .justify_start()
                        .text_size(px((lrc.font_size * (14.0 / 15.0)).round()))
                        .text_color(theme.list_marker)
                        .child(num_label),
                )
                .child(div().flex_grow().child(text_canvas))
                .into_any_element()
        }

        BlockKind::BlockQuote => {
            let is_prev_quote = if line_idx > 0 {
                lines[line_idx - 1].trim_start().starts_with("> ")
            } else {
                false
            };
            let is_next_quote = if line_idx + 1 < lines.len() {
                lines[line_idx + 1].trim_start().starts_with("> ")
            } else {
                false
            };

            let typo = crate::editor::layout::get_block_typography_with_metrics(
                &parsed.kind,
                lrc.font_size,
                lrc.line_height,
            );
            let text_canvas = render_shaped_line(
                line_idx,
                parsed,
                lrc,
                typo.font_size,
                typo.line_height,
                Some(theme.quote_text),
                false,
            );

            let mut div_elem = div()
                .w_full()
                .border_l_4()
                .border_color(theme.quote_border)
                .bg(theme.quote_bg)
                .pl(px(14.0))
                .py(px(4.0));

            if !is_prev_quote {
                div_elem = div_elem.mt(px(4.0)).rounded_tr(px(4.0));
            }
            if !is_next_quote {
                div_elem = div_elem.mb(px(4.0)).rounded_br(px(4.0));
            }

            div_elem.child(text_canvas).into_any_element()
        }

        BlockKind::Image { alt, url } => {
            let is_selected_line = if !selection.is_empty() {
                let (start, end) = selection.range();
                line_idx >= start.line && line_idx <= end.line
            } else {
                false
            };

            let should_show_source = is_active || is_selected_line;

            let mut img_lrc = *lrc;
            img_lrc.is_active_line = should_show_source;

            let typo = crate::editor::layout::get_block_typography_with_metrics(
                &parsed.kind,
                lrc.font_size,
                lrc.line_height,
            );
            let text_canvas = render_shaped_line(
                line_idx,
                parsed,
                &img_lrc,
                typo.font_size,
                typo.line_height,
                Some(theme.text_secondary),
                false,
            );

            let preview_card =
                render_image_preview_card(line_idx, alt, url, lrc.doc_path, theme, content_width);

            if should_show_source {
                div()
                    .w_full()
                    .py(px(4.0))
                    .flex()
                    .flex_col()
                    .gap(px(6.0))
                    .child(div().w_full().h(typo.line_height).child(text_canvas))
                    .child(preview_card)
                    .into_any_element()
            } else {
                div()
                    .w_full()
                    .py(px(4.0))
                    .flex()
                    .flex_col()
                    .child(div().h(px(0.0)).overflow_hidden().child(text_canvas))
                    .child(preview_card)
                    .into_any_element()
            }
        }

        BlockKind::Table {
            is_header,
            is_delimiter,
            alignments,
            cells,
        } => {
            if *is_delimiter {
                // Table delimiter row is visually hidden in Live Preview
                return div().h(px(0.0)).into_any_element();
            }

            let active_cell_idx = if is_active && is_focused {
                Some(table::get_cell_index_for_col(cells, selection.head.col))
            } else {
                None
            };

            let num_cells = cells.len();

            let row_h = super::shaping::calculate_table_row_height(
                cells,
                *is_header,
                is_active && is_focused,
                selection.head.col,
                lrc.content_width,
                lrc.text_system,
            );

            let hovered_link_range = lrc.hovered_link.and_then(|hl| {
                if hl.line_idx == line_idx {
                    Some(hl.col_range)
                } else {
                    None
                }
            });

            let mut row_elem = div()
                .relative()
                .w_full()
                .min_h(row_h)
                .h(row_h)
                .border_l_1()
                .border_r_1()
                .border_color(theme.table_border)
                .flex()
                .flex_row();

            if *is_header {
                row_elem = row_elem
                    .mt(px(8.0))
                    .border_t_1()
                    .border_b_2()
                    .bg(theme.table_header_bg)
                    .rounded_t(px(6.0));
            } else {
                let row_bg = if line_idx.is_multiple_of(2) {
                    theme.table_alt_bg
                } else {
                    gpui::transparent_black()
                };

                let is_last_row = line_idx + 1 >= lines.len()
                    || !lines[line_idx + 1].contains('|')
                    || lines[line_idx + 1].trim().is_empty();

                row_elem = row_elem.border_b_1().bg(row_bg);
                if is_last_row {
                    row_elem = row_elem.rounded_b(px(6.0)).mb(px(8.0));
                }
            }

            let (font_size, line_height) = super::shaping::table_cell_font_metrics(*is_header);

            for (c_idx, cell) in cells.iter().enumerate() {
                let align = alignments
                    .get(c_idx)
                    .copied()
                    .unwrap_or(crate::markdown::TableAlignment::None);
                let is_last = c_idx + 1 == num_cells;
                let is_active_cell = active_cell_idx == Some(c_idx);
                let tcx = super::shaping::TableCellRenderContext {
                    theme,
                    font_size,
                    line_height,
                    font: super::shaping::line_font_for(&parsed.kind, false),
                    is_active_cell,
                    // Disclosure must match what `get_line_layout_height` measured this row with;
                    // the caret target stays on the live caret column.
                    disclosure_col: lrc.disclosure_col,
                    cursor_col: selection.head.col,
                    cursor_opacity: lrc.cursor_opacity,
                    align,
                    hovered_link_range,
                };
                row_elem = row_elem.child(render_table_cell(cell, is_last, tcx));
            }

            row_elem.into_any_element()
        }

        BlockKind::Paragraph => {
            if is_syntactic_separator(lines, line_idx) && !is_active {
                // Syntactic blank separator lines between blocks: collapsed to 0 height in Live Preview
                return div().h(px(0.0)).into_any_element();
            }

            let typo = crate::editor::layout::get_block_typography_with_metrics(
                &parsed.kind,
                lrc.font_size,
                lrc.line_height,
            );
            let text_canvas = render_shaped_line(
                line_idx,
                parsed,
                lrc,
                typo.font_size,
                typo.line_height,
                None,
                false,
            );

            div()
                .w_full()
                .min_h(typo.line_height)
                .py(px(4.0))
                .child(text_canvas)
                .into_any_element()
        }
    }
}

fn render_table_cell(
    cell: &crate::markdown::TableCell,
    is_last: bool,
    tcx: super::shaping::TableCellRenderContext,
) -> impl IntoElement {
    let theme = tcx.theme;
    let cell_canvas = super::shaping::render_shaped_cell_canvas(&cell.spans, tcx);

    let mut cell_elem = div()
        .flex_1()
        .min_w(px(60.0))
        .h_full()
        .px(px(8.0))
        .flex()
        .flex_row()
        .items_center();

    if !is_last {
        cell_elem = cell_elem.border_r_1().border_color(theme.table_cell_border);
    }

    cell_elem.child(cell_canvas)
}

pub fn resolve_image_source(url: &str, doc_path: Option<&std::path::Path>) -> ImageSource {
    let clean = url.trim();
    if clean.starts_with("http://") || clean.starts_with("https://") {
        ImageSource::Resource(Resource::Uri(clean.to_string().into()))
    } else if let Some(path) = crate::http::parse_file_url_path(clean, doc_path) {
        ImageSource::Resource(Resource::Path(path.into()))
    } else {
        let path = std::path::PathBuf::from(clean);
        ImageSource::Resource(Resource::Path(path.into()))
    }
}

fn render_image_preview_card(
    line_idx: usize,
    alt: &str,
    url: &str,
    doc_path: Option<&std::path::Path>,
    theme: &Theme,
    content_width: Pixels,
) -> impl IntoElement {
    let alt_string = if alt.is_empty() {
        "Image".to_string()
    } else {
        alt.to_string()
    };
    let image_source = resolve_image_source(url, doc_path);
    let accent_color = theme.text_accent;
    let muted_color = theme.text_muted;
    let is_loaded = crate::http::is_url_loaded(url);
    let is_failed = crate::http::is_url_failed(url);

    let img_element = img(image_source)
        .id(ElementId::NamedInteger(
            "editor_img".into(),
            line_idx as u64,
        ))
        .w(content_width)
        .max_w_full()
        .rounded(px(6.0))
        .with_fallback(move || {
            div()
                .w_full()
                .h(px(40.0))
                .flex()
                .items_center()
                .justify_center()
                .gap(px(6.0))
                .text_size(px(12.0))
                .text_color(muted_color)
                .child(
                    Icon::new(IconName::ImageBroken)
                        .size(IconSize::Small)
                        .color(muted_color),
                )
                .child(format!("Failed to load image: {}", alt_string))
                .into_any_element()
        });

    if is_loaded || is_failed {
        div()
            .w_full()
            .my(px(6.0))
            .flex()
            .flex_col()
            .items_center()
            .child(img_element)
    } else {
        // While downloading over network, immediately show the animated gooey loading SVG
        // and mount the img element in the background to drive GPUI's network loader
        div()
            .w_full()
            .my(px(6.0))
            .flex()
            .flex_col()
            .items_center()
            .child(
                div()
                    .w_full()
                    .h(px(140.0))
                    .flex()
                    .items_center()
                    .justify_center()
                    .child(render_loading_metaball_animation(
                        ElementId::NamedInteger("loading_metaball".into(), line_idx as u64),
                        accent_color,
                        px(48.0),
                    )),
            )
            .child(div().h(px(0.0)).overflow_hidden().child(img_element))
    }
}

/// Renders an animated loading indicator.
pub fn render_loading_metaball_animation(
    id: impl Into<ElementId>,
    color: Hsla,
    size_px: Pixels,
) -> impl IntoElement {
    div()
        .size(size_px)
        .flex()
        .items_center()
        .justify_center()
        .with_animation(
            id,
            Animation::new(std::time::Duration::from_millis(750)).repeat(),
            move |_container, delta| {
                // Sine harmonic oscillation: 0.0 -> 1.0 -> 0.0
                let t = (1.0 - (delta * 2.0 * std::f32::consts::PI).cos()) / 2.0;
                let s = f32::from(size_px) / 24.0;

                let cx1 = (4.0 + 5.0 * t) * s;
                let r1 = (3.0 + 5.0 * t) * s;

                let cx2 = (15.0 + 5.0 * t) * s;
                let r2 = (8.0 - 5.0 * t) * s;

                let cy = 12.0 * s;

                let mut group = div().size(size_px).relative();

                // Circle 1
                group = group.child(
                    div()
                        .absolute()
                        .left(px(cx1 - r1))
                        .top(px(cy - r1))
                        .size(px(r1 * 2.0))
                        .rounded(px(r1))
                        .bg(color),
                );

                // Circle 2
                group = group.child(
                    div()
                        .absolute()
                        .left(px(cx2 - r2))
                        .top(px(cy - r2))
                        .size(px(r2 * 2.0))
                        .rounded(px(r2))
                        .bg(color),
                );

                // Gooey metaball connecting bridge
                let dist = cx2 - cx1;
                let max_connect_dist = (r1 + r2) * 1.35;
                if dist < max_connect_dist && dist > 0.1 {
                    let overlap = 1.0 - (dist / max_connect_dist);
                    let bridge_h = (r1.min(r2) * 1.6) * overlap;
                    group = group.child(
                        div()
                            .absolute()
                            .left(px(cx1))
                            .top(px(cy - bridge_h / 2.0))
                            .w(px(cx2 - cx1))
                            .h(px(bridge_h))
                            .rounded(px(bridge_h / 2.0))
                            .bg(color),
                    );
                }

                group
            },
        )
}

/// Renders a line in Source mode with a line-number gutter.
pub fn render_source_line(
    line_idx: usize,
    parsed: &ParsedLine,
    lrc: &LineRenderContext,
    editor: Option<&Entity<Editor>>,
) -> AnyElement {
    let font_size = px(lrc.font_size);
    let line_height = px((lrc.font_size * lrc.line_height).round());
    let line_num_str = format!("{:>4} ", line_idx + 1);
    let content_element =
        render_shaped_line(line_idx, parsed, lrc, font_size, line_height, None, true);

    div()
        .id(ElementId::NamedInteger("src_line".into(), line_idx as u64))
        .w_full()
        .min_h(line_height)
        .flex()
        .flex_row()
        .items_start()
        .bg(if lrc.is_active_line && lrc.is_focused {
            lrc.theme.line_highlight
        } else {
            transparent_black()
        })
        // Gutter line number ruler
        .child(
            div()
                .w(crate::editor::layout::SOURCE_GUTTER_WIDTH)
                .h(line_height)
                .flex_none()
                .pr(px(12.0))
                .text_color(if lrc.is_active_line && lrc.is_focused {
                    lrc.theme.text_accent
                } else {
                    lrc.theme.text_muted
                })
                .font_weight(if lrc.is_active_line && lrc.is_focused {
                    FontWeight::BOLD
                } else {
                    FontWeight::NORMAL
                })
                .text_size(px((lrc.font_size * (12.5 / 15.0)).round()))
                .font_family(crate::platform::platform_monospace_font())
                .cursor_pointer()
                .when_some(editor, |this, ed| {
                    let ed = ed.clone();
                    this.on_mouse_down(MouseButton::Left, move |_, window, cx| {
                        ed.update(cx, |this, cx| {
                            this.buffer.select_line_at(line_idx);
                            window.focus(&this.focus_handle);
                            cx.notify();
                        });
                    })
                })
                .child(line_num_str),
        )
        // Line content area
        .child(div().flex_grow().w_full().child(content_element))
        .into_any_element()
}
