use gpui::*;
use std::ops::Range;
use std::time::{Duration, Instant};

use super::Editor;

/// Grace window to recognize an edit key (e.g. Backspace) that the platform IME already consumed
/// when clearing a single-character preedit before forwarding `WM_KEYDOWN`.
const IME_CONSUMED_KEY_GRACE: Duration = Duration::from_millis(20);

impl Editor {
    /// Column used for markdown syntax-marker disclosure.
    /// Pinned to the composition start while an IME preedit is active so transient composition text
    /// does not reflow the document before commit.
    pub(crate) fn disclosure_col(&self) -> usize {
        match &self.marked_range {
            Some(marked) => self.buffer.utf16_offset_to_pos(marked.start).col,
            None => self.buffer.cursor_pos().col,
        }
    }

    fn note_ime_preedit_cleared(&mut self) {
        self.ime_consumed_edit_key_at = Some(Instant::now());
    }

    fn ime_cleared_existing_preedit(&self, range_utf16: Option<&Range<usize>>, text: &str) -> bool {
        range_utf16.is_none() && text.is_empty() && self.marked_range.is_some()
    }

    fn take_ime_consumed_edit_key(&mut self) -> bool {
        self.ime_consumed_edit_key_at
            .take()
            .is_some_and(|at| at.elapsed() <= IME_CONSUMED_KEY_GRACE)
    }

    pub(crate) fn edit_key_belongs_to_ime(&mut self) -> bool {
        let consumed = self.take_ime_consumed_edit_key();
        self.marked_range.is_some() || consumed
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
        if self.ime_cleared_existing_preedit(range_utf16.as_ref(), text) {
            self.note_ime_preedit_cleared();
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

        self.track_buffer_change(cx, |this| {
            if let Some((open, close)) = pair {
                this.buffer.surround_selection(open, close);
            } else {
                this.buffer.insert_text(text);
            }
        });

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
        if self.ime_cleared_existing_preedit(range_utf16.as_ref(), new_text) {
            self.note_ime_preedit_cleared();
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
            self.buffer.cursor_pos()
        } else {
            self.buffer.utf16_offset_to_pos(range_utf16.start)
        };

        let layout = self.layout_snapshot();
        if let Some(parsed) = layout.parsed_lines.get(start_pos.line) {
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

            // The preedit is projected with zero width (see `mark_ime_preedit`), so the anchor box has to
            // be pushed past it — otherwise the candidate window would detach from the pinyin being typed.
            let preedit_width = crate::editor::shaping::ime_preedit_overlay(
                &parsed.spans,
                is_active_caret,
                self.disclosure_col(),
                self.render_mode == super::RenderMode::Source,
            )
            .map(|(text, _)| {
                let line_font = crate::editor::shaping::line_font_for(
                    &parsed.kind,
                    self.render_mode == super::RenderMode::Source,
                );
                crate::editor::shaping::measure_ime_preedit(
                    &text,
                    font_size,
                    &line_font,
                    theme.text_primary,
                    window,
                )
            })
            .unwrap_or(px(0.0));

            let top_offset =
                crate::editor::shaping::get_block_top_offset(&parsed.kind, self.render_mode);
            return Some(Bounds {
                origin: point(
                    line_bounds.origin.x + start_cursor_pos.x + preedit_width,
                    line_bounds.origin.y + start_cursor_pos.y + top_offset,
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
        Some(self.buffer.pos_to_utf16_offset(pos))
    }
}
