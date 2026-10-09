use gpui::*;

use crate::buffer::Position;
use crate::editor::EditorEvent;
use crate::editor::element::EditorElement;

use super::{Editor, RenderMode};

impl Editor {
    /// Resolves target line and buffer column, and if clicking in empty bottom area after a trailing code block, appends a new paragraph
    pub fn find_pos_for_mouse_point_mut(
        &mut self,
        point: Point<Pixels>,
        window: &mut Window,
    ) -> Position {
        if self.buffer.line_count() == 0 {
            return Position::new(0, 0);
        }

        let total_h = self.layout_snapshot().total_height;
        let viewport = self.viewport_bounds_val();
        let scroll_top = self.scroll.current_scroll_top;
        let doc_y = point.y - viewport.origin.y + scroll_top;

        if self.can_edit()
            && doc_y > total_h - px(16.0)
            && let Some(pos) = self.buffer.ensure_trailing_code_fence_newline()
        {
            return pos;
        }

        self.find_pos_for_mouse_point(point, window)
    }

    /// Computes deterministic on-screen bounds for a document line based on layout cache and viewport
    #[inline]
    pub(crate) fn line_screen_bounds(
        &self,
        line_idx: usize,
        viewport: Bounds<Pixels>,
    ) -> Bounds<Pixels> {
        let layout = self.layout_snapshot();
        let content_width =
            crate::editor::shaping::compute_content_width(viewport.size.width, self.render_mode);
        let mut container_x =
            viewport.origin.x + (viewport.size.width - content_width).max(px(0.0)) / 2.0;
        if self.render_mode == RenderMode::Source {
            container_x += crate::editor::layout::SOURCE_GUTTER_WIDTH;
        } else if let Some(parsed) = layout.parsed_lines.get(line_idx) {
            container_x +=
                crate::editor::shaping::get_block_left_offset(&parsed.kind, self.render_mode);
        }
        let line_top = viewport.origin.y
            + layout
                .line_y_offsets
                .get(line_idx)
                .copied()
                .unwrap_or(px(24.0))
            - self.scroll.current_scroll_top;
        let line_h = layout
            .line_heights
            .get(line_idx)
            .copied()
            .unwrap_or(px(24.0));
        Bounds {
            origin: gpui::point(container_x, line_top),
            size: size(content_width, line_h),
        }
    }

    /// Helper to find target line and relative coordinates for a mouse point
    pub(crate) fn find_line_at_point(
        &self,
        point: Point<Pixels>,
        exact_only: bool,
    ) -> Option<(usize, Point<Pixels>, Bounds<Pixels>)> {
        if self.buffer.line_count() == 0 {
            return None;
        }

        let layout = self.layout_snapshot();
        if layout.line_y_offsets.is_empty() {
            return None;
        }

        let viewport = self.viewport_bounds_val();
        let scroll_top = self.scroll.current_scroll_top;

        let doc_y = (point.y - viewport.origin.y + scroll_top).max(px(0.0));
        let idx = layout.line_y_offsets.partition_point(|&y| y <= doc_y);
        let mut line_idx = idx
            .saturating_sub(1)
            .min(self.buffer.line_count().saturating_sub(1));

        if self.is_line_folded(line_idx)
            && let Some(header) = self
                .display_map
                .fold_map()
                .folded_regions()
                .iter()
                .find(|r| line_idx > r.start_line && line_idx <= r.end_line)
                .map(|r| r.start_line)
        {
            line_idx = header;
        }

        if exact_only {
            let top = layout.line_y_offsets[line_idx];
            let bottom = top + layout.line_heights[line_idx];
            if doc_y < top || doc_y > bottom {
                return None;
            }
        }

        if !exact_only && self.render_mode != RenderMode::Source {
            let vdoc = crate::markdown::VisualDocument::from_parsed(
                self.buffer.lines(),
                &layout.parsed_lines,
            );
            let vpos = vdoc.source_to_visual(Position::new(line_idx, 0));
            let spos = vdoc.visual_to_source(vpos);
            line_idx = spos.line;
        }

        let line_bounds = self.line_screen_bounds(line_idx, viewport);

        let rel_point = gpui::point(
            (point.x - line_bounds.origin.x).max(px(0.0)),
            (point.y - line_bounds.origin.y).max(px(0.0)),
        );

        Some((line_idx, rel_point, line_bounds))
    }

    /// Resolves target line and buffer column from a mouse point.
    pub fn find_pos_for_mouse_point(&self, point: Point<Pixels>, window: &mut Window) -> Position {
        let (target_line, rel_point, line_bounds) = match self.find_line_at_point(point, false) {
            Some(res) => res,
            None => return Position::new(0, 0),
        };

        let layout = self.layout_snapshot();
        if let Some(parsed) = layout.parsed_lines.get(target_line) {
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
                    table,
                    &crate::theme::DEFAULT_THEME,
                    window,
                );
                return Position::new(target_line, col);
            }

            let is_active_caret = self.is_active_caret_line(target_line, window);
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

    pub fn stop_drag(&mut self, cx: &mut Context<Self>) {
        if self.is_dragging || self.scroll.is_scrollbar_dragging {
            self.is_dragging = false;
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

    /// Returns `(max_scroll, thumb_y, thumb_h)` for the vertical scrollbar over `track_h`.
    pub(crate) fn scrollbar_thumb_metrics(
        &self,
        track_h: Pixels,
        total_content_h: Pixels,
    ) -> (Pixels, Pixels, Pixels) {
        let scroll_h = self.viewport_bounds_val().size.height.max(px(200.0));
        let max_scroll = (total_content_h - scroll_h).max(px(0.0));
        let overscroll_h = (track_h - px(160.0)).max(px(120.0));
        let thumb_h = if total_content_h > px(0.0) {
            ((track_h / (total_content_h + overscroll_h)) * track_h).clamp(px(28.0), track_h)
        } else {
            track_h
        };
        let scroll_ratio = if max_scroll > px(0.0) {
            (self.scroll.current_scroll_top / max_scroll).clamp(0.0, 1.0)
        } else {
            0.0
        };
        let thumb_y = scroll_ratio * (track_h - thumb_h);
        (max_scroll, thumb_y, thumb_h)
    }

    pub fn handle_drag_move(
        &mut self,
        mouse_pos: Point<Pixels>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.scroll.is_scrollbar_dragging {
            let track_h = self.viewport_bounds_val().size.height.max(px(200.0));
            let (max_scroll, _, thumb_h) =
                self.scrollbar_thumb_metrics(track_h, self.get_total_content_height());

            if let (Some(start_y), Some(start_scroll)) = (
                self.scroll.scrollbar_drag_start_y,
                self.scroll.scrollbar_drag_start_scroll,
            ) {
                let dy = mouse_pos.y - start_y;
                let available_track = track_h - thumb_h;
                if available_track > px(0.0) {
                    let scroll_delta = (dy / available_track) * max_scroll;
                    self.scroll_task = None;
                    self.scroll
                        .set_direct(start_scroll + scroll_delta, max_scroll);
                    self.scroll.last_scroll_action = std::time::Instant::now();
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
            self.buffer.drag_selection_to(pos);
            self.handle_drag_scroll(window);
            self.check_start_drag_scroll(cx);
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
        if self.apply_drag_edge_scroll(mouse_pos.y) {
            let pos = self.find_pos_for_mouse_point(mouse_pos, window);
            self.buffer.drag_selection_to(pos);
        }
    }

    /// Starts a background task to auto-scroll when dragging near or outside viewport edges.
    pub(crate) fn check_start_drag_scroll(&mut self, cx: &mut Context<Self>) {
        if !self.is_dragging || self.drag_scroll_task.is_some() {
            return;
        }
        let Some(mouse_pos) = self.last_mouse_pos else {
            return;
        };
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

        let scrolled = self.apply_drag_edge_scroll(mouse_pos.y);
        if scrolled && let Some((target_line, _, _)) = self.find_line_at_point(mouse_pos, false) {
            let col = self
                .buffer
                .cursor_pos()
                .col
                .min(self.buffer.line_len(target_line));
            self.buffer
                .drag_selection_to(Position::new(target_line, col));
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
        let parsed = layout.parsed_lines.get(line_idx)?;

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

        let is_active_caret = self.is_active_caret_line(line_idx, window)
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

    pub(crate) fn try_handle_scrollbar_click(
        &mut self,
        pos: Point<Pixels>,
        cx: &mut Context<Self>,
    ) -> bool {
        let vp = self.viewport_bounds_val();
        if pos.x < vp.right() - px(14.0) {
            return false;
        }
        let track_h = vp.size.height;
        let (max_scroll, thumb_y, thumb_h) =
            self.scrollbar_thumb_metrics(track_h, self.get_total_content_height());
        if max_scroll <= px(2.0) {
            return false;
        }

        let rel_y = pos.y - vp.origin.y;
        if rel_y >= thumb_y && rel_y <= thumb_y + thumb_h {
            self.scroll.is_scrollbar_dragging = true;
            self.scroll.scrollbar_drag_start_y = Some(pos.y);
            self.scroll.scrollbar_drag_start_scroll = Some(self.scroll.target_scroll_top);
        } else {
            let ratio = (rel_y / track_h).clamp(0.0, 1.0);
            let target = ratio * max_scroll;
            self.scroll_task = None;
            self.scroll.set_direct(target, max_scroll);
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
