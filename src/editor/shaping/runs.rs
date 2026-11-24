use gpui::*;

use crate::markdown::InlineSpan;
use crate::theme::Theme;

#[cfg(test)]
pub(crate) mod preparation_metrics {
    std::thread_local! {
        static CALLS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
    }

    pub fn reset() {
        CALLS.set(0);
    }

    pub fn calls() -> usize {
        CALLS.get()
    }

    pub fn record() {
        CALLS.set(CALLS.get() + 1);
    }
}

/// Maps a raw document column to a byte offset in the visible rendered text.
/// `disclosure_col` must match the column used when building `spans`.
pub fn raw_col_to_visible_byte(
    raw_col: usize,
    spans: &[InlineSpan],
    is_active_caret: bool,
    disclosure_col: usize,
    is_source: bool,
) -> usize {
    raw_col_to_visible_byte_with_selection(
        raw_col,
        spans,
        is_active_caret,
        disclosure_col,
        is_source,
        false,
    )
}

pub fn raw_col_to_visible_byte_with_selection(
    raw_col: usize,
    spans: &[InlineSpan],
    is_active_caret: bool,
    disclosure_col: usize,
    is_source: bool,
    is_selection_disclosure: bool,
) -> usize {
    let mut cur_visible_byte = 0;
    let mut target_byte = None;

    for span in spans {
        let is_visible = span.is_visible_for_selection(
            is_active_caret,
            disclosure_col,
            is_source,
            is_selection_disclosure,
        );
        let (s_start, s_end) = span.span_range;

        if !is_visible {
            if target_byte.is_none() && raw_col <= s_end {
                target_byte = Some(cur_visible_byte);
            }
            continue;
        }

        let start_byte = cur_visible_byte;
        let rendered_text = span.rendered_text(is_active_caret, disclosure_col, is_source);

        if target_byte.is_none() {
            if raw_col <= s_start {
                target_byte = Some(start_byte);
            } else if raw_col <= s_end {
                if span.math.is_some()
                    && !span.math_is_disclosed(is_active_caret, disclosure_col, is_source)
                {
                    let end_byte = start_byte + rendered_text.len();
                    let midpoint = s_start + (s_end - s_start) / 2;
                    target_byte = Some(if raw_col <= midpoint {
                        start_byte
                    } else {
                        end_byte
                    });
                } else {
                    let byte_offset: usize = rendered_text
                        .chars()
                        .take(raw_col - s_start)
                        .map(char::len_utf8)
                        .sum();
                    target_byte = Some(start_byte + byte_offset);
                }
            }
        }

        cur_visible_byte += rendered_text.len();
    }

    target_byte.unwrap_or(cur_visible_byte)
}

/// Maps a byte offset in the visible rendered text back to a raw document column.
pub fn visible_byte_to_raw_col(
    visible_byte: usize,
    spans: &[InlineSpan],
    is_active_caret: bool,
    cursor_col: usize,
    is_source: bool,
    total_raw_len: usize,
) -> usize {
    visible_byte_to_raw_col_with_selection(
        visible_byte,
        spans,
        is_active_caret,
        cursor_col,
        is_source,
        total_raw_len,
        false,
    )
}

pub fn visible_byte_to_raw_col_with_selection(
    visible_byte: usize,
    spans: &[InlineSpan],
    is_active_caret: bool,
    cursor_col: usize,
    is_source: bool,
    total_raw_len: usize,
    is_selection_disclosure: bool,
) -> usize {
    let mut cur_byte = 0;

    for span in spans {
        let is_visible = span.is_visible_for_selection(
            is_active_caret,
            cursor_col,
            is_source,
            is_selection_disclosure,
        );
        if !is_visible {
            continue;
        }

        let rendered_text = span.rendered_text(is_active_caret, cursor_col, is_source);
        let span_len = rendered_text.len();
        let span_end_byte = cur_byte + span_len;

        if visible_byte < span_end_byte {
            let offset_in_span_bytes = visible_byte.saturating_sub(cur_byte);
            if span.math.is_some()
                && !span.math_is_disclosed(is_active_caret, cursor_col, is_source)
            {
                let midpoint = span_len / 2;
                return if offset_in_span_bytes <= midpoint {
                    span.group_range.0
                } else {
                    span.group_range.1.min(total_raw_len)
                };
            }
            if offset_in_span_bytes == 0
                && !is_source
                && !is_active_caret
                && span.group_range.0 < span.span_range.0
                && cur_byte == 0
            {
                return span.group_range.0;
            }
            let mut byte_count = 0;
            let mut char_count = 0;
            for c in rendered_text.chars() {
                if byte_count >= offset_in_span_bytes {
                    break;
                }
                byte_count += c.len_utf8();
                char_count += 1;
            }
            return (span.span_range.0 + char_count).min(total_raw_len);
        } else if visible_byte == span_end_byte {
            if !is_source && !is_active_caret && span.group_range.1 > span.span_range.1 {
                return span.group_range.1.min(total_raw_len);
            } else {
                return span.span_range.1.min(total_raw_len);
            }
        }

        cur_byte = span_end_byte;
    }

    total_raw_len
}

#[derive(Clone, Copy)]
pub struct LineShapingContext<'a> {
    pub theme: &'a Theme,
    pub override_color: Option<Hsla>,
    pub is_source: bool,
    pub is_monospace: bool,
    pub is_active_caret: bool,
    pub cursor_col: usize,
    pub is_selection_disclosure: bool,
    pub is_heading: bool,
    pub ime_preedit_cols: Option<(usize, usize)>,
}

impl<'a> LineShapingContext<'a> {
    pub fn new(
        theme: &'a Theme,
        override_color: Option<Hsla>,
        is_source: bool,
        is_active_caret: bool,
        cursor_col: usize,
    ) -> Self {
        Self {
            theme,
            override_color,
            is_source,
            is_monospace: is_source,
            is_active_caret,
            cursor_col,
            is_selection_disclosure: false,
            is_heading: false,
            ime_preedit_cols: None,
        }
    }

    pub fn with_monospace(mut self, is_monospace: bool) -> Self {
        self.is_monospace = is_monospace;
        self
    }

    pub fn with_selection_disclosure(mut self, is_selection_disclosure: bool) -> Self {
        self.is_selection_disclosure = is_selection_disclosure;
        self
    }

    pub fn with_heading(mut self, is_heading: bool) -> Self {
        self.is_heading = is_heading;
        self
    }

    pub fn with_ime_preedit(mut self, ime_preedit_cols: Option<(usize, usize)>) -> Self {
        self.ime_preedit_cols = ime_preedit_cols.filter(|(s, e)| s < e);
        self
    }
}

#[inline]
fn char_offset_to_byte(s: &str, char_idx: usize) -> usize {
    if char_idx == 0 {
        return 0;
    }
    s.char_indices()
        .nth(char_idx)
        .map_or(s.len(), |(byte_idx, _)| byte_idx)
}

pub fn build_line_runs(
    spans: &[InlineSpan],
    scx: LineShapingContext,
    hovered_link_range: Option<(usize, usize)>,
    base_style: &TextStyle,
) -> (String, Vec<TextRun>) {
    #[cfg(test)]
    preparation_metrics::record();
    let mut line_str = String::new();
    let mut runs = Vec::new();

    for span in spans {
        let is_visible = span.is_visible_for_selection(
            scx.is_active_caret,
            scx.cursor_col,
            scx.is_source,
            scx.is_selection_disclosure,
        );
        if !is_visible {
            continue;
        }

        let rendered_text = span.rendered_text(scx.is_active_caret, scx.cursor_col, scx.is_source);
        if rendered_text.is_empty() {
            continue;
        }

        let span_byte_len = rendered_text.len();
        line_str.push_str(&rendered_text);

        let mut font = base_style.font();
        let mut color = scx.override_color.unwrap_or(scx.theme.text_primary);
        let mut background_color = None;
        let mut strikethrough = None;
        let mut underline = None;

        if !span.math_is_disclosed(scx.is_active_caret, scx.cursor_col, scx.is_source)
            && span.math.is_some()
        {
            color = color.opacity(0.0);
        }

        if scx.is_source || scx.is_monospace || span.is_code || span.syntax_token.is_some() {
            font.family = crate::platform::platform_monospace_font().into();
        } else {
            font.family = crate::platform::platform_ui_font().into();
        }

        if let Some(token) = span.syntax_token {
            color = token.color(scx.theme);
            if token.is_italic() {
                font.style = FontStyle::Italic;
            }
        }

        if span.is_code {
            if span.is_marker {
                color = scx.theme.heading_marker;
            } else {
                color = scx.theme.inline_code_fg;
                background_color = Some(scx.theme.inline_code_bg);
            }
        }

        if span.is_link {
            let is_link_hovered = hovered_link_range
                .is_some_and(|range| span.group_range == range || span.span_range == range);
            if span.is_marker {
                if is_link_hovered {
                    color = scx.theme.link_fg;
                    underline = Some(UnderlineStyle {
                        thickness: px(1.0),
                        color: Some(scx.theme.link_fg),
                        wavy: false,
                    });
                } else {
                    color = scx.theme.heading_marker;
                }
            } else {
                if !span.is_code {
                    color = scx.theme.link_fg;
                }
                if is_link_hovered {
                    underline = Some(UnderlineStyle {
                        thickness: px(1.0),
                        color: Some(scx.theme.link_fg),
                        wavy: false,
                    });
                }
            }
        }

        if (span.is_marker && !span.is_code && !span.is_link)
            || (scx.is_source
                && span
                    .math
                    .as_ref()
                    .is_some_and(|math| math.is_display && math.source == "$$"))
        {
            color = scx.theme.heading_marker;
        }

        if scx.is_heading || span.is_bold {
            font.weight = FontWeight::BOLD;
        }
        if span.is_italic {
            font.style = FontStyle::Italic;
        }
        if span.is_strikethrough {
            strikethrough = Some(StrikethroughStyle {
                thickness: px(1.0),
                color: Some(color),
            });
        }

        if let Some((pre_start, pre_end)) = scx.ime_preedit_cols
            && pre_start < pre_end
            && span.math.is_none()
            && span.span_range.0 < pre_end
            && span.span_range.1 > pre_start
        {
            let s = span.span_range.0;
            let e = span.span_range.1;
            let char_count = rendered_text.chars().count();
            let mid_start = pre_start.max(s);
            let mid_end = pre_end.min(e).min(s + char_count);
            if mid_start < mid_end {
                let b_start = char_offset_to_byte(&rendered_text, mid_start - s);
                let b_end = char_offset_to_byte(&rendered_text, mid_end - s);
                let preedit_underline = Some(UnderlineStyle {
                    thickness: px(1.0),
                    color: Some(color),
                    wavy: false,
                });
                for (len, run_underline) in [
                    (b_start, underline),
                    (b_end.saturating_sub(b_start), preedit_underline),
                    (span_byte_len.saturating_sub(b_end), underline),
                ] {
                    if len > 0 {
                        runs.push(TextRun {
                            len,
                            font: font.clone(),
                            color,
                            background_color,
                            underline: run_underline,
                            strikethrough,
                        });
                    }
                }
                continue;
            }
        }

        runs.push(TextRun {
            len: span_byte_len,
            font,
            color,
            background_color,
            underline,
            strikethrough,
        });
    }

    (line_str, runs)
}
