use gpui::*;

use crate::buffer::{LineSource, SourceLine};
use crate::editor::LineRenderContext;
use crate::editor::geometry::table_row_margin;
use crate::editor::shaping::{
    TableCellRenderContext, calculate_table_row_height_cached, render_shaped_cell_canvas,
    table_cell_font_metrics_for,
};
use crate::markdown::{TableAlignment, TableCell, table};

pub(super) fn render_table_row(
    source_line: SourceLine,
    lines: &(impl LineSource + ?Sized),
    lrc: &LineRenderContext,
    is_header: bool,
    is_delimiter: bool,
    alignments: &[TableAlignment],
    cells: &[TableCell],
) -> AnyElement {
    if is_delimiter {
        // Table delimiter row is visually hidden in Live Preview
        return div().h(px(0.0)).into_any_element();
    }

    let line_idx = source_line.index();
    let theme = lrc.theme;
    let is_active = lrc.is_active_line;
    let is_focused = lrc.is_focused;
    let selection = lrc.selection;

    let active_cell_idx = if is_active && is_focused {
        Some(table::get_cell_index_for_col(cells, selection.head.col))
    } else {
        None
    };
    let disclosure_cell_idx = if lrc.is_disclosure_line {
        Some(table::get_cell_index_for_col(cells, lrc.disclosure_col))
    } else {
        None
    };

    let num_cells = cells.len();
    let hovered_link_range = lrc.hovered_link.and_then(|hl| {
        if hl.line == source_line {
            Some(hl.col_range)
        } else {
            None
        }
    });

    let row_h = calculate_table_row_height_cached(
        cells,
        is_header,
        lrc.is_disclosure_line,
        lrc.disclosure_col,
        lrc.content_width,
        lrc.text_system,
        lrc.is_selection_disclosure,
        lrc.ime_preedit_cols,
        lrc.font_size,
        lrc.line_height,
        Some(source_line),
        lrc.buffer_revision,
        theme,
        hovered_link_range,
        lrc.shaped_rows,
    );

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

    let table_margin = table_row_margin(lrc.font_size);
    if is_header {
        row_elem = row_elem
            .mt(table_margin)
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

        let is_last_row = if line_idx + 1 >= lines.line_count() {
            true
        } else {
            let next = lines.source_line(SourceLine::new(line_idx + 1));
            !next.contains('|') || next.trim().is_empty()
        };

        row_elem = row_elem.border_b_1().bg(row_bg);
        if is_last_row {
            row_elem = row_elem.rounded_b(px(6.0)).mb(table_margin);
        }
    }

    let (font_size, line_height) =
        table_cell_font_metrics_for(is_header, lrc.font_size, lrc.line_height);

    for (c_idx, cell) in cells.iter().enumerate() {
        let align = alignments
            .get(c_idx)
            .copied()
            .unwrap_or(TableAlignment::None);
        let is_last = c_idx + 1 == num_cells;
        let is_active_cell = active_cell_idx == Some(c_idx);
        let is_disclosure_cell = disclosure_cell_idx == Some(c_idx);
        let tcx = TableCellRenderContext {
            source_line,
            cell_idx: c_idx,
            col_range: cell.col_range,
            theme,
            font_size,
            line_height,
            is_active_cell,
            is_disclosure_cell,
            // Disclosure must match what `get_line_layout_height` measured this row with;
            // the caret target stays on the live caret column.
            disclosure_col: lrc.disclosure_col,
            is_selection_disclosure: lrc.is_selection_disclosure,
            ime_preedit_cols: lrc.ime_preedit_cols,
            selection,
            is_dragging: lrc.is_dragging,
            is_focused,
            cursor_col: selection.head.col,
            cursor_opacity: lrc.cursor_opacity,
            align,
            hovered_link_range,
            highlights: lrc.highlights,
            buffer_revision: lrc.buffer_revision,
            shaped_rows: lrc.shaped_rows,
        };
        row_elem = row_elem.child(render_table_cell(cell, is_last, tcx));
    }

    row_elem.into_any_element()
}

fn render_table_cell(
    cell: &TableCell,
    is_last: bool,
    tcx: TableCellRenderContext,
) -> impl IntoElement {
    let theme = tcx.theme;
    let cell_canvas = render_shaped_cell_canvas(&cell.spans, tcx);

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
