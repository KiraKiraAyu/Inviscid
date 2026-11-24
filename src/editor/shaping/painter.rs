use gpui::*;
use std::cell::RefCell;
use std::ops::Range;
use std::rc::Rc;
use std::sync::Arc;

use crate::editor::geometry::{
    PaintedLineGeometry, TextShapingMode, resolve_inner_available_width_scaled,
};
use crate::markdown::{BlockKind, InlineSpan, MathSpan, ParsedLine};
use crate::theme::{ActiveTheme, Theme};

use super::super::{LineHighlight, LineRenderContext};
use super::cache::{ShapedLineKey, ShapedRowCache, shape_line_or_cached};
use super::calculate_caret_size;
use super::hit_test::table_align_offset;
use super::runs::{LineShapingContext, build_line_runs, raw_col_to_visible_byte_with_selection};
use super::wrap::{calculate_visual_lines_with_runs, math_render_size, math_vertical_offset};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum RangeTail {
    Clipped,
    EndOfLine,
    Continues,
}

fn align_quad_origin_x(
    origin_x: Pixels,
    align_bounds_width: Pixels,
    row_width: Pixels,
    align: TextAlign,
) -> Pixels {
    origin_x
        + match align {
            TextAlign::Left => px(0.0),
            TextAlign::Center => ((align_bounds_width - row_width) / 2.0).max(px(0.0)),
            TextAlign::Right => (align_bounds_width - row_width).max(px(0.0)),
        }
}

#[allow(clippy::too_many_arguments)]
fn push_visual_row_quads(
    wrapped_line: &WrappedLine,
    range: Range<usize>,
    tail: RangeTail,
    origin: Point<Pixels>,
    align: TextAlign,
    align_bounds_width: Pixels,
    line_height: Pixels,
    mut push: impl FnMut(Bounds<Pixels>),
) {
    let layout = &wrapped_line.unwrapped_layout;
    let row_count = wrapped_line.wrap_boundaries.len() + 1;
    let mut row_start = 0;
    for row in 0..row_count {
        let row_end = match wrapped_line.wrap_boundaries.get(row) {
            Some(b) => layout.runs[b.run_ix].glyphs[b.glyph_ix].index,
            None => layout.len,
        };

        if range.start < row_end && range.end >= row_start {
            let seg_start = range.start.max(row_start);
            let seg_end = range.end.min(row_end);
            let row_x = layout.x_for_index(row_start);
            let row_width = layout.x_for_index(row_end) - row_x;
            let row_origin_x = align_quad_origin_x(origin.x, align_bounds_width, row_width, align);
            let x1 = layout.x_for_index(seg_start) - row_x;
            let x2 = if seg_end == row_end && tail != RangeTail::Clipped {
                (layout.x_for_index(seg_end) - row_x + px(8.0)).max(x1 + px(4.0))
            } else {
                layout.x_for_index(seg_end) - row_x
            };

            if x2 > x1 || tail == RangeTail::Continues {
                let y = origin.y + (row as f32) * line_height;
                push(Bounds::from_corners(
                    point(row_origin_x + x1, y),
                    point(row_origin_x + x2.max(x1 + px(2.0)), y + line_height),
                ));
            }
        }
        row_start = row_end;
    }
}

#[inline]
pub(crate) fn clip_range_to_bounds(
    range: Range<usize>,
    bounds: (usize, usize),
) -> Option<Range<usize>> {
    let start = range.start.max(bounds.0);
    let end = range.end.min(bounds.1);
    (start < end).then_some(start..end)
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn compute_selection_quads(
    selection: crate::buffer::Selection,
    source_line: crate::buffer::SourceLine,
    raw_line_len: usize,
    col_bounds: (usize, usize),
    clip_to_bounds: bool,
    spans: &[InlineSpan],
    is_active_caret: bool,
    disclosure_col: usize,
    is_source: bool,
    is_selection_disclosure: bool,
    wrapped_line: &WrappedLine,
    origin: Point<Pixels>,
    align: TextAlign,
    align_bounds_width: Pixels,
    line_height: Pixels,
) -> Vec<Bounds<Pixels>> {
    let mut quads = Vec::new();
    if selection.is_empty() {
        return quads;
    }
    let (sel_start, sel_end) = (selection.start(), selection.end());
    if source_line < sel_start.line || source_line > sel_end.line {
        return quads;
    }

    let (line_start_col, line_end_col) = if sel_start.line == sel_end.line {
        (
            sel_start.col.min(raw_line_len),
            sel_end.col.min(raw_line_len),
        )
    } else if source_line == sel_start.line {
        (sel_start.col.min(raw_line_len), raw_line_len)
    } else if source_line == sel_end.line {
        (0, sel_end.col.min(raw_line_len))
    } else {
        (0, raw_line_len)
    };

    let (start_raw_col, end_raw_col) = if clip_to_bounds {
        let Some(clipped) = clip_range_to_bounds(line_start_col..line_end_col, col_bounds) else {
            return quads;
        };
        (clipped.start, clipped.end)
    } else {
        (line_start_col, line_end_col)
    };

    let to_visible = |raw_col: usize| {
        raw_col_to_visible_byte_with_selection(
            raw_col,
            spans,
            is_active_caret,
            disclosure_col,
            is_source,
            is_selection_disclosure,
        )
    };

    let tail = if clip_to_bounds {
        RangeTail::Clipped
    } else if source_line < sel_end.line {
        RangeTail::Continues
    } else if end_raw_col == raw_line_len {
        RangeTail::EndOfLine
    } else {
        RangeTail::Clipped
    };

    push_visual_row_quads(
        wrapped_line,
        to_visible(start_raw_col)..to_visible(end_raw_col),
        tail,
        origin,
        align,
        align_bounds_width,
        line_height,
        |quad| quads.push(quad),
    );
    quads
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn compute_highlight_quads(
    highlights: &[LineHighlight],
    col_bounds: (usize, usize),
    clip_to_bounds: bool,
    spans: &[InlineSpan],
    is_active_caret: bool,
    disclosure_col: usize,
    is_source: bool,
    is_selection_disclosure: bool,
    wrapped_line: &WrappedLine,
    origin: Point<Pixels>,
    align: TextAlign,
    align_bounds_width: Pixels,
    line_height: Pixels,
    theme: &Theme,
) -> Vec<(Bounds<Pixels>, Hsla)> {
    let mut quads = Vec::new();
    let to_visible = |raw_col: usize| {
        raw_col_to_visible_byte_with_selection(
            raw_col,
            spans,
            is_active_caret,
            disclosure_col,
            is_source,
            is_selection_disclosure,
        )
    };
    for highlight in highlights {
        let range = if clip_to_bounds {
            let Some(clipped) = clip_range_to_bounds(highlight.cols.clone(), col_bounds) else {
                continue;
            };
            clipped
        } else {
            highlight.cols.clone()
        };
        let color = highlight.kind.color(theme);
        let tail = if !clip_to_bounds && highlight.continues {
            RangeTail::Continues
        } else {
            RangeTail::Clipped
        };
        push_visual_row_quads(
            wrapped_line,
            to_visible(range.start)..to_visible(range.end),
            tail,
            origin,
            align,
            align_bounds_width,
            line_height,
            |quad| quads.push((quad, color)),
        );
    }
    quads
}

enum ShapedTextPrepaintState {
    Empty {
        caret_bounds: Option<Bounds<Pixels>>,
        cursor_color: Hsla,
    },
    Shaped {
        wrapped_line: Arc<WrappedLine>,
        origin: Point<Pixels>,
        text_align: TextAlign,
        clip_bounds: Option<Bounds<Pixels>>,
        math_paints: Vec<MathPaint>,
        highlight_quads: Vec<(Bounds<Pixels>, Hsla)>,
        selection_quads: Vec<Bounds<Pixels>>,
        selection_color: Hsla,
        caret_bounds: Option<Bounds<Pixels>>,
        cursor_color: Hsla,
    },
    None,
}

fn paint_shaped_text(
    prepaint: ShapedTextPrepaintState,
    line_height: Pixels,
    window: &mut Window,
    cx: &mut App,
) {
    match prepaint {
        ShapedTextPrepaintState::Empty {
            caret_bounds,
            cursor_color,
        } => {
            if let Some(caret) = caret_bounds {
                window.paint_quad(fill(caret, cursor_color));
            }
        }
        ShapedTextPrepaintState::Shaped {
            wrapped_line,
            origin,
            text_align,
            clip_bounds,
            math_paints,
            highlight_quads,
            selection_quads,
            selection_color,
            caret_bounds,
            cursor_color,
        } => {
            for (quad, color) in highlight_quads {
                window.paint_quad(fill(quad, color));
            }
            for sel_quad in selection_quads {
                window.paint_quad(fill(sel_quad, selection_color));
            }
            let _ = wrapped_line.paint_background(
                origin,
                line_height,
                text_align,
                clip_bounds,
                window,
                cx,
            );
            let _ = wrapped_line.paint(origin, line_height, text_align, clip_bounds, window, cx);
            for math in math_paints {
                let _ = window.paint_svg(
                    math.bounds,
                    math.asset_path,
                    TransformationMatrix::default(),
                    math.color,
                    cx,
                );
            }
            if let Some(caret) = caret_bounds {
                window.paint_quad(fill(caret, cursor_color));
            }
        }
        ShapedTextPrepaintState::None => {}
    }
}

struct MathPaint {
    bounds: Bounds<Pixels>,
    asset_path: SharedString,
    color: Hsla,
}

fn collect_math_runs(
    spans: &[InlineSpan],
    is_active_line: bool,
    cursor_col: usize,
    is_source: bool,
    is_selection_disclosure: bool,
) -> Vec<(MathSpan, Range<usize>)> {
    let mut math_runs = Vec::new();
    let mut visible_byte = 0;

    for span in spans {
        if !span.is_visible_for_selection(
            is_active_line,
            cursor_col,
            is_source,
            is_selection_disclosure,
        ) {
            continue;
        }
        let rendered_text = span.rendered_text(is_active_line, cursor_col, is_source);
        if let Some(math) = &span.math
            && !span.math_is_disclosed(is_active_line, cursor_col, is_source)
        {
            math_runs.push((
                math.clone(),
                visible_byte..visible_byte + rendered_text.len(),
            ));
        }
        visible_byte += rendered_text.len();
    }

    math_runs
}

#[allow(clippy::too_many_arguments)]
fn math_paints_for_row(
    math_runs: &[(MathSpan, Range<usize>)],
    wrapped_line: &WrappedLine,
    origin: Point<Pixels>,
    align_x: Pixels,
    content_width: Pixels,
    font_size: Pixels,
    line_height: Pixels,
    color: Hsla,
    center_display: bool,
) -> Vec<MathPaint> {
    math_runs
        .iter()
        .filter_map(|(math, range)| {
            let start = wrapped_line.position_for_index(range.start, line_height)?;
            let end = wrapped_line.position_for_index(range.end, line_height)?;
            let rendered_size = math_render_size(&math.rendered, font_size);
            let intrinsic_width = rendered_size.width;
            let height = rendered_size.height;
            let centered = math.is_display && center_display;
            let reserved_width = if start.y == end.y {
                (end.x - start.x).max(px(1.0))
            } else {
                intrinsic_width
            };
            let width = if centered {
                intrinsic_width
            } else {
                reserved_width
            };
            let x = if centered {
                ((content_width - width) / 2.0).max(px(0.0))
            } else {
                align_x + start.x
            };
            let y = origin.y + start.y + math_vertical_offset(line_height, height);
            let _reserved_width = if start.y == end.y {
                (end.x - start.x).max(px(1.0))
            } else {
                width
            };

            Some(MathPaint {
                bounds: Bounds::new(point(origin.x + x, y), size(width, height)),
                asset_path: math.rendered.asset_path.clone().into(),
                color,
            })
        })
        .collect()
}

pub fn render_shaped_line(
    source_line: crate::buffer::SourceLine,
    parsed: &ParsedLine,
    lrc: &LineRenderContext,
    font_size: Pixels,
    line_height: Pixels,
    override_color: Option<Hsla>,
    shaping_mode: TextShapingMode,
) -> AnyElement {
    let spans = parsed.spans.clone();
    let raw_chars_count = parsed.raw_text.chars().count();
    let is_active = lrc.is_active_line;
    let is_focused = lrc.is_focused;
    let cursor_col = lrc.cursor_col;
    let disclosure_col = lrc.disclosure_col;
    let selection = lrc.selection;
    let is_dragging = lrc.is_dragging;
    let is_selection_disclosure = lrc.is_selection_disclosure;
    let cursor_opacity = lrc.cursor_opacity;
    let hovered_link_range = lrc.hovered_link.and_then(|hl| {
        if hl.line == source_line {
            Some(hl.col_range)
        } else {
            None
        }
    });
    let content_width = lrc.content_width;
    let soft_wrap = lrc.soft_wrap;
    let highlights = lrc.highlights.to_vec();
    let ime_preedit_cols = lrc.ime_preedit_cols;
    let buffer_revision = lrc.buffer_revision;
    let shaped_rows = lrc.shaped_rows.cloned();

    let is_active_caret = lrc.is_disclosure_line;
    let is_source = shaping_mode.is_source();
    let is_monospace = shaping_mode.is_monospace();

    let math_runs = collect_math_runs(
        &spans,
        is_active_caret,
        disclosure_col,
        is_source,
        is_selection_disclosure,
    );

    let available_width = resolve_inner_available_width_scaled(
        content_width,
        &parsed.kind,
        lrc.mode,
        lrc.indent_px,
        lrc.font_size,
    );
    let wrap_width = if soft_wrap && available_width > px(0.0) {
        Some(available_width)
    } else {
        None
    };

    let is_heading = matches!(parsed.kind, BlockKind::Heading { .. });
    let font = if is_monospace {
        font(crate::platform::platform_monospace_font())
    } else if is_heading {
        font(crate::platform::platform_ui_font()).bold()
    } else {
        font(crate::platform::platform_ui_font())
    };

    let scx_measure = LineShapingContext::new(
        lrc.theme,
        override_color,
        is_source,
        is_active_caret,
        disclosure_col,
    )
    .with_monospace(is_monospace)
    .with_selection_disclosure(is_selection_disclosure)
    .with_heading(is_heading)
    .with_ime_preedit(ime_preedit_cols);
    let shape_key = ShapedLineKey::new(
        buffer_revision,
        source_line.index(),
        None,
        lrc.theme,
        font_size,
        wrap_width,
        override_color,
        &scx_measure,
        hovered_link_range,
    );

    let est_lines = if font_size > px(0.0) && available_width > px(0.0) {
        if let Some(ts) = lrc.text_system
            && let Some(cached) = shape_line_or_cached(
                &shape_key,
                &spans,
                scx_measure,
                hovered_link_range,
                font_size,
                wrap_width,
                ts,
                &TextStyle::default(),
                shaped_rows.as_ref(),
            )
        {
            cached.visual_lines
        } else {
            let (line_str_for_measuring, runs_for_measuring) = build_line_runs(
                &spans,
                scx_measure,
                hovered_link_range,
                &TextStyle::default(),
            );
            calculate_visual_lines_with_runs(
                &line_str_for_measuring,
                &runs_for_measuring,
                font,
                font_size,
                available_width,
                soft_wrap,
                lrc.text_system,
                is_monospace,
            )
        }
    } else {
        1
    };

    let math_height = math_runs
        .iter()
        .map(|(math, _)| math_render_size(&math.rendered, font_size).height)
        .fold(px(0.0), Pixels::max);
    let total_canvas_h = (line_height * (est_lines as f32)).max(math_height);
    let painted_lines = lrc.painted_lines.cloned();

    canvas(
        move |bounds, window, cx| {
            let theme = cx.theme();
            let cursor_color = theme.cursor.opacity(cursor_opacity);
            let selection_color = theme.selection;
            let math_color = override_color.unwrap_or(theme.text_primary);
            let scx = LineShapingContext::new(
                theme,
                override_color,
                is_source,
                is_active_caret,
                disclosure_col,
            )
            .with_monospace(is_monospace)
            .with_selection_disclosure(is_selection_disclosure)
            .with_heading(is_heading)
            .with_ime_preedit(ime_preedit_cols);
            let prepaint_key = ShapedLineKey::new(
                buffer_revision,
                source_line.index(),
                None,
                theme,
                font_size,
                wrap_width,
                override_color,
                &scx,
                hovered_link_range,
            );
            let Some(cached) = shape_line_or_cached(
                &prepaint_key,
                &spans,
                scx,
                hovered_link_range,
                font_size,
                wrap_width,
                window.text_system(),
                &window.text_style(),
                shaped_rows.as_ref(),
            ) else {
                return ShapedTextPrepaintState::None;
            };

            let Some(wrapped_line) = cached.wrapped_line else {
                let caret_bounds = if is_active
                    && is_focused
                    && selection.is_empty()
                    && !is_dragging
                    && cursor_opacity > 0.01
                {
                    let (caret_w, caret_h) = calculate_caret_size(font_size, line_height);
                    let caret_y = (bounds.origin.y + (line_height - caret_h) / 2.0).round();
                    let caret_x = bounds.origin.x.round();
                    Some(Bounds::new(
                        point(caret_x, caret_y),
                        size(caret_w, caret_h.round()),
                    ))
                } else {
                    None
                };

                if let Some(cache) = &painted_lines {
                    cache.borrow_mut().record(
                        source_line,
                        PaintedLineGeometry {
                            text_bounds: bounds,
                            line_height,
                            wrapped_line: None,
                            is_source,
                            is_active_caret,
                            disclosure_col,
                            is_selection_disclosure,
                        },
                    );
                }

                return ShapedTextPrepaintState::Empty {
                    caret_bounds,
                    cursor_color,
                };
            };

            let has_other_content = spans.iter().any(|span| {
                span.math.is_none()
                    && span.is_visible_for_selection(
                        is_active_caret,
                        disclosure_col,
                        is_source,
                        is_selection_disclosure,
                    )
                    && !span.text.trim().is_empty()
            });
            let math_paints = math_paints_for_row(
                &math_runs,
                &wrapped_line,
                bounds.origin,
                px(0.0),
                bounds.size.width,
                font_size,
                line_height,
                math_color,
                !has_other_content,
            );

            let selection_quads = compute_selection_quads(
                selection,
                source_line,
                raw_chars_count,
                (0, raw_chars_count),
                false,
                &spans,
                is_active_caret,
                disclosure_col,
                is_source,
                is_selection_disclosure,
                &wrapped_line,
                bounds.origin,
                TextAlign::Left,
                bounds.size.width,
                line_height,
            );

            let highlight_quads = compute_highlight_quads(
                &highlights,
                (0, raw_chars_count),
                false,
                &spans,
                is_active_caret,
                disclosure_col,
                is_source,
                is_selection_disclosure,
                &wrapped_line,
                bounds.origin,
                TextAlign::Left,
                bounds.size.width,
                line_height,
                theme,
            );

            let caret_bounds = if is_active
                && is_focused
                && selection.is_empty()
                && !is_dragging
                && cursor_opacity > 0.01
            {
                let target_byte = raw_col_to_visible_byte_with_selection(
                    cursor_col,
                    &spans,
                    is_active_caret,
                    disclosure_col,
                    is_source,
                    is_selection_disclosure,
                );
                if let Some(pos) = wrapped_line.position_for_index(target_byte, line_height) {
                    let (caret_w, caret_h) = calculate_caret_size(font_size, line_height);
                    let caret_y = (bounds.origin.y + pos.y + (line_height - caret_h) / 2.0).round();
                    let caret_x = (bounds.origin.x + pos.x).round();
                    Some(Bounds::new(
                        point(caret_x, caret_y),
                        size(caret_w, caret_h.round()),
                    ))
                } else {
                    None
                }
            } else {
                None
            };

            if let Some(cache) = &painted_lines {
                cache.borrow_mut().record(
                    source_line,
                    PaintedLineGeometry {
                        text_bounds: bounds,
                        line_height,
                        wrapped_line: Some(Arc::clone(&wrapped_line)),
                        is_source,
                        is_active_caret,
                        disclosure_col,
                        is_selection_disclosure,
                    },
                );
            }

            ShapedTextPrepaintState::Shaped {
                wrapped_line,
                origin: bounds.origin,
                text_align: TextAlign::Left,
                clip_bounds: None,
                math_paints,
                highlight_quads,
                selection_quads,
                selection_color,
                caret_bounds,
                cursor_color,
            }
        },
        move |_bounds, prepaint, window, cx| {
            paint_shaped_text(prepaint, line_height, window, cx);
        },
    )
    .h(total_canvas_h)
    .w_full()
    .into_any_element()
}

#[derive(Clone)]
pub struct TableCellRenderContext<'a> {
    pub source_line: crate::buffer::SourceLine,
    pub cell_idx: usize,
    pub col_range: (usize, usize),
    pub theme: &'a Theme,
    pub font_size: Pixels,
    pub line_height: Pixels,
    pub is_active_cell: bool,
    pub is_disclosure_cell: bool,
    pub disclosure_col: usize,
    pub is_selection_disclosure: bool,
    pub ime_preedit_cols: Option<(usize, usize)>,
    pub selection: crate::buffer::Selection,
    pub is_dragging: bool,
    pub is_focused: bool,
    pub cursor_col: usize,
    pub cursor_opacity: f32,
    pub align: crate::markdown::TableAlignment,
    pub hovered_link_range: Option<(usize, usize)>,
    pub highlights: &'a [LineHighlight],
    pub buffer_revision: u64,
    pub(crate) shaped_rows: Option<&'a Rc<RefCell<ShapedRowCache>>>,
}

pub fn render_shaped_cell_canvas(
    spans: &[InlineSpan],
    tcx: TableCellRenderContext,
) -> impl IntoElement {
    let spans = spans.to_vec();
    let source_line = tcx.source_line;
    let cell_idx = tcx.cell_idx;
    let col_range = tcx.col_range;
    let font_size = tcx.font_size;
    let line_height = tcx.line_height;
    let is_active_cell = tcx.is_active_cell;
    let is_disclosure_cell = tcx.is_disclosure_cell;
    let disclosure_col = tcx.disclosure_col;
    let is_selection_disclosure = tcx.is_selection_disclosure;
    let ime_preedit_cols = tcx.ime_preedit_cols;
    let selection = tcx.selection;
    let is_dragging = tcx.is_dragging;
    let is_focused = tcx.is_focused;
    let cursor_col = tcx.cursor_col;
    let cursor_opacity = tcx.cursor_opacity;
    let align = tcx.align;
    let hovered_link_range = tcx.hovered_link_range;
    let highlights = tcx.highlights.to_vec();
    let buffer_revision = tcx.buffer_revision;
    let shaped_rows = tcx.shaped_rows.cloned();
    let math_runs = collect_math_runs(
        &spans,
        is_disclosure_cell,
        disclosure_col,
        false,
        is_selection_disclosure,
    );

    canvas(
        move |bounds, window, cx| {
            let theme = cx.theme();
            let cursor_color = theme.cursor.opacity(cursor_opacity);
            let selection_color = theme.selection;
            let text_align = match align {
                crate::markdown::TableAlignment::Center => TextAlign::Center,
                crate::markdown::TableAlignment::Right => TextAlign::Right,
                _ => TextAlign::Left,
            };
            let scx =
                LineShapingContext::new(theme, None, false, is_disclosure_cell, disclosure_col)
                    .with_selection_disclosure(is_selection_disclosure)
                    .with_ime_preedit(ime_preedit_cols);
            let cell_wrap_width = Some(bounds.size.width);
            let prepaint_key = ShapedLineKey::new(
                buffer_revision,
                source_line.index(),
                Some(cell_idx),
                theme,
                font_size,
                cell_wrap_width,
                None,
                &scx,
                hovered_link_range,
            );
            let Some(cached) = shape_line_or_cached(
                &prepaint_key,
                &spans,
                scx,
                hovered_link_range,
                font_size,
                cell_wrap_width,
                window.text_system(),
                &window.text_style(),
                shaped_rows.as_ref(),
            ) else {
                return ShapedTextPrepaintState::None;
            };

            let Some(wrapped_line) = cached.wrapped_line else {
                let caret_bounds = if is_active_cell
                    && is_focused
                    && selection.is_empty()
                    && !is_dragging
                    && cursor_opacity > 0.01
                {
                    let (caret_w, caret_h) = calculate_caret_size(font_size, line_height);
                    let align_x = table_align_offset(bounds.size.width, px(0.0), align);
                    let caret_x = (bounds.origin.x + align_x).round();
                    let caret_y = (bounds.origin.y + (bounds.size.height - caret_h) / 2.0).round();
                    Some(Bounds::new(
                        point(caret_x, caret_y),
                        size(caret_w, caret_h.round()),
                    ))
                } else {
                    None
                };
                return ShapedTextPrepaintState::Empty {
                    caret_bounds,
                    cursor_color,
                };
            };

            let total_text_h = (line_height * (cached.visual_lines as f32)).max(
                math_runs
                    .iter()
                    .map(|(math, _)| math_render_size(&math.rendered, font_size).height)
                    .fold(px(0.0), Pixels::max),
            );
            let origin_y = bounds.origin.y + (bounds.size.height - total_text_h).max(px(0.0)) / 2.0;
            let origin = point(bounds.origin.x, origin_y);
            let align_x = table_align_offset(
                bounds.size.width,
                wrapped_line.unwrapped_layout.width,
                align,
            );
            let has_other_content = spans.iter().any(|span| {
                span.math.is_none()
                    && span.is_visible_for_selection(
                        is_disclosure_cell,
                        disclosure_col,
                        false,
                        is_selection_disclosure,
                    )
                    && !span.text.trim().is_empty()
            });
            let math_paints = math_paints_for_row(
                &math_runs,
                &wrapped_line,
                origin,
                align_x,
                bounds.size.width,
                font_size,
                line_height,
                theme.text_primary,
                !has_other_content,
            );

            let raw_row_end = col_range.1;
            let selection_quads = compute_selection_quads(
                selection,
                source_line,
                raw_row_end,
                col_range,
                true,
                &spans,
                is_disclosure_cell,
                disclosure_col,
                false,
                is_selection_disclosure,
                &wrapped_line,
                origin,
                text_align,
                bounds.size.width,
                line_height,
            );

            let highlight_quads = compute_highlight_quads(
                &highlights,
                col_range,
                true,
                &spans,
                is_disclosure_cell,
                disclosure_col,
                false,
                is_selection_disclosure,
                &wrapped_line,
                origin,
                text_align,
                bounds.size.width,
                line_height,
                theme,
            );

            let caret_bounds = if is_active_cell
                && is_focused
                && selection.is_empty()
                && !is_dragging
                && cursor_opacity > 0.01
            {
                let target_byte = raw_col_to_visible_byte_with_selection(
                    cursor_col,
                    &spans,
                    true,
                    disclosure_col,
                    false,
                    is_selection_disclosure,
                );
                if let Some(caret_pos) = wrapped_line.position_for_index(target_byte, line_height) {
                    let (caret_w, caret_h) = calculate_caret_size(font_size, line_height);
                    let row_idx = ((f32::from(caret_pos.y) / f32::from(line_height))
                        .floor()
                        .max(0.0) as usize)
                        .min(wrapped_line.wrap_boundaries.len());
                    let layout = &wrapped_line.unwrapped_layout;
                    let row_start = if row_idx == 0 {
                        0
                    } else {
                        let b = wrapped_line.wrap_boundaries[row_idx - 1];
                        layout.runs[b.run_ix].glyphs[b.glyph_ix].index
                    };
                    let row_end = match wrapped_line.wrap_boundaries.get(row_idx) {
                        Some(b) => layout.runs[b.run_ix].glyphs[b.glyph_ix].index,
                        None => layout.len,
                    };
                    let row_w = layout.x_for_index(row_end) - layout.x_for_index(row_start);
                    let row_origin_x =
                        align_quad_origin_x(origin.x, bounds.size.width, row_w, text_align);
                    let caret_x = (row_origin_x + caret_pos.x).round();
                    let caret_y = (origin.y + caret_pos.y + (line_height - caret_h) / 2.0).round();
                    Some(Bounds::new(
                        point(caret_x, caret_y),
                        size(caret_w, caret_h.round()),
                    ))
                } else {
                    None
                }
            } else {
                None
            };

            ShapedTextPrepaintState::Shaped {
                wrapped_line,
                origin,
                text_align,
                clip_bounds: Some(bounds),
                math_paints,
                highlight_quads,
                selection_quads,
                selection_color,
                caret_bounds,
                cursor_color,
            }
        },
        move |_bounds, prepaint, window, cx| {
            paint_shaped_text(prepaint, line_height, window, cx);
        },
    )
    .size_full()
    .into_any_element()
}
