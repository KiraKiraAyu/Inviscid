use gpui::*;

use crate::markdown::{InlineSpan, ParsedLine};
use crate::theme::Theme;

use super::runs::{
    LineShapingContext, build_line_runs, raw_col_to_visible_byte_with_selection,
    visible_byte_to_raw_col_with_selection,
};
use super::wrap::table_cell_font_metrics_for;

pub fn calculate_pos_from_col(
    col: usize,
    parsed: &ParsedLine,
    font_size: Pixels,
    line_height: Pixels,
    wrap_width: Option<Pixels>,
    scx: LineShapingContext,
    window: &mut Window,
) -> Point<Pixels> {
    if parsed.raw_text.is_empty() {
        return Point::default();
    }

    let (line_str, runs) = build_line_runs(&parsed.spans, scx, None, &window.text_style());

    if line_str.is_empty() || runs.is_empty() {
        return Point::default();
    }

    let target_byte = raw_col_to_visible_byte_with_selection(
        col,
        &parsed.spans,
        scx.is_active_caret,
        scx.cursor_col,
        scx.is_source,
        scx.is_selection_disclosure,
    );

    let shaped_lines =
        window
            .text_system()
            .shape_text(line_str.into(), font_size, &runs, wrap_width, None);

    if let Ok(lines) = shaped_lines {
        let mut y_offset = px(0.0);
        for line in lines.iter() {
            if let Some(pos) = line.position_for_index(target_byte, line_height) {
                return point(pos.x, y_offset + pos.y);
            }
            let num_v = line.wrap_boundaries.len() + 1;
            y_offset += line_height * (num_v as f32);
        }
    }

    Point::default()
}

/// Returns (current_visual_line_idx, total_visual_lines) for a given buffer column on a line
pub fn get_visual_line_info(
    col: usize,
    parsed: &ParsedLine,
    font_size: Pixels,
    line_height: Pixels,
    wrap_width: Option<Pixels>,
    scx: LineShapingContext,
    window: &mut Window,
) -> (usize, usize) {
    if parsed.raw_text.is_empty() {
        return (0, 1);
    }

    let (line_str, runs) = build_line_runs(&parsed.spans, scx, None, &window.text_style());
    if line_str.is_empty() || runs.is_empty() {
        return (0, 1);
    }

    let target_byte = raw_col_to_visible_byte_with_selection(
        col,
        &parsed.spans,
        scx.is_active_caret,
        scx.cursor_col,
        scx.is_source,
        scx.is_selection_disclosure,
    );

    let shaped_lines =
        window
            .text_system()
            .shape_text(line_str.into(), font_size, &runs, wrap_width, None);

    if let Ok(lines) = shaped_lines
        && !lines.is_empty()
    {
        let total_visual_lines = lines
            .iter()
            .map(|l| l.wrap_boundaries.len() + 1)
            .sum::<usize>()
            .max(1);

        let mut visual_offset = 0;
        for line in lines.iter() {
            let num_wrap = line.wrap_boundaries.len() + 1;
            if let Some(pos) = line.position_for_index(target_byte, line_height) {
                let v_idx = if line_height > px(0.0) {
                    ((pos.y / line_height).round() as usize).min(num_wrap.saturating_sub(1))
                } else {
                    0
                };
                return (visual_offset + v_idx, total_visual_lines);
            }
            visual_offset += num_wrap;
        }
        return (total_visual_lines.saturating_sub(1), total_visual_lines);
    }

    (0, 1)
}

/// Layout and alignment geometry context for inline spans
#[derive(Clone, Copy)]
pub struct SpanLayoutContext<'a> {
    pub font_size: Pixels,
    pub line_height: Pixels,
    pub wrap_width: Option<Pixels>,
    pub align_x: Pixels,
    pub total_raw_len: usize,
    pub scx: LineShapingContext<'a>,
}

impl<'a> SpanLayoutContext<'a> {
    pub fn new(
        font_size: Pixels,
        line_height: Pixels,
        wrap_width: Option<Pixels>,
        align_x: Pixels,
        total_raw_len: usize,
        scx: LineShapingContext<'a>,
    ) -> Self {
        Self {
            font_size,
            line_height,
            wrap_width,
            align_x,
            total_raw_len,
            scx,
        }
    }
}

/// Table row structure and alignment metadata for hit-testing
#[derive(Clone, Copy)]
pub struct TableLayoutInfo<'a> {
    pub row_w: Pixels,
    pub cells: &'a [crate::markdown::TableCell],
    pub alignments: &'a [crate::markdown::TableAlignment],
    pub is_header: bool,
    pub base_font_size: f32,
    pub line_height: f32,
}

fn closest_visible_byte_in_wrapped_line(
    relative_point: Point<Pixels>,
    wrapped_line: &WrappedLine,
    line_height: Pixels,
    align_x: Pixels,
) -> usize {
    let text_rel_x = (relative_point.x - align_x).max(px(0.0));
    let num_v = wrapped_line.wrap_boundaries.len() + 1;
    let last_v = num_v - 1;
    let max_safe_y = (last_v as f32) * line_height + line_height * 0.5;
    let clamped_y = relative_point.y.max(px(0.0)).min(max_safe_y);
    let text_rel_point = point(text_rel_x, clamped_y);
    match wrapped_line.closest_index_for_position(text_rel_point, line_height) {
        Ok(idx) => idx,
        Err(idx) => idx,
    }
}

#[allow(clippy::too_many_arguments)]
pub fn col_from_wrapped_line(
    in_canvas_point: Point<Pixels>,
    wrapped_line: &WrappedLine,
    line_height: Pixels,
    spans: &[InlineSpan],
    total_raw_len: usize,
    is_source: bool,
    is_active_caret: bool,
    disclosure_col: usize,
    is_selection_disclosure: bool,
) -> usize {
    let closest_byte =
        closest_visible_byte_in_wrapped_line(in_canvas_point, wrapped_line, line_height, px(0.0));
    visible_byte_to_raw_col_with_selection(
        closest_byte,
        spans,
        is_active_caret,
        disclosure_col,
        is_source,
        total_raw_len,
        is_selection_disclosure,
    )
}

/// Performs subpixel hit-testing on arbitrary InlineSpans to a raw document column using GPUI HarfBuzz TextSystem
fn calculate_col_from_spans(
    relative_point: Point<Pixels>,
    spans: &[InlineSpan],
    lcx: SpanLayoutContext,
    window: &mut Window,
) -> usize {
    if spans.is_empty() {
        return 0;
    }

    let (line_str, runs) = build_line_runs(spans, lcx.scx, None, &window.text_style());

    if line_str.is_empty() || runs.is_empty() {
        return if lcx.scx.is_source {
            0
        } else {
            lcx.total_raw_len
        };
    }

    let shaped_lines = window.text_system().shape_text(
        line_str.into(),
        lcx.font_size,
        &runs,
        lcx.wrap_width,
        None,
    );

    let closest_byte = match shaped_lines {
        Ok(lines) if !lines.is_empty() => {
            let mut remaining_y = relative_point.y.max(px(0.0));
            let mut chosen_line = &lines[0];
            let mut byte_offset = 0;

            for line in lines.iter() {
                let num_v = line.wrap_boundaries.len() + 1;
                let line_h = lcx.line_height * (num_v as f32);
                if remaining_y < line_h || std::ptr::eq(line, lines.last().unwrap()) {
                    chosen_line = line;
                    break;
                }
                remaining_y -= line_h;
                byte_offset += line.unwrapped_layout.len;
            }

            byte_offset
                + closest_visible_byte_in_wrapped_line(
                    point(relative_point.x, remaining_y),
                    chosen_line,
                    lcx.line_height,
                    lcx.align_x,
                )
        }
        _ => 0,
    };

    visible_byte_to_raw_col_with_selection(
        closest_byte,
        spans,
        lcx.scx.is_active_caret,
        lcx.scx.cursor_col,
        lcx.scx.is_source,
        lcx.total_raw_len,
        lcx.scx.is_selection_disclosure,
    )
}

/// Performs subpixel hit-testing from relative pixel point (x, y) to raw column using GPUI HarfBuzz TextSystem with soft wrapping
pub fn calculate_col_from_point(
    relative_point: Point<Pixels>,
    parsed: &ParsedLine,
    font_size: Pixels,
    line_height: Pixels,
    wrap_width: Option<Pixels>,
    scx: LineShapingContext,
    window: &mut Window,
) -> usize {
    let lcx = SpanLayoutContext::new(
        font_size,
        line_height,
        wrap_width,
        px(0.0),
        parsed.raw_text.chars().count(),
        scx,
    );
    calculate_col_from_spans(relative_point, &parsed.spans, lcx, window)
}

struct TableCellGeometry<'a> {
    pub target_cell: &'a crate::markdown::TableCell,
    pub font_size: Pixels,
    pub line_height: Pixels,
    pub align: crate::markdown::TableAlignment,
    pub inner_cell_w: Pixels,
    pub in_cell_x: Pixels,
}

fn resolve_table_cell_geometry<'a>(
    rel_point: Point<Pixels>,
    table: &'a TableLayoutInfo,
) -> Option<TableCellGeometry<'a>> {
    if table.cells.is_empty() {
        return None;
    }
    let num_cells = table.cells.len();
    let total_w = table.row_w.max(px(100.0));
    let cell_w = total_w / (num_cells as f32);
    let cell_idx = ((f32::from(rel_point.x) / f32::from(cell_w))
        .floor()
        .max(0.0) as usize)
        .min(num_cells - 1);
    let target_cell = table.cells.get(cell_idx)?;

    let (font_size, line_height) =
        table_cell_font_metrics_for(table.is_header, table.base_font_size, table.line_height);
    let align = table
        .alignments
        .get(cell_idx)
        .copied()
        .unwrap_or(crate::markdown::TableAlignment::None);

    let cell_start_x = cell_w * (cell_idx as f32);
    let inner_cell_w = (cell_w - px(16.0)).max(px(10.0));
    let in_cell_x = (rel_point.x - cell_start_x - px(8.0)).max(px(0.0));

    Some(TableCellGeometry {
        target_cell,
        font_size,
        line_height,
        align,
        inner_cell_w,
        in_cell_x,
    })
}

#[inline]
pub(super) fn table_align_offset(
    cell_width: Pixels,
    text_width: Pixels,
    align: crate::markdown::TableAlignment,
) -> Pixels {
    match align {
        crate::markdown::TableAlignment::Center => ((cell_width - text_width) / 2.0).max(px(0.0)),
        crate::markdown::TableAlignment::Right => (cell_width - text_width).max(px(0.0)),
        _ => px(0.0),
    }
}

fn compute_table_cell_align_x(
    spans: &[crate::markdown::InlineSpan],
    scx: LineShapingContext<'_>,
    font_size: Pixels,
    inner_cell_w: Pixels,
    align: crate::markdown::TableAlignment,
    window: &mut Window,
) -> Pixels {
    let (line_str, runs) = build_line_runs(spans, scx, None, &window.text_style());
    if let Ok(lines) =
        window
            .text_system()
            .shape_text(line_str.into(), font_size, &runs, Some(inner_cell_w), None)
        && let Some(wrapped_line) = lines.first()
    {
        table_align_offset(inner_cell_w, wrapped_line.unwrapped_layout.width, align)
    } else {
        px(0.0)
    }
}

pub fn calculate_table_cell_col_from_point(
    rel_point: Point<Pixels>,
    table: TableLayoutInfo,
    theme: &Theme,
    window: &mut Window,
) -> usize {
    let geom = match resolve_table_cell_geometry(rel_point, &table) {
        Some(g) => g,
        None => return 0,
    };

    let target_cell = geom.target_cell;
    if target_cell.raw_content.is_empty() || target_cell.spans.is_empty() {
        return target_cell.col_range.0;
    }

    let scx = LineShapingContext::new(theme, None, false, true, target_cell.col_range.0);
    let align_x = compute_table_cell_align_x(
        &target_cell.spans,
        scx,
        geom.font_size,
        geom.inner_cell_w,
        geom.align,
        window,
    );

    let lcx = SpanLayoutContext::new(
        geom.font_size,
        geom.line_height,
        Some(geom.inner_cell_w),
        align_x,
        target_cell.col_range.1,
        scx,
    );

    let col = calculate_col_from_spans(
        point(geom.in_cell_x, rel_point.y),
        &target_cell.spans,
        lcx,
        window,
    );
    col.clamp(target_cell.col_range.0, target_cell.col_range.1)
}

#[allow(clippy::too_many_arguments)]
pub fn find_link_in_wrapped_line(
    source_line: crate::buffer::SourceLine,
    relative_point: Point<Pixels>,
    wrapped_line: &WrappedLine,
    line_height: Pixels,
    align_x: Pixels,
    spans: &[InlineSpan],
    is_source: bool,
    is_active_caret: bool,
    disclosure_col: usize,
    is_selection_disclosure: bool,
) -> Option<crate::editor::HoveredLink> {
    if relative_point.x < px(0.0) || relative_point.y < px(0.0) || spans.is_empty() {
        return None;
    }

    let text_w = wrapped_line.unwrapped_layout.width;
    if relative_point.x < align_x || relative_point.x > align_x + text_w + px(4.0) {
        return None;
    }

    let closest_byte =
        closest_visible_byte_in_wrapped_line(relative_point, wrapped_line, line_height, align_x);

    let mut cur_byte = 0;
    for span in spans {
        let is_visible = span.is_visible_for_selection(
            is_active_caret,
            disclosure_col,
            is_source,
            is_selection_disclosure,
        );
        if !is_visible || span.text.is_empty() {
            continue;
        }

        let span_len = span
            .rendered_text(is_active_caret, disclosure_col, is_source)
            .len();
        let span_end_byte = cur_byte + span_len;

        if closest_byte >= cur_byte
            && closest_byte <= span_end_byte
            && span.is_link
            && !span.is_image
            && let Some(url) = &span.link_url
        {
            return Some(crate::editor::HoveredLink {
                url: url.clone(),
                line: source_line,
                col_range: span.group_range,
            });
        }

        cur_byte = span_end_byte;
    }

    None
}

fn find_link_at_spans(
    source_line: crate::buffer::SourceLine,
    relative_point: Point<Pixels>,
    spans: &[InlineSpan],
    lcx: SpanLayoutContext,
    window: &mut Window,
) -> Option<crate::editor::HoveredLink> {
    if relative_point.x < px(0.0) || relative_point.y < px(0.0) || spans.is_empty() {
        return None;
    }

    let (line_str, runs) = build_line_runs(spans, lcx.scx, None, &window.text_style());

    if line_str.is_empty() || runs.is_empty() {
        return None;
    }

    let shaped_lines = window.text_system().shape_text(
        line_str.into(),
        lcx.font_size,
        &runs,
        lcx.wrap_width,
        None,
    );

    let Ok(lines) = shaped_lines else {
        return None;
    };
    let wrapped_line = lines.first()?;

    find_link_in_wrapped_line(
        source_line,
        relative_point,
        wrapped_line,
        lcx.line_height,
        lcx.align_x,
        spans,
        lcx.scx.is_source,
        lcx.scx.is_active_caret,
        lcx.scx.cursor_col,
        lcx.scx.is_selection_disclosure,
    )
}

pub fn find_link_in_table_row(
    source_line: crate::buffer::SourceLine,
    rel_point: Point<Pixels>,
    table: TableLayoutInfo,
    theme: &Theme,
    window: &mut Window,
) -> Option<crate::editor::HoveredLink> {
    let geom = resolve_table_cell_geometry(rel_point, &table)?;
    let target_cell = geom.target_cell;

    if !target_cell
        .spans
        .iter()
        .any(|s| s.is_link && !s.is_image && s.link_url.is_some())
    {
        return None;
    }

    let scx = LineShapingContext::new(theme, None, false, false, 0);
    let align_x = compute_table_cell_align_x(
        &target_cell.spans,
        scx,
        geom.font_size,
        geom.inner_cell_w,
        geom.align,
        window,
    );

    let lcx = SpanLayoutContext::new(
        geom.font_size,
        geom.line_height,
        Some(geom.inner_cell_w),
        align_x,
        target_cell.col_range.1,
        scx,
    );

    find_link_at_spans(
        source_line,
        point(geom.in_cell_x, rel_point.y),
        &target_cell.spans,
        lcx,
        window,
    )
}

pub fn find_link_at_pixel_point(
    source_line: crate::buffer::SourceLine,
    relative_point: Point<Pixels>,
    parsed: &ParsedLine,
    lcx: SpanLayoutContext,
    window: &mut Window,
) -> Option<crate::editor::HoveredLink> {
    find_link_at_spans(source_line, relative_point, &parsed.spans, lcx, window)
}
