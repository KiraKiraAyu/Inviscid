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
    pub padding_vertical: Pixels,
    pub available_width: Pixels,
}

#[inline]
pub fn resolve_inner_available_width(
    inner_width: Pixels,
    kind: &BlockKind,
    mode: RenderMode,
    inner_indent: Pixels,
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
        BlockKind::TaskList { .. } | BlockKind::OrderedList { .. } => {
            (inner_width - px(24.0) - inner_indent).max(MIN_LINE_CONTENT_WIDTH)
        }
        BlockKind::BulletList => {
            (inner_width - px(22.0) - inner_indent).max(MIN_LINE_CONTENT_WIDTH)
        }
        _ => (inner_width - inner_indent).max(MIN_LINE_CONTENT_WIDTH),
    }
}

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
            padding_vertical: px(0.0),
            available_width: resolve_inner_available_width(
                content_width,
                &parsed.kind,
                mode,
                px(0.0),
            ),
        };
    }

    let (outer_quote_left, quote_top, inner_width, inner_indent) = if let Some(quote) = parsed.quote
    {
        let quote_depth = quote.depth.max(1);
        let outer_left = QUOTE_GUTTER * quote_depth as f32 + indent_px;
        let top = if quote.is_start {
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
        (outer_left, top, inner_w, inner_ind)
    } else {
        (px(0.0), px(0.0), content_width, indent_px)
    };

    let available_width =
        resolve_inner_available_width(inner_width, &parsed.kind, mode, inner_indent);

    if matches!(
        parsed.kind,
        BlockKind::CodeBlock {
            is_fence_start: false,
        } | BlockKind::MermaidBlock {
            is_fence_start: false,
        } | BlockKind::MathBlock { is_start: false }
            | BlockKind::ThematicBreak
    ) {
        return LineTextSpec {
            font_size: px(0.0),
            line_height: px(0.0),
            override_color: None,
            shaping_mode: TextShapingMode::Prose,
            is_heading: false,
            left_offset: outer_quote_left + inner_indent,
            top_offset: quote_top,
            padding_vertical: px(0.0),
            available_width,
        };
    }

    let typo = get_block_typography_with_metrics(&parsed.kind, base_font_size, line_height_factor);
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
            padding_vertical: MATH_EDITOR_PADDING_Y * 2.0,
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
            padding_vertical: typo.padding_vertical,
            available_width,
        },
        BlockKind::CodeBlock {
            is_fence_start: true,
        }
        | BlockKind::MermaidBlock {
            is_fence_start: true,
        } => LineTextSpec {
            font_size: typo.font_size,
            line_height: typo.line_height,
            override_color,
            shaping_mode: TextShapingMode::Code,
            is_heading: false,
            left_offset: outer_quote_left + CODE_BLOCK_PADDING_X,
            top_offset: quote_top + px(10.0),
            padding_vertical: typo.padding_vertical,
            available_width,
        },
        BlockKind::Heading { .. } => LineTextSpec {
            font_size: typo.font_size,
            line_height: typo.line_height,
            override_color,
            shaping_mode: TextShapingMode::Prose,
            is_heading: true,
            left_offset: outer_quote_left + inner_indent,
            top_offset: quote_top + px(14.0),
            padding_vertical: typo.padding_vertical,
            available_width,
        },
        BlockKind::MathBlock { is_start: true } => LineTextSpec {
            font_size: typo.font_size,
            line_height: typo.line_height,
            override_color,
            shaping_mode: TextShapingMode::Prose,
            is_heading: false,
            left_offset: outer_quote_left + inner_indent,
            top_offset: quote_top + px(8.0),
            padding_vertical: typo.padding_vertical,
            available_width,
        },
        BlockKind::BulletList => LineTextSpec {
            font_size: typo.font_size,
            line_height: typo.line_height,
            override_color,
            shaping_mode: TextShapingMode::Prose,
            is_heading: false,
            left_offset: outer_quote_left + inner_indent + px(22.0),
            top_offset: quote_top + px(4.0),
            padding_vertical: typo.padding_vertical,
            available_width,
        },
        BlockKind::TaskList { .. } | BlockKind::OrderedList { .. } => LineTextSpec {
            font_size: typo.font_size,
            line_height: typo.line_height,
            override_color,
            shaping_mode: TextShapingMode::Prose,
            is_heading: false,
            left_offset: outer_quote_left + inner_indent + px(24.0),
            top_offset: quote_top + px(4.0),
            padding_vertical: typo.padding_vertical,
            available_width,
        },
        BlockKind::Table { is_header, .. } => LineTextSpec {
            font_size: typo.font_size,
            line_height: typo.line_height,
            override_color,
            shaping_mode: TextShapingMode::Prose,
            is_heading: false,
            left_offset: outer_quote_left + inner_indent,
            top_offset: quote_top + if *is_header { px(8.0) } else { px(0.0) },
            padding_vertical: typo.padding_vertical,
            available_width,
        },
        BlockKind::Paragraph | BlockKind::Image { .. } => LineTextSpec {
            font_size: typo.font_size,
            line_height: typo.line_height,
            override_color,
            shaping_mode: TextShapingMode::Prose,
            is_heading: false,
            left_offset: outer_quote_left + inner_indent,
            top_offset: quote_top + px(4.0),
            padding_vertical: typo.padding_vertical,
            available_width,
        },
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
        let scale = f32::from(available_width / rendered_size.width);
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
    pub has_marked_range: bool,
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
