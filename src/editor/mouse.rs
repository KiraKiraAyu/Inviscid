
use super::{Editor, RenderMode};

impl Editor {
    /// Resolves target line and buffer column, and if clicking in empty bottom area after a trailing code block, appends a new paragraph
    return Some(Position::new(0, 0));
        }

        let total_h = self.layout_snapshot().total_height;
        let viewport = self.viewport_bounds_val();
        let scroll_top = self.scroll.current_scroll_top;
        let doc_y = point.y - viewport.origin.y + scroll_top;

        self.resolve_mouse_position(point, window)
    }

    /// Computes deterministic on-screen bounds for a document line based on layout cache and viewport
    #[inline]
    let line_idx = source_line.index();
        let layout = self.layout_snapshot();
        let content_width =
            crate::editor::shaping::compute_content_width(viewport.size.width, self.render_mode);
        let container_x =
            viewport.origin.x + (viewport.size.width - content_width).max(px(0.0)) / 2.0;
        let left_offset = if self.render_mode == RenderMode::Source {
            crate::editor::layout::SOURCE_GUTTER_WIDTH
        } else if let Some(parsed) = layout.parsed_lines.get(line_idx) {
            crate::editor::shaping::get_block_left_offset_with_indent(
                &parsed.kind,
                self.render_mode,
                layout.indent_px(line_idx),
                
        }
    }

    /// Helper to find target line and relative coordinates for a mouse point
    let layout_line = layout.layout_line_at_y(doc_y)?;
        let mut source_line = layout_line.source_line();

        if exact_only {
            let geometry = layout_line.geometry();
            let (top, bottom) = (geometry.top(), geometry.bottom());
            if doc_y < top || doc_y > bottom {
                return None;
            }
        }
        let line_bounds = self.line_screen_bounds(source_line, viewport);

        let rel_point = gpui::point(
            (point.x - line_bounds.origin.x).max(px(0.0)),
            (point.y - line_bounds.origin.y).max(px(0.0)),
            fn resolve_mouse_position(
        &mut self,
        point: Point<Pixels>,
        window: &mut Window,
    ) -> Option<Position> {
        match self.resolve_mouse_target(point, window)? {
            CaretTarget::Position(position) => Some(position.to_position()),
            CaretTarget::CreateMathContent { start_line } => self
                .buffer
                .ensure_math_block_content_line(start_line.index()),
        }
    }

    pub(crate) fn utf16_offset_for_mouse_target(&self, target: CaretTarget) -> usize {
        match target {
            CaretTarget::Position(position) => {
                self.buffer.pos_to_utf16_offset(position.to_position())
            }
            CaretTarget::CreateMathContent { start_line } => {
                let layout = self.layout_snapshot();
                let end_line = self
                    .current_projection(&layout.parsed_lines)
                    .math_range(start_line)
                    .map(|(_, end)| end.index())
                    .unwrap_or(start_line.index() + 1);
                self.buffer.pos_to_utf16_offset(Position::new(end_line, 0))
            }
        }
    }

    pub(crate) fn resolve_mouse_target(
        &self,
        point: Point<Pixels>,
        window: &mut Window,
    ) -> Option<CaretTarget> {
        let (target_line, rel_point, line_bounds) = match self.find_line_at_point(point, false) {
            Some(res) => res,
            None => return None,
        };
        let target_line_index = target_line.index();

        let layout = self.layout_snapshot();
        if self.render_mode != RenderMode::Source {
            let projection = self.current_projection(&layout.parsed_lines);
            let target = self
                .display_map
                .resolve_hit_target(&projection, target_line)?;
            match target {
                CaretTarget::CreateMathContent { start_line } => {
                    return Some(CaretTarget::CreateMathContent { start_line });
                }
                CaretTarget::Position(position) if position.line.index() != target_line_index => {
                    return Some(CaretTarget::Position(SourcePosition::new(position.line, 0)));
                }
                CaretTarget::Position(_) => {}
            }
        }
        if let Some(parsed) = layout.parsed_lines.get(target_line_index) {
            let is_source = self.render_mode == RenderMode::Source;
            if let crate::markdown::BlockKind::Table {
                cells,
                is_delimiter,
                alignments,
                is_header,
            } = &parsed.kind
                && !is_source
                && !*is_delimiter
                && !cells.is_empty()
            {
                let table = super::shaping::TableLayoutInfo {
                    row_w: line_bounds.size.width,
                    cells,
                    alignments,
                    is_header: *is_header,
                };
                let col = super::shaping::calculate_table_cell_col_from_point(
                    rel_point,
                    let is_active_caret = self.is_active_disclosure_line(target_line_index, window);
            let (font_size, line_height, wrap_width, scx) =
                self.line_shaping_metrics(parsed, is_active_caret, &crate::theme::DEFAULT_THEME);

            let top_offset = super::shaping::get_block_top_offset(&parsed.kind, self.render_mode);
            let in_canvas_point = gpui::point(rel_point.x, (rel_point.y - top_offset).max(px(0.0)));

            let mut col = super::shaping::calculate_col_from_point(
                in_canvas_point,
                parsed,
                font_size,
                line_height,
                wrap_width,
                scx,
                window,
            );
            if !is_source {
                let prefix_len = parsed.syntax_prefix_len();
                Some(CaretTarget::Position(SourcePosition::new(target_line, 0)))
        }
    }

    fn code_block_content_bounds(&self, line_idx: usize) -> Option<(Position, Position)> {
        let layout = self.layout_snapshot();
        if !matches!(
            &layout.parsed_lines.get(line_idx)?.kind,
            crate::markdown::BlockKind::CodeBlockContent
        ) {
            return None;
        }

        let mut start_line = line_idx;
        while start_line > 0
            && matches!(
                &layout.parsed_lines[start_line - 1].kind,
                crate::markdown::BlockKind::CodeBlockContent
            )
        {
            start_line -= 1;
        }

        let mut end_line = line_idx;
        while end_line + 1 < layout.parsed_lines.len()
            && matches!(
                &layout.parsed_lines[end_line + 1].kind,
                crate::markdown::BlockKind::CodeBlockContent
            )
        {
            end_line += 1;
        }

        Some((
            Position::new(start_line, 0),
            Position::new(end_line, self.buffer.line_len(end_line)),
        ))
    }

    
        ))
    }

    fn clamp_drag_position(&self, pos: Position) -> Position {
        let Some((start, end)) = self.drag_code_block_bounds else {
            return pos;
        };
        pos.max(start).min(end)
    }

    fn is_within_drag_code_block(&self, line_idx: usize) -> bool {
        let Some((start, end)) = self.drag_code_block_bounds else {
            return true;
        };
        (start.line..=end.line).contains(&line_idx)
    }

    
        if self.is_dragging || self.scroll.is_scrollbar_dragging {
            self.is_dragging = false;
            self.drag_code_block_bounds = None;
            self.drag_scroll_task = None;
            self.scroll.is_scrollbar_dragging = false;
            self.scroll.scrollbar_drag_start_y = None;
            self.scroll.scrollbar_drag_start_scroll = None;
            self.last_mouse_pos = None;
            self.last_cursor_action = std::time::Instant::now();
            self.reset_cursor_blink(cx);
            cx.notify();
        }
    }

    pub fn handle_drag_move(
        &mut self,
        mouse_pos: Point<Pixels>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.scroll.is_scrollbar_dragging {
            let track_h = self.viewport_height();
            let metrics = self.scrollbar_metrics(track_h, self.get_total_content_height());

            if let (Some(start_y), Some(start_scroll)) = (
                self.scroll.scrollbar_drag_start_y,
                self.scroll.scrollbar_drag_start_scroll,
            ) {
                let dy = mouse_pos.y - start_y;
                let available_track = track_h - metrics.thumb_h;
                if available_track > px(0.0) {
                    let scroll_delta = (dy / available_track) * metrics.max_scroll;
                    self.scroll_task = None;
                    self.scroll
                        .set_direct(start_scroll + scroll_delta, metrics.max_scroll);
                    self.scroll.last_scroll_action = std::time::Instant::now();
                    cx.notify();
                }
            }
            return;
        }

        
            if within_code_block {
                self.handle_drag_scroll(window);
                self.check_start_drag_scroll(cx);
            } else {
                self.drag_scroll_task = None;
            }
            if self.buffer.cursor_pos() != prev_head {
                cx.emit(crate::editor::EditorEvent::CursorMoved);
            }
            cx.notify();
        }
    }

    fn apply_drag_edge_scroll(&mut self, mouse_y: Pixels) -> bool {
        let viewport = self.viewport_bounds_val();
        let top_edge = viewport.top() + px(40.0);
        let bottom_edge = viewport.bottom() - px(40.0);

        let new_top = if mouse_y < top_edge {
            let dist = f32::from(top_edge - mouse_y).max(0.0);
            let speed = px((dist / 40.0 * 20.0 + 8.0).min(50.0));
            (self.scroll.target_scroll_top - speed).max(px(0.0))
        } else if mouse_y > bottom_edge {
            let max_scroll = self.get_max_scroll_top();
            let dist = f32::from(mouse_y - bottom_edge).max(0.0);
            let speed = px((dist / 40.0 * 20.0 + 8.0).min(50.0));
            (self.scroll.target_scroll_top + speed).min(max_scroll)
        } else {
            return false;
        };

        if (new_top - self.scroll.target_scroll_top).abs() > px(0.1) {
            self.scroll.target_scroll_top = new_top;
            self.scroll.current_scroll_top = new_top;
            self.scroll.last_scroll_action = std::time::Instant::now();
            true
        } else {
            false
        }
    }

    /// Scrolls the viewport and expands selection when dragging near or past viewport boundaries.
    pub fn handle_drag_scroll(&mut self, window: &mut Window) {
        if !self.is_dragging {
            return;
        }
        let Some(mouse_pos) = self.last_mouse_pos else {
            return;
        };
        if !self.drag_mouse_is_within_code_block(mouse_pos) {
            

    /// Starts a background task to auto-scroll when dragging near or outside viewport edges.
    pub(crate) fn check_start_drag_scroll(&mut self, cx: &mut Context<Self>) {
        if !self.is_dragging {
            return;
        }
        let Some(mouse_pos) = self.last_mouse_pos else {
            return;
        };
        if !self.drag_mouse_is_within_code_block(mouse_pos) {
            self.drag_scroll_task = None;
            return;
        }
        if self.drag_scroll_task.is_some() {
            return;
        }
        let viewport = self.viewport_bounds_val();
        let top_edge = viewport.top() + px(40.0);
        let bottom_edge = viewport.bottom() - px(40.0);
        if mouse_pos.y < top_edge || mouse_pos.y > bottom_edge {
            let executor = cx.background_executor().clone();
            self.drag_scroll_task = Some(cx.spawn(async move |this, cx| {
                loop {
                    executor.timer(std::time::Duration::from_millis(16)).await;
                    let Ok(still_dragging) = this.update(cx, |editor, cx| {
                        if !editor.is_dragging {
                            return false;
                        }
                        let prev_head = editor.buffer.cursor_pos();
                        let scrolled = editor.step_drag_scroll();
                        if scrolled {
                            if editor.buffer.cursor_pos() != prev_head {
                                cx.emit(crate::editor::EditorEvent::CursorMoved);
                            }
                            cx.notify();
                            true
                        } else {
                            false
                        }
                    }) else {
                        break;
                    };
                    if !still_dragging {
                        break;
                    }
                }
                let _ = this.update(cx, |editor, _cx| {
                    editor.drag_scroll_task = None;
                });
            }));
        }
    }

    /// Advances edge auto-scrolling by one step during mouse drag.
    pub(crate) fn step_drag_scroll(&mut self) -> bool {
        if !self.is_dragging {
            return false;
        }
        let Some(mouse_pos) = self.last_mouse_pos else {
            return false;
        };
        if !self.drag_mouse_is_within_code_block(mouse_pos) {
            return false;
                .drag_selection_to(self.clamp_drag_position(position));
        }

        scrolled
    }

    /// Resolves whether a mouse point is over a hyperlink span.
    pub fn get_link_at_point(
        &self,
        point: Point<Pixels>,
        

        if let crate::markdown::BlockKind::Table {
            cells,
            alignments,
            is_header,
            is_delimiter,
        } = &parsed.kind
            && !is_source
            && !*is_delimiter
            && !cells.is_empty()
        {
            let table = super::shaping::TableLayoutInfo {
                row_w: line_bounds.size.width,
                cells,
                alignments,
                is_header: *is_header,
            };
            return super::shaping::find_link_in_table_row(
                line_idx,
                rel_point,
                table,
                &crate::theme::DEFAULT_THEME,
                window,
            );
        }

        let is_active_caret = self.is_active_disclosure_line(line_idx, window)
            && self.buffer.selection().is_empty()
            && !self.is_dragging;

        let (font_size, line_height, wrap_width, scx) =
            self.line_shaping_metrics(parsed, is_active_caret, &crate::theme::DEFAULT_THEME);

        let lcx = super::shaping::SpanLayoutContext::new(
            font_size,
            line_height,
            wrap_width,
            px(0.0),
            parsed.raw_text.chars().count(),
            scx,
        );

        let top_offset = super::shaping::get_block_top_offset(&parsed.kind, self.render_mode);
        let in_canvas_point = gpui::point(rel_point.x, (rel_point.y - top_offset).max(px(0.0)));

        super::shaping::find_link_at_pixel_point(line_idx, in_canvas_point, parsed, lcx, window)
    }

    pub fn open_url(&self, url: &str) {
        let clean_url = url.trim();
        if clean_url.is_empty() {
            return;
        }
        let final_url = if clean_url.starts_with("www.") {
            format!("https://{}", clean_url)
        } else {
            clean_url.to_string()
        };

        crate::platform::open_url_in_browser(&final_url);
    }

    pub(crate) fn register_mouse_listeners(
        element: Stateful<EditorElement>,
        cx: &Context<Editor>,
    ) -> Stateful<EditorElement> {
        element
            .on_scroll_wheel(cx.listener(|this, event: &ScrollWheelEvent, _window, cx| {
                this.handle_scroll_wheel(event, cx);
            }))
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, event: &MouseDownEvent, window, cx| {
                    this.handle_mouse_down(event, window, cx);
                }),
            )
            .on_mouse_move(
                cx.listener(move |this, event: &MouseMoveEvent, window, cx| {
                    this.handle_mouse_move(event, window, cx);
                }),
            )
            .on_mouse_up(
                MouseButton::Left,
                cx.listener(|this, _, _, cx| {
                    this.handle_mouse_up(cx);
                }),
            )
    }

    pub fn handle_scroll_wheel(&mut self, event: &ScrollWheelEvent, cx: &mut Context<Self>) {
        let max_scroll = self.get_max_scroll_top();
        match event.delta {
            ScrollDelta::Pixels(p) => {
                if p.y.abs() > px(0.01) {
                    self.scroll_task = None;
                    self.scroll.scroll_direct(p.y, max_scroll);
                    cx.notify();
                }
            }
            ScrollDelta::Lines(l) => {
                let delta_y = px(l.y * crate::ui::WHEEL_LINE_STEP_PX);
                if delta_y.abs() > px(0.01) {
                    self.scroll.scroll_by_wheel(delta_y, max_scroll);
                    self.start_scroll_animation(cx);
                    cx.notify();
                }
            }
        }
    }

    const SCROLLBAR_TRACK_HIT_WIDTH: Pixels = px(14.0);

    pub(crate) fn try_handle_scrollbar_click(
        &mut self,
        pos: Point<Pixels>,
        cx: &mut Context<Self>,
    ) -> bool {
        let vp = self.viewport_bounds_val();
        if pos.x < vp.right() - Self::SCROLLBAR_TRACK_HIT_WIDTH {
            return false;
        }
        let track_h = self.viewport_height();
        let metrics = self.scrollbar_metrics(track_h, self.get_total_content_height());
        if !metrics.has_overflow {
            return false;
        }

        let rel_y = pos.y - vp.origin.y;
        if rel_y >= metrics.thumb_y && rel_y <= metrics.thumb_y + metrics.thumb_h {
            self.scroll.is_scrollbar_dragging = true;
            self.scroll.scrollbar_drag_start_y = Some(pos.y);
            self.scroll.scrollbar_drag_start_scroll = Some(self.scroll.target_scroll_top);
        } else {
            let ratio = (rel_y / track_h).clamp(0.0, 1.0);
            let target = ratio * metrics.max_scroll;
            self.scroll_task = None;
            self.scroll.set_direct(target, metrics.max_scroll);
            self.scroll.last_scroll_action = std::time::Instant::now();
        }
        cx.notify();
        true
    }

    pub(crate) fn handle_mouse_down(
        &mut self,
        event: &MouseDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        window.focus(&self.focus_handle);
        self.reset_cursor_blink(cx);
        self.drag_code_block_bounds = None;

        // Check if clicking on the right scrollbar track/thumb
        if self.try_handle_scrollbar_click(event.position, cx) {
            return;
        }

        // Ctrl+Click (or Cmd+Click) on a hyperlink opens the URL in the system browser
        if (event.modifiers.control || event.modifiers.platform)
            && let Some(link) = self.get_link_at_point(event.position, window)
        {
            self.open_url(&link.url);
            self.stop_drag(cx);
            return;
        }

        // Text selection
        let prev_head = self.buffer.cursor_pos();
        let Some(pos) = self.resolve_mouse_position(event.position, window) else {
                return;
            };
            self.buffer.select_line_at(pos.line);
        } else {
            let Some(pos) = self.find_pos_for_mouse_point_mut(event.position, window) else {
                return;
            };
            self.buffer.set_cursor(pos);
        }
        self.drag_code_block_bounds = self
            .code_block_content_bounds(self.buffer.selection().anchor.line)
            .or_else(|| self.math_block_content_bounds(self.buffer.selection().anchor.line));
        if self.buffer.cursor_pos() != prev_head {
            cx.emit(EditorEvent::CursorMoved);
        }
        self.scroll_to_cursor();
        self.start_scroll_animation_if_needed(cx);
        cx.notify();
    }

    pub(crate) fn handle_mouse_move(
        &mut self,
        event: &MouseMoveEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        // When dragging, all mouse tracking is handled globally by Window Capture listener.
        if self.is_dragging || self.scroll.is_scrollbar_dragging {
            return;
        }
        // Track link hover for PointingHand cursor and hover highlight
        let mouse_pos = event.position;
        let new_hovered = self.get_link_at_point(mouse_pos, window);
        if self.hovered_link != new_hovered {
            self.hovered_link = new_hovered;
            cx.notify();
        }
    }

    pub(crate) fn handle_mouse_up(&mut self, cx: &mut Context<Self>) {
        // Fallback cleanup if release was not captured
        if self.is_dragging || self.scroll.is_scrollbar_dragging {
            self.stop_drag(cx);
        }
    }
}
