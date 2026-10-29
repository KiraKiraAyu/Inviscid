use gpui::*;
use std::path::PathBuf;

pub const SOURCE_GUTTER_WIDTH: Pixels = px(52.0);

/// Total horizontal padding subtracted from window width when sizing the document container.
pub const CONTENT_HORIZONTAL_PADDING: Pixels = px(48.0);

/// Maximum content container width in Source mode.
pub const MAX_CONTENT_WIDTH_SOURCE: Pixels = px(960.0);

/// Maximum content container width in Live Preview mode.
pub const MAX_CONTENT_WIDTH_LIVE: Pixels = px(860.0);

/// Bottom overscroll buffer allowing the last line to sit comfortably above the window bottom.
pub const BOTTOM_OVERSCROLL: Pixels = px(96.0);

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
        BlockKind::CodeBlock { is_fence_start } => {
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
        BlockKind::CodeBlockContent => BlockTypography::new(
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
        BlockKind::BlockQuote => BlockTypography::new(
            px(base_font_size),
            px((base_font_size * line_height_factor).round()),
            px((8.0 * scale).round()),
        ),
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
        BlockKind::BlockQuote => Some(theme.quote_text),
        BlockKind::CodeBlock {
            is_fence_start: true,
        } => Some(theme.code_block_header_fg),
        BlockKind::CodeBlockContent => Some(theme.code_block_text),
        BlockKind::Image { .. } => Some(theme.text_secondary),
        _ => None,
    }
}

    pub active_col: usize,
    pub active_math_block: Option<(usize, usize)>,
    pub doc_path: Option<PathBuf>,
    pub image_cache_version: usize,
    pub syntax_cache_version: usize,
    pub fold_version: usize,
    }

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(super) struct LineGeometry {
    top: Pixels,
    height: Pixels,
}

impl LineGeometry {
    pub(super) const fn new(top: Pixels, height: Pixels) -> Self {
        Self { top, height }
    }

    pub(super) const fn top(self) -> Pixels {
        self.top
    }

    #[cfg(test)]
    pub(super) const fn height(self) -> Pixels {
        self.height
    }

    pub(super) fn bottom(self) -> Pixels {
        self.top + self.height
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct VisibleLayoutLine {
    source_line: SourceLine,
    geometry: LineGeometry,
}

impl VisibleLayoutLine {
    pub(super) const fn source_line(self) -> SourceLine {
        self.source_line
    }

    pub(super) const fn geometry(self) -> LineGeometry {
        self.geometry
    }
}

impl Default for DocumentLayoutCache {
    fn default() -> Self {
    
            active_col: 0,
            active_math_block: None,
            doc_path: None,
            image_cache_version: 0,
            syntax_cache_version: 0,
            fold_version: 0,
            pub(super) fn source_line_count(&self) -> usize {
        self.line_geometry.len()
    }

    pub(super) fn line_geometry(&self, source_line: SourceLine) -> Option<LineGeometry> {
        self.line_geometry.get(source_line.index()).copied()
    }

    #[cfg(test)]
    pub(super) fn line_geometries(&self) -> &[LineGeometry] {
        &self.line_geometry
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
    pub fn indent_px(&self, line_idx: usize) -> Pixels {
        let Some(parsed) = self.parsed_lines.get(line_idx) else {
        
pub struct LineLayoutContext<'a> {
    pub is_active: bool,
    pub math_block_editing: bool,
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
    pub text_system: Option<Arc<WindowTextSystem>>,
}

impl<'a> std::fmt::Debug for LineLayoutContext<'a> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LineLayoutContext")
            .field("is_active", &self.is_active)
            .field("math_block_editing", &self.math_block_editing)
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
            .field("has_text_system", &self.text_system.is_some())
            .finish()
    }
}

impl<'a> Default for LineLayoutContext<'a> {
    fn default() -> Self {
        Self {
            is_active: false,
            math_block_editing: false,
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
    lines: &[String],
    idx: usize,
    parsed: &ParsedLine,
    lcx: LineLayoutContext,
) -> Pixels {
    if lcx.is_folded {
        return px(0.0);
    }

    use crate::editor::shaping::{
        LineShapingContext, build_line_runs, calculate_visual_lines_with_runs,
        compute_available_line_width_with_indent,
    };

    let content_width =
        if lcx.math_block_editing && matches!(parsed.kind, BlockKind::MathBlockContent) {
            (lcx.content_width / 2.0 + px(40.0)).max(px(0.0))
        } else {
            lcx.content_width
        };
    let available_w = compute_available_line_width_with_indent(
        content_width,
        &parsed.kind,
        lcx.mode,
        lcx.indent_px,
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

    match &parsed.kind {
        BlockKind::Heading { .. } => {
            let color = get_block_color_override(&parsed.kind, &crate::theme::DEFAULT_THEME);
            wrapped_prose_height(
                font(crate::platform::platform_ui_font()).bold(),
                color,
                true,
            )
        }
        BlockKind::CodeBlock { .. } | BlockKind::CodeBlockContent | BlockKind::ThematicBreak => {
            typo.base_layout_height
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
            let preview_height = if idx > 0 && lines[idx - 1].trim() == "$$" {
                let close_idx = (idx + 1..lines.len()).find(|&line| lines[line].trim() == "$$");
                close_idx
                    .and_then(|close_idx| {
                        crate::math::prepare_latex(&lines[idx..close_idx].join("\n"), true)
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
        BlockKind::BlockQuote => {
            let is_prev_quote = idx > 0 && lines[idx - 1].trim_start().starts_with("> ");
            let is_next_quote =
                idx + 1 < lines.len() && lines[idx + 1].trim_start().starts_with("> ");
            let color = get_block_color_override(&parsed.kind, &crate::theme::DEFAULT_THEME);
            let mut h =
                wrapped_prose_height(font(crate::platform::platform_ui_font()), color, false);
            if !is_prev_quote {
                h += px(4.0);
            }
            if !is_next_quote {
                h += px(4.0);
            }
            h
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
                    let img_h = px((h as f32) * f32::from(lcx.content_width) / (w as f32));
                    return base_h + img_h + px(12.0);
                }
                base_h + px(40.0) + px(12.0)
            } else if crate::http::is_url_failed(url) {
                base_h + px(40.0) + px(12.0)
            } else {
                base_h + px(140.0) + px(12.0)
            }
        }
        BlockKind::Paragraph => {
            let is_sep = lcx
                .is_separator
                .unwrap_or_else(|| crate::markdown::is_syntactic_separator(lines, idx));
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
                let is_last = idx + 1 >= lines.len()
                    || !lines[idx + 1].contains('|')
                    || lines[idx + 1].trim().is_empty()
                    || lines[idx + 1].trim_start().starts_with('#')
                    || lines[idx + 1].trim_start().starts_with("```");
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
    }
}

impl Editor {
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
        let Some(line_idx) = start.line.checked_sub(first_line) else {
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

    fn cache_matches_view(&self, cache: &DocumentLayoutCache, content_width: Pixels) -> bool {
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
        
            Some(is_sep),
            cursor_col,
            self.render_mode,
            content_width,
            self.buffer.file_path(),
        )
        .with_folded(is_folded)
        .with_math_block_editing(
            active_math_block.is_some_and(|(start, end)| (start..=end).contains(&line_idx)),
        )
        .with_selection_disclosure(self.is_selection_disclosure())
        .with_metrics(self.font_size, self.line_height, self.soft_wrap)
        .with_indent(indent_px)
        .with_text_system(self.text_system.clone())
    }

    
        let viewport = self.viewport_bounds_val();
        let content_width = compute_content_width(viewport.size.width, self.render_mode);
        let disclosure_position = self.disclosure_position();
        let active_line = disclosure_position.line;
        let active_col = disclosure_position.col;
        let active_math_block = self.active_math_block(active_line);
        let current_img_v = crate::http::image_cache_version();
        let current_syntax_v = crate::syntax::syntax_cache_version();
        
            && current_cache.image_cache_version == current_img_v
            && current_cache.syntax_cache_version == current_syntax_v
            && current_cache.active_line == active_line
            && current_cache.active_math_block == active_math_block
            && (self.render_mode == RenderMode::Source || current_cache.active_col == active_col)
        {
            return;
        }

        // Same line, only cursor column changed in Live Preview
        if buffer_matches
            && view_matches
            let is_sep = self
                .buffer
                .line_index()
                .caret_map()
                .is_syntactic_separator(SourceLine::new(active_line));
            let new_h = if let Some(parsed) = cache.parsed_lines.get(active_line) {
                let lcx = self.line_layout_ctx(
                    parsed,
                    true,
                    is_sep,
                    active_col,
                    content_width,
                    active_line,
                    active_math_block,
                );
                get_line_layout_height(self.buffer.lines(), active_line, parsed, lcx)
                
                *self.layout_cache.borrow_mut() = Arc::new(cache);
                return;
                cache.total_height = if cache.line_geometry.is_empty() {
                px(48.0)
            } else {
                y + px(24.0)
            };
            cache.active_col = active_col;
            cache.active_math_block = active_math_block;
            *self.layout_cache.borrow_mut() = Arc::new(cache);
            return;
        }

        // Only active_line changed (vertical cursor movement without buffer edit)
        if buffer_matches
        let caret_map = self.buffer.line_index().caret_map();

            let calc_h = |line_idx: usize, is_active: bool| -> Pixels {
                if let Some(parsed) = cache.parsed_lines.get(line_idx) {
                    let is_sep = self.render_mode != RenderMode::Source
                        && caret_map.is_syntactic_separator(SourceLine::new(line_idx));
                    let lcx = self.line_layout_ctx(
                        parsed,
                        is_active,
                        is_sep,
                        active_col,
                        content_width,
                        line_idx,
                        active_math_block,
                    );
                    get_line_layout_height(self.buffer.lines(), line_idx, parsed, lcx)
                } else {
                    px(24.0)
                }
            };

            let old_active_new_h = calc_h(old_active, false);
            .map(|geometry| geometry.height)
                    .unwrap_or(old_active_new_h);
            let new_diff = new_active_new_h
                - cache
                    .line_geometry
                    .get(new_active)
                    .map(|geometry| geometry.height)
                    .unwrap_or(new_active_new_h);

            if old_diff.abs() < px(0.01) && new_diff.abs() < px(0.01) {
                cache.active_line = active_line;
                cache.active_col = active_col;
                cache.active_math_block = active_math_block;
                if new_active < cache.source_line_count() {
                cache.line_geometry[new_active].height = new_active_new_h;
            }

            let start_rebuild = old_active.min(new_active);
            let mut y = if start_rebuild == 0 {
                for geometry in &mut cache.line_geometry[start_rebuild..] {
                geometry.top = y;
                y += geometry.height;
            }
            cache.total_height = if cache.line_geometry.is_empty() {
                px(48.0)
            } else {
                y + px(24.0)
            };
            cache.active_line = active_line;
            cache.active_col = active_col;
            cache.active_math_block = active_math_block;
            *self.layout_cache.borrow_mut() = Arc::new(cache);
            return;
        }

        // Incremental block update on intra-block edits
        
                *self.layout_cache.borrow_mut() = Arc::new(cache);
                return;
            }
        }

        // Full layout rebuild
        let parsed_doc = if buffer_matches
            && view_matches
            && current_cache.syntax_cache_version == current_syntax_v
            && !current_cache.parsed_lines.is_empty()
        {
            current_cache.parsed_lines.clone()
        } else {
            let mut scanned = MarkdownScanner::scan_document(self.buffer.lines());
            let mut line_geometry = Vec::with_capacity(parsed_doc.len());
        let mut y = px(24.0);

        for (idx, parsed) in parsed_doc.iter().enumerate() {
            let is_active = idx == active_line;
            let is_sep = self.render_mode != RenderMode::Source
                && caret_map.is_syntactic_separator(SourceLine::new(idx));
            let lcx = self.line_layout_ctx(
                parsed,
                is_active,
                is_sep,
                active_col,
                content_width,
                let total_height = if line_geometry.is_empty() {
            px(48.0)
        } else {
            y + px(24.0)
        };

                
            active_col,
            active_math_block,
            doc_path: self.buffer.file_path_buf(),
            image_cache_version: current_img_v,
            syntax_cache_version: current_syntax_v,
            fold_version: self.display_map.version(),
            
    }

    fn try_incremental_layout_update(
        &self,
        cache: &mut DocumentLayoutCache,
        delta: crate::buffer::EditDelta,
        active_line: usize,
        content_width: Pixels,
    ) -> bool {
        let buf_lines = self.buffer.lines();
        let buf_len = buf_lines.len();
        let cache_len = cache.parsed_lines.len();

        if delta.start_line + delta.old_line_count > cache_len
            || delta.start_line + delta.new_line_count > buf_len
        {
            return false;
        }
        let active_col = self.disclosure_col();
        let active_math_block = self.active_math_block(active_line);

        
                    line.kind,
                    BlockKind::CodeBlock { .. } | BlockKind::MathBlock { .. }
                )
            {
                return false;
            }
        }
        for line in &buf_lines[delta.start_line..delta.start_line + delta.new_line_count] {
            if line.trim_start().starts_with("```") || line.trim() == "$$" {
                return false;
            }
        }

        let in_code_block = delta.start_line < cache_len
            && matches!(
                cache.parsed_lines[delta.start_line].kind,
                BlockKind::CodeBlockContent
            );
        let in_math_block = delta.start_line < cache_len
            && matches!(
                cache.parsed_lines[delta.start_line].kind,
                BlockKind::MathBlockContent
            );

        if !in_code_block {
            for line in
                &cache.parsed_lines[delta.start_line..delta.start_line + delta.old_line_count]
            {
                if line.raw_text.trim().is_empty() {
                    return false;
                }
            }
            for line in &buf_lines[delta.start_line..delta.start_line + delta.new_line_count] {
                if line.trim().is_empty() {
                    return false;
                }
            }
        }

        let mut block_start = delta.start_line;
        if in_code_block {
            while block_start > 0 {
                let prev_trimmed = buf_lines[block_start - 1].trim_start();
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
            while block_start > 0 && !buf_lines[block_start - 1].trim().is_empty() {
                let prev_trimmed = buf_lines[block_start - 1].trim_start();
                if is_fence_line(prev_trimmed) || prev_trimmed.starts_with('#') {
                    break;
                }
                block_start -= 1;
            }
        }

        let mut block_end = delta.start_line + delta.new_line_count;
        if in_code_block {
            while block_end < buf_len {
                let next_trimmed = buf_lines[block_end].trim_start();
                if is_fence_line(next_trimmed) {
                    break;
                }
                block_end += 1;
            }
        } else if in_math_block {
            while block_end < buf_len {
                let is_close = buf_lines[block_end].trim() == "$$";
                block_end += 1;
                if is_close {
                    break;
                }
            }
        } else {
            while block_end < buf_len && !buf_lines[block_end].trim().is_empty() {
                let next_trimmed = buf_lines[block_end].trim_start();
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

        if cache.active_line != active_line
            && (cache.active_line < block_start || cache.active_line >= old_block_end)
        {
            return false;
        }

        
        }
        let is_sep = !in_code_block
                && self.render_mode != RenderMode::Source
                && caret_map.is_syntactic_separator(SourceLine::new(line_idx));
            let lcx = self.line_layout_ctx(
                parsed,
                is_active,
                is_sep,
                active_col,
                content_width,
                line_idx,
                active_math_block,
            );
            let h = get_line_layout_height(buf_lines, line_idx, parsed, lcx);
                .map(|geometry| geometry.height)
            .fold(px(0.0), |acc, height| acc + height);
        let height_delta = new_height_sum - old_height_sum;

        Arc::make_mut(&mut cache.parsed_lines).splice(block_start..old_block_end, new_parsed);
        let mut y = if block_start == 0 {
            px(24.0)
        } else {
            cache
                .line_geometry
                .get(block_start)
                .map(|geometry| geometry.top)
                .unwrap_or(cache.total_height - px(24.0))
        };
        let new_geometry = new_heights
            .into_iter()
            .map(|height| {
                let geometry = LineGeometry::new(y, height);
                y += height;
                geometry
            })
            .collect::<Vec<_>>();
        cache
            .line_geometry
            .splice(block_start..old_block_end, new_geometry);

        if height_delta.abs() > px(0.001) {
            for geometry in &mut cache.line_geometry[block_end..] {
                geometry.top += height_delta;
            }
            cache.total_height += height_delta;
        }

        cache.buffer_version = self.buffer.version();
        cache.active_line = active_line;
        cache.active_col = active_col;
        cache.active_math_block = active_math_block;
        cache.fold_version = self.display_map.version();
            layout.source_line_y_range(target_line)
    }

    pub fn get_total_content_height(&self) -> Pixels {
        self.layout_snapshot().total_height
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
        self.scrollbar_metrics(self.viewport_height(), self.get_total_content_height())
            .max_scroll
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
            let scroll_ratio = (self.scroll.current_scroll_top / max_scroll).clamp(0.0, 1.0);
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
