use gpui::*;

use crate::buffer::{Position, SourceLine};
use crate::editor::EditorEvent;
use crate::editor::element::EditorElement;

use super::{Editor, RenderMode};

impl Editor {
    fn math_block_range(&self, line: SourceLine) -> Option<(SourceLine, SourceLine)> {
        let block = crate::markdown::block_at_buffer(&self.buffer, line)?;
        matches!(block.kind, crate::markdown::ProjectedBlockKind::MathBlock)
            .then(|| (block.source_range.start, block.source_range.end - 1))
    }

    fn visual_anchor_for_hidden_line(
        &self,
        layout: &crate::editor::layout::DocumentLayoutCache,
        line: SourceLine,
    ) -> Option<SourceLine> {
        let idx = line.index();
        let previous = (0..idx).rev().find(|&i| {
            layout
                .metrics
                .height(i)
                .is_some_and(|height| height > px(0.0))
        });
        let next = layout.metrics.next_visible_line_after(idx);
        let anchor = SourceLine::new(previous.or(next)?);
        let block = crate::markdown::block_at_buffer(&self.buffer, anchor)?;
        match block.kind {
            crate::markdown::ProjectedBlockKind::CodeBlock { .. }
            | crate::markdown::ProjectedBlockKind::Table { .. } => Some(block.source_range.start),
            _ => Some(anchor),
        }
    }

    /// Resolves target line and buffer column, and if clicking in empty bottom area after a trailing code block, appends a new paragraph
    pub fn find_pos_for_mouse_point_mut(
        &mut self,
        point: Point<Pixels>,
        window: &mut Window,
    ) -> Position {
        if self.buffer.line_count() == 0 {
            return Position::new(crate::buffer::SourceLine::new(0), 0);
        }

        let total_h = self.layout_snapshot().metrics.total_height();
        let viewport = self.viewport_bounds_val();
        let scroll_top = self.scroll.current_scroll_top;
        let doc_y = point.y - viewport.origin.y + scroll_top + px(self.scroll_origin_y() as f32);

        if self.can_edit()
            && doc_y > total_h - px(16.0)
            && let Some(pos) = self.buffer.ensure_trailing_code_fence_newline()
        {
            return pos;
        }

        let pos = self.find_pos_for_mouse_point(point, window);
        if self.render_mode != RenderMode::Source
            && let Some(start_line) = pos.line.checked_sub(1)
            && self
                .layout_snapshot()
                .parsed_lines
                .row(start_line.index())
                .is_some_and(|line| {
                    matches!(
                        line.kind,
                        crate::markdown::BlockKind::MathBlock { is_start: true }
                    )
                })
            && let Some(pos) = self.buffer.ensure_math_block_content_line(start_line)
        {
            return pos;
        }

        pos
    }

    /// Computes deterministic on-screen bounds for a document line based on layout cache and viewport
    #[inline]
    pub(crate) fn line_screen_bounds(
        &self,
        source_line: SourceLine,
        viewport: Bounds<Pixels>,
    ) -> Bounds<Pixels> {
        let layout = self.layout_snapshot();
        let content_width =
            crate::editor::shaping::compute_content_width(viewport.size.width, self.render_mode);
        let container_x =
            viewport.origin.x + (viewport.size.width - content_width).max(px(0.0)) / 2.0;
        let left_offset = if self.render_mode == RenderMode::Source {
            crate::editor::layout::SOURCE_GUTTER_WIDTH
        } else if let Some(row) = layout.parsed_lines.row(source_line.index()) {
            let parsed = &*row;
            crate::editor::shaping::get_parsed_line_left_offset(
                parsed,
                self.render_mode,
                layout.indent_px(source_line),
                crate::editor::shaping::quote_content_indent_px(parsed, layout.space_width),
            )
        } else {
            px(0.0)
        };
        let line_doc_top = layout
            .metrics
            .top_relative(source_line.index(), self.scroll.anchor_line)
            .unwrap_or(px(24.0));
        let line_top = viewport.origin.y + line_doc_top - self.scroll.current_scroll_top;
        let line_h = layout
            .metrics
            .height(source_line.index())
            .unwrap_or(px(24.0));
        Bounds {
            origin: gpui::point(container_x + left_offset, line_top),
            size: size((content_width - left_offset).max(px(0.0)), line_h),
        }
    }

    /// Helper to find target line and relative coordinates for a mouse point
    pub(crate) fn find_line_at_point(
        &self,
        point: Point<Pixels>,
        exact_only: bool,
    ) -> Option<(SourceLine, Point<Pixels>, Bounds<Pixels>)> {
        if self.buffer.line_count() == 0 {
            return None;
        }

        let layout = self.layout_snapshot();
        let viewport = self.viewport_bounds_val();
        let scroll_top = self.scroll.current_scroll_top;

        let doc_y = point.y - viewport.origin.y + scroll_top;
        let mut source_line = SourceLine::new(
            layout
                .metrics
                .line_at_relative_y(self.scroll.anchor_line, doc_y)?,
        );
        source_line = source_line.min(SourceLine::new(self.buffer.line_count() - 1));

        if self.is_line_folded(source_line)
            && let Some(header) = self
                .display_map
                .fold_map()
                .folded_regions()
                .iter()
                .find(|r| source_line > r.start_line && source_line <= r.end_line)
                .map(|r| r.start_line)
        {
            source_line = header;
        }

        if exact_only {
            let top = layout
                .metrics
                .top_relative(source_line.index(), self.scroll.anchor_line)?;
            let bottom = top + layout.metrics.height(source_line.index())?;
            if doc_y < top || doc_y > bottom {
                return None;
            }
        }

        if !exact_only && self.render_mode != RenderMode::Source {
            let disclosure_line = self.disclosure_position().line;
            let active_math_block = self.math_block_range(disclosure_line);
            if active_math_block
                .is_some_and(|(start, end)| source_line >= start && source_line <= end)
            {
                // Preserve source rows while the split formula editor is open.
            } else {
                match self.math_block_range(source_line) {
                    Some((start, _)) => source_line = start,
                    None if layout.separator(source_line.index())
                        || matches!(
                            layout.parsed_lines.row(source_line.index())?.kind,
                            crate::markdown::BlockKind::Table {
                                is_delimiter: true,
                                ..
                            }
                        ) =>
                    {
                        if let Some(anchor) =
                            self.visual_anchor_for_hidden_line(&layout, source_line)
                        {
                            source_line = anchor;
                        }
                    }
                    None => {}
                }
            }
        }

        let line_bounds = self.line_screen_bounds(source_line, viewport);

        let rel_point = gpui::point(
            (point.x - line_bounds.origin.x).max(px(0.0)),
            (point.y - line_bounds.origin.y).max(px(0.0)),
        );

        Some((source_line, rel_point, line_bounds))
    }

    /// Resolves target line and buffer column from a mouse point.
    pub fn find_pos_for_mouse_point(&self, point: Point<Pixels>, window: &mut Window) -> Position {
        let (target_line, rel_point, line_bounds) = match self.find_line_at_point(point, false) {
            Some(res) => res,
            None => return Position::new(crate::buffer::SourceLine::new(0), 0),
        };
        let target_line_idx = target_line.index();

        let layout = self.layout_snapshot();
        if self.render_mode == RenderMode::LivePreview
            && crate::markdown::block_at_buffer(&self.buffer, target_line)
                .is_some_and(|block| block.is_closing_fence(target_line))
        {
            return crate::markdown::normalize_buffer_live_cursor(
                &self.buffer,
                Position::new(target_line, 0),
            );
        }
        if let Some(row) = layout.parsed_lines.row(target_line_idx) {
            let parsed = &*row;
            let is_source = self.render_mode == RenderMode::Source;
            if !is_source
                && matches!(
                    parsed.kind,
                    crate::markdown::BlockKind::MathBlock { is_start: true }
                )
                && let Some((start, end)) = self.math_block_range(target_line)
                && start == target_line
                && end > start
            {
                return Position::new(start + 1, 0);
            }

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
                    table,
                    &crate::theme::DEFAULT_THEME,
                    window,
                );
                return Position::new(target_line, col);
            }

            let is_active_caret = self.is_active_disclosure_line(target_line, window);
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
                if col < prefix_len {
                    col = prefix_len.min(parsed.raw_text.chars().count());
                }
            }
            Position::new(target_line, col)
        } else {
            Position::new(target_line, 0)
        }
    }

    fn code_block_content_bounds(
        &self,
        source_line: crate::buffer::SourceLine,
    ) -> Option<(Position, Position)> {
        let block = crate::markdown::block_at_buffer(&self.buffer, source_line)?;
        if !matches!(
            block.kind,
            crate::markdown::ProjectedBlockKind::CodeBlock { .. }
        ) || !block.is_enclosed_container
        {
            return None;
        }
        let start_line = block.source_range.start + 1;
        let end_line = block.editable_lines.end.checked_sub(1)?;
        if source_line < start_line || source_line > end_line {
            return None;
        }
        Some((
            Position::new(start_line, 0),
            Position::new(end_line, self.buffer.line_len(end_line)),
        ))
    }

    fn math_block_content_bounds(
        &self,
        source_line: crate::buffer::SourceLine,
    ) -> Option<(Position, Position)> {
        if self.render_mode == RenderMode::Source {
            return None;
        }
        let (start, end) = self.math_block_range(source_line)?;
        (start < source_line && source_line < end).then_some((
            Position::new(start + 1, 0),
            Position::new(
                end - 1,
                self.buffer
                    .line_len(crate::buffer::SourceLine::new(end.index() - 1)),
            ),
        ))
    }

    fn clamp_drag_position(&self, pos: Position) -> Position {
        let Some((start, end)) = self.drag_code_block_bounds else {
            return pos;
        };
        pos.max(start).min(end)
    }

    fn is_within_drag_code_block(&self, source_line: crate::buffer::SourceLine) -> bool {
        let Some((start, end)) = self.drag_code_block_bounds else {
            return true;
        };
        source_line >= start.line && source_line <= end.line
    }

    fn drag_mouse_is_within_code_block(&self, mouse_pos: Point<Pixels>) -> bool {
        let Some((line_idx, _, _)) = self.find_line_at_point(mouse_pos, false) else {
            return false;
        };
        self.is_within_drag_code_block(line_idx)
    }

    pub fn stop_drag(&mut self, cx: &mut Context<Self>) {
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
                    self.set_scroll_document_y(
                        self.scroll_origin_y() + f64::from(f32::from(start_scroll + scroll_delta)),
                    );
                    self.scroll.last_scroll_action = std::time::Instant::now();
                    cx.emit(EditorEvent::ScrollChanged);
                    cx.notify();
                }
            }
            return;
        }

        if self.is_dragging {
            let prev_head = self.buffer.cursor_pos();
            self.last_mouse_pos = Some(mouse_pos);
            self.last_cursor_action = std::time::Instant::now();
            let pos = self.find_pos_for_mouse_point(mouse_pos, window);
            let within_code_block = self.is_within_drag_code_block(pos.line);
            let pos = self.clamp_drag_position(pos);
            self.buffer.drag_selection_to(pos);
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
            (self.scroll.target_scroll_top - speed).max(self.scroll.min_scroll_top)
        } else if mouse_y > bottom_edge {
            let max_scroll = self.get_max_scroll_top();
            let dist = f32::from(mouse_y - bottom_edge).max(0.0);
            let speed = px((dist / 40.0 * 20.0 + 8.0).min(50.0));
            (self.scroll.target_scroll_top + speed).min(max_scroll)
        } else {
            return false;
        };

        if (new_top - self.scroll.target_scroll_top).abs() > px(0.1) {
            let max = self.get_max_scroll_top();
            self.scroll.set_direct(new_top, max);
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
            self.drag_scroll_task = None;
            return;
        }
        if self.apply_drag_edge_scroll(mouse_pos.y) {
            let pos = self.clamp_drag_position(self.find_pos_for_mouse_point(mouse_pos, window));
            self.buffer.drag_selection_to(pos);
        }
    }

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
                            cx.emit(crate::editor::EditorEvent::ScrollChanged);
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
        }

        let scrolled = self.apply_drag_edge_scroll(mouse_pos.y);
        if scrolled && let Some((target_line, _, _)) = self.find_line_at_point(mouse_pos, false) {
            let col = self
                .buffer
                .cursor_pos()
                .col
                .min(self.buffer.line_len(target_line));
            self.buffer
                .drag_selection_to(self.clamp_drag_position(Position::new(target_line, col)));
        }

        scrolled
    }

    /// Resolves whether a mouse point is over a hyperlink span.
    pub fn get_link_at_point(
        &self,
        point: Point<Pixels>,
        window: &mut Window,
    ) -> Option<super::HoveredLink> {
        let (line_idx, rel_point, line_bounds) = self.find_line_at_point(point, true)?;
        let layout = self.layout_snapshot();
        let row = layout.parsed_lines.row(line_idx.index())?;
        let parsed = &*row;

        let is_source = self.render_mode == RenderMode::Source;

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
                    cx.emit(EditorEvent::ScrollChanged);
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
            self.set_scroll_document_y(f64::from(f32::from(target)));
            self.scroll.last_scroll_action = std::time::Instant::now();
            cx.emit(EditorEvent::ScrollChanged);
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
        self.is_dragging = true;
        self.last_mouse_pos = Some(event.position);
        self.pixel_column_goal = None;
        if event.click_count == 2 {
            let pos = self.find_pos_for_mouse_point(event.position, window);
            self.buffer.select_word_at(pos);
        } else if event.click_count >= 3 {
            let pos = self.find_pos_for_mouse_point(event.position, window);
            self.buffer.select_line_at(pos.line);
        } else {
            let pos = self.find_pos_for_mouse_point_mut(event.position, window);
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
