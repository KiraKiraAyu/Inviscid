use gpui::*;

use super::{Editor, RenderMode};
use crate::buffer::Position;
use crate::editor::shaping::{
    LineShapingContext, calculate_col_from_point, calculate_pos_from_col, compute_content_width,
    get_block_style_for_line_with_metrics,
};
use crate::markdown::{VisualDocument, get_block_prefix_len};
use crate::theme::Theme;

impl Editor {
    #[inline]
    pub(crate) fn is_active_caret_line(&self, line_idx: usize, window: &Window) -> bool {
        self.focus_handle.is_focused(window) && line_idx == self.buffer.cursor_pos().line
    }

    pub fn line_shaping_context<'a>(
        &self,
        override_color: Option<Hsla>,
        is_active_caret: bool,
        cursor_col: usize,
        theme: &'a Theme,
    ) -> LineShapingContext<'a> {
        LineShapingContext::new(
            theme,
            override_color,
            self.render_mode == RenderMode::Source,
            is_active_caret,
            cursor_col,
        )
    }

    /// Bundles font metrics, available line width, and line shaping context for text layout.
    pub fn line_shaping_metrics<'a>(
        &self,
        parsed: &crate::markdown::ParsedLine,
        is_active_caret: bool,
        theme: &'a Theme,
    ) -> (Pixels, Pixels, Option<Pixels>, LineShapingContext<'a>) {
        let is_source = self.render_mode == RenderMode::Source;
        let (font_size, line_height, override_color) = get_block_style_for_line_with_metrics(
            parsed,
            theme,
            is_source,
            self.font_size,
            self.line_height,
        );

        let viewport = self.viewport_bounds_val();
        let content_width = compute_content_width(viewport.size.width, self.render_mode);
        let available_width = crate::editor::shaping::compute_available_line_width_with_mode(
            content_width,
            &parsed.kind,
            self.render_mode,
        );
        let wrap_width = if self.soft_wrap {
            Some(available_width)
        } else {
            None
        };

        let scx = self.line_shaping_context(
            override_color,
            is_active_caret,
            self.disclosure_col(),
            theme,
        );
        (font_size, line_height, wrap_width, scx)
    }

    /// Computes the horizontal pixel X position of a buffer column on its line.
    pub fn get_pixel_x_for_col(&self, pos: Position, window: &mut Window) -> Pixels {
        if pos.line >= self.buffer.line_count() {
            return px(0.0);
        }
        let layout = self.layout_snapshot();
        if let Some(parsed) = layout.parsed_lines.get(pos.line) {
            let is_active_caret = self.is_active_caret_line(pos.line, window);
            let (font_size, line_height, wrap_width, scx) =
                self.line_shaping_metrics(parsed, is_active_caret, &crate::theme::DEFAULT_THEME);
            let in_canvas_x = calculate_pos_from_col(
                pos.col,
                parsed,
                font_size,
                line_height,
                wrap_width,
                scx,
                window,
            )
            .x;
            let left_offset =
                crate::editor::shaping::get_block_left_offset(&parsed.kind, self.render_mode);
            left_offset + in_canvas_x
        } else {
            px(0.0)
        }
    }

    /// Returns `(current_visual_line_idx, total_visual_lines)` for a buffer position.
    pub fn get_visual_line_info(&self, pos: Position, window: &mut Window) -> (usize, usize) {
        if pos.line >= self.buffer.line_count() {
            return (0, 1);
        }
        let layout = self.layout_snapshot();
        if let Some(parsed) = layout.parsed_lines.get(pos.line) {
            let is_active_caret = self.is_active_caret_line(pos.line, window);
            let (font_size, line_height, wrap_width, scx) =
                self.line_shaping_metrics(parsed, is_active_caret, &crate::theme::DEFAULT_THEME);
            crate::editor::shaping::get_visual_line_info(
                pos.col,
                parsed,
                font_size,
                line_height,
                wrap_width,
                scx,
                window,
            )
        } else {
            (0, 1)
        }
    }

    /// Maps a target pixel `(x, visual_line_idx)` to the closest raw buffer column on the line.
    pub fn get_col_for_pixel_coords(
        &self,
        line_idx: usize,
        goal_x: Pixels,
        visual_line_idx: usize,
        window: &mut Window,
    ) -> usize {
        if line_idx >= self.buffer.line_count() {
            return 0;
        }
        let layout = self.layout_snapshot();
        if let Some(parsed) = layout.parsed_lines.get(line_idx) {
            let is_active_caret = self.is_active_caret_line(line_idx, window);
            let (font_size, line_height, wrap_width, scx) =
                self.line_shaping_metrics(parsed, is_active_caret, &crate::theme::DEFAULT_THEME);
            let target_left_offset =
                crate::editor::shaping::get_block_left_offset(&parsed.kind, self.render_mode);
            let in_canvas_x = (goal_x - target_left_offset).max(px(0.0));
            let target_y = line_height * (visual_line_idx as f32) + line_height * 0.5;
            let mut col = calculate_col_from_point(
                point(in_canvas_x, target_y),
                parsed,
                font_size,
                line_height,
                wrap_width,
                scx,
                window,
            );
            if self.render_mode != RenderMode::Source {
                let prefix_len = parsed.syntax_prefix_len();
                if col < prefix_len {
                    col = prefix_len.min(self.buffer.line_len(line_idx));
                }
            }
            col
        } else {
            0
        }
    }

    fn move_vertical_live(&mut self, is_shift: bool, is_down: bool, window: &mut Window) {
        let pos = self.buffer.cursor_pos();
        let goal_x = match self.pixel_column_goal {
            Some(x) => x,
            None => {
                let current_x = self.get_pixel_x_for_col(pos, window);
                self.pixel_column_goal = Some(current_x);
                current_x
            }
        };

        let (current_v_idx, total_v_lines) = self.get_visual_line_info(pos, window);

        if is_down {
            // Intra-line stepping down across soft wrapped visual lines
            if current_v_idx + 1 < total_v_lines {
                let col =
                    self.get_col_for_pixel_coords(pos.line, goal_x, current_v_idx + 1, window);
                let new_pos = Position::new(pos.line, col);
                self.buffer.update_cursor(is_shift, new_pos);
                return;
            }

            // Step to next physical line, respecting folds and visual blocks
            let layout = self.layout_snapshot();
            let vdoc = VisualDocument::from_parsed(self.buffer.lines(), &layout.parsed_lines);
            let vpos = vdoc.source_to_visual(pos);
            let target_vpos = vdoc.next_visual_line(vpos);

            let next_line = if let Some(target_vpos) = target_vpos {
                let cand_line = vdoc.visual_to_source(target_vpos).line;
                if self.display_map.fold_map().is_line_folded(cand_line) {
                    self.display_map
                        .fold_map()
                        .next_visible_line(cand_line - 1, self.buffer.line_count())
                } else {
                    Some(cand_line)
                }
            } else {
                self.display_map
                    .fold_map()
                    .next_visible_line(pos.line, self.buffer.line_count())
            };

            if let Some(target_line) = next_line {
                let target_line_str = self.buffer.line(target_line).unwrap_or("");
                let prefix = get_block_prefix_len(target_line_str);
                let col = if target_line_str.trim_start().starts_with("```") {
                    self.buffer.line_len(target_line)
                } else {
                    let target_col = self.get_col_for_pixel_coords(target_line, goal_x, 0, window);
                    target_col
                        .max(prefix)
                        .min(self.buffer.line_len(target_line))
                };
                let new_pos = Position::new(target_line, col);
                self.buffer.update_cursor(is_shift, new_pos);
            } else if !is_shift
                && (self.buffer.is_in_code_fence(pos.line)
                    || self.buffer.is_closing_code_fence(pos.line))
            {
                self.pixel_column_goal = None;
                self.buffer.ensure_trailing_code_fence_newline();
            }
        } else {
            // Moving UP
            // Intra-line stepping up across soft wrapped visual lines
            if current_v_idx > 0 {
                let col =
                    self.get_col_for_pixel_coords(pos.line, goal_x, current_v_idx - 1, window);
                let new_pos = Position::new(pos.line, col);
                self.buffer.update_cursor(is_shift, new_pos);
                return;
            }

            // Step to previous physical line, respecting folds and visual blocks
            let layout = self.layout_snapshot();
            let vdoc = VisualDocument::from_parsed(self.buffer.lines(), &layout.parsed_lines);
            let vpos = vdoc.source_to_visual(pos);
            let target_vpos = vdoc.prev_visual_line(vpos);

            let prev_line = if let Some(target_vpos) = target_vpos {
                let cand_line = vdoc.visual_to_source(target_vpos).line;
                if self.display_map.fold_map().is_line_folded(cand_line) {
                    self.display_map.fold_map().prev_visible_line(cand_line + 1)
                } else {
                    Some(cand_line)
                }
            } else {
                self.display_map.fold_map().prev_visible_line(pos.line)
            };

            if let Some(target_line) = prev_line {
                let target_line_str = self.buffer.line(target_line).unwrap_or("");
                let prefix = get_block_prefix_len(target_line_str);
                let col = if target_line_str.trim_start().starts_with("```") {
                    self.buffer.line_len(target_line)
                } else {
                    let (_, target_total_v) =
                        self.get_visual_line_info(Position::new(target_line, 0), window);
                    let target_v_idx = target_total_v.saturating_sub(1);
                    let target_col =
                        self.get_col_for_pixel_coords(target_line, goal_x, target_v_idx, window);
                    target_col
                        .max(prefix)
                        .min(self.buffer.line_len(target_line))
                };
                let new_pos = Position::new(target_line, col);
                self.buffer.update_cursor(is_shift, new_pos);
            } else {
                self.buffer.update_cursor(is_shift, Position::new(0, 0));
            }
        }
    }

    /// Moves cursor or selection head one line up, honoring Live Preview visual block tree
    pub fn move_up(&mut self, is_shift: bool, window: &mut Window) {
        if self.render_mode == RenderMode::Source {
            self.buffer.move_up_mode(is_shift, true);
        } else {
            self.move_vertical_live(is_shift, false, window);
        }
    }

    /// Moves cursor or selection head one line down, honoring Live Preview visual block tree
    pub fn move_down(&mut self, is_shift: bool, window: &mut Window) {
        if self.render_mode == RenderMode::Source {
            self.buffer.move_down_mode(is_shift, true);
        } else {
            self.move_vertical_live(is_shift, true, window);
        }
    }

    /// Automatically adjusts the scroll viewport to keep the active cursor comfortably within view (Autoscroll)
    pub fn scroll_to_cursor(&mut self) {
        let cursor_line = self.buffer.cursor_pos().line;
        let (line_top, line_bottom) = self.get_line_y_range(cursor_line);

        let viewport_height = self.viewport_height();

        let margin = px(56.0); // Comfort margin in pixels (~2.5 lines)
        let scroll_top = self.scroll.target_scroll_top;

        let mut new_scroll_top = scroll_top;

        if line_top < scroll_top + margin {
            // Cursor is above the top visible comfort line -> scroll UP
            new_scroll_top = (line_top - margin).max(px(0.0));
        } else if line_bottom > scroll_top + viewport_height - margin {
            // If the element is taller than the viewport, ensure the top of the element is visible
            let element_height = line_bottom - line_top;
            if element_height > viewport_height - margin * 2.0 {
                new_scroll_top = (line_top - margin).max(px(0.0));
            } else {
                new_scroll_top = (line_bottom + margin - viewport_height).max(px(0.0));
            }
        }

        let max_scroll = self.get_max_scroll_top();
        self.scroll
            .set_target_scroll_top(new_scroll_top, max_scroll);
    }
}
