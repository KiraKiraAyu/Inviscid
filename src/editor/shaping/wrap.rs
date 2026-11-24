use gpui::*;
use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;

use crate::editor::RenderMode;
use crate::editor::geometry::MIN_LINE_CONTENT_WIDTH;
use crate::markdown::ParsedLine;
use crate::theme::Theme;

use super::QUOTE_GUTTER;
use super::cache::{ShapedLineKey, ShapedRowCache, shape_line_or_cached};
use super::runs::{LineShapingContext, build_line_runs};

#[inline]
pub fn quote_content_indent_px(parsed: &ParsedLine, space_width: Pixels) -> Pixels {
    px(parsed
        .quote
        .map_or(0.0, |quote| quote.content_indent as f32)
        * f32::from(space_width))
}

/// Computes the centered document content container width based on window width and render mode
pub fn compute_content_width(window_width: Pixels, mode: RenderMode) -> Pixels {
    let max_w = if mode == RenderMode::Source {
        crate::editor::layout::MAX_CONTENT_WIDTH_SOURCE
    } else {
        crate::editor::layout::MAX_CONTENT_WIDTH_LIVE
    };
    (window_width - crate::editor::layout::CONTENT_HORIZONTAL_PADDING)
        .min(max_w - crate::editor::layout::CONTENT_HORIZONTAL_PADDING)
        .max(px(200.0))
}

/// Effective width available to a parsed line after its outer quote and raw
/// visual indentation have been applied.
pub fn effective_content_width(
    content_width: Pixels,
    parsed: &ParsedLine,
    mode: RenderMode,
    indent_px: Pixels,
) -> Pixels {
    if mode == RenderMode::LivePreview
        && let Some(quote) = parsed.quote
    {
        let quote_depth = quote.depth.max(1);
        (content_width - QUOTE_GUTTER * quote_depth as f32 - indent_px).max(MIN_LINE_CONTENT_WIDTH)
    } else {
        content_width
    }
}

#[inline]
pub fn table_cell_font_metrics_for(
    is_header: bool,
    base_font_size: f32,
    line_height_factor: f32,
) -> (Pixels, Pixels) {
    let scale = base_font_size / 15.0;
    let lh_ratio = line_height_factor / 1.6;
    if is_header {
        (
            px((14.0 * scale).round()),
            px((22.0 * scale * lh_ratio).round()),
        )
    } else {
        (
            px((13.5 * scale).round()),
            px((20.0 * scale * lh_ratio).round()),
        )
    }
}

#[inline]
fn is_wide_char(c: char) -> bool {
    matches!(
        c as u32,
        0x1100..=0x115f
            | 0x2329..=0x232a
            | 0x2e80..=0xa4cf
            | 0xac00..=0xd7a3
            | 0xf900..=0xfaff
            | 0xfe10..=0xfe19
            | 0xfe30..=0xfe6f
            | 0xff00..=0xff60
            | 0xffe0..=0xffe6
            | 0x20000..=0x3fffd
    )
}

/// Approximate character advance ratio (relative to `font_size`) used when no `TextSystem` is available.
#[inline]
pub fn char_advance_ratio_with_mono(c: char, is_monospace: bool) -> f32 {
    if is_wide_char(c) {
        1.05
    } else if is_monospace {
        0.60
    } else {
        match c {
            ' ' => 0.28,
            '.' | ',' | ':' | ';' | '!' | '|' | '\'' | '`' => 0.30,
            'i' | 'l' | 'j' | 'I' | 't' | 'f' | 'r' => 0.38,
            '[' | ']' | '(' | ')' | '{' | '}' | '/' | '\\' | '-' | '_' => 0.45,
            '0'..='9' => 0.58,
            'm' => 0.85,
            'w' => 0.75,
            'M' | 'W' => 0.90,
            'A'..='Z' => 0.68,
            '@' | '#' | '%' | '&' | '+' | '=' | '<' | '>' | '~' | '$' | '^' | '*' => 0.65,
            _ => 0.53,
        }
    }
}

/// Computes wrapped visual line count by shaping the styled runs with GPUI's `shape_text`,
/// the same path used for painting, so layout heights match rendered wrapping exactly.
/// Falls back to token- and boundary-aware estimation when no text system is available.
#[allow(clippy::too_many_arguments)]
pub fn calculate_visual_lines_with_runs(
    text: &str,
    runs: &[TextRun],
    font: Font,
    font_size: Pixels,
    available_width: Pixels,
    soft_wrap: bool,
    text_system: Option<&Arc<WindowTextSystem>>,
    is_monospace: bool,
) -> usize {
    if !soft_wrap || text.is_empty() || available_width <= px(0.0) {
        return 1;
    }

    if let Some(ts) = text_system {
        let default_runs;
        let runs = if runs.is_empty() {
            default_runs = [TextRun {
                len: text.len(),
                font,
                color: Hsla::default(),
                background_color: None,
                underline: None,
                strikethrough: None,
            }];
            &default_runs[..]
        } else {
            runs
        };
        match ts.shape_text(
            SharedString::from(text.to_string()),
            font_size,
            runs,
            Some(available_width),
            None,
        ) {
            Ok(lines) => {
                return lines
                    .iter()
                    .map(|line| line.wrap_boundaries.len() + 1)
                    .sum::<usize>()
                    .max(1);
            }
            Err(err) => {
                eprintln!("shape_text failed while measuring wrapped lines: {err:#}");
            }
        }
    }

    calculate_wrapped_visual_lines_with_runs_fallback(
        text,
        runs,
        font_size,
        available_width,
        soft_wrap,
        is_monospace,
    )
}

/// Token- and boundary-aware line wrap estimation with per-run monospace awareness used when `TextSystem` is unavailable.
pub fn calculate_wrapped_visual_lines_with_runs_fallback(
    text: &str,
    runs: &[TextRun],
    font_size: Pixels,
    available_width: Pixels,
    soft_wrap: bool,
    fallback_is_monospace: bool,
) -> usize {
    if !soft_wrap || text.is_empty() || available_width <= px(0.0) {
        return 1;
    }
    let fs = f32::from(font_size);
    let avail = f32::from(available_width);
    if avail <= 0.0 {
        return 1;
    }

    let is_run_mono: Vec<bool> = runs
        .iter()
        .map(|r| {
            fallback_is_monospace
                || r.font.family.to_lowercase().contains("mono")
                || r.font.family.as_ref() == crate::platform::platform_monospace_font()
        })
        .collect();

    let mut visual_lines = 1;
    let mut current_line_w = 0.0;
    let mut pending_word_w = 0.0;

    let mut current_run_idx = 0;
    let mut current_run_end = runs.first().map(|r| r.len).unwrap_or(text.len());

    for (byte_ix, c) in text.char_indices() {
        while byte_ix >= current_run_end && current_run_idx + 1 < runs.len() {
            current_run_idx += 1;
            current_run_end += runs[current_run_idx].len;
        }
        let is_mono = is_run_mono
            .get(current_run_idx)
            .copied()
            .unwrap_or(fallback_is_monospace);
        let char_w = fs * char_advance_ratio_with_mono(c, is_mono);
        let is_cjk = is_wide_char(c);
        let is_space = c == ' ' || c == '\t';
        let is_break_punct = matches!(
            c,
            '/' | '\\'
                | '-'
                | '_'
                | '.'
                | ','
                | ':'
                | ';'
                | '?'
                | '!'
                | ')'
                | ']'
                | '}'
                | '&'
                | '='
                | '%'
        );

        if is_space {
            current_line_w += pending_word_w;
            pending_word_w = 0.0;
            if current_line_w + char_w > avail {
                visual_lines += 1;
                current_line_w = 0.0;
            } else {
                current_line_w += char_w;
            }
        } else if is_cjk {
            current_line_w += pending_word_w;
            pending_word_w = 0.0;
            if current_line_w + char_w > avail {
                visual_lines += 1;
                current_line_w = char_w;
            } else {
                current_line_w += char_w;
            }
        } else if is_break_punct {
            pending_word_w += char_w;
            if current_line_w + pending_word_w > avail {
                if current_line_w > 0.0 {
                    visual_lines += 1;
                }
                while pending_word_w > avail {
                    visual_lines += 1;
                    pending_word_w -= avail;
                }
                current_line_w = pending_word_w;
                pending_word_w = 0.0;
            } else {
                current_line_w += pending_word_w;
                pending_word_w = 0.0;
            }
        } else {
            pending_word_w += char_w;
            if pending_word_w > avail {
                if current_line_w > 0.0 {
                    visual_lines += 1;
                }
                while pending_word_w > avail {
                    visual_lines += 1;
                    pending_word_w -= avail;
                }
            } else if current_line_w + pending_word_w > avail && current_line_w > 0.0 {
                visual_lines += 1;
                current_line_w = 0.0;
            }
        }
    }

    if current_line_w + pending_word_w > avail {
        visual_lines += 1;
    }

    visual_lines.max(1)
}

#[allow(clippy::too_many_arguments)]
pub fn calculate_table_row_height_with_metrics(
    cells: &[crate::markdown::TableCell],
    is_header: bool,
    is_active: bool,
    cursor_col: usize,
    container_width: Pixels,
    text_system: Option<&Arc<WindowTextSystem>>,
    is_selection_disclosure: bool,
    ime_preedit_cols: Option<(usize, usize)>,
    base_font_size: f32,
    line_height_factor: f32,
) -> Pixels {
    calculate_table_row_height_cached(
        cells,
        is_header,
        is_active,
        cursor_col,
        container_width,
        text_system,
        is_selection_disclosure,
        ime_preedit_cols,
        base_font_size,
        line_height_factor,
        None,
        0,
        &crate::theme::DEFAULT_THEME,
        None,
        None,
    )
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn calculate_table_row_height_cached(
    cells: &[crate::markdown::TableCell],
    is_header: bool,
    is_active: bool,
    cursor_col: usize,
    container_width: Pixels,
    text_system: Option<&Arc<WindowTextSystem>>,
    is_selection_disclosure: bool,
    ime_preedit_cols: Option<(usize, usize)>,
    base_font_size: f32,
    line_height_factor: f32,
    source_line: Option<crate::buffer::SourceLine>,
    buffer_revision: u64,
    theme: &Theme,
    hovered_link_range: Option<(usize, usize)>,
    shaped_rows: Option<&Rc<RefCell<ShapedRowCache>>>,
) -> Pixels {
    let scale = base_font_size / 15.0;
    let lh_ratio = line_height_factor / 1.6;
    let min_h = if is_header {
        px((34.0 * scale * lh_ratio).round())
    } else {
        px((30.0 * scale * lh_ratio).round())
    };
    if cells.is_empty() {
        return min_h;
    }

    let num_cells = cells.len();
    let row_w = container_width.max(px(100.0));
    let cell_w = row_w / (num_cells as f32);
    let inner_cell_w = (cell_w - px(16.0)).max(px(10.0));

    let (font_size, line_height) =
        table_cell_font_metrics_for(is_header, base_font_size, line_height_factor);
    let cell_font = font(crate::platform::platform_ui_font());

    let mut max_visual_lines = 1;
    let mut max_math_height = px(0.0);
    for (c_idx, cell) in cells.iter().enumerate() {
        let scx = LineShapingContext::new(theme, None, false, is_active, cursor_col)
            .with_selection_disclosure(is_selection_disclosure)
            .with_ime_preedit(ime_preedit_cols);

        for span in &cell.spans {
            if !span.math_is_disclosed(is_active, cursor_col, false)
                && let Some(math) = &span.math
            {
                max_math_height =
                    max_math_height.max(math_render_size(&math.rendered, font_size).height);
            }
        }

        let lines = if let (Some(ts), Some(src_line), Some(cache)) =
            (text_system, source_line, shaped_rows)
        {
            let wrap_width = Some(inner_cell_w);
            let key = ShapedLineKey::new(
                buffer_revision,
                src_line.index(),
                Some(c_idx),
                theme,
                font_size,
                wrap_width,
                None,
                &scx,
                hovered_link_range,
            );
            if let Some(cached) = shape_line_or_cached(
                &key,
                &cell.spans,
                scx,
                hovered_link_range,
                font_size,
                wrap_width,
                ts,
                &TextStyle::default(),
                Some(cache),
            ) {
                cached.visual_lines
            } else {
                let (cell_text, runs) =
                    build_line_runs(&cell.spans, scx, hovered_link_range, &TextStyle::default());
                calculate_visual_lines_with_runs(
                    &cell_text,
                    &runs,
                    cell_font.clone(),
                    font_size,
                    inner_cell_w,
                    true,
                    text_system,
                    false,
                )
            }
        } else {
            let (cell_text, runs) =
                build_line_runs(&cell.spans, scx, hovered_link_range, &TextStyle::default());
            calculate_visual_lines_with_runs(
                &cell_text,
                &runs,
                cell_font.clone(),
                font_size,
                inner_cell_w,
                true,
                text_system,
                false,
            )
        };
        max_visual_lines = max_visual_lines.max(lines);
    }

    let pad = if is_header {
        px((12.0 * scale).round())
    } else {
        px((10.0 * scale).round())
    };
    (pad + line_height.max(max_math_height) * (max_visual_lines as f32)).max(min_h)
}

pub(crate) fn math_render_size(math: &crate::math::MathSvg, font_size: Pixels) -> Size<Pixels> {
    size(
        px(math.width_milli_em as f32 * f32::from(font_size) * crate::math::MATH_SCALE / 1000.0),
        px(math.height_milli_em as f32 * f32::from(font_size) * crate::math::MATH_SCALE / 1000.0),
    )
}

pub(super) fn math_vertical_offset(line_height: Pixels, math_height: Pixels) -> Pixels {
    (line_height.max(math_height) - math_height) / 2.0
}

#[cfg(test)]
mod math_render_geometry_tests {
    use super::{math_render_size, math_vertical_offset};
    use gpui::px;

    #[test]
    fn math_is_scaled_up_from_the_editor_font_size() {
        let math = crate::math::prepare_latex(r"\omega", false).unwrap();
        let font_size = px(16.0);
        let size = math_render_size(&math, font_size);

        assert_eq!(
            size.width,
            px(math.width_milli_em as f32 * 16.0 * 1.2 / 1000.0)
        );
        assert_eq!(
            size.height,
            px(math.height_milli_em as f32 * 16.0 * 1.2 / 1000.0)
        );
    }

    #[test]
    fn math_is_centered_in_the_actual_row_height() {
        assert_eq!(math_vertical_offset(px(24.0), px(16.0)), px(4.0));
        assert_eq!(math_vertical_offset(px(24.0), px(32.0)), px(0.0));
    }
}
