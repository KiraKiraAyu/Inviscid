use gpui::*;
use std::ops::Range;
use std::path::PathBuf;
use std::sync::Arc;

use super::line_metrics::DOCUMENT_BOTTOM_PADDING;
#[cfg(test)]
use super::line_metrics::LineMetrics;
use super::{Editor, RenderMode};
use crate::buffer::{LineSource, SourceLine};
use crate::editor::shaping::compute_content_width;
#[cfg(test)]
use crate::markdown::MarkdownScanner;
use crate::markdown::{BlockKind, ParsedLine, QuoteProjection};
use crate::theme::Theme;

/// Width reserved for the line-number gutter in Source mode.
pub const SOURCE_GUTTER_WIDTH: Pixels = px(52.0);

/// Total horizontal padding subtracted from window width when sizing the document container.
pub const CONTENT_HORIZONTAL_PADDING: Pixels = px(48.0);

/// Maximum content container width in Source mode.
pub const MAX_CONTENT_WIDTH_SOURCE: Pixels = px(960.0);

/// Maximum content container width in Live Preview mode.
pub const MAX_CONTENT_WIDTH_LIVE: Pixels = px(860.0);

/// Bottom overscroll buffer allowing the last line to sit comfortably above the window bottom.
pub const BOTTOM_OVERSCROLL: Pixels = px(96.0);

const LAYOUT_OVERSCAN_LINES: usize = 32;

#[cfg(test)]
mod lazy_tests;

#[cfg(test)]
pub(crate) fn quote_ranges(parsed_lines: &[ParsedLine]) -> Vec<Range<usize>> {
    let mut ranges = Vec::new();
    let mut start = None;

    for (idx, parsed) in parsed_lines.iter().enumerate() {
        if parsed.quote.is_some() {
            start.get_or_insert(idx);
        } else if let Some(range_start) = start.take() {
            ranges.push(range_start..idx);
        }
    }
    if let Some(range_start) = start {
        ranges.push(range_start..parsed_lines.len());
    }
    ranges
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BlockTypography {
    pub font_size: Pixels,
    pub line_height: Pixels,
    pub padding_vertical: Pixels,
    pub base_layout_height: Pixels,
}

impl BlockTypography {
    pub fn new(font_size: Pixels, line_height: Pixels, padding_vertical: Pixels) -> Self {
        Self {
            font_size,
            line_height,
            padding_vertical,
            base_layout_height: line_height + padding_vertical,
        }
    }
}

pub fn get_block_typography(kind: &BlockKind) -> BlockTypography {
    get_block_typography_with_metrics(kind, 15.0, 1.6)
}

pub fn get_block_typography_with_metrics(
    kind: &BlockKind,
    base_font_size: f32,
    line_height_factor: f32,
) -> BlockTypography {
    let scale = base_font_size / 15.0;
    let lh_ratio = line_height_factor / 1.6;

    match kind {
        BlockKind::Heading { level } => match level {
            1 => BlockTypography::new(
                px((26.0 * scale).round()),
                px((36.0 * scale * lh_ratio).round()),
                px((18.0 * scale).round()),
            ),
            2 => BlockTypography::new(
                px((22.0 * scale).round()),
                px((32.0 * scale * lh_ratio).round()),
                px((18.0 * scale).round()),
            ),
            3 => BlockTypography::new(
                px((18.0 * scale).round()),
                px((28.0 * scale * lh_ratio).round()),
                px((18.0 * scale).round()),
            ),
            _ => BlockTypography::new(
                px((16.0 * scale).round()),
                px((24.0 * scale * lh_ratio).round()),
                px((18.0 * scale).round()),
            ),
        },
        BlockKind::CodeBlock { is_fence_start } | BlockKind::MermaidBlock { is_fence_start } => {
            if *is_fence_start {
                BlockTypography::new(
                    px((12.0 * scale).round()),
                    px((20.0 * scale * lh_ratio).round()),
                    px((14.0 * scale).round()),
                )
            } else {
                BlockTypography::new(
                    px((14.0 * scale).round()),
                    px((12.0 * scale * lh_ratio).round()),
                    px(0.0),
                )
            }
        }
        BlockKind::CodeBlockContent | BlockKind::MermaidBlockContent => BlockTypography::new(
            px((13.5 * scale).round()),
            px((22.0 * scale * lh_ratio).round()),
            px(2.0),
        ),
        BlockKind::MathBlock { is_start: true } | BlockKind::MathBlockContent => {
            BlockTypography::new(
                px(base_font_size),
                px((base_font_size * line_height_factor).round()),
                px((8.0 * scale).round()),
            )
        }
        BlockKind::MathBlock { is_start: false } => BlockTypography::new(
            px(base_font_size),
            px((base_font_size * line_height_factor).round()),
            px(0.0),
        ),
        BlockKind::Table { .. } => BlockTypography::new(
            px((14.0 * scale).round()),
            px((22.0 * scale * lh_ratio).round()),
            px((16.0 * scale).round()),
        ),
        BlockKind::BulletList | BlockKind::OrderedList { .. } | BlockKind::TaskList { .. } => {
            BlockTypography::new(
                px(base_font_size),
                px((base_font_size * line_height_factor).round()),
                px((8.0 * scale).round()),
            )
        }
        BlockKind::ThematicBreak => BlockTypography::new(
            px(base_font_size),
            px((18.0 * scale * lh_ratio).round()),
            px((14.0 * scale).round()),
        ),
        BlockKind::Image { .. } => BlockTypography::new(
            px(base_font_size),
            px((base_font_size * line_height_factor).round()),
            px((8.0 * scale).round()),
        ),
        BlockKind::Paragraph => BlockTypography::new(
            px(base_font_size),
            px((base_font_size * line_height_factor).round()),
            px((8.0 * scale).round()),
        ),
    }
}

pub fn get_block_color_override(kind: &BlockKind, theme: &Theme) -> Option<Hsla> {
    match kind {
        BlockKind::Heading { level } => match level {
            1 => Some(theme.heading_h1),
            2 => Some(theme.heading_h2),
            3 => Some(theme.heading_h3),
            4 => Some(theme.heading_h4),
            _ => None,
        },
        BlockKind::CodeBlock {
            is_fence_start: true,
        }
        | BlockKind::MermaidBlock {
            is_fence_start: true,
        } => Some(theme.code_block_header_fg),
        BlockKind::CodeBlockContent | BlockKind::MermaidBlockContent => Some(theme.code_block_text),
        BlockKind::Image { .. } => Some(theme.text_secondary),
        _ => None,
    }
}

#[derive(Clone, Debug)]
pub struct DocumentLayoutCache {
    pub buffer_version: usize,
    pub buffer_revision: u64,
    pub lines_len: usize,
    pub render_mode: RenderMode,
    pub content_width: Pixels,
    pub active_line: SourceLine,
    pub active_col: usize,
    pub active_math_block: Option<(SourceLine, SourceLine)>,
    pub active_mermaid_block: Option<(SourceLine, SourceLine)>,
    pub doc_path: Option<PathBuf>,
    pub image_cache_version: usize,
    pub syntax_cache_version: usize,
    pub fold_version: usize,
    pub font_size: f32,
    pub line_height: f32,
    pub soft_wrap: bool,
    pub parsed_lines: super::parsed_line_store::ParsedLineStore,
    pub metrics: super::line_metrics::LineMetrics,
    pub space_width: Pixels,
    #[cfg(test)]
    pub separators: Arc<Vec<bool>>,
    pub quote_projection: Arc<QuoteProjection>,
    #[cfg(test)]
    pub quote_ranges: Arc<Vec<Range<usize>>>,
}

impl Default for DocumentLayoutCache {
    fn default() -> Self {
        Self {
            buffer_version: 0,
            buffer_revision: 0,
            lines_len: 0,
            render_mode: RenderMode::LivePreview,
            content_width: px(0.0),
            active_line: SourceLine::new(0),
            active_col: 0,
            active_math_block: None,
            active_mermaid_block: None,
            doc_path: None,
            image_cache_version: 0,
            syntax_cache_version: 0,
            fold_version: 0,
            font_size: 15.0,
            line_height: 1.6,
            soft_wrap: true,
            parsed_lines: Default::default(),
            metrics: Default::default(),
            space_width: px(0.0),
            #[cfg(test)]
            separators: Arc::default(),
            quote_projection: Arc::new(QuoteProjection::default()),
            #[cfg(test)]
            quote_ranges: Arc::default(),
        }
    }
}

impl DocumentLayoutCache {
    /// Code footers draw a block border but have no editable projected text.
    pub(crate) fn project_code_block_cursor(
        &self,
        position: crate::buffer::Position,
    ) -> crate::buffer::Position {
        let Some(row) = self.parsed_lines.row(position.line.index()) else {
            return position;
        };
        if !matches!(
            row.kind,
            BlockKind::CodeBlock {
                is_fence_start: false
            } | BlockKind::MermaidBlock {
                is_fence_start: false
            }
        ) {
            return position;
        }
        let Some(previous) = position.line.checked_sub(1) else {
            return position;
        };
        self.parsed_lines
            .row(previous.index())
            .map_or(position, |row| {
                crate::buffer::Position::new(previous, row.raw_text.chars().count())
            })
    }

    pub(crate) fn is_live_cursor_line(&self, line: SourceLine) -> bool {
        self.parsed_lines.row(line.index()).is_some_and(|row| {
            !self.separator(line.index())
                && !matches!(
                    row.kind,
                    BlockKind::CodeBlock {
                        is_fence_start: false
                    } | BlockKind::MermaidBlock {
                        is_fence_start: false
                    } | BlockKind::MathBlock { .. }
                        | BlockKind::Table {
                            is_delimiter: true,
                            ..
                        }
                )
        })
    }

    pub(crate) fn separator(&self, line: usize) -> bool {
        #[cfg(test)]
        if let Some(value) = self.separators.get(line) {
            return *value;
        }
        self.render_mode != RenderMode::Source && self.quote_projection.separator(line)
    }

    pub(crate) fn visible_quote_ranges(&self, range: Range<usize>) -> Vec<Range<usize>> {
        self.quote_projection.quote_ranges(range)
    }
    #[inline]
    pub fn compute_indent_px(parsed: &ParsedLine, mode: RenderMode, space_width: Pixels) -> Pixels {
        if mode != RenderMode::LivePreview {
            return px(0.0);
        }
        let chars = crate::markdown::line_visual_indent_chars(parsed);
        px((chars as f32) * f32::from(space_width))
    }

    /// Visual indent in pixels for `line_idx` (0 in Source mode or out of range).
    /// Single source of truth dynamically derived from `parsed_lines` and `space_width`.
    #[inline]
    pub fn indent_px(&self, source_line: SourceLine) -> Pixels {
        let Some(row) = self.parsed_lines.row(source_line.index()) else {
            return px(0.0);
        };
        let parsed = &*row;
        Self::compute_indent_px(parsed, self.render_mode, self.space_width)
    }

    pub fn source_line_y_range(&self, source_line: SourceLine) -> Option<(Pixels, Pixels)> {
        self.metrics.y_range(source_line.index())
    }

    /// Source rows intersecting the viewport, including a partially visible tall row.
    /// Collapsed rows within the range must be skipped by the renderer.
    pub(crate) fn visible_source_lines(&self, top: Pixels, height: Pixels) -> Range<usize> {
        self.metrics.visible_lines(top, height)
    }

    pub fn source_line_at_y(&self, y: Pixels) -> Option<SourceLine> {
        self.metrics.line_at_y(y).map(SourceLine::new)
    }
}

#[derive(Clone)]
pub struct LineLayoutContext<'a> {
    pub(crate) math_preview: Option<crate::math::MathSvg>,
    pub is_active: bool,
    pub math_block_editing: bool,
    pub mermaid_block_editing: bool,
    pub is_separator: Option<bool>,
    pub cursor_col: usize,
    pub is_selection_disclosure: bool,
    pub mode: RenderMode,
    pub content_width: Pixels,
    pub doc_path: Option<&'a std::path::Path>,
    pub is_folded: bool,
    pub font_size: f32,
    pub line_height: f32,
    pub soft_wrap: bool,
    pub indent_px: Pixels,
    pub quote_content_indent_px: Pixels,
    pub text_system: Option<Arc<WindowTextSystem>>,
}

impl<'a> std::fmt::Debug for LineLayoutContext<'a> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LineLayoutContext")
            .field("is_active", &self.is_active)
            .field("math_block_editing", &self.math_block_editing)
            .field("mermaid_block_editing", &self.mermaid_block_editing)
            .field("is_separator", &self.is_separator)
            .field("cursor_col", &self.cursor_col)
            .field("is_selection_disclosure", &self.is_selection_disclosure)
            .field("mode", &self.mode)
            .field("content_width", &self.content_width)
            .field("doc_path", &self.doc_path)
            .field("is_folded", &self.is_folded)
            .field("font_size", &self.font_size)
            .field("line_height", &self.line_height)
            .field("soft_wrap", &self.soft_wrap)
            .field("indent_px", &self.indent_px)
            .field("quote_content_indent_px", &self.quote_content_indent_px)
            .field("has_text_system", &self.text_system.is_some())
            .finish()
    }
}

impl<'a> Default for LineLayoutContext<'a> {
    fn default() -> Self {
        Self {
            is_active: false,
            math_preview: None,
            math_block_editing: false,
            mermaid_block_editing: false,
            is_separator: None,
            cursor_col: 0,
            is_selection_disclosure: false,
            mode: RenderMode::LivePreview,
            content_width: px(800.0),
            doc_path: None,
            is_folded: false,
            font_size: 15.0,
            line_height: 1.6,
            soft_wrap: true,
            indent_px: px(0.0),
            quote_content_indent_px: px(0.0),
            text_system: None,
        }
    }
}

impl<'a> LineLayoutContext<'a> {
    pub fn new(
        is_active: bool,
        is_separator: Option<bool>,
        cursor_col: usize,
        mode: RenderMode,
        content_width: Pixels,
        doc_path: Option<&'a std::path::Path>,
    ) -> Self {
        Self {
            is_active,
            math_preview: None,
            math_block_editing: false,
            mermaid_block_editing: false,
            is_separator,
            cursor_col,
            is_selection_disclosure: false,
            mode,
            content_width,
            doc_path,
            is_folded: false,
            font_size: 15.0,
            line_height: 1.6,
            soft_wrap: true,
            indent_px: px(0.0),
            quote_content_indent_px: px(0.0),
            text_system: None,
        }
    }

    pub fn with_folded(mut self, is_folded: bool) -> Self {
        self.is_folded = is_folded;
        self
    }

    pub fn with_math_block_editing(mut self, math_block_editing: bool) -> Self {
        self.math_block_editing = math_block_editing;
        self
    }

    pub fn with_mermaid_block_editing(mut self, mermaid_block_editing: bool) -> Self {
        self.mermaid_block_editing = mermaid_block_editing;
        self
    }

    pub fn with_selection_disclosure(mut self, is_selection_disclosure: bool) -> Self {
        self.is_selection_disclosure = is_selection_disclosure;
        self
    }

    pub fn with_metrics(mut self, font_size: f32, line_height: f32, soft_wrap: bool) -> Self {
        self.font_size = font_size;
        self.line_height = line_height;
        self.soft_wrap = soft_wrap;
        self
    }

    pub fn with_indent(mut self, indent_px: Pixels) -> Self {
        self.indent_px = indent_px;
        self
    }

    pub fn with_quote_content_indent(mut self, indent_px: Pixels) -> Self {
        self.quote_content_indent_px = indent_px;
        self
    }

    pub fn with_text_system(mut self, text_system: Option<Arc<WindowTextSystem>>) -> Self {
        self.text_system = text_system;
        self
    }

    #[cfg(test)]
    pub fn live_preview(content_width: Pixels) -> Self {
        Self {
            content_width,
            ..Default::default()
        }
    }

    #[cfg(test)]
    pub fn source(content_width: Pixels) -> Self {
        Self {
            mode: RenderMode::Source,
            content_width,
            ..Default::default()
        }
    }
}

/// Computes the layout height in pixels for a single document line.
pub fn get_line_layout_height(
    lines: &(impl LineSource + ?Sized),
    idx: usize,
    parsed: &ParsedLine,
    lcx: LineLayoutContext,
) -> Pixels {
    if lcx.is_folded {
        return px(0.0);
    }

    use crate::editor::shaping::{
        LineShapingContext, build_line_runs, calculate_visual_lines_with_runs,
        effective_content_width, parsed_line_available_width,
    };

    let base_content_width =
        if lcx.math_block_editing && matches!(parsed.kind, BlockKind::MathBlockContent) {
            (lcx.content_width / 2.0 + px(40.0)).max(px(0.0))
        } else {
            lcx.content_width
        };
    let content_width =
        effective_content_width(base_content_width, parsed, lcx.mode, lcx.indent_px);
    let available_w = parsed_line_available_width(
        base_content_width,
        parsed,
        lcx.mode,
        lcx.indent_px,
        lcx.quote_content_indent_px,
    );

    if lcx.mode == RenderMode::Source {
        let font_size = px(lcx.font_size);
        let line_height = px((lcx.font_size * lcx.line_height).round());
        let font = font(crate::platform::platform_monospace_font());
        let scx = LineShapingContext::new(
            &crate::theme::DEFAULT_THEME,
            None,
            true,
            lcx.is_active,
            lcx.cursor_col,
        )
        .with_selection_disclosure(lcx.is_selection_disclosure);
        let (line_str, runs) = build_line_runs(&parsed.spans, scx, None, &TextStyle::default());
        let visual_lines = calculate_visual_lines_with_runs(
            &line_str,
            &runs,
            font,
            font_size,
            available_w,
            lcx.soft_wrap,
            lcx.text_system.as_ref(),
            true,
        );
        return line_height * (visual_lines as f32);
    }

    let typo = get_block_typography_with_metrics(&parsed.kind, lcx.font_size, lcx.line_height);
    let wrapped_code_height = || {
        let scx = LineShapingContext::new(
            &crate::theme::DEFAULT_THEME,
            Some(crate::theme::DEFAULT_THEME.code_block_text),
            false,
            lcx.is_active,
            lcx.cursor_col,
        )
        .with_selection_disclosure(lcx.is_selection_disclosure);
        let (line_str, runs) = build_line_runs(&parsed.spans, scx, None, &TextStyle::default());
        let visual_lines = calculate_visual_lines_with_runs(
            &line_str,
            &runs,
            font(crate::platform::platform_monospace_font()),
            typo.font_size,
            available_w,
            lcx.soft_wrap,
            lcx.text_system.as_ref(),
            true,
        );

        typo.padding_vertical + typo.line_height * (visual_lines as f32)
    };
    let wrapped_prose_height = |font: Font, color_override: Option<Hsla>, is_heading: bool| {
        let scx = LineShapingContext::new(
            &crate::theme::DEFAULT_THEME,
            color_override,
            false,
            lcx.is_active,
            lcx.cursor_col,
        )
        .with_selection_disclosure(lcx.is_selection_disclosure)
        .with_heading(is_heading);
        let (line_str, runs) = build_line_runs(&parsed.spans, scx, None, &TextStyle::default());
        let visual_lines = calculate_visual_lines_with_runs(
            &line_str,
            &runs,
            font,
            typo.font_size,
            available_w,
            lcx.soft_wrap,
            lcx.text_system.as_ref(),
            false,
        );
        let math_height = parsed
            .spans
            .iter()
            .filter(|span| !span.math_is_disclosed(lcx.is_active, lcx.cursor_col, false))
            .filter_map(|span| span.math.as_ref())
            .map(|math| {
                crate::editor::shaping::math_render_size(&math.rendered, px(lcx.font_size)).height
            })
            .fold(px(0.0), Pixels::max);
        typo.padding_vertical + typo.line_height.max(math_height) * (visual_lines as f32)
    };

    let height = match &parsed.kind {
        BlockKind::Heading { .. } => {
            let color = get_block_color_override(&parsed.kind, &crate::theme::DEFAULT_THEME);
            wrapped_prose_height(
                font(crate::platform::platform_ui_font()).bold(),
                color,
                true,
            )
        }
        BlockKind::CodeBlock { .. } | BlockKind::ThematicBreak => typo.base_layout_height,
        BlockKind::CodeBlockContent => wrapped_code_height(),
        BlockKind::MermaidBlock {
            is_fence_start: true,
        } => {
            if lcx.mermaid_block_editing {
                typo.base_layout_height
            } else {
                parsed
                    .spans
                    .iter()
                    .find_map(|span| span.mermaid.as_ref())
                    .map(|diagram| {
                        crate::mermaid::display_size(
                            &diagram.rendered,
                            content_width,
                            if parsed.quote.is_some() {
                                if parsed.kind.has_syntax_prefix() {
                                    lcx.quote_content_indent_px
                                } else {
                                    px(0.0)
                                }
                            } else {
                                lcx.indent_px
                            },
                        )
                        .1 + px(26.0)
                    })
                    .unwrap_or(typo.base_layout_height)
            }
        }
        BlockKind::MermaidBlock {
            is_fence_start: false,
        }
        | BlockKind::MermaidBlockContent => {
            if lcx.mermaid_block_editing {
                wrapped_code_height()
            } else {
                px(0.0)
            }
        }
        BlockKind::MathBlock { is_start: true } => {
            if lcx.math_block_editing {
                px(0.0)
            } else {
                wrapped_prose_height(font(crate::platform::platform_ui_font()), None, false)
            }
        }
        BlockKind::MathBlock { is_start: false } => px(0.0),
        BlockKind::MathBlockContent => {
            if !lcx.math_block_editing {
                return px(0.0);
            }

            let source_font_size = px(lcx.font_size);
            let source_line_height = px((lcx.font_size * lcx.line_height).round());
            let source_scx = LineShapingContext::new(
                &crate::theme::DEFAULT_THEME,
                None,
                true,
                lcx.is_active,
                lcx.cursor_col,
            );
            let (source, source_runs) =
                build_line_runs(&parsed.spans, source_scx, None, &TextStyle::default());
            let source_visual_lines = calculate_visual_lines_with_runs(
                &source,
                &source_runs,
                font(crate::platform::platform_monospace_font()),
                source_font_size,
                (content_width - SOURCE_GUTTER_WIDTH).max(px(0.0)),
                lcx.soft_wrap,
                lcx.text_system.as_ref(),
                true,
            );
            let source_height =
                source_line_height * source_visual_lines as f32 + typo.padding_vertical;
            let preview_height = lcx
                .math_preview
                .as_ref()
                .map(|math| {
                    crate::editor::shaping::math_render_size(&math, px(lcx.font_size)).height
                        + typo.padding_vertical
                })
                .unwrap_or(px(0.0));
            source_height.max(preview_height)
        }
        BlockKind::Image { url, .. } => {
            let base_h = if lcx.is_active {
                px(38.0)
            } else {
                typo.padding_vertical
            };
            if crate::http::is_url_loaded(url) {
                if let Some((w, h)) = crate::http::read_cache(|cache| {
                    cache.get_size(&crate::http::image_size_key(url, lcx.doc_path))
                }) && w > 0
                {
                    let img_h = px((h as f32) * f32::from(content_width) / (w as f32));
                    base_h + img_h + px(12.0)
                } else {
                    base_h + px(40.0) + px(12.0)
                }
            } else if crate::http::is_url_failed(url) {
                base_h + px(40.0) + px(12.0)
            } else {
                base_h + px(140.0) + px(12.0)
            }
        }
        BlockKind::Paragraph => {
            let is_sep = lcx.is_separator.unwrap_or_else(|| {
                crate::markdown::is_syntactic_separator(lines, crate::buffer::SourceLine::new(idx))
            });
            if is_sep && !lcx.is_active {
                px(0.0)
            } else {
                wrapped_prose_height(font(crate::platform::platform_ui_font()), None, false)
            }
        }
        BlockKind::Table {
            is_header,
            is_delimiter,
            cells,
            ..
        } => {
            if *is_delimiter {
                px(0.0)
            } else {
                let is_last = if idx + 1 >= lines.line_count() {
                    true
                } else {
                    let next = lines.source_line(SourceLine::new(idx + 1));
                    !next.contains('|')
                        || next.trim().is_empty()
                        || next.trim_start().starts_with('#')
                        || next.trim_start().starts_with("```")
                };
                let row_h = crate::editor::shaping::calculate_table_row_height(
                    cells,
                    *is_header,
                    lcx.is_active,
                    lcx.cursor_col,
                    lcx.content_width,
                    lcx.text_system.as_ref(),
                    lcx.is_selection_disclosure,
                );
                if *is_header {
                    px(8.0) + row_h
                } else if is_last {
                    row_h + px(8.0)
                } else {
                    row_h
                }
            }
        }
        BlockKind::TaskList { .. } | BlockKind::BulletList | BlockKind::OrderedList { .. } => {
            wrapped_prose_height(font(crate::platform::platform_ui_font()), None, false)
        }
    };

    if let Some(quote) = parsed.quote {
        height + blockquote_edge_spacing(quote)
    } else {
        height
    }
}

fn blockquote_edge_spacing(quote: crate::markdown::QuoteContext) -> Pixels {
    (if quote.is_start {
        crate::editor::shaping::QUOTE_EDGE_PADDING
    } else {
        px(0.0)
    }) + if quote.is_end {
        crate::editor::shaping::QUOTE_EDGE_PADDING
    } else {
        px(0.0)
    }
}

impl Editor {
    pub(super) fn measure_line_layout_height(
        &self,
        idx: usize,
        parsed: &ParsedLine,
        lcx: LineLayoutContext,
    ) -> Pixels {
        #[cfg(test)]
        self.layout_stats.update(|mut stats| {
            stats.lines_measured += 1;
            stats
        });
        get_line_layout_height(&self.buffer, idx, parsed, lcx)
    }

    /// Marks active IME preedit text so it participates in line shaping and receives an underline.
    pub(super) fn project_ime_preedit(&self, first_line: usize, parsed_lines: &mut [ParsedLine]) {
        let Some(marked) = self.marked_range.clone() else {
            return;
        };
        let start = self.buffer.utf16_offset_to_pos(marked.start);
        let end = self.buffer.utf16_offset_to_pos(marked.end);
        if start.line != end.line {
            return;
        }
        let Some(line_idx) = start.line.index().checked_sub(first_line) else {
            return;
        };
        if let Some(line) = parsed_lines.get_mut(line_idx) {
            line.spans = crate::markdown::mark_ime_preedit(&line.spans, (start.col, end.col));
            if let BlockKind::Table { cells, .. } = &mut line.kind {
                for cell in cells.iter_mut() {
                    cell.spans =
                        crate::markdown::mark_ime_preedit(&cell.spans, (start.col, end.col));
                }
            }
        }
    }

    pub(crate) fn invalidate_layout_cache(&mut self) {
        *self.layout_cache.borrow_mut() = Arc::new(DocumentLayoutCache::default());
    }

    #[inline]
    pub fn layout_snapshot(&self) -> Arc<DocumentLayoutCache> {
        self.ensure_layout_cache();
        self.layout_cache.borrow().clone()
    }

    pub(super) fn cache_matches_view(
        &self,
        cache: &DocumentLayoutCache,
        content_width: Pixels,
    ) -> bool {
        cache.render_mode == self.render_mode
            && cache.content_width == content_width
            && cache.doc_path.as_deref() == self.buffer.file_path()
            && cache.fold_version == self.display_map.version()
            && cache.font_size == self.font_size
            && cache.line_height == self.line_height
            && cache.soft_wrap == self.soft_wrap
    }

    /// Width of one space character in the UI font at the current font size.
    /// Memoized per font size; the layout cache stores the measured value so
    /// rendering, cursor positioning and hit-testing agree on indent widths.
    pub(crate) fn space_width(&self) -> Pixels {
        if let Some((fs, w)) = self.space_width_memo.get()
            && (fs - self.font_size).abs() < f32::EPSILON
        {
            return w;
        }
        let measured = self
            .text_system
            .as_ref()
            .and_then(|ts| {
                let font_id = ts.resolve_font(&font(crate::platform::platform_ui_font()));
                ts.advance(font_id, px(self.font_size), ' ').ok()
            })
            .map(|advance| advance.width)
            .filter(|w| *w > px(0.0))
            .unwrap_or_else(|| px((self.font_size * 0.5).round()));
        self.space_width_memo.set(Some((self.font_size, measured)));
        measured
    }

    /// Visual indent in pixels for a parsed line (0 in Source mode).
    #[inline]
    pub(crate) fn parsed_line_indent_px(&self, parsed: &ParsedLine) -> Pixels {
        DocumentLayoutCache::compute_indent_px(parsed, self.render_mode, self.space_width())
    }

    pub(super) fn line_layout_ctx(
        &self,
        parsed: &ParsedLine,
        is_active: bool,
        is_sep: bool,
        cursor_col: usize,
        content_width: Pixels,
        source_line: SourceLine,
        active_math_block: Option<(SourceLine, SourceLine)>,
        active_mermaid_block: Option<(SourceLine, SourceLine)>,
    ) -> LineLayoutContext<'_> {
        let is_folded = self.display_map.fold_map().is_line_folded(source_line);
        let indent_px = self.parsed_line_indent_px(parsed);
        LineLayoutContext::new(
            is_active,
            Some(is_sep),
            cursor_col,
            self.render_mode,
            content_width,
            self.buffer.file_path(),
        )
        .with_folded(is_folded)
        .with_math_block_editing(
            active_math_block.is_some_and(|(start, end)| (start..=end).contains(&source_line)),
        )
        .with_mermaid_block_editing(
            active_mermaid_block.is_some_and(|(start, end)| (start..=end).contains(&source_line)),
        )
        .with_selection_disclosure(self.is_selection_disclosure())
        .with_metrics(self.font_size, self.line_height, self.soft_wrap)
        .with_indent(indent_px)
        .with_quote_content_indent(crate::editor::shaping::quote_content_indent_px(
            parsed,
            self.space_width(),
        ))
        .with_text_system(self.text_system.clone())
    }

    #[cfg(test)]

    pub(super) fn measure_range(
        &self,
        range: Range<usize>,
        parsed: &[ParsedLine],
        separators: &[bool],
        content_width: Pixels,
        active_math_block: Option<(SourceLine, SourceLine)>,
        active_mermaid_block: Option<(SourceLine, SourceLine)>,
    ) -> Vec<Pixels> {
        assert_eq!(range.len(), parsed.len());
        let parsed_rows = parsed;
        let disclosure = self.disclosure_position();
        range
            .zip(parsed)
            .map(|(idx, parsed)| {
                let source_line = SourceLine::new(idx);
                let mut lcx = self.line_layout_ctx(
                    parsed,
                    source_line == disclosure.line,
                    separators.get(idx).copied().unwrap_or(false),
                    disclosure.col,
                    content_width,
                    source_line,
                    active_math_block,
                    active_mermaid_block,
                );
                lcx.math_preview = active_math_block
                    .filter(|(start, _)| source_line == *start + 1)
                    .and_then(|(start, _)| parsed_rows.get(start.index()))
                    .and_then(|row| {
                        row.spans
                            .iter()
                            .find_map(|span| span.math.as_ref().map(|math| math.rendered.clone()))
                    });
                self.measure_line_layout_height(idx, parsed, lcx)
            })
            .collect()
    }

    pub(super) fn measure_pending_range(
        &self,
        cache: &mut DocumentLayoutCache,
        range: Range<usize>,
    ) {
        let scroll_top = self.scroll.current_scroll_top + self.layout_scroll_adjustment.get();
        let anchor = cache
            .metrics
            .line_at_relative_y(self.scroll.anchor_line, scroll_top);
        let old_top =
            anchor.and_then(|line| cache.metrics.top_relative(line, self.scroll.anchor_line));
        let mut from = range.start;
        while let Some(start) = cache
            .metrics
            .next_unmeasured(from)
            .filter(|&line| line < range.end)
        {
            let end = cache
                .metrics
                .unmeasured_end(start, (start + 32).min(range.end));
            let parsed = cache.parsed_lines.read_range(start..end);
            let mut parsed = parsed.to_vec();
            self.project_ime_preedit(start, &mut parsed);
            #[cfg(test)]
            self.layout_stats.update(|mut stats| {
                stats.lines_scanned += cache.parsed_lines.take_scanned();
                stats
            });
            #[cfg(not(test))]
            cache.parsed_lines.take_scanned();
            let disclosure = self.disclosure_position();
            let heights = parsed
                .iter()
                .zip(start..end)
                .map(|(parsed, line)| {
                    let source_line = SourceLine::new(line);
                    let mut context = self.line_layout_ctx(
                        parsed,
                        source_line == disclosure.line,
                        cache.separator(line),
                        disclosure.col,
                        cache.content_width,
                        source_line,
                        cache.active_math_block,
                        cache.active_mermaid_block,
                    );
                    context.math_preview = cache
                        .active_math_block
                        .filter(|(start, _)| source_line == *start + 1)
                        .and_then(|(start, _)| cache.parsed_lines.row(start.index()))
                        .and_then(|row| {
                            row.spans.iter().find_map(|span| {
                                span.math.as_ref().map(|math| math.rendered.clone())
                            })
                        });
                    self.measure_line_layout_height(line, parsed, context)
                })
                .collect();
            cache.metrics.set_measured(start..end, heights);
            from = end;
        }
        if let Some((old_top, new_top)) = old_top
            .zip(anchor.and_then(|line| cache.metrics.top_relative(line, self.scroll.anchor_line)))
        {
            self.layout_scroll_adjustment
                .set(self.layout_scroll_adjustment.get() + new_top - old_top);
        }
    }

    pub(super) fn measure_viewport(&self, cache: &mut DocumentLayoutCache) {
        if cache.metrics.unmeasured_count() == 0 {
            return;
        }
        let visible = cache.metrics.visible_lines_relative(
            self.scroll.anchor_line,
            self.scroll.current_scroll_top + self.layout_scroll_adjustment.get(),
            self.viewport_height(),
        );
        let range = visible.start.saturating_sub(LAYOUT_OVERSCAN_LINES)
            ..visible
                .end
                .saturating_add(LAYOUT_OVERSCAN_LINES)
                .min(cache.metrics.len());
        self.measure_pending_range(cache, range);
    }

    pub(crate) fn prepare_viewport_layout(&mut self) {
        self.ensure_layout_cache();
        {
            let mut cache = self.layout_cache.borrow_mut();
            if cache.metrics.unmeasured_count() > 0 {
                self.measure_viewport(Arc::make_mut(&mut cache));
            }
        }
        self.apply_layout_scroll_adjustment();
        self.rebase_scroll();
    }

    pub(super) fn apply_layout_scroll_adjustment(&mut self) {
        let adjustment = self.layout_scroll_adjustment.replace(px(0.0));
        self.scroll.compensate_layout(adjustment);
        if let Some(start) = self.scroll.scrollbar_drag_start_scroll.as_mut() {
            *start += adjustment;
        }
    }

    #[cfg(test)]
    pub(super) fn eager_metrics_for_test(&self) -> LineMetrics {
        let cache = self.layout_snapshot();
        let mut parsed = MarkdownScanner::scan_document(&self.buffer.lines());
        self.project_ime_preedit(0, &mut parsed);
        let separators = if self.render_mode == RenderMode::Source {
            Vec::new()
        } else {
            crate::markdown::compute_syntactic_separators(&self.buffer)
        };
        LineMetrics::from_heights(self.measure_range(
            0..parsed.len(),
            &parsed,
            &separators,
            cache.content_width,
            cache.active_math_block,
            cache.active_mermaid_block,
        ))
    }

    #[cfg(test)]
    pub(super) fn measure_all_pending_for_test(&mut self) {
        loop {
            self.ensure_layout_cache();
            {
                let mut cache = self.layout_cache.borrow_mut();
                let len = cache.metrics.len();
                self.measure_pending_range(Arc::make_mut(&mut cache), 0..len);
            }
            self.ensure_layout_cache();
            if self.layout_cache.borrow().metrics.unmeasured_count() == 0 {
                break;
            }
        }
        self.apply_layout_scroll_adjustment();
        self.rebase_scroll();
    }

    pub fn ensure_layout_cache(&self) {
        self.display_map.synchronize(&self.buffer);
        let viewport = self.viewport_bounds_val();
        let content_width = compute_content_width(viewport.size.width, self.render_mode);
        let disclosure = self.disclosure_position();
        self.ensure_window_layout(
            content_width,
            disclosure.line,
            disclosure.col,
            crate::http::image_cache_version(),
            crate::syntax::syntax_cache_version(),
        );
    }

    pub fn get_line_y_range(&self, target_line: SourceLine) -> (Pixels, Pixels) {
        let layout = self.layout_snapshot();
        layout.source_line_y_range(target_line).unwrap_or_else(|| {
            let total = layout.metrics.total_height();
            ((total - DOCUMENT_BOTTOM_PADDING).max(px(0.0)), total)
        })
    }

    pub fn get_total_content_height(&self) -> Pixels {
        self.layout_snapshot().metrics.total_height()
    }

    /// Viewport height with a fallback before initial layout measurement.
    #[inline]
    pub fn viewport_height(&self) -> Pixels {
        if self.viewport_bounds.size.height > px(50.0) {
            self.viewport_bounds.size.height
        } else {
            px(700.0)
        }
    }

    #[inline]
    pub fn get_max_scroll_top(&self) -> Pixels {
        px((f64::from(f32::from(
            self.scrollbar_metrics(self.viewport_height(), self.get_total_content_height())
                .max_scroll,
        )) - self.scroll_origin_y()) as f32)
    }

    pub(crate) fn scrollbar_metrics(
        &self,
        track_h: Pixels,
        total_content_h: Pixels,
    ) -> ScrollbarMetrics {
        let (max_scroll, virtual_h) = if total_content_h <= track_h {
            (px(0.0), total_content_h)
        } else {
            let v_h = total_content_h + BOTTOM_OVERSCROLL;
            ((v_h - track_h).max(px(0.0)), v_h)
        };

        let thumb_h = if virtual_h > px(0.0) {
            // Guard against panics when track height is smaller than the minimum thumb height (28px).
            if track_h < px(28.0) {
                track_h
            } else {
                ((track_h / virtual_h) * track_h).clamp(px(28.0), track_h)
            }
        } else {
            track_h
        };

        let thumb_y = if max_scroll > px(0.0) {
            let scroll_ratio = ((self.scroll_origin_y()
                + f64::from(f32::from(self.scroll.current_scroll_top)))
                / f64::from(f32::from(max_scroll)))
            .clamp(0.0, 1.0) as f32;
            scroll_ratio * (track_h - thumb_h)
        } else {
            px(0.0)
        };

        ScrollbarMetrics {
            max_scroll,
            thumb_y,
            thumb_h,
            has_overflow: max_scroll > px(2.0),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ScrollbarMetrics {
    pub max_scroll: Pixels,
    pub thumb_y: Pixels,
    pub thumb_h: Pixels,
    pub has_overflow: bool,
}

#[cfg(test)]
mod incremental_tests {
    use super::{Editor, RenderMode, SourceLine};
    use crate::buffer::{Position, Selection, TextBuffer};
    use gpui::px;

    #[test]
    fn viewport_line_range_matches_intersections_including_tall_and_collapsed_rows() {
        let cache = super::DocumentLayoutCache {
            metrics: crate::editor::line_metrics::LineMetrics::from_heights(
                [0.0, 32.0, 0.0, 1000.0, 0.0, 24.0, 0.0]
                    .into_iter()
                    .map(px)
                    .collect(),
            ),
            ..Default::default()
        };
        for top in (-20..1120).step_by(17) {
            for height in [0.0, 1.0, 23.0, 100.0, 999.0] {
                let top = px(top as f32);
                let height = px(height);
                let expected: Vec<_> = (0..cache.metrics.heights().len())
                    .filter(|&idx| {
                        height > px(0.0)
                            && cache.metrics.height(idx).unwrap() > px(0.0)
                            && cache.metrics.top(idx).unwrap() < top + height
                            && cache.metrics.top(idx).unwrap() + cache.metrics.height(idx).unwrap()
                                > top
                    })
                    .collect();
                let actual: Vec<_> = cache
                    .visible_source_lines(top, height)
                    .filter(|&idx| cache.metrics.height(idx).unwrap() > px(0.0))
                    .collect();
                assert_eq!(actual, expected, "viewport {top:?} + {height:?}");
            }
        }
        assert_eq!(
            super::DocumentLayoutCache::default().visible_source_lines(px(0.0), px(700.0)),
            0..0
        );
    }

    #[gpui::test]
    fn normalized_blank_quote_edits_update_separator_state_locally(cx: &mut gpui::TestAppContext) {
        let (editor, cx) = cx.add_window_view(|_window, cx| {
            Editor::new_with_buffer(
                TextBuffer::from_str("old active\n\n> ## heading\n> \n\n\nlast", None),
                cx,
            )
        });
        cx.update(|_window, cx| {
            editor.update(cx, |ed, _| {
                ed.render_mode = RenderMode::LivePreview;
                ed.buffer.set_cursor(Position::new(SourceLine::new(3), 2));
                ed.buffer.insert_text("text");
                ed.ensure_layout_cache();
                let after = ed.layout_snapshot();
                ed.invalidate_layout_cache();
                let full = ed.layout_snapshot();
                assert_eq!(after.parsed_lines, full.parsed_lines);
                assert_eq!(after.metrics.heights(), full.metrics.heights());
                assert_eq!(after.metrics.offsets(), full.metrics.offsets());
                assert_eq!(after.separators, full.separators);
            });
        });
    }

    #[gpui::test]
    fn local_window_updates_preserve_unchanged_text_and_old_snapshots(
        cx: &mut gpui::TestAppContext,
    ) {
        let (editor, cx) = cx.add_window_view(|_window, cx| {
            Editor::new_with_buffer(TextBuffer::from_str("first\n\nedit here\n\nlast", None), cx)
        });
        cx.update(|_window, cx| {
            editor.update(cx, |ed, _| {
                let before = ed.layout_snapshot();
                let unchanged = before.parsed_lines.row(0).unwrap();
                drop(before);
                ed.buffer
                    .record_single_line_edit(SourceLine::new(2), |line, _| {
                        line.push_str(" changed")
                    });
                let after = ed.layout_snapshot();
                assert_eq!(
                    after.parsed_lines.row(0).unwrap().raw_text,
                    unchanged.raw_text
                );
                assert_eq!(
                    after.parsed_lines.row(2).unwrap().raw_text,
                    "edit here changed"
                );
                let retained = after.parsed_lines.clone();
                drop(after);
                ed.buffer
                    .record_single_line_edit(SourceLine::new(2), |line, _| line.push_str(" again"));
                assert_eq!(
                    ed.layout_snapshot().parsed_lines.row(2).unwrap().raw_text,
                    "edit here changed again"
                );
                assert_eq!(retained.row(2).unwrap().raw_text, "edit here changed");
            });
        });
    }

    #[gpui::test]
    fn cursor_jump_and_local_edit_match_full_layout_without_full_document_scan(
        cx: &mut gpui::TestAppContext,
    ) {
        let content = "**a long bold heading with markers that change wrapping**\n\nmiddle paragraph\n\nedit this **bold text** with wrapping\nfollowing paragraph\n\nlast **paragraph** with a long line";
        let (editor, cx) = cx.add_window_view(|_window, cx| {
            Editor::new_with_buffer(TextBuffer::from_str(content, None), cx)
        });
        cx.update(|_window, cx| {
            editor.update(cx, |ed, _| {
                for mode in [RenderMode::Source, RenderMode::LivePreview] {
                    for (old_line, edit_line, replacement, delete_next_line) in [
                        (0, 4, "extra words ", false),
                        (7, 2, "inserted\nnew ", false),
                        (0, 4, "joined ", true),
                    ] {
                        ed.buffer = TextBuffer::from_str(content, None);
                        ed.render_mode = mode;
                        ed.viewport_bounds.size = gpui::size(px(260.0), px(600.0));
                        ed.buffer
                            .set_cursor(Position::new(SourceLine::new(old_line), 0));
                        ed.invalidate_layout_cache();
                        ed.buffer
                            .set_cursor(Position::new(SourceLine::new(edit_line), 0));
                        if delete_next_line {
                            ed.buffer.set_selection(Selection {
                                anchor: Position::new(SourceLine::new(edit_line), 0),
                                head: Position::new(SourceLine::new(edit_line + 1), 0),
                            });
                        }
                        ed.buffer.insert_text(replacement);
                        ed.ensure_layout_cache();
                        let cache = ed.layout_snapshot();
                        ed.invalidate_layout_cache();
                        let full = ed.layout_snapshot();
                        assert_eq!(cache.parsed_lines, full.parsed_lines);
                        assert_eq!(cache.metrics.heights(), full.metrics.heights());
                        assert_eq!(cache.metrics.offsets(), full.metrics.offsets());
                        assert_eq!(cache.metrics.total_height(), full.metrics.total_height());
                        assert_eq!(cache.separators, full.separators);
                        assert_eq!(cache.quote_ranges, full.quote_ranges);
                    }
                }
            });
        });
    }

    #[gpui::test]
    fn cursor_jump_and_edit_preserve_preview_geometry_and_old_snapshots(
        cx: &mut gpui::TestAppContext,
    ) {
        let (editor, cx) = cx.add_window_view(|_window, cx| Editor::new(cx));
        cx.update(|_window, cx| {
            editor.update(cx, |ed, _| {
                for (content, preview_line, paragraph_line) in [
                    ("$$\nx^2\n$$\n\nparagraph", 1, 4),
                    ("```mermaid\nflowchart LR\nA --> B\n```\n\nparagraph", 2, 5),
                ] {
                    for (old_line, edit_line) in [
                        (preview_line, paragraph_line),
                        (paragraph_line, preview_line),
                    ] {
                        ed.buffer = TextBuffer::from_str(content, None);
                        ed.render_mode = RenderMode::LivePreview;
                        ed.viewport_bounds.size = gpui::size(px(260.0), px(600.0));
                        ed.buffer
                            .set_cursor(Position::new(SourceLine::new(old_line), 0));
                        ed.invalidate_layout_cache();
                        let before = ed.layout_snapshot();
                        let before_lines = before.quote_projection.normalized_lines().to_vec();
                        ed.buffer
                            .set_cursor(Position::new(SourceLine::new(edit_line), 0));
                        ed.buffer.insert_text(" ");
                        let after = ed.layout_snapshot();
                        ed.invalidate_layout_cache();
                        let full = ed.layout_snapshot();
                        assert_eq!(after.parsed_lines, full.parsed_lines);
                        assert_eq!(after.metrics.heights(), full.metrics.heights());
                        assert_eq!(after.metrics.offsets(), full.metrics.offsets());
                        assert_eq!(after.metrics.total_height(), full.metrics.total_height());
                        assert_eq!(before.quote_projection.normalized_lines(), before_lines);
                    }
                }
            });
        });
    }

    #[gpui::test]
    fn cursor_jump_edits_match_full_layout_for_quotes_tables_and_code(
        cx: &mut gpui::TestAppContext,
    ) {
        let (editor, cx) = cx.add_window_view(|_window, cx| Editor::new(cx));
        cx.update(|_window, cx| {
            editor.update(cx, |ed, _| {
                for mode in [RenderMode::Source, RenderMode::LivePreview] {
                    for (content, edit_line, edit_col) in [
                        ("old **active text**\n\n> 引用 **内容**\n> 引用第二行\n\nlast", 2, 2),
                        ("old **active text**\n\n| Name | Value |\n| --- | --- |\n| row | 界🙂 |\n| end | data |\n\nlast", 4, 2),
                        ("old **active text**\n\n```text\nfirst\nedit code\n```\n\nlast", 4, 2),
                        ("old **active text**\n\n> ## quoted heading\n> paragraph\n\nlast", 3, 0),
                        ("old **active text**\n\n> paragraph\n> ## quoted heading\n\nlast", 2, 0),
                    ] {
                        ed.buffer = TextBuffer::from_str(content, None);
                        ed.render_mode = mode;
                        ed.viewport_bounds.size = gpui::size(px(260.0), px(600.0));
                        ed.invalidate_layout_cache();
                        let before = ed.layout_snapshot();
                        let before_parsed = before.parsed_lines.clone();
                                                ed.buffer.set_cursor(Position::new(SourceLine::new(edit_line), edit_col));
                        ed.buffer.insert_text("changed ");
                                                ed.ensure_layout_cache();
                let cache = ed.layout_snapshot();
                        ed.invalidate_layout_cache();
                        let full = ed.layout_snapshot();
                        assert_eq!(cache.parsed_lines, full.parsed_lines);
                        assert_eq!(cache.metrics.heights(), full.metrics.heights());
                        assert_eq!(cache.metrics.offsets(), full.metrics.offsets());
                        assert_eq!(cache.metrics.total_height(), full.metrics.total_height());
                        assert_eq!(before.parsed_lines, before_parsed);
                    }
                }
            });
        });
    }
}
