use crate::markdown::{EditorProjection, ProjectionMode, get_block_prefix_len};
use crate::theme::Theme;

impl Editor {
    pub(crate) fn move_horizontally<F>(&mut self, select: bool, is_right: bool, move_buffer: F)
    where
        F: FnOnce(&mut crate::buffer::TextBuffer, bool),
    {
        let is_source = self.render_mode == RenderMode::Source;
        let start = self.buffer.cursor_pos();
        let was_selection = !self.buffer.selection().is_empty();
        let version = self.buffer.version();
        let is_table = !is_source
            && crate::markdown::table::get_table_metadata(self.buffer.lines(), start.line)
                .is_some();

        if !is_source && !is_table && !was_selection {
            let line = self.buffer.line(start.line).unwrap_or("");
            let prefix = get_block_prefix_len(line);
            let at_projected_edge = if is_right {
                start.col >= self.buffer.line_len(start.line)
            } else {
                start.col <= prefix
            };

            if at_projected_edge {
                let layout = self.layout_snapshot();
                let projection = self.current_projection(&layout.parsed_lines);
                if let Some(target_line) =
                    self.display_map
                        .adjacent_caret_line(&projection, start, is_right)
                {
                    let target_line = target_line.index();
                    let target_col = if is_right {
                        get_block_prefix_len(self.buffer.line(target_line).unwrap_or(""))
                    } else {
                        self.buffer.line_len(target_line)
                    };
                    self.buffer
                        .update_cursor(select, Position::new(target_line, target_col));
                } else if is_right
                    && !select
                    && (self.buffer.is_in_code_fence(start.line)
                        || self.buffer.is_closing_code_fence(start.line))
                {
                    move_buffer(&mut self.buffer, false);
                }
                return;
            }
        }

        move_buffer(&mut self.buffer, is_source || !is_table);

        let current = self.buffer.cursor_pos();
        if is_source
            || self.buffer.version() != version
            || (!select && was_selection)
            || current.line == start.line
        {
            return;
        }

        let layout = self.layout_snapshot();
        let projection = self.current_projection(&layout.parsed_lines);
        let target = if current.line != start.line {
            self.display_map
                .adjacent_caret_line(&projection, start, is_right)
                .map(|line| {
                    let line = line.index();
                    let col = current
                        .col
                        .max(get_block_prefix_len(self.buffer.line(line).unwrap_or("")))
                        .min(self.buffer.line_len(line));
                    Position::new(line, col)
                })
        } else {
            self.display_map.normalize_position(&projection, current)
        };
        if let Some(target) = target {
            self.buffer.update_cursor(select, target);
        }
    }

    pub(crate) fn normalize_cursor_to_projection(&mut self, select: bool) {
        self.normalize_cursor_for_projection(self.render_mode, select);
    }

    pub(crate) fn normalize_cursor_for_projection(
        &mut self,
        render_mode: RenderMode,
        select: bool,
    ) {
        if render_mode == RenderMode::Source || !self.buffer.selection().is_empty() {
            return;
        }

        let current = self.buffer.cursor_pos();
        let layout = self.layout_snapshot();
        let projection = self.projection_for_render_mode(&layout.parsed_lines, render_mode);
        if let Some((start, end)) = projection.math_range(current.source_line())
            && end.index() == start.index() + 1
        {
            self.buffer.ensure_math_block_content_line(start.index());
            return;
        }
        if let Some(target) = self.display_map.normalize_position(&projection, current)
            && target != current
        {
            self.buffer.update_cursor(select, target);
        }
    }

    pub(crate) fn current_projection(
        &self,
        parsed_lines: &[crate::markdown::ParsedLine],
    ) -> EditorProjection {
        self.projection_for_render_mode(parsed_lines, self.render_mode)
    }

    pub(crate) fn projection_for_render_mode(
        &self,
        parsed_lines: &[crate::markdown::ParsedLine],
        render_mode: RenderMode,
    ) -> EditorProjection {
        let caret_map = self.buffer.line_index().caret_map();
        let mode = if render_mode == RenderMode::Source {
            ProjectionMode::Source
        } else {
            let line = self.disclosure_position().line;
            let source_line = SourceLine::new(line);
            let active_math = caret_map
                .math_pair(source_line)
                .filter(|(start, end)| source_line > *start && source_line < *end);
            ProjectionMode::live_preview(active_math)
        };
        EditorProjection::from_parsed_with_caret_map(
            self.buffer.lines(),
            parsed_lines,
            caret_map,
            mode,
        )
    }

    #[inline]
    pub(crate) fn is_active_disclosure_line(&self, line_idx: usize, window: &Window) -> bool {
        self.focus_handle.is_focused(window) && line_idx == self.disclosure_position().line
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
        let indent_px = self.parsed_line_indent_px(parsed);
        let available_width = crate::editor::shaping::compute_available_line_width_with_indent(
            content_width,
            &parsed.kind,
            self.render_mode,
            indent_px,
        );
        let wrap_width = if self.soft_wrap {
            Some(available_width)
        } else {
            None
        };

        let scx = self
            .line_shaping_context(
                override_color,
                is_active_caret,
                self.disclosure_col(),
                theme,
            )
            .with_selection_disclosure(self.is_selection_disclosure());
        (font_size, line_height, wrap_width, scx)
    }

    /// Computes the horizontal pixel X position of a buffer column on its line.
    pub fn get_pixel_x_for_col(&self, pos: Position, window: &mut Window) -> Pixels {
        if pos.line >= self.buffer.line_count() {
            return px(0.0);
        }
        let layout = self.layout_snapshot();
        if let Some(parsed) = layout.parsed_lines.get(pos.line) {
            let is_active_caret = self.is_active_disclosure_line(pos.line, window);
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
            let left_offset = crate::editor::shaping::get_block_left_offset_with_indent(
                &parsed.kind,
                self.render_mode,
                layout.indent_px(pos.line),
            );
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
            let is_active_caret = self.is_active_disclosure_line(pos.line, window);
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
    let line_idx = source_line.index();
        if line_idx >= self.buffer.line_count() {
            return 0;
        }
        let layout = self.layout_snapshot();
        if let Some(parsed) = layout.parsed_lines.get(line_idx) {
            let is_active_caret = self.is_active_disclosure_line(line_idx, window);
            let (font_size, line_height, wrap_width, scx) =
                self.line_shaping_metrics(parsed, is_active_caret, &crate::theme::DEFAULT_THEME);
            let target_left_offset = crate::editor::shaping::get_block_left_offset_with_indent(
                &parsed.kind,
                self.render_mode,
                layout.indent_px(line_idx),
            );
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
            
            Some(x) => x,
            None => {
                let current_x = self.get_pixel_x_for_col(pos, window);
                self.pixel_column_goal = Some(current_x);
                current_x
            }
        };

        let (current_v_idx, total_v_lines) = self.get_visual_line_info(pos, window);
            
            }

            let target_line = target_line.index();
                let target_line_str = self.buffer.line(target_line).unwrap_or("");
                let prefix = get_block_prefix_len(target_line_str);
                let col = if target_line_str.trim_start().starts_with("```") {
                    self.buffer.line_len(target_line)
                } else {
                    let target_col = self.get_col_for_pixel_coords(
                        SourceLine::new(target_line),
                        goal_x,
                        0,
                        window,
                    );
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
            
            }

            let target_line = target_line.index();
                let target_line_str = self.buffer.line(target_line).unwrap_or("");
                let prefix = get_block_prefix_len(target_line_str);
                let col = if target_line_str.trim_start().starts_with("```") {
                    self.buffer.line_len(target_line)
                    
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
