use gpui::*;
use std::ops::Range;

use super::{Editor, RenderMode};

impl Editor {
    pub(crate) fn is_selection_disclosure(&self) -> bool {
        self.marked_range.is_none()
            && self.render_mode != RenderMode::Source
            && !self.buffer.selection().is_empty()
    }

    /// Position used for markdown syntax-marker disclosure.
    pub(crate) fn disclosure_position(&self) -> crate::buffer::Position {
        match &self.marked_range {
            Some(marked) => self.buffer.utf16_offset_to_pos(marked.start).to_position(),
            None if self.is_selection_disclosure() => self.buffer.selection().anchor,
            None => self.buffer.cursor_pos(),
        }
    }

    pub(crate) fn disclosure_col(&self) -> usize {
        self.disclosure_position().col
    }
}

impl EntityInputHandler for Editor {
    fn text_for_range(
        &mut self,
        range_utf16: Range<usize>,
        adjusted_range: &mut Option<Range<usize>>,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<String> {
        let (text, adjusted) = self.buffer.text_for_utf16_range(range_utf16);
        *adjusted_range = Some(adjusted);
        Some(text)
    }

    fn selected_text_range(
        &mut self,
        _ignore_disabled_input: bool,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<UTF16Selection> {
        let (range, reversed) = self.buffer.selected_utf16_range();
        Some(UTF16Selection { range, reversed })
    }

    fn marked_text_range(
        &self,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<Range<usize>> {
        self.marked_range.clone()
    }

    fn unmark_text(&mut self, _window: &mut Window, cx: &mut Context<Self>) {
        if self.marked_range.is_some() {
            self.marked_range = None;
            self.invalidate_layout_cache();
        }
        cx.notify();
    }

    fn replace_text_in_range(
        &mut self,
        range_utf16: Option<Range<usize>>,
        text: &str,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.can_edit() {
            return;
        }
        let had_marked = self.marked_range.is_some();
        let fallback_marked = self.marked_range.take();
        self.buffer.select_utf16_range(range_utf16, fallback_marked);

        let pair = if !had_marked && !self.buffer.selection().is_empty() {
            match text {
                "(" | ")" => Some(("(", ")")),
                "[" | "]" => Some(("[", "]")),
                "{" | "}" => Some(("{", "}")),
                "\"" => Some(("\"", "\"")),
                "'" => Some(("'", "'")),
                "`" => Some(("`", "`")),
                "*" => Some(("*", "*")),
                "~" => Some(("~~", "~~")),
                "《" | "》" => Some(("《", "》")),
                "“" | "”" => Some(("“", "”")),
                "‘" | "’" => Some(("‘", "’")),
                "【" | "】" => Some(("【", "】")),
                "（" | "）" => Some(("（", "）")),
                _ => None,
            }
        } else {
            None
        };

        let is_source = self.render_mode == RenderMode::Source;
        if !is_source {
            crate::markdown::normalize_buffer_math_cursor(&mut self.buffer);
        }
        self.track_buffer_change(cx, |this| {
            if let Some((open, close)) = pair {
                this.buffer.surround_selection(open, close);
            } else {
                this.buffer.insert_text(text);
            }
        });
        if !is_source {
            crate::markdown::normalize_buffer_math_cursor(&mut self.buffer);
        }

        self.marked_range = None;
        self.pixel_column_goal = None;
        self.last_cursor_action = std::time::Instant::now();
        self.scroll_to_cursor();
        self.start_scroll_animation_if_needed(cx);
        self.reset_cursor_blink(cx);
        cx.notify();
    }

    fn replace_and_mark_text_in_range(
        &mut self,
        range_utf16: Option<Range<usize>>,
        new_text: &str,
        new_selected_range: Option<Range<usize>>,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.can_edit() {
            return;
        }
        let fallback_marked = self.marked_range.clone();

        self.track_buffer_change(cx, |this| {
            this.marked_range = this.buffer.replace_and_mark_utf16(
                range_utf16,
                fallback_marked,
                new_text,
                new_selected_range,
            );
        });

        self.pixel_column_goal = None;
        self.last_cursor_action = std::time::Instant::now();
        if self.marked_range.is_none() {
            self.scroll_to_cursor();
            self.start_scroll_animation_if_needed(cx);
        }
        self.reset_cursor_blink(cx);
        cx.notify();
    }

    fn bounds_for_range(
        &mut self,
        range_utf16: Range<usize>,
        element_bounds: Bounds<Pixels>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<Bounds<Pixels>> {
        let start_pos = if range_utf16.is_empty() {
            crate::buffer::SourcePosition::from_position(self.buffer.cursor_pos())
        } else {
            self.buffer.utf16_offset_to_pos(range_utf16.start)
        };

        let layout = self.layout_snapshot();
        if let Some(row) = layout.parsed_lines.row(start_pos.line.index()) {
            let parsed = &*row;
            let is_active_caret = true;
            let anchor_pos = self
                .marked_range
                .as_ref()
                .map(|marked| self.buffer.utf16_offset_to_pos(marked.end))
                .filter(|pos| pos.line == start_pos.line)
                .unwrap_or(start_pos);

            if element_bounds == self.viewport_bounds
                && let Some(painted) = self.painted_line_geometry(start_pos.line, is_active_caret)
            {
                let pos_for_col = |col: usize| {
                    let target_byte =
                        crate::editor::shaping::raw_col_to_visible_byte_with_selection(
                            col,
                            &parsed.spans,
                            painted.is_active_caret,
                            painted.disclosure_col,
                            painted.is_source,
                            painted.is_selection_disclosure,
                        );
                    painted
                        .wrapped_line
                        .as_ref()
                        .and_then(|wl| wl.position_for_index(target_byte, painted.line_height))
                        .unwrap_or_default()
                };
                let start_cursor_pos = pos_for_col(start_pos.col);
                let width = if !range_utf16.is_empty() {
                    let end_pos = self.buffer.utf16_offset_to_pos(range_utf16.end);
                    if end_pos.line == start_pos.line && end_pos.col >= start_pos.col {
                        (pos_for_col(end_pos.col).x - start_cursor_pos.x)
                            .abs()
                            .max(px(1.5))
                    } else {
                        px(1.5)
                    }
                } else {
                    px(1.5)
                };
                let anchor = pos_for_col(anchor_pos.col);
                return Some(Bounds {
                    origin: point(
                        painted.text_bounds.origin.x + anchor.x,
                        painted.text_bounds.origin.y + anchor.y,
                    ),
                    size: size(width, painted.line_height),
                });
            }

            let line_bounds = self.line_screen_bounds(start_pos.line, element_bounds);
            let theme = cx
                .try_global::<crate::theme::ThemeManager>()
                .map(|m| m.theme())
                .unwrap_or(&crate::theme::DEFAULT_THEME);
            let spec = self.line_text_spec(parsed, theme);
            let (wrap_width, scx) = self.line_shaping_from_spec(&spec, is_active_caret, theme);
            let font_size = spec.font_size;
            let line_height = spec.line_height;

            let start_cursor_pos = crate::editor::shaping::calculate_pos_from_col(
                start_pos.col,
                parsed,
                font_size,
                line_height,
                wrap_width,
                scx,
                window,
            );

            let width = if !range_utf16.is_empty() {
                let end_pos = self.buffer.utf16_offset_to_pos(range_utf16.end);
                if end_pos.line == start_pos.line && end_pos.col >= start_pos.col {
                    let end_cursor_pos = crate::editor::shaping::calculate_pos_from_col(
                        end_pos.col,
                        parsed,
                        font_size,
                        line_height,
                        wrap_width,
                        scx,
                        window,
                    );
                    (end_cursor_pos.x - start_cursor_pos.x).abs().max(px(1.5))
                } else {
                    px(1.5)
                }
            } else {
                px(1.5)
            };

            let anchor = crate::editor::shaping::calculate_pos_from_col(
                anchor_pos.col,
                parsed,
                font_size,
                line_height,
                wrap_width,
                scx,
                window,
            );

            return Some(Bounds {
                origin: point(
                    line_bounds.origin.x + anchor.x,
                    line_bounds.origin.y + anchor.y + spec.top_offset,
                ),
                size: size(width, line_height),
            });
        }

        None
    }

    fn character_index_for_point(
        &mut self,
        point: Point<Pixels>,
        window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<usize> {
        let pos = self.find_pos_for_mouse_point(point, window);
        Some(
            self.buffer
                .pos_to_utf16_offset(crate::buffer::SourcePosition::from_position(pos)),
        )
    }
}
