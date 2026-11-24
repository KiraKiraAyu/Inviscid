use gpui::*;

mod cache;
mod hit_test;
mod painter;
mod runs;
mod wrap;

/// Horizontal space reserved for the outer blockquote border and inset.
pub const QUOTE_GUTTER: Pixels = px(18.0);
/// Vertical space added at the first and last line of a quote group.
pub const QUOTE_EDGE_PADDING: Pixels = px(4.0);

pub use crate::ui::cursor::calculate_caret_size;

pub(crate) use cache::ShapedRowCache;

pub use hit_test::{
    SpanLayoutContext, TableCellGeometry, TableLayoutInfo, calculate_col_from_point,
    calculate_pos_from_col, calculate_table_cell_col_from_point, col_from_wrapped_line,
    find_link_at_pixel_point, find_link_in_table_row, find_link_in_wrapped_line,
    get_visual_line_info,
};

pub use painter::{TableCellRenderContext, render_shaped_cell_canvas, render_shaped_line};
#[cfg(test)]
pub(crate) use painter::{compute_highlight_quads, compute_selection_quads};

#[cfg(test)]
pub(super) use runs::preparation_metrics;
pub use runs::{
    LineShapingContext, build_line_runs, raw_col_to_visible_byte,
    raw_col_to_visible_byte_with_selection, visible_byte_to_raw_col,
    visible_byte_to_raw_col_with_selection,
};

pub use wrap::{
    calculate_table_row_height, calculate_table_row_height_with_metrics,
    calculate_table_row_height_with_preedit, calculate_visual_lines,
    calculate_visual_lines_with_runs, calculate_wrapped_visual_lines,
    calculate_wrapped_visual_lines_with_mode, calculate_wrapped_visual_lines_with_runs_fallback,
    calculate_wrapped_visual_lines_with_wrap, char_advance_ratio, char_advance_ratio_with_mono,
    compute_available_line_width, compute_available_line_width_with_indent,
    compute_available_line_width_with_mode, compute_content_width, effective_content_width,
    get_block_left_offset, get_block_left_offset_with_indent, get_block_style_for_line,
    get_block_style_for_line_with_metrics, get_block_top_offset, get_parsed_line_left_offset,
    line_font_for, parsed_line_available_width, quote_content_indent_px, table_cell_font_metrics,
    table_cell_font_metrics_for,
};
pub(crate) use wrap::{calculate_table_row_height_cached, math_render_size};
