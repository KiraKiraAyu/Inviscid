use gpui::*;
use std::cell::RefCell;
use std::collections::HashMap;
use std::ops::Range;
use std::rc::Rc;
use std::sync::Arc;

use crate::editor::RenderMode;
use crate::editor::geometry::{
    CODE_BLOCK_PADDING_X, MATH_EDITOR_PADDING_X, MIN_LINE_CONTENT_WIDTH, PaintedLineGeometry,
    TextShapingMode, resolve_inner_available_width, resolve_inner_available_width_scaled,
};
use crate::markdown::{BlockKind, InlineSpan, MathSpan, ParsedLine};
use crate::theme::{ActiveTheme, Theme};

/// Horizontal space reserved for the outer blockquote border and inset.
pub const QUOTE_GUTTER: Pixels = px(18.0);
/// Vertical space added at the first and last line of a quote group.
pub const QUOTE_EDGE_PADDING: Pixels = px(4.0);

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

/// Computes the effective text content wrapping width for a specific block kind and render mode
pub fn compute_available_line_width_with_mode(
    content_width: Pixels,
    kind: &BlockKind,
    mode: RenderMode,
) -> Pixels {
    resolve_inner_available_width(content_width, kind, mode, px(0.0))
}

/// Computes the effective text content wrapping width for a specific block kind (defaults to Live Preview)
pub fn compute_available_line_width(content_width: Pixels, kind: &BlockKind) -> Pixels {
    compute_available_line_width_with_mode(content_width, kind, RenderMode::LivePreview)
}

/// Like [`get_block_left_offset`], plus a per-line visual indent in pixels.
pub fn get_block_left_offset_with_indent(
    kind: &BlockKind,
    mode: RenderMode,
    indent_px: Pixels,
) -> Pixels {
    get_block_left_offset(kind, mode) + indent_px
}

pub fn get_parsed_line_left_offset(
    parsed: &ParsedLine,
    mode: RenderMode,
    indent_px: Pixels,
    quote_content_indent_px: Pixels,
) -> Pixels {
    let quote_depth = parsed.quote.map_or(0, |quote| quote.depth);
    get_block_left_offset_with_indent(&parsed.kind, mode, indent_px)
        + if mode == RenderMode::LivePreview && parsed.quote.is_some() {
            QUOTE_GUTTER * quote_depth as f32
        } else {
            px(0.0)
        }
        + if mode == RenderMode::LivePreview
            && parsed.quote.is_some()
            && parsed.kind.has_syntax_prefix()
        {
            quote_content_indent_px
        } else {
            px(0.0)
        }
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

/// Like [`compute_available_line_width_with_mode`], minus a per-line visual indent.
pub fn compute_available_line_width_with_indent(
    content_width: Pixels,
    kind: &BlockKind,
    mode: RenderMode,
    indent_px: Pixels,
) -> Pixels {
    resolve_inner_available_width(content_width, kind, mode, indent_px)
}

pub fn parsed_line_available_width(
    content_width: Pixels,
    parsed: &ParsedLine,
    mode: RenderMode,
    indent_px: Pixels,
    quote_content_indent_px: Pixels,
) -> Pixels {
    let content_width = effective_content_width(content_width, parsed, mode, indent_px);
    let inner_indent = if mode == RenderMode::LivePreview && parsed.quote.is_some() {
        if parsed.kind.has_syntax_prefix() {
            quote_content_indent_px
        } else {
            px(0.0)
        }
    } else {
        indent_px
    };
    compute_available_line_width_with_indent(content_width, &parsed.kind, mode, inner_indent)
}

/// Computes the left horizontal offset of text content within a block container in Live Preview.
pub fn get_block_left_offset(kind: &BlockKind, mode: RenderMode) -> Pixels {
    if mode == RenderMode::Source {
        return px(0.0);
    }
    match kind {
        BlockKind::TaskList { .. } | BlockKind::BulletList | BlockKind::OrderedList { .. } => {
            crate::editor::geometry::list_marker_width(kind, 15.0)
        }
        BlockKind::CodeBlockContent | BlockKind::MermaidBlockContent => CODE_BLOCK_PADDING_X,
        BlockKind::CodeBlock {
            is_fence_start: true,
        }
        | BlockKind::MermaidBlock {
            is_fence_start: true,
        } => CODE_BLOCK_PADDING_X,
        BlockKind::MathBlockContent => MATH_EDITOR_PADDING_X,
        _ => px(0.0),
    }
}

/// Computes the top vertical offset of text content within a block container in Live Preview.
pub fn get_block_top_offset(kind: &BlockKind, mode: RenderMode) -> Pixels {
    if mode == RenderMode::Source {
        return px(0.0);
    }
    let dummy = ParsedLine {
        raw_text: String::new(),
        kind: kind.clone(),
        spans: Vec::new(),
        quote: None,
    };
    crate::editor::geometry::resolve_line_text_spec(
        &dummy,
        mode,
        px(800.0),
        px(0.0),
        px(0.0),
        15.0,
        1.6,
        &crate::theme::DEFAULT_THEME,
    )
    .top_offset
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
    raw_col_to_visible_byte_with_selection(
        raw_col,
        spans,
        is_active_caret,
        disclosure_col,
        is_source,
        false,
    )
}

pub fn raw_col_to_visible_byte_with_selection(
    raw_col: usize,
    spans: &[InlineSpan],
    is_active_caret: bool,
    disclosure_col: usize,
    is_source: bool,
    is_selection_disclosure: bool,
) -> usize {
    let mut cur_visible_byte = 0;
    let mut target_byte = None;

    for span in spans {
        let is_visible = span.is_visible_for_selection(
            is_active_caret,
            disclosure_col,
            is_source,
            is_selection_disclosure,
        );
        let (s_start, s_end) = span.span_range;

        if !is_visible {
            if target_byte.is_none() && raw_col <= s_end {
                target_byte = Some(cur_visible_byte);
            }
            continue;
        }

        let start_byte = cur_visible_byte;
        let rendered_text = span.rendered_text(is_active_caret, disclosure_col, is_source);

        if target_byte.is_none() {
            if raw_col <= s_start {
                target_byte = Some(start_byte);
            } else if raw_col <= s_end {
                if span.math.is_some()
                    && !span.math_is_disclosed(is_active_caret, disclosure_col, is_source)
                {
                    let end_byte = start_byte + rendered_text.len();
                    let midpoint = s_start + (s_end - s_start) / 2;
                    target_byte = Some(if raw_col <= midpoint {
                        start_byte
                    } else {
                        end_byte
                    });
                } else {
                    let byte_offset: usize = rendered_text
                        .chars()
                        .take(raw_col - s_start)
                        .map(char::len_utf8)
                        .sum();
                    target_byte = Some(start_byte + byte_offset);
                }
            }
        }

        cur_visible_byte += rendered_text.len();
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
    visible_byte_to_raw_col_with_selection(
        visible_byte,
        spans,
        is_active_caret,
        cursor_col,
        is_source,
        total_raw_len,
        false,
    )
}

pub fn visible_byte_to_raw_col_with_selection(
    visible_byte: usize,
    spans: &[InlineSpan],
    is_active_caret: bool,
    cursor_col: usize,
    is_source: bool,
    total_raw_len: usize,
    is_selection_disclosure: bool,
) -> usize {
    let mut cur_byte = 0;

    for span in spans {
        let is_visible = span.is_visible_for_selection(
            is_active_caret,
            cursor_col,
            is_source,
            is_selection_disclosure,
        );
        if !is_visible {
            continue;
        }

        let rendered_text = span.rendered_text(is_active_caret, cursor_col, is_source);
        let span_len = rendered_text.len();
        let span_end_byte = cur_byte + span_len;

        if visible_byte < span_end_byte {
            let offset_in_span_bytes = visible_byte.saturating_sub(cur_byte);
            if span.math.is_some()
                && !span.math_is_disclosed(is_active_caret, cursor_col, is_source)
            {
                let midpoint = span_len / 2;
                return if offset_in_span_bytes <= midpoint {
                    span.group_range.0
                } else {
                    span.group_range.1.min(total_raw_len)
                };
            }
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
            for c in rendered_text.chars() {
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
        | BlockKind::MermaidBlock {
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
    pub is_monospace: bool,
    pub is_active_caret: bool,
    pub cursor_col: usize,
    pub is_selection_disclosure: bool,
    pub is_heading: bool,
    pub ime_preedit_cols: Option<(usize, usize)>,
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
            is_monospace: is_source,
            is_active_caret,
            cursor_col,
            is_selection_disclosure: false,
            is_heading: false,
            ime_preedit_cols: None,
        }
    }

    pub fn with_monospace(mut self, is_monospace: bool) -> Self {
        self.is_monospace = is_monospace;
        self
    }

    pub fn with_selection_disclosure(mut self, is_selection_disclosure: bool) -> Self {
        self.is_selection_disclosure = is_selection_disclosure;
        self
    }

    pub fn with_heading(mut self, is_heading: bool) -> Self {
        self.is_heading = is_heading;
        self
    }

    pub fn with_ime_preedit(mut self, ime_preedit_cols: Option<(usize, usize)>) -> Self {
        self.ime_preedit_cols = ime_preedit_cols.filter(|(s, e)| s < e);
        self
    }
}

#[inline]
pub fn table_cell_font_metrics(is_header: bool) -> (Pixels, Pixels) {
    table_cell_font_metrics_for(is_header, 15.0, 1.6)
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

pub use crate::ui::cursor::calculate_caret_size;

#[cfg(test)]
pub(super) mod preparation_metrics {
    std::thread_local! {
        static CALLS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
    }

    pub fn reset() {
        CALLS.set(0);
    }

    pub fn calls() -> usize {
        CALLS.get()
    }

    pub fn record() {
        CALLS.set(CALLS.get() + 1);
    }
}

#[inline]
fn char_offset_to_byte(s: &str, char_idx: usize) -> usize {
    if char_idx == 0 {
        return 0;
    }
    s.char_indices()
        .nth(char_idx)
        .map_or(s.len(), |(byte_idx, _)| byte_idx)
}

pub fn build_line_runs(
    spans: &[InlineSpan],
    scx: LineShapingContext,
    hovered_link_range: Option<(usize, usize)>,
    base_style: &TextStyle,
) -> (String, Vec<TextRun>) {
    #[cfg(test)]
    preparation_metrics::record();
    let mut line_str = String::new();
    let mut runs = Vec::new();

    for span in spans {
        let is_visible = span.is_visible_for_selection(
            scx.is_active_caret,
            scx.cursor_col,
            scx.is_source,
            scx.is_selection_disclosure,
        );
        if !is_visible {
            continue;
        }

        let rendered_text = span.rendered_text(scx.is_active_caret, scx.cursor_col, scx.is_source);
        if rendered_text.is_empty() {
            continue;
        }

        let span_byte_len = rendered_text.len();
        line_str.push_str(&rendered_text);

        let mut font = base_style.font();
        let mut color = scx.override_color.unwrap_or(scx.theme.text_primary);
        let mut background_color = None;
        let mut strikethrough = None;
        let mut underline = None;

        if span.math_is_disclosed(scx.is_active_caret, scx.cursor_col, scx.is_source) == false
            && span.math.is_some()
        {
            color = color.opacity(0.0);
        }

        if scx.is_source || scx.is_monospace || span.is_code || span.syntax_token.is_some() {
            font.family = crate::platform::platform_monospace_font().into();
        } else {
            font.family = crate::platform::platform_ui_font().into();
        }

        if let Some(token) = span.syntax_token {
            color = token.color(scx.theme);
            if token.is_italic() {
                font.style = FontStyle::Italic;
            }
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

        if (span.is_marker && !span.is_code && !span.is_link)
            || (scx.is_source
                && span
                    .math
                    .as_ref()
                    .is_some_and(|math| math.is_display && math.source == "$$"))
        {
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

        if let Some((pre_start, pre_end)) = scx.ime_preedit_cols
            && pre_start < pre_end
            && span.math.is_none()
            && span.span_range.0 < pre_end
            && span.span_range.1 > pre_start
        {
            let s = span.span_range.0;
            let e = span.span_range.1;
            let char_count = rendered_text.chars().count();
            let mid_start = pre_start.max(s);
            let mid_end = pre_end.min(e).min(s + char_count);
            if mid_start < mid_end {
                let b_start = char_offset_to_byte(&rendered_text, mid_start - s);
                let b_end = char_offset_to_byte(&rendered_text, mid_end - s);
                let preedit_underline = Some(UnderlineStyle {
                    thickness: px(1.0),
                    color: Some(color),
                    wavy: false,
                });
                for (len, run_underline) in [
                    (b_start, underline),
                    (b_end.saturating_sub(b_start), preedit_underline),
                    (span_byte_len.saturating_sub(b_end), underline),
                ] {
                    if len > 0 {
                        runs.push(TextRun {
                            len,
                            font: font.clone(),
                            color,
                            background_color,
                            underline: run_underline,
                            strikethrough,
                        });
                    }
                }
                continue;
            }
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
    if is_source
        || matches!(
            kind,
            BlockKind::CodeBlockContent
                | BlockKind::MermaidBlockContent
                | BlockKind::MathBlockContent
        )
    {
        font(crate::platform::platform_monospace_font())
    } else if matches!(kind, BlockKind::Heading { .. }) {
        font(crate::platform::platform_ui_font()).bold()
    } else {
        font(crate::platform::platform_ui_font())
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

/// Computes wrapped visual line count by shaping the styled runs with GPUI's `shape_text`,
/// the same path used for painting, so layout heights match rendered wrapping exactly.
/// Falls back to token- and boundary-aware estimation when no text system is available.
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

/// Computes wrapped visual line count using `calculate_visual_lines_with_runs` with default single-run styling.
pub fn calculate_visual_lines(
    text: &str,
    font: Font,
    font_size: Pixels,
    available_width: Pixels,
    soft_wrap: bool,
    text_system: Option<&Arc<WindowTextSystem>>,
    is_monospace: bool,
) -> usize {
    calculate_visual_lines_with_runs(
        text,
        &[],
        font,
        font_size,
        available_width,
        soft_wrap,
        text_system,
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

/// Token- and boundary-aware line wrap estimation used when `TextSystem` is unavailable.
pub fn calculate_wrapped_visual_lines_with_mode(
    text: &str,
    font_size: Pixels,
    available_width: Pixels,
    soft_wrap: bool,
    is_monospace: bool,
) -> usize {
    calculate_wrapped_visual_lines_with_runs_fallback(
        text,
        &[],
        font_size,
        available_width,
        soft_wrap,
        is_monospace,
    )
}

pub fn calculate_table_row_height(
    cells: &[crate::markdown::TableCell],
    is_header: bool,
    is_active: bool,
    cursor_col: usize,
    container_width: Pixels,
    text_system: Option<&Arc<WindowTextSystem>>,
    is_selection_disclosure: bool,
) -> Pixels {
    calculate_table_row_height_with_preedit(
        cells,
        is_header,
        is_active,
        cursor_col,
        container_width,
        text_system,
        is_selection_disclosure,
        None,
    )
}

pub fn calculate_table_row_height_with_preedit(
    cells: &[crate::markdown::TableCell],
    is_header: bool,
    is_active: bool,
    cursor_col: usize,
    container_width: Pixels,
    text_system: Option<&Arc<WindowTextSystem>>,
    is_selection_disclosure: bool,
    ime_preedit_cols: Option<(usize, usize)>,
) -> Pixels {
    calculate_table_row_height_with_metrics(
        cells,
        is_header,
        is_active,
        cursor_col,
        container_width,
        text_system,
        is_selection_disclosure,
        ime_preedit_cols,
        15.0,
        1.6,
    )
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

use super::{LineHighlight, LineRenderContext};

const SHAPED_ROW_CACHE_LIMIT: usize = 512;

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub(crate) struct ShapedLineKey {
    pub buffer_revision: u64,
    pub line_idx: usize,
    pub cell_idx: Option<usize>,
    pub theme_fingerprint: u64,
    pub font_size_bits: u32,
    pub wrap_width_bits: Option<u32>,
    pub override_color_bits: Option<(u32, u32, u32, u32)>,
    pub is_source: bool,
    pub is_monospace: bool,
    pub is_heading: bool,
    pub is_active_caret: bool,
    pub disclosure_col: usize,
    pub is_selection_disclosure: bool,
    pub ime_preedit_cols: Option<(usize, usize)>,
    pub hovered_link_range: Option<(usize, usize)>,
}

impl ShapedLineKey {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        buffer_revision: u64,
        line_idx: usize,
        cell_idx: Option<usize>,
        theme: &Theme,
        font_size: Pixels,
        wrap_width: Option<Pixels>,
        override_color: Option<Hsla>,
        scx: &LineShapingContext<'_>,
        hovered_link_range: Option<(usize, usize)>,
    ) -> Self {
        let effective_active = !scx.is_source && scx.is_active_caret;
        Self {
            buffer_revision,
            line_idx,
            cell_idx,
            theme_fingerprint: theme.fingerprint(),
            font_size_bits: f32::from(font_size).to_bits(),
            wrap_width_bits: wrap_width.map(|w| f32::from(w).to_bits()),
            override_color_bits: override_color
                .map(|c| (c.h.to_bits(), c.s.to_bits(), c.l.to_bits(), c.a.to_bits())),
            is_source: scx.is_source,
            is_monospace: scx.is_monospace,
            is_heading: scx.is_heading,
            is_active_caret: effective_active,
            disclosure_col: if effective_active { scx.cursor_col } else { 0 },
            is_selection_disclosure: effective_active && scx.is_selection_disclosure,
            ime_preedit_cols: scx.ime_preedit_cols,
            hovered_link_range,
        }
    }
}

#[derive(Clone)]
pub(crate) struct CachedShapedLine {
    pub wrapped_line: Option<Arc<WrappedLine>>,
    pub visual_lines: usize,
}

#[derive(Default)]
pub(crate) struct ShapedRowCache {
    entries: HashMap<ShapedLineKey, (CachedShapedLine, u64)>,
    tick: u64,
}

impl ShapedRowCache {
    pub fn get(&mut self, key: &ShapedLineKey) -> Option<CachedShapedLine> {
        let (entry, last_used) = self.entries.get_mut(key)?;
        self.tick = self.tick.wrapping_add(1);
        *last_used = self.tick;
        Some(entry.clone())
    }

    pub fn insert(&mut self, key: ShapedLineKey, value: CachedShapedLine) {
        if !self.entries.contains_key(&key)
            && self.entries.len() >= SHAPED_ROW_CACHE_LIMIT
            && let Some(oldest_key) = self
                .entries
                .iter()
                .min_by_key(|(_, (_, used))| *used)
                .map(|(k, _)| k.clone())
        {
            self.entries.remove(&oldest_key);
        }
        self.tick = self.tick.wrapping_add(1);
        self.entries.insert(key, (value, self.tick));
    }

    pub fn clear(&mut self) {
        self.entries.clear();
    }

    pub fn invalidate_range(&mut self, range: Range<usize>) {
        self.entries
            .retain(|key, _| key.line_idx < range.start || key.line_idx >= range.end);
    }

    #[allow(clippy::too_many_arguments)]
    pub fn apply_edit(
        &mut self,
        old_rev: u64,
        new_rev: u64,
        line_delta: isize,
        dirty_old_range: Range<usize>,
        edit_old_range: Range<usize>,
        structural: bool,
        mut can_reuse_dirty_neighbor: impl FnMut(usize, usize) -> bool,
    ) {
        let previous: Vec<_> = self.entries.drain().collect();
        for (mut key, value) in previous {
            if key.buffer_revision != old_rev {
                continue;
            }
            if key.line_idx < dirty_old_range.start {
                key.buffer_revision = new_rev;
                self.entries.insert(key, value);
            } else if !structural && key.line_idx >= dirty_old_range.end {
                key.buffer_revision = new_rev;
                key.line_idx = ((key.line_idx as isize) + line_delta).max(0) as usize;
                self.entries.insert(key, value);
            } else if !structural
                && (key.line_idx < edit_old_range.start || key.line_idx >= edit_old_range.end)
            {
                let new_line_idx = if key.line_idx < edit_old_range.start {
                    key.line_idx
                } else {
                    ((key.line_idx as isize) + line_delta).max(0) as usize
                };
                if can_reuse_dirty_neighbor(key.line_idx, new_line_idx) {
                    key.buffer_revision = new_rev;
                    key.line_idx = new_line_idx;
                    self.entries.insert(key, value);
                }
            }
        }
    }

    #[cfg(test)]
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    #[cfg(test)]
    pub fn contains_line(&self, line_idx: usize, buffer_revision: u64) -> bool {
        self.entries
            .keys()
            .any(|key| key.line_idx == line_idx && key.buffer_revision == buffer_revision)
    }
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn shape_line_or_cached(
    key: &ShapedLineKey,
    spans: &[InlineSpan],
    scx: LineShapingContext<'_>,
    hovered_link_range: Option<(usize, usize)>,
    font_size: Pixels,
    wrap_width: Option<Pixels>,
    text_system: &Arc<WindowTextSystem>,
    base_style: &TextStyle,
    cache: Option<&Rc<RefCell<ShapedRowCache>>>,
) -> Option<CachedShapedLine> {
    if let Some(cache) = cache
        && let Some(hit) = cache.borrow_mut().get(key)
    {
        return Some(hit);
    }

    let (line_str, runs) = build_line_runs(spans, scx, hovered_link_range, base_style);
    let entry = if line_str.is_empty() || runs.is_empty() {
        CachedShapedLine {
            wrapped_line: None,
            visual_lines: 1,
        }
    } else {
        let shaped = text_system
            .shape_text(line_str.into(), font_size, &runs, wrap_width, None)
            .ok()?;
        let mut iter = shaped.into_iter();
        let wrapped_line = iter.next()?;
        let visual_lines = wrapped_line.wrap_boundaries.len() + 1;
        CachedShapedLine {
            wrapped_line: Some(Arc::new(wrapped_line)),
            visual_lines,
        }
    };

    if let Some(cache) = cache {
        cache.borrow_mut().insert(key.clone(), entry.clone());
    }
    Some(entry)
}

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
    range: std::ops::Range<usize>,
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

pub(crate) fn math_render_size(math: &crate::math::MathSvg, font_size: Pixels) -> Size<Pixels> {
    size(
        px(math.width_milli_em as f32 * f32::from(font_size) * crate::math::MATH_SCALE / 1000.0),
        px(math.height_milli_em as f32 * f32::from(font_size) * crate::math::MATH_SCALE / 1000.0),
    )
}

fn math_vertical_offset(line_height: Pixels, math_height: Pixels) -> Pixels {
    (line_height.max(math_height) - math_height) / 2.0
}

fn collect_math_runs(
    spans: &[InlineSpan],
    is_active_line: bool,
    cursor_col: usize,
    is_source: bool,
    is_selection_disclosure: bool,
) -> Vec<(MathSpan, std::ops::Range<usize>)> {
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

fn math_paints_for_row(
    math_runs: &[(MathSpan, std::ops::Range<usize>)],
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
