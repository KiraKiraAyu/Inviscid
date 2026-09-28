use gpui::*;
use std::path::PathBuf;
use std::sync::Arc;

use super::{Editor, RenderMode};
use crate::editor::shaping::compute_content_width;
use crate::markdown::{BlockKind, MarkdownScanner, ParsedLine};
use crate::theme::Theme;

/// Width reserved for the line-number gutter in Source mode.
pub const SOURCE_GUTTER_WIDTH: Pixels = px(52.0);

/// Total horizontal padding subtracted from window width when sizing the document container.
pub const CONTENT_HORIZONTAL_PADDING: Pixels = px(48.0);

/// Maximum content container width in Source mode.
pub const MAX_CONTENT_WIDTH_SOURCE: Pixels = px(960.0);

/// Maximum content container width in Live Preview mode.
pub const MAX_CONTENT_WIDTH_LIVE: Pixels = px(860.0);

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

#[derive(Clone, Debug)]
pub struct DocumentLayoutCache {
    pub buffer_version: usize,
    pub lines_len: usize,
    pub render_mode: RenderMode,
    pub content_width: Pixels,
    pub active_line: usize,
    pub active_col: usize,
    pub doc_path: Option<PathBuf>,
    pub image_cache_version: usize,
    pub fold_version: usize,
    pub font_size: f32,
    pub line_height: f32,
    pub soft_wrap: bool,
    pub parsed_lines: Arc<Vec<ParsedLine>>,
    pub line_heights: Vec<Pixels>,
    pub line_y_offsets: Vec<Pixels>,
    pub total_height: Pixels,
    pub separators: Vec<bool>,
}

impl Default for DocumentLayoutCache {
    fn default() -> Self {
        Self {
            buffer_version: 0,
            lines_len: 0,
            render_mode: RenderMode::LivePreview,
            content_width: px(0.0),
            active_line: 0,
            active_col: 0,
            doc_path: None,
            image_cache_version: 0,
            fold_version: 0,
            font_size: 15.0,
            line_height: 1.6,
            soft_wrap: true,
            parsed_lines: Arc::new(Vec::new()),
            line_heights: Vec::new(),
            line_y_offsets: Vec::new(),
            total_height: px(0.0),
            separators: Vec::new(),
        }
    }
}

#[derive(Clone)]
pub struct LineLayoutContext<'a> {
    pub is_active: bool,
    pub is_separator: Option<bool>,
    pub cursor_col: usize,
    pub mode: RenderMode,
    pub content_width: Pixels,
    pub doc_path: Option<&'a std::path::Path>,
    pub is_folded: bool,
    pub font_size: f32,
    pub line_height: f32,
    pub soft_wrap: bool,
    pub text_system: Option<Arc<TextSystem>>,
}

impl<'a> std::fmt::Debug for LineLayoutContext<'a> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LineLayoutContext")
            .field("is_active", &self.is_active)
            .field("is_separator", &self.is_separator)
            .field("cursor_col", &self.cursor_col)
            .field("mode", &self.mode)
            .field("content_width", &self.content_width)
            .field("doc_path", &self.doc_path)
            .field("is_folded", &self.is_folded)
            .field("font_size", &self.font_size)
            .field("line_height", &self.line_height)
            .field("soft_wrap", &self.soft_wrap)
            .field("has_text_system", &self.text_system.is_some())
            .finish()
    }
}

impl<'a> Default for LineLayoutContext<'a> {
    fn default() -> Self {
        Self {
            is_active: false,
            is_separator: None,
            cursor_col: 0,
            mode: RenderMode::LivePreview,
            content_width: px(800.0),
            doc_path: None,
            is_folded: false,
            font_size: 15.0,
            line_height: 1.6,
            soft_wrap: true,
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
            is_separator,
            cursor_col,
            mode,
            content_width,
            doc_path,
            is_folded: false,
            font_size: 15.0,
            line_height: 1.6,
            soft_wrap: true,
            text_system: None,
        }
    }

    pub fn with_folded(mut self, is_folded: bool) -> Self {
        self.is_folded = is_folded;
        self
    }

    pub fn with_metrics(mut self, font_size: f32, line_height: f32, soft_wrap: bool) -> Self {
        self.font_size = font_size;
        self.line_height = line_height;
        self.soft_wrap = soft_wrap;
        self
    }

    pub fn with_text_system(mut self, text_system: Option<Arc<TextSystem>>) -> Self {
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
        LineShapingContext, build_line_runs, calculate_visual_lines,
        compute_available_line_width_with_mode,
    };

    let available_w =
        compute_available_line_width_with_mode(lcx.content_width, &parsed.kind, lcx.mode);

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
        );
        let (line_str, _) = build_line_runs(&parsed.spans, scx, None, &TextStyle::default());
        let visual_lines = calculate_visual_lines(
            &line_str,
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
        .with_heading(is_heading);
        let (line_str, _) = build_line_runs(&parsed.spans, scx, None, &TextStyle::default());
        let visual_lines = calculate_visual_lines(
            &line_str,
            font,
            typo.font_size,
            available_w,
            lcx.soft_wrap,
            lcx.text_system.as_ref(),
            false,
        );
        typo.padding_vertical + typo.line_height * (visual_lines as f32)
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
    /// Marks any active IME preedit range in `parsed_lines` so composition text is excluded from
    /// line-width measurement and rendered as an overlay at the composition anchor.
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

    fn line_layout_ctx(
        &self,
        is_active: bool,
        is_sep: bool,
        cursor_col: usize,
        content_width: Pixels,
        line_idx: usize,
    ) -> LineLayoutContext<'_> {
        let is_folded = self.display_map.fold_map().is_line_folded(line_idx);
        LineLayoutContext::new(
            is_active,
            Some(is_sep),
            cursor_col,
            self.render_mode,
            content_width,
            self.buffer.file_path(),
        )
        .with_folded(is_folded)
        .with_metrics(self.font_size, self.line_height, self.soft_wrap)
        .with_text_system(self.text_system.clone())
    }

    pub fn ensure_layout_cache(&self) {
        let viewport = self.viewport_bounds_val();
        let content_width = compute_content_width(viewport.size.width, self.render_mode);
        let cursor_pos = self.buffer.cursor_pos();
        let cursor_line = cursor_pos.line;
        let cursor_col = self.disclosure_col();
        let current_img_v = crate::http::image_cache_version();

        let current_cache = self.layout_cache.borrow().clone();
        let view_matches = self.cache_matches_view(&current_cache, content_width);
        let buffer_matches = current_cache.buffer_version == self.buffer.version()
            && current_cache.lines_len == self.buffer.line_count();

        if buffer_matches
            && view_matches
            && current_cache.image_cache_version == current_img_v
            && current_cache.active_line == cursor_line
            && (self.render_mode == RenderMode::Source || current_cache.active_col == cursor_col)
        {
            return;
        }

        // Same line, only cursor column changed in Live Preview
        if buffer_matches
            && view_matches
            && current_cache.image_cache_version == current_img_v
            && current_cache.active_line == cursor_line
            && !current_cache.parsed_lines.is_empty()
            && current_cache.line_heights.len() == current_cache.parsed_lines.len()
            && cursor_line < current_cache.line_heights.len()
        {
            let mut cache = (*current_cache).clone();
            let is_sep = if cache.separators.len() == cache.lines_len {
                cache.separators.get(cursor_line).copied().unwrap_or(false)
            } else {
                crate::markdown::is_syntactic_separator(self.buffer.lines(), cursor_line)
            };
            let lcx = self.line_layout_ctx(true, is_sep, cursor_col, content_width, cursor_line);

            let new_h = if let Some(parsed) = cache.parsed_lines.get(cursor_line) {
                get_line_layout_height(self.buffer.lines(), cursor_line, parsed, lcx)
            } else {
                px(24.0)
            };

            let old_h = cache.line_heights[cursor_line];
            if (new_h - old_h).abs() < px(0.01) {
                cache.active_col = cursor_col;
                *self.layout_cache.borrow_mut() = Arc::new(cache);
                return;
            }

            cache.line_heights[cursor_line] = new_h;
            let mut y = cache.line_y_offsets[cursor_line] + new_h;
            for idx in (cursor_line + 1)..cache.line_heights.len() {
                cache.line_y_offsets[idx] = y;
                y += cache.line_heights[idx];
            }
            cache.total_height = if cache.line_heights.is_empty() {
                px(48.0)
            } else {
                y + px(24.0)
            };
            cache.active_col = cursor_col;
            *self.layout_cache.borrow_mut() = Arc::new(cache);
            return;
        }

        // Only active_line changed (vertical cursor movement without buffer edit)
        if buffer_matches
            && view_matches
            && current_cache.image_cache_version == current_img_v
            && !current_cache.parsed_lines.is_empty()
            && current_cache.line_heights.len() == current_cache.parsed_lines.len()
        {
            let mut cache = (*current_cache).clone();
            let old_active = cache.active_line;
            let new_active = cursor_line;

            let separators = if self.render_mode == RenderMode::Source {
                Vec::new()
            } else if cache.separators.len() == cache.lines_len {
                cache.separators.clone()
            } else {
                crate::markdown::compute_syntactic_separators(self.buffer.lines())
            };

            let calc_h = |line_idx: usize, is_active: bool| -> Pixels {
                if let Some(parsed) = cache.parsed_lines.get(line_idx) {
                    let is_sep = separators.get(line_idx).copied().unwrap_or(false);
                    let lcx = self.line_layout_ctx(
                        is_active,
                        is_sep,
                        cursor_col,
                        content_width,
                        line_idx,
                    );
                    get_line_layout_height(self.buffer.lines(), line_idx, parsed, lcx)
                } else {
                    px(24.0)
                }
            };

            let old_active_new_h = calc_h(old_active, false);
            let new_active_new_h = calc_h(new_active, true);

            let old_diff = old_active_new_h
                - cache
                    .line_heights
                    .get(old_active)
                    .copied()
                    .unwrap_or(old_active_new_h);
            let new_diff = new_active_new_h
                - cache
                    .line_heights
                    .get(new_active)
                    .copied()
                    .unwrap_or(new_active_new_h);

            if old_diff.abs() < px(0.01) && new_diff.abs() < px(0.01) {
                cache.active_line = cursor_line;
                cache.active_col = cursor_col;
                *self.layout_cache.borrow_mut() = Arc::new(cache);
                return;
            }

            if old_active < cache.line_heights.len() {
                cache.line_heights[old_active] = old_active_new_h;
            }
            if new_active < cache.line_heights.len() {
                cache.line_heights[new_active] = new_active_new_h;
            }

            let start_rebuild = old_active.min(new_active);
            let mut y = if start_rebuild == 0 {
                px(24.0)
            } else {
                cache.line_y_offsets[start_rebuild]
            };

            for idx in start_rebuild..cache.line_heights.len() {
                cache.line_y_offsets[idx] = y;
                y += cache.line_heights[idx];
            }
            cache.total_height = if cache.line_heights.is_empty() {
                px(48.0)
            } else {
                y + px(24.0)
            };
            cache.active_line = cursor_line;
            cache.active_col = cursor_col;
            *self.layout_cache.borrow_mut() = Arc::new(cache);
            return;
        }

        // Incremental block update on intra-block edits
        if let Some(delta) = self.buffer.last_edit_delta()
            && view_matches
            && current_cache.image_cache_version == current_img_v
            && current_cache.lines_len == current_cache.parsed_lines.len()
            && current_cache.line_heights.len() == current_cache.lines_len
            && current_cache.line_y_offsets.len() == current_cache.lines_len
        {
            let mut cache = (*current_cache).clone();
            if self.try_incremental_layout_update(&mut cache, delta, cursor_line, content_width) {
                *self.layout_cache.borrow_mut() = Arc::new(cache);
                return;
            }
        }

        // Full layout rebuild
        let parsed_doc = if buffer_matches && view_matches && !current_cache.parsed_lines.is_empty()
        {
            current_cache.parsed_lines.clone()
        } else {
            let mut scanned = MarkdownScanner::scan_document(self.buffer.lines());
            self.project_ime_preedit(0, &mut scanned);
            Arc::new(scanned)
        };

        let separators = if self.render_mode == RenderMode::Source {
            Vec::new()
        } else {
            crate::markdown::compute_syntactic_separators(self.buffer.lines())
        };

        let mut line_heights = Vec::with_capacity(parsed_doc.len());
        let mut line_y_offsets = Vec::with_capacity(parsed_doc.len());
        let mut y = px(24.0);

        for (idx, parsed) in parsed_doc.iter().enumerate() {
            let is_active = idx == cursor_line;
            let is_sep = separators.get(idx).copied().unwrap_or(false);
            let lcx = self.line_layout_ctx(is_active, is_sep, cursor_col, content_width, idx);
            let h = get_line_layout_height(self.buffer.lines(), idx, parsed, lcx);
            line_y_offsets.push(y);
            line_heights.push(h);
            y += h;
        }

        let total_height = if line_heights.is_empty() {
            px(48.0)
        } else {
            y + px(24.0)
        };

        let new_cache = DocumentLayoutCache {
            buffer_version: self.buffer.version(),
            lines_len: self.buffer.line_count(),
            render_mode: self.render_mode,
            content_width,
            active_line: cursor_line,
            active_col: cursor_col,
            doc_path: self.buffer.file_path_buf(),
            image_cache_version: current_img_v,
            fold_version: self.display_map.version(),
            font_size: self.font_size,
            line_height: self.line_height,
            soft_wrap: self.soft_wrap,
            parsed_lines: parsed_doc,
            line_heights,
            line_y_offsets,
            total_height,
            separators,
        };

        *self.layout_cache.borrow_mut() = Arc::new(new_cache);
    }

    fn try_incremental_layout_update(
        &self,
        cache: &mut DocumentLayoutCache,
        delta: crate::buffer::EditDelta,
        cursor_line: usize,
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
        let cursor_col = self.disclosure_col();

        if cache_len.saturating_sub(delta.old_line_count) + delta.new_line_count != buf_len {
            return false;
        }

        if self.render_mode != RenderMode::Source && cache.separators.len() != cache_len {
            return false;
        }

        for line in &cache.parsed_lines[delta.start_line..delta.start_line + delta.old_line_count] {
            if line.raw_text.trim_start().starts_with("```")
                || matches!(line.kind, BlockKind::CodeBlock { .. })
            {
                return false;
            }
        }
        for line in &buf_lines[delta.start_line..delta.start_line + delta.new_line_count] {
            if line.trim_start().starts_with("```") {
                return false;
            }
        }

        let in_code_block = delta.start_line < cache_len
            && matches!(
                cache.parsed_lines[delta.start_line].kind,
                BlockKind::CodeBlockContent
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
        while block_start > 0 && !buf_lines[block_start - 1].trim().is_empty() {
            let prev_trimmed = buf_lines[block_start - 1].trim_start();
            if prev_trimmed.starts_with("```") || prev_trimmed.starts_with('#') {
                break;
            }
            block_start -= 1;
        }

        let mut block_end = delta.start_line + delta.new_line_count;
        while block_end < buf_len && !buf_lines[block_end].trim().is_empty() {
            let next_trimmed = buf_lines[block_end].trim_start();
            if next_trimmed.starts_with("```") || next_trimmed.starts_with('#') {
                break;
            }
            block_end += 1;
        }

        let old_block_end = block_end - delta.new_line_count + delta.old_line_count;
        if old_block_end > cache_len {
            return false;
        }

        if cache.active_line != cursor_line
            && (cache.active_line < block_start || cache.active_line >= old_block_end)
        {
            return false;
        }

        let mut new_parsed =
            MarkdownScanner::scan_range(buf_lines, block_start..block_end, in_code_block);
        if new_parsed.len() != block_end - block_start {
            return false;
        }
        self.project_ime_preedit(block_start, &mut new_parsed);

        let mut new_heights = Vec::with_capacity(new_parsed.len());
        let mut new_height_sum = px(0.0);
        for (i, parsed) in new_parsed.iter().enumerate() {
            let line_idx = block_start + i;
            let is_active = line_idx == cursor_line;
            let is_sep = if in_code_block {
                false
            } else {
                cache.separators.get(line_idx).copied().unwrap_or(false)
            };
            let lcx = self.line_layout_ctx(is_active, is_sep, cursor_col, content_width, line_idx);
            let h = get_line_layout_height(buf_lines, line_idx, parsed, lcx);
            new_heights.push(h);
            new_height_sum += h;
        }

        let old_height_sum: Pixels = cache.line_heights[block_start..old_block_end]
            .iter()
            .copied()
            .fold(px(0.0), |acc, h| acc + h);
        let height_delta = new_height_sum - old_height_sum;

        Arc::make_mut(&mut cache.parsed_lines).splice(block_start..old_block_end, new_parsed);
        cache
            .line_heights
            .splice(block_start..old_block_end, new_heights.clone());

        let mut y = if block_start == 0 {
            px(24.0)
        } else {
            cache.line_y_offsets[block_start]
        };
        let mut new_offsets = Vec::with_capacity(new_heights.len());
        for h in &new_heights {
            new_offsets.push(y);
            y += *h;
        }
        cache
            .line_y_offsets
            .splice(block_start..old_block_end, new_offsets);

        if height_delta.abs() > px(0.001) {
            for offset in &mut cache.line_y_offsets[block_end..] {
                *offset += height_delta;
            }
            cache.total_height += height_delta;
        }

        if cache.separators.len() >= old_block_end {
            cache.separators.splice(
                block_start..old_block_end,
                vec![false; block_end - block_start],
            );
        }

        cache.buffer_version = self.buffer.version();
        cache.lines_len = buf_len;
        cache.active_line = cursor_line;
        cache.active_col = cursor_col;
        cache.fold_version = self.display_map.version();

        true
    }

    pub fn get_line_y_range(&self, target_line: usize) -> (Pixels, Pixels) {
        let layout = self.layout_snapshot();
        if target_line < layout.line_y_offsets.len() {
            let top = layout.line_y_offsets[target_line];
            let height = layout.line_heights[target_line];
            (top, top + height)
        } else {
            let total = layout.total_height;
            ((total - px(24.0)).max(px(0.0)), total)
        }
    }

    pub fn get_total_content_height(&self) -> Pixels {
        self.layout_snapshot().total_height
    }

    pub fn get_max_scroll_top(&self) -> Pixels {
        let total_h = self.get_total_content_height();
        let viewport = self.viewport_bounds_val();
        let vp_h = if viewport.size.height > px(50.0) {
            viewport.size.height
        } else {
            px(700.0)
        };
        let overscroll = (vp_h - px(160.0)).max(px(120.0));
        (total_h + overscroll - vp_h).max(px(0.0))
    }
}
