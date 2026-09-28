use gpui::*;
use std::sync::Arc;

use crate::editor::RenderMode;
use crate::markdown::{BlockKind, InlineSpan, ParsedLine};
use crate::theme::{ActiveTheme, Theme};

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

/// Computes the effective text content wrapping width for a specific block kind and render mode
pub fn compute_available_line_width_with_mode(
    content_width: Pixels,
    kind: &BlockKind,
    mode: RenderMode,
) -> Pixels {
    if mode == RenderMode::Source {
        return (content_width - crate::editor::layout::SOURCE_GUTTER_WIDTH).max(px(100.0));
    }
    match kind {
        BlockKind::BlockQuote => (content_width - px(18.0)).max(px(100.0)),
        BlockKind::TaskList { .. } | BlockKind::OrderedList { .. } => {
            (content_width - px(24.0)).max(px(100.0))
        }
        BlockKind::BulletList => (content_width - px(22.0)).max(px(100.0)),
        BlockKind::CodeBlockContent => (content_width - px(28.0)).max(px(100.0)),
        _ => content_width.max(px(100.0)),
    }
}

/// Computes the effective text content wrapping width for a specific block kind (defaults to Live Preview)
pub fn compute_available_line_width(content_width: Pixels, kind: &BlockKind) -> Pixels {
    compute_available_line_width_with_mode(content_width, kind, RenderMode::LivePreview)
}

/// Computes the left horizontal offset of text content within a block container in Live Preview
pub fn get_block_left_offset(kind: &BlockKind, mode: RenderMode) -> Pixels {
    if mode == RenderMode::Source {
        return px(0.0);
    }
    match kind {
        BlockKind::TaskList { .. } => px(24.0),
        BlockKind::BulletList => px(22.0),
        BlockKind::OrderedList { .. } => px(24.0),
        BlockKind::BlockQuote => px(18.0),
        BlockKind::CodeBlockContent => px(15.0),
        BlockKind::CodeBlock {
            is_fence_start: true,
        } => px(15.0),
        _ => px(0.0),
    }
}

/// Maps a raw document column to a byte offset in the visible rendered text.
/// `disclosure_col` must match the column used when building `spans`.
pub fn raw_col_to_visible_byte(
    raw_col: usize,
    spans: &[InlineSpan],
    is_active_caret: bool,
    disclosure_col: usize,
    is_source: bool,
) -> usize {
    let mut cur_visible_byte = 0;
    let mut target_byte = None;

    for span in spans {
        let is_visible = span.is_visible(is_active_caret, disclosure_col, is_source);
        let (s_start, s_end) = span.span_range;

        if !is_visible {
            if target_byte.is_none() && raw_col <= s_end {
                target_byte = Some(cur_visible_byte);
            }
            continue;
        }

        let start_byte = cur_visible_byte;

        if target_byte.is_none() {
            if raw_col <= s_start {
                target_byte = Some(start_byte);
            } else if raw_col <= s_end {
                let byte_offset: usize = span
                    .text
                    .chars()
                    .take(raw_col - s_start)
                    .map(char::len_utf8)
                    .sum();
                target_byte = Some(start_byte + byte_offset);
            }
        }

        cur_visible_byte += span.text.len();
    }

    target_byte.unwrap_or(cur_visible_byte)
}

/// Maps a byte offset in the visible rendered text back to a raw document column.
pub fn visible_byte_to_raw_col(
    visible_byte: usize,
    spans: &[InlineSpan],
    is_active_caret: bool,
    cursor_col: usize,
    is_source: bool,
    total_raw_len: usize,
) -> usize {
    let mut cur_byte = 0;

    for span in spans {
        let is_visible = span.is_visible(is_active_caret, cursor_col, is_source);
        if !is_visible {
            continue;
        }

        let span_len = span.text.len();
        let span_end_byte = cur_byte + span_len;

        if visible_byte < span_end_byte {
            let offset_in_span_bytes = visible_byte.saturating_sub(cur_byte);
            if offset_in_span_bytes == 0
                && !is_source
                && !is_active_caret
                && span.group_range.0 < span.span_range.0
                && cur_byte == 0
            {
                return span.group_range.0;
            }
            let mut byte_count = 0;
            let mut char_count = 0;
            for c in span.text.chars() {
                if byte_count >= offset_in_span_bytes {
                    break;
                }
                byte_count += c.len_utf8();
                char_count += 1;
            }
            return (span.span_range.0 + char_count).min(total_raw_len);
        } else if visible_byte == span_end_byte {
            if !is_source && !is_active_caret && span.group_range.1 > span.span_range.1 {
                return span.group_range.1.min(total_raw_len);
            } else {
                return span.span_range.1.min(total_raw_len);
            }
        }

        cur_byte = span_end_byte;
    }

    total_raw_len
}

pub fn get_block_style_for_line(
    parsed: &ParsedLine,
    theme: &Theme,
    is_source: bool,
) -> (Pixels, Pixels, Option<Hsla>) {
    get_block_style_for_line_with_metrics(parsed, theme, is_source, 15.0, 1.6)
}

pub fn get_block_style_for_line_with_metrics(
    parsed: &ParsedLine,
    theme: &Theme,
    is_source: bool,
    base_font_size: f32,
    line_height_factor: f32,
) -> (Pixels, Pixels, Option<Hsla>) {
    use crate::editor::layout::{get_block_color_override, get_block_typography_with_metrics};

    if is_source {
        return (
            px(base_font_size),
            px((base_font_size * line_height_factor).round()),
            None,
        );
    }
    match &parsed.kind {
        BlockKind::CodeBlock {
            is_fence_start: false,
        }
        | BlockKind::ThematicBreak => (px(0.0), px(0.0), None),
        _ => {
            let typo =
                get_block_typography_with_metrics(&parsed.kind, base_font_size, line_height_factor);
            let color = get_block_color_override(&parsed.kind, theme);
            (typo.font_size, typo.line_height, color)
        }
    }
}

#[derive(Clone, Copy)]
pub struct LineShapingContext<'a> {
    pub theme: &'a Theme,
    pub override_color: Option<Hsla>,
    pub is_source: bool,
    pub is_active_caret: bool,
    pub cursor_col: usize,
    pub is_heading: bool,
}

impl<'a> LineShapingContext<'a> {
    pub fn new(
        theme: &'a Theme,
        override_color: Option<Hsla>,
        is_source: bool,
        is_active_caret: bool,
        cursor_col: usize,
    ) -> Self {
        Self {
            theme,
            override_color,
            is_source,
            is_active_caret,
            cursor_col,
            is_heading: false,
        }
    }

    pub fn with_heading(mut self, is_heading: bool) -> Self {
        self.is_heading = is_heading;
        self
    }
}

#[inline]
pub fn table_cell_font_metrics(is_header: bool) -> (Pixels, Pixels) {
    if is_header {
        (px(14.0), px(22.0))
    } else {
        (px(13.5), px(20.0))
    }
}

pub use crate::ui::cursor::calculate_caret_size;

pub fn build_line_runs(
    spans: &[InlineSpan],
    scx: LineShapingContext,
    hovered_link_range: Option<(usize, usize)>,
    base_style: &TextStyle,
) -> (String, Vec<TextRun>) {
    let mut line_str = String::new();
    let mut runs = Vec::new();

    for span in spans {
        let is_visible = span.is_visible(scx.is_active_caret, scx.cursor_col, scx.is_source);
        if !is_visible || span.text.is_empty() {
            continue;
        }

        let span_byte_len = span.text.len();
        line_str.push_str(&span.text);

        let mut font = base_style.font();
        let mut color = scx.override_color.unwrap_or(scx.theme.text_primary);
        let mut background_color = None;
        let mut strikethrough = None;
        let mut underline = None;

        if scx.is_source
            || scx.override_color == Some(scx.theme.code_block_text)
            || scx.override_color == Some(scx.theme.code_block_header_fg)
            || span.is_code
        {
            font.family = crate::platform::platform_monospace_font().into();
        } else {
            font.family = crate::platform::platform_ui_font().into();
        }

        if span.is_code {
            if span.is_marker {
                color = scx.theme.heading_marker;
            } else {
                color = scx.theme.inline_code_fg;
                background_color = Some(scx.theme.inline_code_bg);
            }
        }

        if span.is_link {
            let is_link_hovered = hovered_link_range
                .is_some_and(|range| span.group_range == range || span.span_range == range);
            if span.is_marker {
                if is_link_hovered {
                    color = scx.theme.link_fg;
                    underline = Some(UnderlineStyle {
                        thickness: px(1.0),
                        color: Some(scx.theme.link_fg),
                        wavy: false,
                    });
                } else {
                    color = scx.theme.heading_marker;
                }
            } else {
                if !span.is_code {
                    color = scx.theme.link_fg;
                }
                if is_link_hovered {
                    underline = Some(UnderlineStyle {
                        thickness: px(1.0),
                        color: Some(scx.theme.link_fg),
                        wavy: false,
                    });
                }
            }
        }

        if span.is_marker && !span.is_code && !span.is_link {
            color = scx.theme.heading_marker;
        }

        if scx.is_heading || span.is_bold {
            font.weight = FontWeight::BOLD;
        }
        if span.is_italic {
            font.style = FontStyle::Italic;
        }
        if span.is_strikethrough {
            strikethrough = Some(StrikethroughStyle {
                thickness: px(1.0),
                color: Some(color),
            });
        }

        runs.push(TextRun {
            len: span_byte_len,
            font,
            color,
            background_color,
            underline,
            strikethrough,
        });
    }

    (line_str, runs)
}

pub fn line_font_for(kind: &BlockKind, is_source: bool) -> Font {
    if is_source || *kind == BlockKind::CodeBlockContent {
        font(crate::platform::platform_monospace_font())
    } else if matches!(kind, BlockKind::Heading { .. }) {
        font(crate::platform::platform_ui_font()).bold()
    } else {
        font(crate::platform::platform_ui_font())
    }
}

fn shape_ime_preedit(
    text: &str,
    line_font: &Font,
    font_size: Pixels,
    color: Hsla,
    window: &mut Window,
) -> Option<Box<WrappedLine>> {
    if text.is_empty() {
        return None;
    }
    let runs = vec![TextRun {
        len: text.len(),
        font: line_font.clone(),
        color,
        background_color: None,
        underline: Some(UnderlineStyle {
            thickness: px(1.0),
            color: Some(color),
            wavy: false,
        }),
        strikethrough: None,
    }];
    window
        .text_system()
        .shape_text(text.to_owned().into(), font_size, &runs, None, None)
        .ok()
        .and_then(|mut lines| lines.pop())
        .map(Box::new)
}

/// Extracts the active IME preedit text and the byte offset in visible text where it is anchored.
pub fn ime_preedit_overlay(
    spans: &[InlineSpan],
    is_active_caret: bool,
    disclosure_col: usize,
    is_source: bool,
) -> Option<(String, usize)> {
    let anchor_col = spans.iter().find(|span| span.is_ime_preedit)?.span_range.0;
    let text: String = spans
        .iter()
        .filter(|span| span.is_ime_preedit)
        .map(|span| span.text.as_str())
        .collect();
    let anchor_byte = raw_col_to_visible_byte(
        anchor_col,
        spans,
        is_active_caret,
        disclosure_col,
        is_source,
    );
    Some((text, anchor_byte))
}

/// Computes the IME preedit overlay origin on its visual row and shifts the caret to the end of the preedit.
pub fn ime_preedit_placement(
    line_origin: Point<Pixels>,
    anchor: Point<Pixels>,
    preedit_width: Pixels,
    caret_bounds: Option<Bounds<Pixels>>,
) -> (Point<Pixels>, Option<Bounds<Pixels>>) {
    let row_top = point(line_origin.x + anchor.x, line_origin.y + anchor.y);
    let caret = caret_bounds.map(|mut caret| {
        caret.origin.x = row_top.x + preedit_width;
        caret
    });
    (row_top, caret)
}

pub fn measure_ime_preedit(
    text: &str,
    font_size: Pixels,
    line_font: &Font,
    color: Hsla,
    window: &mut Window,
) -> Pixels {
    if text.is_empty() {
        return px(0.0);
    }
    let runs = vec![TextRun {
        len: text.len(),
        font: line_font.clone(),
        color,
        background_color: None,
        underline: None,
        strikethrough: None,
    }];
    window
        .text_system()
        .shape_text(text.to_owned().into(), font_size, &runs, None, None)
        .ok()
        .and_then(|mut lines| lines.pop())
        .map(|line| line.unwrapped_layout.width)
        .unwrap_or(px(0.0))
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

#[inline]
pub fn char_advance_ratio(c: char) -> f32 {
    char_advance_ratio_with_mono(c, false)
}

pub fn calculate_wrapped_visual_lines(
    text: &str,
    font_size: Pixels,
    available_width: Pixels,
) -> usize {
    calculate_wrapped_visual_lines_with_mode(text, font_size, available_width, true, false)
}

pub fn calculate_wrapped_visual_lines_with_wrap(
    text: &str,
    font_size: Pixels,
    available_width: Pixels,
    soft_wrap: bool,
) -> usize {
    calculate_wrapped_visual_lines_with_mode(text, font_size, available_width, soft_wrap, false)
}

/// Computes wrapped visual line count using `TextSystem::line_wrapper` when available,
/// falling back to character-width estimation in headless tests.
pub fn calculate_visual_lines(
    text: &str,
    font: Font,
    font_size: Pixels,
    available_width: Pixels,
    soft_wrap: bool,
    text_system: Option<&Arc<TextSystem>>,
    is_monospace: bool,
) -> usize {
    if !soft_wrap || text.is_empty() || available_width <= px(0.0) {
        return 1;
    }
    if let Some(ts) = text_system {
        let mut wrapper = ts.line_wrapper(font, font_size);
        wrapper
            .wrap_line(&[LineFragment::text(text)], available_width)
            .count()
            + 1
    } else {
        calculate_wrapped_visual_lines_with_mode(
            text,
            font_size,
            available_width,
            soft_wrap,
            is_monospace,
        )
    }
}

/// Token- and boundary-aware line wrap estimation used when `TextSystem` is unavailable.
pub fn calculate_wrapped_visual_lines_with_mode(
    text: &str,
    font_size: Pixels,
    available_width: Pixels,
    soft_wrap: bool,
    is_monospace: bool,
) -> usize {
    if !soft_wrap || text.is_empty() || available_width <= px(0.0) {
        return 1;
    }
    let fs = f32::from(font_size);
    let avail = f32::from(available_width);
    if avail <= 0.0 {
        return 1;
    }

    let mut visual_lines = 1;
    let mut current_line_w = 0.0;
    let mut pending_word_w = 0.0;

    for c in text.chars() {
        let char_w = fs * char_advance_ratio_with_mono(c, is_monospace);
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
                    current_line_w = 0.0;
                }
                while pending_word_w > avail {
                    visual_lines += 1;
                    pending_word_w -= avail;
                }
            } else if current_line_w + pending_word_w > avail {
                if current_line_w > 0.0 {
                    visual_lines += 1;
                    current_line_w = 0.0;
                }
            }
        }
    }

    if current_line_w + pending_word_w > avail {
        visual_lines += 1;
    }

    visual_lines.max(1)
}

pub fn calculate_table_row_height(
    cells: &[crate::markdown::TableCell],
    is_header: bool,
    is_active: bool,
    cursor_col: usize,
    container_width: Pixels,
    text_system: Option<&Arc<TextSystem>>,
) -> Pixels {
    if cells.is_empty() {
        return if is_header { px(34.0) } else { px(30.0) };
    }

    let num_cells = cells.len();
    let row_w = container_width.max(px(100.0));
    let cell_w = row_w / (num_cells as f32);
    let inner_cell_w = (cell_w - px(16.0)).max(px(10.0));

    let (font_size, line_height) = table_cell_font_metrics(is_header);
    let cell_font = font(crate::platform::platform_ui_font());

    let mut max_visual_lines = 1;
    for cell in cells {
        let cell_text = cell
            .spans
            .iter()
            .filter(|s| s.is_visible(is_active, cursor_col, false))
            .map(|s| s.text.as_str())
            .collect::<String>();

        let lines = calculate_visual_lines(
            &cell_text,
            cell_font.clone(),
            font_size,
            inner_cell_w,
            true,
            text_system,
            false,
        );
        max_visual_lines = max_visual_lines.max(lines);
    }

    let pad = if is_header { px(12.0) } else { px(10.0) };
    let min_h = if is_header { px(34.0) } else { px(30.0) };
    (pad + line_height * (max_visual_lines as f32)).max(min_h)
}

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

    let target_byte = raw_col_to_visible_byte(
        col,
        &parsed.spans,
        scx.is_active_caret,
        scx.cursor_col,
        scx.is_source,
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

    let target_byte = raw_col_to_visible_byte(
        col,
        &parsed.spans,
        scx.is_active_caret,
        scx.cursor_col,
        scx.is_source,
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
            let text_rel_x = (relative_point.x - lcx.align_x).max(px(0.0));
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

            let text_rel_point = point(text_rel_x, remaining_y);
            let idx_in_line =
                match chosen_line.closest_index_for_position(text_rel_point, lcx.line_height) {
                    Ok(idx) => idx,
                    Err(idx) => idx,
                };
            byte_offset + idx_in_line
        }
        _ => 0,
    };

    visible_byte_to_raw_col(
        closest_byte,
        spans,
        lcx.scx.is_active_caret,
        lcx.scx.cursor_col,
        lcx.scx.is_source,
        lcx.total_raw_len,
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

pub struct TableCellGeometry<'a> {
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

    let (font_size, line_height) = table_cell_font_metrics(table.is_header);
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
fn table_align_offset(
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

fn find_link_at_spans(
    line_idx: usize,
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

    let text_w = wrapped_line.unwrapped_layout.width;
    if relative_point.x < lcx.align_x || relative_point.x > lcx.align_x + text_w + px(4.0) {
        return None;
    }

    let text_rel_x = relative_point.x - lcx.align_x;
    let text_rel_point = point(text_rel_x, relative_point.y);

    let closest_byte =
        match wrapped_line.closest_index_for_position(text_rel_point, lcx.line_height) {
            Ok(idx) => idx,
            Err(idx) => idx,
        };

    let mut cur_byte = 0;
    for span in spans {
        let is_visible = span.is_visible(
            lcx.scx.is_active_caret,
            lcx.scx.cursor_col,
            lcx.scx.is_source,
        );
        if !is_visible || span.text.is_empty() {
            continue;
        }

        let span_len = span.text.len();
        let span_end_byte = cur_byte + span_len;

        if closest_byte >= cur_byte
            && closest_byte <= span_end_byte
            && span.is_link
            && !span.is_image
            && let Some(url) = &span.link_url
        {
            return Some(crate::editor::HoveredLink {
                url: url.clone(),
                line_idx,
                col_range: span.group_range,
            });
        }

        cur_byte = span_end_byte;
    }

    None
}

pub fn find_link_in_table_row(
    line_idx: usize,
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
        line_idx,
        point(geom.in_cell_x, rel_point.y),
        &target_cell.spans,
        lcx,
        window,
    )
}

pub fn find_link_at_pixel_point(
    line_idx: usize,
    relative_point: Point<Pixels>,
    parsed: &ParsedLine,
    lcx: SpanLayoutContext,
    window: &mut Window,
) -> Option<crate::editor::HoveredLink> {
    find_link_at_spans(line_idx, relative_point, &parsed.spans, lcx, window)
}

use super::LineRenderContext;

enum LinePrepaintState {
    Empty {
        caret_bounds: Option<Bounds<Pixels>>,
        cursor_color: Hsla,
        preedit: Option<(Box<WrappedLine>, Point<Pixels>)>,
    },
    Shaped {
        wrapped_line: Box<WrappedLine>,
        selection_quads: Vec<Bounds<Pixels>>,
        selection_color: Hsla,
        caret_bounds: Option<Bounds<Pixels>>,
        cursor_color: Hsla,
        preedit: Option<(Box<WrappedLine>, Point<Pixels>)>,
    },
    None,
}

pub fn render_shaped_line(
    line_idx: usize,
    parsed: &ParsedLine,
    lrc: &LineRenderContext,
    font_size: Pixels,
    line_height: Pixels,
    override_color: Option<Hsla>,
    is_source: bool,
) -> AnyElement {
    let spans = parsed.spans.clone();
    let raw_chars_count = parsed.raw_text.chars().count();
    let is_active = lrc.is_active_line;
    let is_focused = lrc.is_focused;
    let cursor_col = lrc.cursor_col;
    let disclosure_col = lrc.disclosure_col;
    let selection = lrc.selection;
    let is_dragging = lrc.is_dragging;
    let cursor_opacity = lrc.cursor_opacity;
    let hovered_link_range = lrc.hovered_link.and_then(|hl| {
        if hl.line_idx == line_idx {
            Some(hl.col_range)
        } else {
            None
        }
    });
    let content_width = lrc.content_width;
    let soft_wrap = lrc.soft_wrap;

    let is_active_caret = is_active;

    let ime_preedit =
        ime_preedit_overlay(&parsed.spans, is_active_caret, disclosure_col, is_source);

    let available_width = compute_available_line_width_with_mode(
        content_width,
        &parsed.kind,
        if is_source {
            RenderMode::Source
        } else {
            RenderMode::LivePreview
        },
    );

    let is_monospace = is_source || parsed.kind == BlockKind::CodeBlockContent;
    let is_heading = matches!(parsed.kind, BlockKind::Heading { .. });
    let font = line_font_for(&parsed.kind, is_source);
    let preedit_font = font.clone();
    let preedit_color = override_color.unwrap_or(lrc.theme.text_primary);

    let scx_measure = LineShapingContext::new(
        lrc.theme,
        override_color,
        is_source,
        is_active_caret,
        disclosure_col,
    )
    .with_heading(is_heading);
    let (line_str_for_measuring, _) = build_line_runs(
        &spans,
        scx_measure,
        hovered_link_range,
        &TextStyle::default(),
    );

    let est_lines = if font_size > px(0.0) && available_width > px(0.0) {
        calculate_visual_lines(
            &line_str_for_measuring,
            font,
            font_size,
            available_width,
            soft_wrap,
            lrc.text_system,
            is_monospace,
        )
    } else {
        1
    };
    let total_canvas_h = line_height * (est_lines as f32);

    canvas(
        move |bounds, window, cx| {
            let (line_str, runs, cursor_color, selection_color) = {
                let theme = cx.theme();
                let scx = LineShapingContext::new(
                    theme,
                    override_color,
                    is_source,
                    is_active_caret,
                    disclosure_col,
                )
                .with_heading(is_heading);
                let (line_str, runs) =
                    build_line_runs(&spans, scx, hovered_link_range, &window.text_style());
                let cursor_color = theme.cursor.opacity(cursor_opacity);
                let selection_color = theme.selection;
                (line_str, runs, cursor_color, selection_color)
            };

            if line_str.is_empty() || runs.is_empty() {
                let mut caret_bounds = if is_active
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

                let mut preedit = None;
                if let Some((text, _)) = ime_preedit.as_ref()
                    && let Some(line) =
                        shape_ime_preedit(text, &preedit_font, font_size, preedit_color, window)
                {
                    let (origin, caret) = ime_preedit_placement(
                        bounds.origin,
                        point(px(0.0), px(0.0)),
                        line.unwrapped_layout.width,
                        caret_bounds,
                    );
                    caret_bounds = caret;
                    preedit = Some((line, origin));
                }

                return LinePrepaintState::Empty {
                    caret_bounds,
                    cursor_color,
                    preedit,
                };
            }

            let wrap_width = if soft_wrap && available_width > px(100.0) {
                Some(available_width)
            } else {
                None
            };

            let shaped_lines = window.text_system().shape_text(
                line_str.into(),
                font_size,
                &runs,
                wrap_width,
                None,
            );

            let Ok(lines) = shaped_lines else {
                return LinePrepaintState::None;
            };

            let mut lines_iter = lines.into_iter();
            let Some(wrapped_line) = lines_iter.next() else {
                return LinePrepaintState::None;
            };

            let num_visual_lines = wrapped_line.wrap_boundaries.len() + 1;

            let mut selection_quads = Vec::new();
            if !selection.is_empty() {
                let (sel_start, sel_end) = (selection.start(), selection.end());
                if line_idx >= sel_start.line && line_idx <= sel_end.line {
                    let total_raw_len = raw_chars_count;

                    let (start_raw_col, end_raw_col) = if sel_start.line == sel_end.line {
                        (
                            sel_start.col.min(total_raw_len),
                            sel_end.col.min(total_raw_len),
                        )
                    } else if line_idx == sel_start.line {
                        (sel_start.col.min(total_raw_len), total_raw_len)
                    } else if line_idx == sel_end.line {
                        (0, sel_end.col.min(total_raw_len))
                    } else {
                        (0, total_raw_len)
                    };

                    let start_byte = raw_col_to_visible_byte(
                        start_raw_col,
                        &spans,
                        is_active_caret,
                        disclosure_col,
                        is_source,
                    );
                    let end_byte = raw_col_to_visible_byte(
                        end_raw_col,
                        &spans,
                        is_active_caret,
                        disclosure_col,
                        is_source,
                    );

                    let mut line_start_ix = 0;
                    for v_ix in 0..num_visual_lines {
                        let line_end_ix = if v_ix < wrapped_line.wrap_boundaries.len() {
                            let b = wrapped_line.wrap_boundaries[v_ix];
                            let run = &wrapped_line.unwrapped_layout.runs[b.run_ix];
                            run.glyphs[b.glyph_ix].index
                        } else {
                            wrapped_line.unwrapped_layout.len
                        };

                        if start_byte < line_end_ix && end_byte >= line_start_ix {
                            let seg_s = start_byte.max(line_start_ix);
                            let seg_e = end_byte.min(line_end_ix);
                            let line_start_x =
                                wrapped_line.unwrapped_layout.x_for_index(line_start_ix);
                            let x1 =
                                wrapped_line.unwrapped_layout.x_for_index(seg_s) - line_start_x;
                            let x2 = if seg_e == line_end_ix
                                && (line_idx < sel_end.line || end_raw_col == total_raw_len)
                            {
                                (wrapped_line.unwrapped_layout.x_for_index(seg_e) - line_start_x
                                    + px(8.0))
                                .max(x1 + px(4.0))
                            } else {
                                wrapped_line.unwrapped_layout.x_for_index(seg_e) - line_start_x
                            };

                            if x2 > x1 || line_idx < sel_end.line {
                                let y = bounds.origin.y + (v_ix as f32) * line_height;
                                let sel_bounds = Bounds::from_corners(
                                    point(bounds.origin.x + x1, y),
                                    point(bounds.origin.x + x2.max(x1 + px(2.0)), y + line_height),
                                );
                                selection_quads.push(sel_bounds);
                            }
                        }
                        line_start_ix = line_end_ix;
                    }
                }
            }

            let mut caret_bounds = if is_active
                && is_focused
                && selection.is_empty()
                && !is_dragging
                && cursor_opacity > 0.01
            {
                let target_byte = raw_col_to_visible_byte(
                    cursor_col,
                    &spans,
                    is_active_caret,
                    disclosure_col,
                    is_source,
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

            let mut preedit = None;
            if let Some((text, anchor_byte)) = &ime_preedit
                && let Some(anchor) = wrapped_line.position_for_index(*anchor_byte, line_height)
                && let Some(line) =
                    shape_ime_preedit(text, &preedit_font, font_size, preedit_color, window)
            {
                let (origin, caret) = ime_preedit_placement(
                    bounds.origin,
                    anchor,
                    line.unwrapped_layout.width,
                    caret_bounds,
                );
                caret_bounds = caret;
                preedit = Some((line, origin));
            }

            LinePrepaintState::Shaped {
                wrapped_line: Box::new(wrapped_line),
                selection_quads,
                selection_color,
                caret_bounds,
                cursor_color,
                preedit,
            }
        },
        move |bounds, prepaint, window, cx| match prepaint {
            LinePrepaintState::Empty {
                caret_bounds,
                cursor_color,
                preedit,
            } => {
                if let Some((preedit_line, origin)) = preedit {
                    let _ = preedit_line.paint(
                        origin,
                        line_height,
                        TextAlign::default(),
                        None,
                        window,
                        cx,
                    );
                }
                if let Some(caret) = caret_bounds {
                    window.paint_quad(fill(caret, cursor_color));
                }
            }
            LinePrepaintState::Shaped {
                wrapped_line,
                selection_quads,
                selection_color,
                caret_bounds,
                cursor_color,
                preedit,
            } => {
                for sel_quad in selection_quads {
                    window.paint_quad(fill(sel_quad, selection_color));
                }
                let _ = wrapped_line.paint_background(
                    bounds.origin,
                    line_height,
                    TextAlign::default(),
                    None,
                    window,
                    cx,
                );
                let _ = wrapped_line.paint(
                    bounds.origin,
                    line_height,
                    TextAlign::default(),
                    None,
                    window,
                    cx,
                );
                if let Some((preedit_line, origin)) = preedit {
                    let _ = preedit_line.paint(
                        origin,
                        line_height,
                        TextAlign::default(),
                        None,
                        window,
                        cx,
                    );
                }
                if let Some(caret) = caret_bounds {
                    window.paint_quad(fill(caret, cursor_color));
                }
            }
            LinePrepaintState::None => {}
        },
    )
    .h(total_canvas_h)
    .w_full()
    .into_any_element()
}

#[derive(Clone)]
pub struct TableCellRenderContext<'a> {
    pub theme: &'a Theme,
    pub font_size: Pixels,
    pub line_height: Pixels,
    pub font: Font,
    pub is_active_cell: bool,
    pub disclosure_col: usize,
    pub cursor_col: usize,
    pub cursor_opacity: f32,
    pub align: crate::markdown::TableAlignment,
    pub hovered_link_range: Option<(usize, usize)>,
}

enum CellPrepaintState {
    Shaped {
        wrapped_line: Box<WrappedLine>,
        origin: Point<Pixels>,
        text_align: TextAlign,
        caret_bounds: Option<Bounds<Pixels>>,
        cursor_color: Hsla,
        preedit: Option<(Box<WrappedLine>, Point<Pixels>)>,
    },
    None,
}

pub fn render_shaped_cell_canvas(
    spans: &[InlineSpan],
    tcx: TableCellRenderContext,
) -> impl IntoElement {
    let spans = spans.to_vec();
    let font_size = tcx.font_size;
    let line_height = tcx.line_height;
    let font = tcx.font;
    let is_active_cell = tcx.is_active_cell;
    let disclosure_col = tcx.disclosure_col;
    let cursor_col = tcx.cursor_col;
    let cursor_opacity = tcx.cursor_opacity;
    let align = tcx.align;
    let hovered_link_range = tcx.hovered_link_range;

    canvas(
        move |bounds, window, cx| {
            let (line_str, runs, cursor_color, text_color) = {
                let theme = cx.theme();
                let scx =
                    LineShapingContext::new(theme, None, false, is_active_cell, disclosure_col);
                let (line_str, runs) =
                    build_line_runs(&spans, scx, hovered_link_range, &window.text_style());
                let cursor_color = theme.cursor.opacity(cursor_opacity);
                (line_str, runs, cursor_color, theme.text_primary)
            };

            let shaped = window.text_system().shape_text(
                line_str.into(),
                font_size,
                &runs,
                Some(bounds.size.width),
                None,
            );

            let Ok(lines) = shaped else {
                return CellPrepaintState::None;
            };

            let mut lines_iter = lines.into_iter();
            let Some(wrapped_line) = lines_iter.next() else {
                return CellPrepaintState::None;
            };

            let text_align = match align {
                crate::markdown::TableAlignment::Center => TextAlign::Center,
                crate::markdown::TableAlignment::Right => TextAlign::Right,
                _ => TextAlign::Left,
            };

            let num_visual_lines = wrapped_line.wrap_boundaries.len() + 1;
            let total_text_h = line_height * (num_visual_lines as f32);
            let origin_y = bounds.origin.y + (bounds.size.height - total_text_h).max(px(0.0)) / 2.0;
            let origin = point(bounds.origin.x, origin_y);
            let align_x = table_align_offset(
                bounds.size.width,
                wrapped_line.unwrapped_layout.width,
                align,
            );

            let mut caret_bounds = if is_active_cell && cursor_opacity > 0.01 {
                let target_byte =
                    raw_col_to_visible_byte(cursor_col, &spans, true, disclosure_col, false);
                if let Some(caret_pos) = wrapped_line.position_for_index(target_byte, line_height) {
                    let (caret_w, caret_h) = calculate_caret_size(font_size, line_height);
                    let caret_x = (origin.x + align_x + caret_pos.x).round();
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

            let cell_preedit = ime_preedit_overlay(&spans, true, disclosure_col, false);
            let mut preedit = None;
            if let Some((text, anchor_byte)) = cell_preedit.as_ref()
                && let Some(anchor) = wrapped_line.position_for_index(*anchor_byte, line_height)
                && let Some(line) = shape_ime_preedit(text, &font, font_size, text_color, window)
            {
                let (preedit_origin, caret) = ime_preedit_placement(
                    point(origin.x + align_x, origin.y),
                    anchor,
                    line.unwrapped_layout.width,
                    caret_bounds,
                );
                caret_bounds = caret;
                preedit = Some((line, preedit_origin));
            }

            CellPrepaintState::Shaped {
                wrapped_line: Box::new(wrapped_line),
                origin,
                text_align,
                caret_bounds,
                cursor_color,
                preedit,
            }
        },
        move |bounds, prepaint, window, cx| match prepaint {
            CellPrepaintState::Shaped {
                wrapped_line,
                origin,
                text_align,
                caret_bounds,
                cursor_color,
                preedit,
            } => {
                let _ = wrapped_line.paint_background(
                    origin,
                    line_height,
                    text_align,
                    Some(bounds),
                    window,
                    cx,
                );
                if let Some(caret) = caret_bounds {
                    window.paint_quad(fill(caret, cursor_color));
                }
                let _ =
                    wrapped_line.paint(origin, line_height, text_align, Some(bounds), window, cx);
                if let Some((preedit_line, preedit_origin)) = preedit {
                    let _ = preedit_line.paint(
                        preedit_origin,
                        line_height,
                        TextAlign::default(),
                        None,
                        window,
                        cx,
                    );
                }
            }
            CellPrepaintState::None => {}
        },
    )
    .size_full()
    .into_any_element()
}
