use gpui::*;
use std::collections::HashMap;
use std::sync::Arc;

use super::RenderMode;
use super::layout::{
    SOURCE_GUTTER_WIDTH, get_block_color_override, get_block_typography_with_metrics,
};
use super::shaping::{QUOTE_EDGE_PADDING, QUOTE_GUTTER, effective_content_width, math_render_size};
use crate::buffer::SourceLine;
use crate::markdown::{BlockKind, ParsedLine};
use crate::theme::Theme;

pub const MATH_EDITOR_PADDING_X: Pixels = px(12.0);
pub const MATH_EDITOR_PADDING_Y: Pixels = px(4.0);
pub const CODE_BLOCK_PADDING_X: Pixels = px(14.0);
pub const MIN_LINE_CONTENT_WIDTH: Pixels = px(100.0);
pub const IMAGE_CARD_MARGIN_Y: Pixels = px(6.0);
pub const IMAGE_SOURCE_GAP: Pixels = px(6.0);

#[inline]
pub fn table_row_margin(base_font_size: f32) -> Pixels {
    px((8.0 * (base_font_size / 15.0)).round())
}

#[inline]
pub fn mermaid_preview_chrome(base_font_size: f32) -> (Pixels, Pixels) {
    let scale = base_font_size / 15.0;
    (px((6.0 * scale).round()), px((10.0 * scale).round()))
}

#[inline]
pub fn list_marker_width(kind: &BlockKind, base_font_size: f32) -> Pixels {
    let scale = base_font_size / 15.0;
    match kind {
        BlockKind::TaskList { .. } | BlockKind::OrderedList { .. } => px((24.0 * scale).round()),
        BlockKind::BulletList => px((22.0 * scale).round()),
        _ => px(0.0),
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TextShapingMode {
    Prose,
    Code,
    RawSource,
}

impl TextShapingMode {
    #[inline]
    pub const fn is_source(self) -> bool {
        matches!(self, Self::RawSource)
    }

    #[inline]
    pub const fn is_monospace(self) -> bool {
        matches!(self, Self::Code | Self::RawSource)
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LineTextSpec {
    pub font_size: Pixels,
    pub line_height: Pixels,
    pub override_color: Option<Hsla>,
    pub shaping_mode: TextShapingMode,
    pub is_heading: bool,
    pub left_offset: Pixels,
    pub top_offset: Pixels,
    pub margin_top: Pixels,
    pub margin_bottom: Pixels,
    pub padding_top: Pixels,
    pub padding_bottom: Pixels,
    pub padding_vertical: Pixels,
    pub padding_x: Pixels,
    pub marker_width: Pixels,
    pub fixed_height: Option<Pixels>,
    pub quote_top_padding: Pixels,
    pub quote_bottom_padding: Pixels,
    pub available_width: Pixels,
}

impl LineTextSpec {
    #[inline]
    pub fn text_block_content_height(
        &self,
        visual_lines: usize,
        max_math_height: Pixels,
    ) -> Pixels {
        (self.line_height * (visual_lines as f32)).max(max_math_height)
    }
}

#[inline]
pub fn resolve_inner_available_width(
    inner_width: Pixels,
    kind: &BlockKind,
    mode: RenderMode,
    inner_indent: Pixels,
) -> Pixels {
    resolve_inner_available_width_scaled(inner_width, kind, mode, inner_indent, 15.0)
}

#[inline]
pub fn resolve_inner_available_width_scaled(
    inner_width: Pixels,
    kind: &BlockKind,
    mode: RenderMode,
    inner_indent: Pixels,
    base_font_size: f32,
) -> Pixels {
    if mode == RenderMode::Source {
        return (inner_width - SOURCE_GUTTER_WIDTH).max(MIN_LINE_CONTENT_WIDTH);
    }

    match kind {
        BlockKind::MathBlockContent => {
            (inner_width / 2.0 - MATH_EDITOR_PADDING_X * 2.0).max(MIN_LINE_CONTENT_WIDTH)
        }
        BlockKind::CodeBlockContent
        | BlockKind::MermaidBlockContent
        | BlockKind::CodeBlock {
            is_fence_start: true,
        }
        | BlockKind::MermaidBlock {
            is_fence_start: true,
        } => (inner_width - CODE_BLOCK_PADDING_X * 2.0).max(MIN_LINE_CONTENT_WIDTH),
        BlockKind::TaskList { .. } | BlockKind::OrderedList { .. } | BlockKind::BulletList => {
            let marker_w = list_marker_width(kind, base_font_size);
            (inner_width - marker_w - inner_indent).max(MIN_LINE_CONTENT_WIDTH)
        }
        _ => (inner_width - inner_indent).max(MIN_LINE_CONTENT_WIDTH),
    }
}

#[allow(clippy::too_many_arguments)]
pub fn resolve_line_text_spec(
    parsed: &ParsedLine,
    mode: RenderMode,
    content_width: Pixels,
    indent_px: Pixels,
    quote_content_indent_px: Pixels,
    base_font_size: f32,
    line_height_factor: f32,
    theme: &Theme,
) -> LineTextSpec {
    if mode == RenderMode::Source {
        return LineTextSpec {
            font_size: px(base_font_size),
            line_height: px((base_font_size * line_height_factor).round()),
            override_color: None,
            shaping_mode: TextShapingMode::RawSource,
            is_heading: false,
            left_offset: SOURCE_GUTTER_WIDTH,
            top_offset: px(0.0),
            margin_top: px(0.0),
            margin_bottom: px(0.0),
            padding_top: px(0.0),
            padding_bottom: px(0.0),
            padding_vertical: px(0.0),
            padding_x: SOURCE_GUTTER_WIDTH,
            marker_width: SOURCE_GUTTER_WIDTH,
            fixed_height: None,
            quote_top_padding: px(0.0),
            quote_bottom_padding: px(0.0),
            available_width: resolve_inner_available_width_scaled(
                content_width,
                &parsed.kind,
                mode,
                px(0.0),
                base_font_size,
            ),
        };
    }

    let (outer_quote_left, quote_top, quote_bottom, inner_width, inner_indent) =
        if let Some(quote) = parsed.quote {
            let quote_depth = quote.depth.max(1);
            let outer_left = QUOTE_GUTTER * quote_depth as f32 + indent_px;
            let top = if quote.is_start {
                QUOTE_EDGE_PADDING
            } else {
                px(0.0)
            };
            let bottom = if quote.is_end {
                QUOTE_EDGE_PADDING
            } else {
                px(0.0)
            };
            let inner_w = effective_content_width(content_width, parsed, mode, indent_px);
            let inner_ind = if parsed.kind.has_syntax_prefix() {
                quote_content_indent_px
            } else {
                px(0.0)
            };
            (outer_left, top, bottom, inner_w, inner_ind)
        } else {
            (px(0.0), px(0.0), px(0.0), content_width, indent_px)
        };

    let available_width = resolve_inner_available_width_scaled(
        inner_width,
        &parsed.kind,
        mode,
        inner_indent,
        base_font_size,
    );
    let scale = base_font_size / 15.0;
    let typo = get_block_typography_with_metrics(&parsed.kind, base_font_size, line_height_factor);

    match &parsed.kind {
        BlockKind::CodeBlock {
            is_fence_start: false,
        }
        | BlockKind::MermaidBlock {
            is_fence_start: false,
        } => {
            let box_h = px((6.0 * scale).round());
            let mb = px((6.0 * scale).round());
            return LineTextSpec {
                font_size: px(0.0),
                line_height: px(0.0),
                override_color: None,
                shaping_mode: TextShapingMode::Prose,
                is_heading: false,
                left_offset: outer_quote_left + inner_indent,
                top_offset: quote_top,
                margin_top: px(0.0),
                margin_bottom: mb,
                padding_top: px(0.0),
                padding_bottom: px(0.0),
                padding_vertical: px(0.0),
                padding_x: px(0.0),
                marker_width: px(0.0),
                fixed_height: Some(box_h),
                quote_top_padding: quote_top,
                quote_bottom_padding: quote_bottom,
                available_width,
            };
        }
        BlockKind::MathBlock { is_start: false } => {
            return LineTextSpec {
                font_size: px(0.0),
                line_height: px(0.0),
                override_color: None,
                shaping_mode: TextShapingMode::Prose,
                is_heading: false,
                left_offset: outer_quote_left + inner_indent,
                top_offset: quote_top,
                margin_top: px(0.0),
                margin_bottom: px(0.0),
                padding_top: px(0.0),
                padding_bottom: px(0.0),
                padding_vertical: px(0.0),
                padding_x: px(0.0),
                marker_width: px(0.0),
                fixed_height: Some(px(0.0)),
                quote_top_padding: quote_top,
                quote_bottom_padding: quote_bottom,
                available_width,
            };
        }
        BlockKind::ThematicBreak => {
            let pt = px((7.0 * scale).round());
            let pb = px((7.0 * scale).round());
            return LineTextSpec {
                font_size: px(0.0),
                line_height: px(0.0),
                override_color: None,
                shaping_mode: TextShapingMode::Prose,
                is_heading: false,
                left_offset: outer_quote_left + inner_indent,
                top_offset: quote_top,
                margin_top: px(0.0),
                margin_bottom: px(0.0),
                padding_top: pt,
                padding_bottom: pb,
                padding_vertical: typo.padding_vertical,
                padding_x: px(0.0),
                marker_width: px(0.0),
                fixed_height: Some(typo.base_layout_height),
                quote_top_padding: quote_top,
                quote_bottom_padding: quote_bottom,
                available_width,
            };
        }
        _ => {}
    }

    let override_color = get_block_color_override(&parsed.kind, theme);

    match &parsed.kind {
        BlockKind::MathBlockContent => LineTextSpec {
            font_size: px(base_font_size),
            line_height: px((base_font_size * line_height_factor).round()),
            override_color: None,
            shaping_mode: TextShapingMode::RawSource,
            is_heading: false,
            left_offset: outer_quote_left + MATH_EDITOR_PADDING_X,
            top_offset: quote_top + MATH_EDITOR_PADDING_Y,
            margin_top: px(0.0),
            margin_bottom: px(0.0),
            padding_top: MATH_EDITOR_PADDING_Y,
            padding_bottom: MATH_EDITOR_PADDING_Y,
            padding_vertical: MATH_EDITOR_PADDING_Y * 2.0,
            padding_x: MATH_EDITOR_PADDING_X,
            marker_width: px(0.0),
            fixed_height: None,
            quote_top_padding: quote_top,
            quote_bottom_padding: quote_bottom,
            available_width,
        },
        BlockKind::CodeBlockContent | BlockKind::MermaidBlockContent => LineTextSpec {
            font_size: typo.font_size,
            line_height: typo.line_height,
            override_color,
            shaping_mode: TextShapingMode::Code,
            is_heading: false,
            left_offset: outer_quote_left + CODE_BLOCK_PADDING_X,
            top_offset: quote_top + px(1.0),
            margin_top: px(0.0),
            margin_bottom: px(0.0),
            padding_top: px(1.0),
            padding_bottom: px(1.0),
            padding_vertical: px(2.0),
            padding_x: CODE_BLOCK_PADDING_X,
            marker_width: px(0.0),
            fixed_height: None,
            quote_top_padding: quote_top,
            quote_bottom_padding: quote_bottom,
            available_width,
        },
        BlockKind::CodeBlock {
            is_fence_start: true,
        }
        | BlockKind::MermaidBlock {
            is_fence_start: true,
        } => {
            let mt = px((6.0 * scale).round());
            let pt = px((4.0 * scale).round());
            let pb = px((4.0 * scale).round());
            LineTextSpec {
                font_size: typo.font_size,
                line_height: typo.line_height,
                override_color,
                shaping_mode: TextShapingMode::Code,
                is_heading: false,
                left_offset: outer_quote_left + CODE_BLOCK_PADDING_X,
                top_offset: quote_top + mt + pt,
                margin_top: mt,
                margin_bottom: px(0.0),
                padding_top: pt,
                padding_bottom: pb,
                padding_vertical: mt + pt + pb,
                padding_x: CODE_BLOCK_PADDING_X,
                marker_width: px(0.0),
                fixed_height: None,
                quote_top_padding: quote_top,
                quote_bottom_padding: quote_bottom,
                available_width,
            }
        }
        BlockKind::Heading { .. } => {
            let pt = px((14.0 * scale).round());
            let pb = px((4.0 * scale).round());
            LineTextSpec {
                font_size: typo.font_size,
                line_height: typo.line_height,
                override_color,
                shaping_mode: TextShapingMode::Prose,
                is_heading: true,
                left_offset: outer_quote_left + inner_indent,
                top_offset: quote_top + pt,
                margin_top: px(0.0),
                margin_bottom: px(0.0),
                padding_top: pt,
                padding_bottom: pb,
                padding_vertical: pt + pb,
                padding_x: px(0.0),
                marker_width: px(0.0),
                fixed_height: None,
                quote_top_padding: quote_top,
                quote_bottom_padding: quote_bottom,
                available_width,
            }
        }
        BlockKind::MathBlock { is_start: true } => {
            let pt = px((8.0 * scale).round());
            let pb = px((8.0 * scale).round());
            LineTextSpec {
                font_size: typo.font_size,
                line_height: typo.line_height,
                override_color,
                shaping_mode: TextShapingMode::Prose,
                is_heading: false,
                left_offset: outer_quote_left + inner_indent,
                top_offset: quote_top + pt,
                margin_top: px(0.0),
                margin_bottom: px(0.0),
                padding_top: pt,
                padding_bottom: pb,
                padding_vertical: pt + pb,
                padding_x: px(0.0),
                marker_width: px(0.0),
                fixed_height: None,
                quote_top_padding: quote_top,
                quote_bottom_padding: quote_bottom,
                available_width,
            }
        }
        BlockKind::BulletList | BlockKind::TaskList { .. } | BlockKind::OrderedList { .. } => {
            let marker_w = list_marker_width(&parsed.kind, base_font_size);
            let pt = px((4.0 * scale).round());
            let pb = px((4.0 * scale).round());
            LineTextSpec {
                font_size: typo.font_size,
                line_height: typo.line_height,
                override_color,
                shaping_mode: TextShapingMode::Prose,
                is_heading: false,
                left_offset: outer_quote_left + inner_indent + marker_w,
                top_offset: quote_top + pt,
                margin_top: px(0.0),
                margin_bottom: px(0.0),
                padding_top: pt,
                padding_bottom: pb,
                padding_vertical: pt + pb,
                padding_x: px(0.0),
                marker_width: marker_w,
                fixed_height: None,
                quote_top_padding: quote_top,
                quote_bottom_padding: quote_bottom,
                available_width,
            }
        }
        BlockKind::Table { is_header, .. } => {
            let mt = if *is_header {
                table_row_margin(base_font_size)
            } else {
                px(0.0)
            };
            LineTextSpec {
                font_size: typo.font_size,
                line_height: typo.line_height,
                override_color,
                shaping_mode: TextShapingMode::Prose,
                is_heading: false,
                left_offset: outer_quote_left + inner_indent,
                top_offset: quote_top + mt,
                margin_top: mt,
                margin_bottom: table_row_margin(base_font_size),
                padding_top: px(0.0),
                padding_bottom: px(0.0),
                padding_vertical: typo.padding_vertical,
                padding_x: px(0.0),
                marker_width: px(0.0),
                fixed_height: None,
                quote_top_padding: quote_top,
                quote_bottom_padding: quote_bottom,
                available_width,
            }
        }
        BlockKind::Paragraph | BlockKind::Image { .. } => {
            let pt = px((4.0 * scale).round());
            let pb = px((4.0 * scale).round());
            LineTextSpec {
                font_size: typo.font_size,
                line_height: typo.line_height,
                override_color,
                shaping_mode: TextShapingMode::Prose,
                is_heading: false,
                left_offset: outer_quote_left + inner_indent,
                top_offset: quote_top + pt,
                margin_top: px(0.0),
                margin_bottom: px(0.0),
                padding_top: pt,
                padding_bottom: pb,
                padding_vertical: pt + pb,
                padding_x: px(0.0),
                marker_width: px(0.0),
                fixed_height: None,
                quote_top_padding: quote_top,
                quote_bottom_padding: quote_bottom,
                available_width,
            }
        }
        BlockKind::CodeBlock {
            is_fence_start: false,
        }
        | BlockKind::MermaidBlock {
            is_fence_start: false,
        }
        | BlockKind::MathBlock { is_start: false }
        | BlockKind::ThematicBreak => unreachable!("handled above"),
    }
}

pub(crate) fn math_preview_display_size(
    math: &crate::math::MathSvg,
    font_size: Pixels,
    content_width: Pixels,
) -> Size<Pixels> {
    let available_width = (content_width / 2.0 - MATH_EDITOR_PADDING_X * 2.0).max(px(0.0));
    let rendered_size = math_render_size(math, font_size);
    if rendered_size.width > available_width && rendered_size.width > px(0.0) {
        let scale = available_width / rendered_size.width;
        size(rendered_size.width * scale, rendered_size.height * scale)
    } else {
        rendered_size
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PaintedFrameKey {
    pub buffer_revision: u64,
    pub render_mode: RenderMode,
    pub viewport_bounds: Bounds<Pixels>,
    pub scroll_anchor_line: Option<usize>,
    pub scroll_top: Pixels,
    pub fold_version: usize,
    pub font_size: f32,
    pub line_height: f32,
    pub soft_wrap: bool,
    pub active_math_block: Option<(SourceLine, SourceLine)>,
    pub active_mermaid_block: Option<(SourceLine, SourceLine)>,
    pub marked_range: Option<(usize, usize)>,
}

#[derive(Clone)]
pub struct PaintedLineGeometry {
    pub text_bounds: Bounds<Pixels>,
    pub line_height: Pixels,
    pub wrapped_line: Option<Arc<WrappedLine>>,
    pub is_source: bool,
    pub is_active_caret: bool,
    pub disclosure_col: usize,
    pub is_selection_disclosure: bool,
}

#[derive(Default)]
pub struct PaintedLineGeometryCache {
    frame_key: Option<PaintedFrameKey>,
    lines: HashMap<SourceLine, PaintedLineGeometry>,
}

impl PaintedLineGeometryCache {
    pub fn begin_frame(&mut self, key: PaintedFrameKey) {
        self.frame_key = Some(key);
        self.lines.clear();
    }

    pub fn clear(&mut self) {
        self.frame_key = None;
        self.lines.clear();
    }

    pub fn record(&mut self, line: SourceLine, entry: PaintedLineGeometry) {
        if entry.text_bounds.size.height <= px(0.0) {
            return;
        }
        self.lines.insert(line, entry);
    }

    pub fn get(
        &self,
        line: SourceLine,
        expected_key: PaintedFrameKey,
        is_active_caret: bool,
        disclosure_col: usize,
        is_selection_disclosure: bool,
    ) -> Option<PaintedLineGeometry> {
        if self.frame_key != Some(expected_key) {
            return None;
        }
        let entry = self.lines.get(&line)?;
        if !entry.is_source
            && (entry.is_active_caret != is_active_caret
                || entry.is_selection_disclosure != is_selection_disclosure
                || (is_active_caret && entry.disclosure_col != disclosure_col))
        {
            return None;
        }
        Some(entry.clone())
    }

    #[cfg(test)]
    pub(crate) fn len(&self) -> usize {
        self.lines.len()
    }
}
