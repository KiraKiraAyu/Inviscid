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
        self.buffer.ensure_cursor_not_on_math_delimiter(is_source);
        self.track_buffer_change(cx, |this| {
            if let Some((open, close)) = pair {
                this.buffer.surround_selection(open, close);
            } else {
                this.buffer.insert_text(text);
            }
        });
        self.buffer.ensure_cursor_not_on_math_delimiter(is_source);

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
        if let Some(parsed) = layout.parsed_lines.get(start_pos.line.index()) {
            let line_bounds = self.line_screen_bounds(start_pos.line, element_bounds);
            let is_active_caret = true;
            let theme = cx
                .try_global::<crate::theme::ThemeManager>()
                .map(|m| m.theme())
                .unwrap_or(&crate::theme::DEFAULT_THEME);
            let (font_size, line_height, wrap_width, scx) =
                self.line_shaping_metrics(parsed, is_active_caret, theme);

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

            let anchor_pos = self
                .marked_range
                .as_ref()
                .map(|marked| self.buffer.utf16_offset_to_pos(marked.end))
                .filter(|pos| pos.line == start_pos.line)
                .unwrap_or(start_pos);
            let anchor = crate::editor::shaping::calculate_pos_from_col(
                anchor_pos.col,
                parsed,
                font_size,
                line_height,
                wrap_width,
                scx,
                window,
            );

            let top_offset =
                crate::editor::shaping::get_block_top_offset(&parsed.kind, self.render_mode);
            return Some(Bounds {
                origin: point(
                    line_bounds.origin.x + anchor.x,
                    line_bounds.origin.y + anchor.y + top_offset,
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
