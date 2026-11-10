use gpui::*;
use std::ops::Range;
use std::path::PathBuf;
use std::sync::Arc;

use super::line_metrics::{DOCUMENT_BOTTOM_PADDING, LineMetrics};
use super::{Editor, RenderMode};
use crate::buffer::{LineSource, SourceLine};
use crate::editor::shaping::compute_content_width;
use crate::markdown::prefix::is_fence_line;
use crate::markdown::{BlockKind, MarkdownScanner, ParsedLine, QuoteProjection};
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

pub(crate) const LAZY_LAYOUT_MIN_LINES: usize = 20_000;
const LAYOUT_OVERSCAN_LINES: usize = 32;

#[cfg(test)]
mod lazy_tests;

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
    pub separators: Arc<Vec<bool>>,
    pub quote_projection: Arc<QuoteProjection>,
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
            separators: Arc::default(),
            quote_projection: Arc::new(QuoteProjection::default()),
            quote_ranges: Arc::default(),
        }
    }
}

impl DocumentLayoutCache {
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
            let preview_height = if idx > 0
                && lines.source_line(SourceLine::new(idx - 1)).trim() == "$$"
            {
                let close_idx = (idx + 1..lines.line_count())
                    .find(|&line| lines.source_line(SourceLine::new(line)).trim() == "$$");
                close_idx
                    .and_then(|close_idx| {
                        let mut source = String::new();
                        for line in idx..close_idx {
                            if line > idx {
                                source.push('\n');
                            }
                            source.push_str(&lines.source_line(SourceLine::new(line)));
                        }
                        crate::math::prepare_latex(&source, true)
                    })
                    .map(|math| {
                        crate::editor::shaping::math_render_size(&math, px(lcx.font_size)).height
                            + typo.padding_vertical
                    })
                    .unwrap_or(px(0.0))
            } else {
                px(0.0)
            };
            source_height.max(preview_height)
        }
        BlockKind::Image { url, .. } => {
            let base_h = if lcx.is_active {
                px(38.0)
            } else {
                typo.padding_vertical
            };
            if crate::http::is_url_loaded(url) {
                if let Some((w, h)) = crate::http::get_image_size(url, lcx.doc_path)
                    && w > 0
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
    fn measure_line_layout_height(
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
    fn project_ime_preedit(&self, first_line: usize, parsed_lines: &mut [ParsedLine]) {
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
        self.layout_epoch
            .set(self.layout_epoch.get().wrapping_add(1));
        self.layout_task = None;
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

    fn line_layout_ctx(
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

    fn active_math_block_with_projection(
        &self,
        active_line: SourceLine,
        projection: &QuoteProjection,
    ) -> Option<(SourceLine, SourceLine)> {
        if self.render_mode == RenderMode::Source {
            return None;
        }
        #[cfg(test)]
        self.layout_validation_scans.update(|count| count + 1);
        crate::markdown::math_block_range_with_projection(projection, active_line)
            .filter(|(start, end)| active_line > *start && active_line < *end)
    }

    fn active_mermaid_block_with_projection(
        &self,
        active_line: SourceLine,
        projection: &QuoteProjection,
    ) -> Option<(SourceLine, SourceLine)> {
        if self.render_mode == RenderMode::Source {
            return None;
        }
        #[cfg(test)]
        self.layout_validation_scans.update(|count| count + 1);
        crate::markdown::mermaid_block_range_with_projection(projection, active_line)
    }

    /// Reuse projection storage only when no reader retains the old snapshot.
    #[cfg(test)]
    fn try_update_quote_projection(&self) -> Option<Arc<QuoteProjection>> {
        let delta = self.buffer.last_edit_delta()?;
        let mut cached = self.layout_cache.borrow_mut();
        if cached.buffer_revision.wrapping_add(1) != self.buffer.revision()
            || cached.quote_projection.len() != cached.lines_len
        {
            return None;
        }
        let cache = Arc::get_mut(&mut cached)?;
        let projection = Arc::get_mut(&mut cache.quote_projection)?;
        if !projection.try_update_source(&self.buffer, delta) {
            return None;
        }
        Some(cache.quote_projection.clone())
    }

    fn parse_range(
        &self,
        range: Range<usize>,
        _projection: &QuoteProjection,
        _in_code_block: bool,
    ) -> Vec<ParsedLine> {
        #[cfg(test)]
        let mut scanned = 0;
        let (window, parsed) = if range == (0..self.buffer.line_count()) {
            let result = MarkdownScanner::scan_window(&self.buffer, range.clone());
            #[cfg(test)]
            {
                scanned += result.1.len();
            }
            result
        } else {
            let store = super::parsed_line_store::ParsedLineStore::windowed(&self.buffer);
            let parsed = store.read_range(range.clone()).to_vec();
            #[cfg(test)]
            {
                scanned += store.resident_rows();
            }
            (range.clone(), parsed)
        };
        let mut parsed = if window == range {
            parsed
        } else {
            parsed
                .into_iter()
                .skip(range.start - window.start)
                .take(range.len())
                .collect()
        };
        self.project_ime_preedit(range.start, &mut parsed);
        #[cfg(test)]
        self.layout_stats.update(|mut stats| {
            stats.lines_scanned += scanned;
            stats
        });
        parsed
    }

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
        let disclosure = self.disclosure_position();
        range
            .zip(parsed)
            .map(|(idx, parsed)| {
                let source_line = SourceLine::new(idx);
                let lcx = self.line_layout_ctx(
                    parsed,
                    source_line == disclosure.line,
                    separators.get(idx).copied().unwrap_or(false),
                    disclosure.col,
                    content_width,
                    source_line,
                    active_math_block,
                    active_mermaid_block,
                );
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
            let end = (start + 32).min(range.end);
            let heights = self.measure_range(
                start..end,
                cache.parsed_lines.range(start..end),
                &cache.separators,
                cache.content_width,
                cache.active_math_block,
                cache.active_mermaid_block,
            );
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

    fn measure_viewport(&self, cache: &mut DocumentLayoutCache) {
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
    pub(super) fn measure_all_pending_for_test(&mut self) {
        self.ensure_layout_cache();
        {
            let mut cache = self.layout_cache.borrow_mut();
            let len = cache.metrics.len();
            self.measure_pending_range(Arc::make_mut(&mut cache), 0..len);
        }
        self.apply_layout_scroll_adjustment();
        self.rebase_scroll();
    }

    pub fn ensure_layout_cache(&self) {
        let viewport = self.viewport_bounds_val();
        let content_width = compute_content_width(viewport.size.width, self.render_mode);
        let disclosure_position = self.disclosure_position();
        let active_line = disclosure_position.line;
        let active_col = disclosure_position.col;
        let current_img_v = crate::http::image_cache_version();
        let current_syntax_v = crate::syntax::syntax_cache_version();

        // Derived block state cannot change while its inputs and disclosure position match.
        {
            let cache = self.layout_cache.borrow();
            if cache.buffer_version == self.buffer.version()
                && cache.buffer_revision == self.buffer.revision()
                && cache.lines_len == self.buffer.line_count()
                && self.cache_matches_view(&cache, content_width)
                && cache.image_cache_version == current_img_v
                && cache.syntax_cache_version == current_syntax_v
                && cache.active_line == active_line
                && (self.render_mode == RenderMode::Source || cache.active_col == active_col)
            {
                return;
            }
        }

        #[cfg(test)]
        let updated_projection = self.try_update_quote_projection();
        #[cfg(not(test))]
        let updated_projection: Option<Arc<QuoteProjection>> = None;
        let current_cache = self.layout_cache.borrow().clone();
        let view_matches = self.cache_matches_view(&current_cache, content_width);
        let buffer_matches = current_cache.buffer_version == self.buffer.version()
            && current_cache.buffer_revision == self.buffer.revision()
            && current_cache.lines_len == self.buffer.line_count();
        let quote_projection =
            if buffer_matches && current_cache.quote_projection.len() == self.buffer.line_count() {
                current_cache.quote_projection.clone()
            } else if let Some(projection) = updated_projection {
                projection
            } else {
                Arc::new(QuoteProjection::from_source(&self.buffer))
            };
        let active_math_block =
            self.active_math_block_with_projection(active_line, &quote_projection);
        let active_mermaid_block =
            self.active_mermaid_block_with_projection(active_line, &quote_projection);
        let cached_active_mermaid_block = current_cache.active_mermaid_block;

        if buffer_matches
            && view_matches
            && current_cache.image_cache_version == current_img_v
            && current_cache.syntax_cache_version == current_syntax_v
            && current_cache.active_line == active_line
            && current_cache.active_math_block == active_math_block
            && current_cache.active_mermaid_block == active_mermaid_block
            && (self.render_mode == RenderMode::Source || current_cache.active_col == active_col)
        {
            return;
        }

        self.layout_epoch
            .set(self.layout_epoch.get().wrapping_add(1));

        if buffer_matches
            && !view_matches
            && current_cache.metrics.len() >= LAZY_LAYOUT_MIN_LINES
            && current_cache.render_mode == self.render_mode
            && current_cache.doc_path.as_deref() == self.buffer.file_path()
            && current_cache.image_cache_version == current_img_v
            && current_cache.syntax_cache_version == current_syntax_v
        {
            let scroll_top = self.scroll.current_scroll_top + self.layout_scroll_adjustment.get();
            let anchor = current_cache
                .metrics
                .line_at_relative_y(self.scroll.anchor_line, scroll_top);
            let old_top = anchor.and_then(|line| {
                current_cache
                    .metrics
                    .top_relative(line, self.scroll.anchor_line)
            });
            drop(current_cache);
            let mut cached = self.layout_cache.borrow_mut();
            let cache = Arc::make_mut(&mut cached);
            cache
                .metrics
                .set_estimate(px(self.font_size * self.line_height));
            cache.metrics.mark_unmeasured(0..cache.metrics.len());
            for region in self.display_map.fold_map().folded_regions() {
                let start = (region.start_line.index() + 1).min(cache.metrics.len());
                let end = (region.end_line.index() + 1).min(cache.metrics.len());
                cache.metrics.set_uniform_measured(start..end, px(0.0));
            }
            if let Some((old_top, new_top)) = old_top.zip(
                anchor.and_then(|line| cache.metrics.top_relative(line, self.scroll.anchor_line)),
            ) {
                self.layout_scroll_adjustment
                    .set(self.layout_scroll_adjustment.get() + new_top - old_top);
            }
            cache.content_width = content_width;
            cache.font_size = self.font_size;
            cache.line_height = self.line_height;
            cache.soft_wrap = self.soft_wrap;
            cache.fold_version = self.display_map.version();
            cache.active_line = active_line;
            cache.active_col = active_col;
            cache.active_math_block = active_math_block;
            cache.active_mermaid_block = active_mermaid_block;
            cache.space_width = self.space_width();
            self.measure_viewport(cache);
            return;
        }

        // Same line, only cursor column changed in Live Preview
        if buffer_matches
            && view_matches
            && current_cache.image_cache_version == current_img_v
            && current_cache.syntax_cache_version == current_syntax_v
            && current_cache.active_line == active_line
            && !current_cache.parsed_lines.is_empty()
            && current_cache.metrics.len() == current_cache.parsed_lines.len()
            && active_line.index() < current_cache.metrics.len()
        {
            drop(current_cache);
            let mut cached = self.layout_cache.borrow_mut();
            #[cfg(test)]
            if Arc::strong_count(&cached) > 1 {
                self.layout_stats.update(|mut stats| {
                    stats.cache_deep_clones += 1;
                    stats
                });
            }
            let cache = Arc::make_mut(&mut cached);
            let active_line_idx = active_line.index();
            let is_sep = if cache.separators.len() == cache.lines_len {
                cache
                    .separators
                    .get(active_line_idx)
                    .copied()
                    .unwrap_or(false)
            } else {
                crate::markdown::is_syntactic_separator_with_projection(
                    &quote_projection,
                    active_line,
                )
            };
            let new_h = if let Some(parsed) = cache.parsed_lines.get(active_line_idx) {
                let lcx = self.line_layout_ctx(
                    parsed,
                    true,
                    is_sep,
                    active_col,
                    content_width,
                    active_line,
                    active_math_block,
                    active_mermaid_block,
                );
                self.measure_line_layout_height(active_line_idx, parsed, lcx)
            } else {
                px(24.0)
            };

            let old_h = cache.metrics.height(active_line_idx).unwrap();
            if (new_h - old_h).abs() < px(0.01) {
                cache.active_col = active_col;
                cache.active_math_block = active_math_block;
                cache.active_mermaid_block = active_mermaid_block;
                return;
            }

            cache.metrics.set_height(active_line_idx, new_h);

            cache.active_col = active_col;
            cache.active_math_block = active_math_block;
            cache.active_mermaid_block = active_mermaid_block;
            return;
        }

        // Only active_line changed (vertical cursor movement without buffer edit)
        if buffer_matches
            && view_matches
            && current_cache.image_cache_version == current_img_v
            && current_cache.syntax_cache_version == current_syntax_v
            && !current_cache.parsed_lines.is_empty()
            && current_cache.metrics.len() == current_cache.parsed_lines.len()
            && current_cache.active_math_block == active_math_block
            && cached_active_mermaid_block == active_mermaid_block
        {
            drop(current_cache);
            let mut cached = self.layout_cache.borrow_mut();
            #[cfg(test)]
            if Arc::strong_count(&cached) > 1 {
                self.layout_stats.update(|mut stats| {
                    stats.cache_deep_clones += 1;
                    stats
                });
            }
            let cache = Arc::make_mut(&mut cached);
            let old_active = cache.active_line;
            let new_active = active_line;

            let separators = if self.render_mode == RenderMode::Source {
                std::borrow::Cow::Borrowed(&[][..])
            } else if cache.separators.len() == cache.lines_len {
                std::borrow::Cow::Borrowed(cache.separators.as_slice())
            } else {
                std::borrow::Cow::Owned(
                    crate::markdown::compute_syntactic_separators_with_projection(
                        &quote_projection,
                    ),
                )
            };

            let calc_h = |source_line: SourceLine, is_active: bool| -> Pixels {
                let line_idx = source_line.index();
                if let Some(parsed) = cache.parsed_lines.get(line_idx) {
                    let is_sep = separators.get(line_idx).copied().unwrap_or(false);
                    let lcx = self.line_layout_ctx(
                        parsed,
                        is_active,
                        is_sep,
                        active_col,
                        content_width,
                        source_line,
                        active_math_block,
                        active_mermaid_block,
                    );
                    self.measure_line_layout_height(line_idx, parsed, lcx)
                } else {
                    px(24.0)
                }
            };

            let old_active_new_h = calc_h(old_active, false);
            let new_active_new_h = calc_h(new_active, true);

            let old_active_idx = old_active.index();
            let new_active_idx = new_active.index();
            let old_diff = old_active_new_h
                - cache
                    .metrics
                    .height(old_active_idx)
                    .unwrap_or(old_active_new_h);
            let new_diff = new_active_new_h
                - cache
                    .metrics
                    .height(new_active_idx)
                    .unwrap_or(new_active_new_h);

            if old_diff.abs() < px(0.01) && new_diff.abs() < px(0.01) {
                cache.active_line = active_line;
                cache.active_col = active_col;
                cache.active_math_block = active_math_block;
                cache.active_mermaid_block = active_mermaid_block;
                return;
            }

            if old_active_idx < cache.metrics.len() {
                cache.metrics.set_height(old_active_idx, old_active_new_h);
            }
            if new_active_idx < cache.metrics.len() {
                cache.metrics.set_height(new_active_idx, new_active_new_h);
            }

            cache.active_line = active_line;
            cache.active_col = active_col;
            cache.active_math_block = active_math_block;
            cache.active_mermaid_block = active_mermaid_block;
            return;
        }

        // Incremental block update on intra-block edits
        if let Some(delta) = self.buffer.last_edit_delta()
            && current_cache.buffer_revision.wrapping_add(1) == self.buffer.revision()
            && view_matches
            && current_cache.image_cache_version == current_img_v
            && current_cache.syntax_cache_version == current_syntax_v
            && current_cache.lines_len == current_cache.parsed_lines.len()
            && current_cache.metrics.len() == current_cache.lines_len
        {
            // Release our read handle so an unshared cache can be updated in place.
            // Arc::make_mut preserves snapshots retained by other readers.
            drop(current_cache);
            let mut cached = self.layout_cache.borrow_mut();
            #[cfg(test)]
            if Arc::strong_count(&cached) > 1 {
                self.layout_stats.update(|mut stats| {
                    stats.cache_deep_clones += 1;
                    stats
                });
            }
            if self.try_incremental_layout_update(
                Arc::make_mut(&mut cached),
                delta,
                active_line,
                content_width,
                quote_projection.clone(),
            ) {
                return;
            }
        }

        // Full layout rebuild
        #[cfg(test)]
        self.layout_stats.update(|mut stats| {
            stats.full_rebuilds += 1;
            stats
        });
        let current_cache = self.layout_cache.borrow().clone();
        let parsed_doc = if buffer_matches
            && view_matches
            && current_cache.syntax_cache_version == current_syntax_v
            && !current_cache.parsed_lines.is_empty()
        {
            current_cache.parsed_lines.clone()
        } else {
            Arc::new(self.parse_range(0..self.buffer.line_count(), &quote_projection, false)).into()
        };

        let separators = if self.render_mode == RenderMode::Source {
            Vec::new()
        } else {
            crate::markdown::compute_syntactic_separators_with_projection(&quote_projection)
        };

        let metrics = if parsed_doc.len() >= LAZY_LAYOUT_MIN_LINES {
            LineMetrics::estimated(parsed_doc.len(), px(self.font_size * self.line_height))
        } else {
            LineMetrics::from_heights(self.measure_range(
                0..parsed_doc.len(),
                &parsed_doc,
                &separators,
                content_width,
                active_math_block,
                active_mermaid_block,
            ))
        };

        let parsed_quote_ranges = quote_ranges(&parsed_doc);

        let mut new_cache = DocumentLayoutCache {
            buffer_version: self.buffer.version(),
            buffer_revision: self.buffer.revision(),
            lines_len: self.buffer.line_count(),
            render_mode: self.render_mode,
            content_width,
            active_line,
            active_col,
            active_math_block,
            active_mermaid_block,
            doc_path: self.buffer.file_path_buf(),
            image_cache_version: current_img_v,
            syntax_cache_version: current_syntax_v,
            fold_version: self.display_map.version(),
            font_size: self.font_size,
            line_height: self.line_height,
            soft_wrap: self.soft_wrap,
            parsed_lines: parsed_doc,
            metrics,
            space_width: self.space_width(),

            separators: separators.into(),
            quote_projection,
            quote_ranges: parsed_quote_ranges.into(),
        };

        for region in self.display_map.fold_map().folded_regions() {
            let start = (region.start_line.index() + 1).min(new_cache.metrics.len());
            let end = (region.end_line.index() + 1).min(new_cache.metrics.len());
            new_cache.metrics.set_uniform_measured(start..end, px(0.0));
        }
        self.measure_viewport(&mut new_cache);

        *self.layout_cache.borrow_mut() = Arc::new(new_cache);
    }

    fn try_incremental_layout_update(
        &self,
        cache: &mut DocumentLayoutCache,
        delta: crate::buffer::EditDelta,
        active_line: SourceLine,
        content_width: Pixels,
        quote_projection: Arc<QuoteProjection>,
    ) -> bool {
        let buf_lines = &self.buffer;
        let buf_len = buf_lines.line_count();
        let cache_len = cache.parsed_lines.len();
        let edit_start_idx = delta.start_line.index();

        if edit_start_idx + delta.old_line_count > cache_len
            || edit_start_idx + delta.new_line_count > buf_len
        {
            return false;
        }
        if cache.parsed_lines.get(edit_start_idx).is_some_and(|line| {
            matches!(
                line.kind,
                BlockKind::MermaidBlockContent | BlockKind::MermaidBlock { .. }
            )
        }) {
            return false;
        }
        let active_col = self.disclosure_col();
        let active_math_block =
            self.active_math_block_with_projection(active_line, &quote_projection);
        let active_mermaid_block =
            self.active_mermaid_block_with_projection(active_line, &quote_projection);

        if cache_len.saturating_sub(delta.old_line_count) + delta.new_line_count != buf_len {
            return false;
        }

        // Entering/leaving a preview changes every row of that block.
        if cache.active_math_block != active_math_block
            || cache.active_mermaid_block != active_mermaid_block
        {
            return false;
        }

        if self.render_mode != RenderMode::Source && cache.separators.len() != cache_len {
            return false;
        }

        for line in &cache.parsed_lines[edit_start_idx..edit_start_idx + delta.old_line_count] {
            if crate::markdown::structure::classify(crate::markdown::strip_quote_prefix(
                &line.raw_text,
            ))
            .is_fence
                || matches!(
                    line.kind,
                    BlockKind::CodeBlock { .. }
                        | BlockKind::MermaidBlock { .. }
                        | BlockKind::MathBlock { .. }
                )
            {
                return false;
            }
        }
        for idx in edit_start_idx..edit_start_idx + delta.new_line_count {
            let line = buf_lines.source_line(SourceLine::new(idx));
            let structure =
                crate::markdown::structure::classify(crate::markdown::strip_quote_prefix(&line));
            if structure.is_fence || structure.is_math_delim {
                return false;
            }
        }

        let in_code_block = edit_start_idx < cache_len
            && matches!(
                cache.parsed_lines[edit_start_idx].kind,
                BlockKind::CodeBlockContent | BlockKind::MermaidBlockContent
            );
        let in_math_block = edit_start_idx < cache_len
            && matches!(
                cache.parsed_lines[edit_start_idx].kind,
                BlockKind::MathBlockContent
            );

        // At EOF, blank lines cannot change a following block's interpretation.
        // Keep edits to the final paragraph/blank lines incremental; internal
        // separators still take the conservative full-rebuild path.
        let edits_document_tail = edit_start_idx + delta.old_line_count == cache_len
            && edit_start_idx + delta.new_line_count == buf_len;
        if !in_code_block && !edits_document_tail {
            for line in &cache.parsed_lines[edit_start_idx..edit_start_idx + delta.old_line_count] {
                let content = if line.quote.is_some() {
                    crate::markdown::strip_quote_prefix(&line.raw_text)
                } else {
                    &line.raw_text
                };
                if content.trim().is_empty() {
                    return false;
                }
            }
            for idx in edit_start_idx..edit_start_idx + delta.new_line_count {
                if quote_projection.normalized_line(idx).trim().is_empty() {
                    return false;
                }
            }
        }

        let mut block_start = edit_start_idx;
        if in_code_block {
            while block_start > 0 {
                let previous = buf_lines.source_line(SourceLine::new(block_start - 1));
                let prev_trimmed = crate::markdown::strip_quote_prefix(&previous).trim_start();
                if is_fence_line(prev_trimmed) {
                    break;
                }
                block_start -= 1;
            }
        } else if in_math_block {
            while block_start > 0 {
                if matches!(
                    cache.parsed_lines[block_start - 1].kind,
                    BlockKind::MathBlock { is_start: true }
                ) {
                    block_start -= 1;
                    break;
                }
                block_start -= 1;
            }
        } else {
            while block_start > 0
                && !buf_lines
                    .source_line(SourceLine::new(block_start - 1))
                    .trim()
                    .is_empty()
            {
                let previous = buf_lines.source_line(SourceLine::new(block_start - 1));
                let prev_trimmed = crate::markdown::strip_quote_prefix(&previous).trim_start();
                if is_fence_line(prev_trimmed) || prev_trimmed.starts_with('#') {
                    break;
                }
                block_start -= 1;
            }
        }

        let mut block_end = edit_start_idx + delta.new_line_count;
        if in_code_block {
            while block_end < buf_len {
                let next = buf_lines.source_line(SourceLine::new(block_end));
                let next_trimmed = crate::markdown::strip_quote_prefix(&next).trim_start();
                if is_fence_line(next_trimmed) {
                    break;
                }
                block_end += 1;
            }
        } else if in_math_block {
            while block_end < buf_len {
                let is_close = crate::markdown::strip_quote_prefix(
                    &buf_lines.source_line(SourceLine::new(block_end)),
                )
                .trim()
                    == "$$";
                block_end += 1;
                if is_close {
                    break;
                }
            }
        } else {
            while block_end < buf_len
                && !buf_lines
                    .source_line(SourceLine::new(block_end))
                    .trim()
                    .is_empty()
            {
                let next = buf_lines.source_line(SourceLine::new(block_end));
                let next_trimmed = crate::markdown::strip_quote_prefix(&next).trim_start();
                if is_fence_line(next_trimmed) || next_trimmed.starts_with('#') {
                    break;
                }
                block_end += 1;
            }
        }

        let old_block_end = block_end - delta.new_line_count + delta.old_line_count;
        if old_block_end > cache_len {
            return false;
        }

        let new_parsed = self.parse_range(block_start..block_end, &quote_projection, in_code_block);
        if new_parsed.len() != block_end - block_start {
            return false;
        }

        cache.separators = Arc::new(if self.render_mode == RenderMode::Source {
            Vec::new()
        } else {
            crate::markdown::compute_syntactic_separators_with_projection(&quote_projection)
        });
        let lazy_replacement =
            buf_len >= LAZY_LAYOUT_MIN_LINES && new_parsed.len() > LAYOUT_OVERSCAN_LINES * 4;
        let scroll_anchor = lazy_replacement
            .then(|| {
                let top = self.scroll.current_scroll_top + self.layout_scroll_adjustment.get();
                let line = cache
                    .metrics
                    .line_at_relative_y(self.scroll.anchor_line, top)?;
                Some((
                    line,
                    cache.metrics.top_relative(line, self.scroll.anchor_line)?,
                ))
            })
            .flatten();
        if lazy_replacement {
            cache
                .metrics
                .splice_estimated(block_start..old_block_end, new_parsed.len());
        } else {
            let new_heights = self.measure_range(
                block_start..block_end,
                &new_parsed,
                if in_code_block {
                    &[]
                } else {
                    &cache.separators
                },
                content_width,
                active_math_block,
                active_mermaid_block,
            );
            cache
                .metrics
                .splice(block_start..old_block_end, new_heights);
        }
        let mut quote_edge_rows = [None, None];
        {
            let parsed_lines = cache.parsed_lines.make_mut();
            parsed_lines.splice(block_start..old_block_end, new_parsed);
            // Quote groups can start/end on a neighboring heading excluded
            // from the reparsed block. Its edge padding must change as well.
            for (slot, row) in [
                block_start.checked_sub(1),
                (block_end < buf_len).then_some(block_end),
            ]
            .into_iter()
            .enumerate()
            {
                let Some(idx) = row else { continue };
                let Some(quote) = parsed_lines[idx].quote.as_mut() else {
                    continue;
                };
                let is_start = idx == 0 || quote_projection.quote_prefix(idx - 1).is_none();
                let is_end = idx + 1 == buf_len || quote_projection.quote_prefix(idx + 1).is_none();
                if quote.is_start != is_start || quote.is_end != is_end {
                    quote.is_start = is_start;
                    quote.is_end = is_end;
                    quote_edge_rows[slot] = Some(idx);
                }
            }
        }
        // Map the former disclosure row through any inserted/deleted lines.
        // Both old and new rows can lie outside the reparsed block.
        let old_active_idx = cache.active_line.index();
        let old_disclosure = if old_active_idx < block_start {
            Some(old_active_idx)
        } else if old_active_idx >= old_block_end {
            Some(old_active_idx - delta.old_line_count + delta.new_line_count)
        } else {
            None
        };
        let new_disclosure = (!(block_start..block_end).contains(&active_line.index()))
            .then_some(active_line.index());
        let affected_rows = [
            old_disclosure,
            new_disclosure,
            quote_edge_rows[0],
            quote_edge_rows[1],
        ];
        for (slot, candidate) in affected_rows.into_iter().enumerate() {
            let Some(line_idx) = candidate.filter(|&idx| idx < buf_len) else {
                continue;
            };
            if affected_rows[..slot].contains(&candidate) {
                continue;
            }
            let parsed = &cache.parsed_lines[line_idx];
            let source_line = SourceLine::new(line_idx);
            let lcx = self.line_layout_ctx(
                parsed,
                source_line == active_line,
                cache.separators.get(line_idx).copied().unwrap_or(false),
                active_col,
                content_width,
                source_line,
                active_math_block,
                active_mermaid_block,
            );
            let height = self.measure_line_layout_height(line_idx, parsed, lcx);
            if Some(height) != cache.metrics.height(line_idx) {
                cache.metrics.set_height(line_idx, height);
            }
        }

        cache.buffer_version = self.buffer.version();
        cache.buffer_revision = self.buffer.revision();
        cache.lines_len = buf_len;
        cache.active_line = active_line;
        cache.active_col = active_col;
        cache.active_math_block = active_math_block;
        cache.active_mermaid_block = active_mermaid_block;
        cache.quote_projection = quote_projection;
        cache.quote_ranges = quote_ranges(&cache.parsed_lines).into();
        cache.fold_version = self.display_map.version();

        if lazy_replacement {
            for region in self.display_map.fold_map().folded_regions() {
                let start = (region.start_line.index() + 1)
                    .max(block_start)
                    .min(block_end);
                let end = (region.end_line.index() + 1).min(block_end);
                if start < end {
                    cache.metrics.set_uniform_measured(start..end, px(0.0));
                }
            }
            if let Some((line, old_top)) = scroll_anchor {
                let mapped = if line >= edit_start_idx + delta.old_line_count {
                    line - delta.old_line_count + delta.new_line_count
                } else if line >= edit_start_idx {
                    edit_start_idx
                        + (line - edit_start_idx).min(delta.new_line_count.saturating_sub(1))
                } else {
                    line
                }
                .min(buf_len - 1);
                if let Some(new_top) = cache.metrics.top_relative(mapped, self.scroll.anchor_line) {
                    self.layout_scroll_adjustment
                        .set(self.layout_scroll_adjustment.get() + new_top - old_top);
                }
            }
            self.measure_viewport(cache);
        }

        true
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
    use super::{Editor, QuoteProjection, RenderMode, SourceLine, compute_content_width};
    use crate::buffer::{Position, Selection, TextBuffer};
    use gpui::px;
    use std::sync::Arc;

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
    fn normalized_blank_quote_edits_require_a_separator_state_rebuild(
        cx: &mut gpui::TestAppContext,
    ) {
        let (editor, cx) = cx.add_window_view(|_window, cx| {
            Editor::new_with_buffer(
                TextBuffer::from_str("old active\n\n> ## heading\n> \n\n\nlast", None),
                cx,
            )
        });
        cx.update(|_window, cx| {
            editor.update(cx, |ed, _| {
                ed.render_mode = RenderMode::LivePreview;
                let mut cache = (*ed.layout_snapshot()).clone();
                ed.buffer.set_cursor(Position::new(SourceLine::new(3), 2));
                ed.buffer.insert_text("text");
                assert!(!ed.try_incremental_layout_update(
                    &mut cache,
                    ed.buffer.last_edit_delta().unwrap(),
                    ed.disclosure_position().line,
                    compute_content_width(ed.viewport_bounds.size.width, ed.render_mode),
                    Arc::new(QuoteProjection::from_source(&ed.buffer)),
                ));
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
    fn unshared_local_layout_updates_reuse_unchanged_parsed_text(cx: &mut gpui::TestAppContext) {
        let (editor, cx) = cx.add_window_view(|_window, cx| {
            Editor::new_with_buffer(TextBuffer::from_str("first\n\nedit here\n\nlast", None), cx)
        });
        cx.update(|_window, cx| {
            editor.update(cx, |ed, _| {
                let before = ed.layout_snapshot();
                let unchanged = before.parsed_lines[0].raw_text.as_ptr();
                drop(before);
                ed.buffer
                    .record_single_line_edit(SourceLine::new(2), |line, _| {
                        line.push_str(" changed")
                    });
                let after = ed.layout_snapshot();
                assert_eq!(after.parsed_lines[0].raw_text.as_ptr(), unchanged);
                assert_eq!(after.parsed_lines[2].raw_text, "edit here changed");
                let retained = after.parsed_lines.clone();
                drop(after);
                ed.buffer
                    .record_single_line_edit(SourceLine::new(2), |line, _| line.push_str(" again"));
                assert_eq!(
                    ed.layout_snapshot().parsed_lines[2].raw_text,
                    "edit here changed again"
                );
                assert_eq!(retained[2].raw_text, "edit here changed");
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
                        ed.buffer.set_cursor(Position::new(SourceLine::new(old_line), 0));
                        ed.invalidate_layout_cache();
                        let mut cache = (*ed.layout_snapshot()).clone();
                        ed.buffer.set_cursor(Position::new(SourceLine::new(edit_line), 0));
                        if delete_next_line {
                            ed.buffer.set_selection(Selection {
                                anchor: Position::new(SourceLine::new(edit_line), 0),
                                head: Position::new(SourceLine::new(edit_line + 1), 0),
                            });
                        }
                        ed.buffer.insert_text(replacement);
                        let active_line = ed.disclosure_position().line;
                        let projection = Arc::new(QuoteProjection::from_source(&ed.buffer));
                        assert!(ed.try_incremental_layout_update(
                            &mut cache,
                            ed.buffer.last_edit_delta().unwrap(),
                            active_line,
                            compute_content_width(ed.viewport_bounds.size.width, mode),
                            projection,
                        ), "cursor jump should permit a local layout update ({mode:?}, {old_line} -> {edit_line})");
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
                        let mut cache = (*before).clone();
                        ed.buffer.set_cursor(Position::new(SourceLine::new(edit_line), edit_col));
                        ed.buffer.insert_text("changed ");
                        let projection = Arc::new(QuoteProjection::from_source(&ed.buffer));
                        assert!(ed.try_incremental_layout_update(
                            &mut cache,
                            ed.buffer.last_edit_delta().unwrap(),
                            ed.disclosure_position().line,
                            compute_content_width(ed.viewport_bounds.size.width, mode),
                            projection,
                        ));
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
