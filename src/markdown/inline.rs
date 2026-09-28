#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub struct InlineSpan {
    pub text: String,
    pub is_bold: bool,
    pub is_italic: bool,
    pub is_code: bool,
    pub is_strikethrough: bool,
    pub is_link: bool,
    pub link_url: Option<String>,
    pub is_image: bool,
    pub is_marker: bool,
    pub is_always_hidden_in_live: bool,
    pub is_ime_preedit: bool,
    pub group_range: (usize, usize),
    pub span_range: (usize, usize),
}

impl InlineSpan {
    pub fn plain(text: impl Into<String>, range: (usize, usize)) -> Self {
        Self {
            text: text.into(),
            group_range: range,
            span_range: range,
            ..Default::default()
        }
    }

    pub fn hidden_marker(
        text: impl Into<String>,
        group_range: (usize, usize),
        span_range: (usize, usize),
    ) -> Self {
        Self {
            text: text.into(),
            is_marker: true,
            is_always_hidden_in_live: true,
            group_range,
            span_range,
            ..Default::default()
        }
    }

    pub fn is_visible(
        &self,
        is_active_line: bool,
        cursor_col: usize,
        is_source_mode: bool,
    ) -> bool {
        if self.is_ime_preedit {
            return false;
        }
        if is_source_mode {
            return true;
        }
        if self.is_always_hidden_in_live {
            return false;
        }
        if !self.is_marker {
            return true;
        }
        if self.is_image {
            return is_active_line;
        }
        if is_active_line {
            cursor_col >= self.group_range.0 && cursor_col <= self.group_range.1
        } else {
            false
        }
    }
}

pub fn mark_ime_preedit(spans: &[InlineSpan], range: (usize, usize)) -> Vec<InlineSpan> {
    let (start, end) = range;
    if start >= end {
        return spans.to_vec();
    }

    let mut out = Vec::with_capacity(spans.len() + 2);
    for span in spans {
        let (s, e) = span.span_range;
        if e <= start || s >= end || span.is_ime_preedit {
            out.push(span.clone());
            continue;
        }

        let chars: Vec<char> = span.text.chars().collect();
        let mid_start = start.max(s);
        let mid_end = end.min(e).min(s + chars.len());
        for (from, to, is_preedit) in [
            (s, mid_start, false),
            (mid_start, mid_end, true),
            (mid_end, e, false),
        ] {
            if from >= to {
                continue;
            }
            let mut part = span.clone();
            part.text = chars[from - s..to - s].iter().collect();
            part.span_range = (from, to);
            part.is_ime_preedit = is_preedit;
            out.push(part);
        }
    }
    out
}

#[derive(Clone, Copy, Default, Debug)]
struct InlineStyleContext<'a> {
    is_bold: bool,
    is_italic: bool,
    is_strikethrough: bool,
    is_link: bool,
    link_url: Option<&'a str>,
}

impl InlineStyleContext<'_> {
    fn make_span(
        &self,
        text: impl Into<String>,
        group_range: (usize, usize),
        span_range: (usize, usize),
        is_marker: bool,
    ) -> InlineSpan {
        InlineSpan {
            text: text.into(),
            is_bold: self.is_bold,
            is_italic: self.is_italic,
            is_strikethrough: self.is_strikethrough,
            is_link: self.is_link,
            link_url: self.link_url.map(str::to_string),
            is_marker,
            group_range,
            span_range,
            ..Default::default()
        }
    }

    fn make_code_span(
        &self,
        text: impl Into<String>,
        group_range: (usize, usize),
        span_range: (usize, usize),
        is_marker: bool,
    ) -> InlineSpan {
        let mut span = self.make_span(text, group_range, span_range, is_marker);
        span.is_code = true;
        span
    }
}

fn find_matching_code_span(chars: &[char], start: usize) -> Option<usize> {
    if start >= chars.len() || chars[start] != '`' {
        return None;
    }
    let tick_count = chars[start..].iter().take_while(|&&c| c == '`').count();
    if tick_count >= 3 {
        return None;
    }
    let mut j = start + tick_count;
    while j + tick_count <= chars.len() {
        if chars[j..(j + tick_count)].iter().all(|&c| c == '`')
            && (j + tick_count == chars.len() || chars[j + tick_count] != '`')
            && (j == 0 || chars[j - 1] != '`')
        {
            return Some(j + tick_count);
        }
        j += 1;
    }
    None
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum DelimiterStyle {
    BoldItalic,
    Bold,
    Strikethrough,
    Italic,
}

fn find_closing_delimiter(chars: &[char], start: usize, ch: char, m_len: usize) -> Option<usize> {
    let len = chars.len();
    let mut j = start;

    while j + m_len <= len {
        if chars[j] == '`'
            && let Some(next_j) = find_matching_code_span(chars, j)
        {
            j = next_j;
            continue;
        }

        if chars[j..j + m_len].iter().all(|&c| c == ch) {
            let is_isolated = match m_len {
                1 => (j == 0 || chars[j - 1] != ch) && (j + 1 == len || chars[j + 1] != ch),
                2 if ch == '*' || ch == '_' => {
                    (j == 0 || chars[j - 1] != ch) && (j + 2 == len || chars[j + 2] != ch)
                }
                _ => true,
            };

            if is_isolated {
                return Some(j);
            }
        }
        j += 1;
    }
    None
}

struct DelimitedSpan<'a> {
    open_marker: &'a str,
    close_marker: &'a str,
    inner_text: &'a str,
    start_idx: usize,
    inner_start: usize,
    inner_end: usize,
    end_idx: usize,
}

fn push_delimited_with_nesting<'a, F>(
    spans: &mut Vec<InlineSpan>,
    delim: DelimitedSpan<'_>,
    base_offset: usize,
    ctx: InlineStyleContext<'a>,
    style_update: F,
    depth: usize,
) where
    F: FnOnce(&mut InlineStyleContext<'a>),
{
    let group_start = base_offset + delim.start_idx;
    let group_end = base_offset + delim.end_idx;
    let mut inner_ctx = ctx;
    style_update(&mut inner_ctx);

    spans.push(inner_ctx.make_span(
        delim.open_marker,
        (group_start, group_end),
        (
            base_offset + delim.start_idx,
            base_offset + delim.inner_start,
        ),
        true,
    ));

    if !delim.inner_text.is_empty() {
        if depth < 8 {
            let mut inner_spans = parse_inline_spans_internal(
                delim.inner_text,
                base_offset + delim.inner_start,
                inner_ctx,
                depth + 1,
            );
            if let Some(first) = inner_spans.first_mut()
                && !first.is_marker
            {
                first.group_range.0 = group_start;
            }
            if let Some(last) = inner_spans.last_mut()
                && !last.is_marker
            {
                last.group_range.1 = group_end;
            }
            spans.append(&mut inner_spans);
        } else {
            spans.push(inner_ctx.make_span(
                delim.inner_text,
                (group_start, group_end),
                (
                    base_offset + delim.inner_start,
                    base_offset + delim.inner_end,
                ),
                false,
            ));
        }
    }

    spans.push(inner_ctx.make_span(
        delim.close_marker,
        (group_start, group_end),
        (base_offset + delim.inner_end, base_offset + delim.end_idx),
        true,
    ));
}

pub fn parse_inline_spans(text: &str, base_offset: usize) -> Vec<InlineSpan> {
    parse_inline_spans_internal(text, base_offset, InlineStyleContext::default(), 0)
}

fn parse_inline_spans_internal(
    text: &str,
    base_offset: usize,
    ctx: InlineStyleContext,
    depth: usize,
) -> Vec<InlineSpan> {
    let mut spans = Vec::new();
    let chars: Vec<char> = text.chars().collect();
    let len = chars.len();
    let mut i = 0;

    let mut normal_buffer = String::new();
    let mut normal_start = 0;

    let flush_normal = |buf: &mut String, start: usize, end: usize, spans: &mut Vec<InlineSpan>| {
        if !buf.is_empty() {
            let range = (base_offset + start, base_offset + end);
            spans.push(ctx.make_span(std::mem::take(buf), range, range, false));
        }
    };

    while i < len {
        // Inline code (`code` or ``code``)
        if chars[i] == '`' {
            let tick_count = chars[i..].iter().take_while(|&&c| c == '`').count();
            if tick_count >= 3 {
                normal_buffer.extend(std::iter::repeat_n('`', tick_count));
                i += tick_count;
                continue;
            }

            if let Some(close_end) = find_matching_code_span(&chars, i) {
                let end_idx = close_end - tick_count;
                if end_idx > i + tick_count {
                    flush_normal(&mut normal_buffer, normal_start, i, &mut spans);

                    let marker_str = "`".repeat(tick_count);
                    let code_text: String = chars[(i + tick_count)..end_idx].iter().collect();
                    let group_start = base_offset + i;
                    let group_end = base_offset + close_end;

                    spans.push(ctx.make_code_span(
                        marker_str.clone(),
                        (group_start, group_end),
                        (group_start, base_offset + i + tick_count),
                        true,
                    ));
                    spans.push(ctx.make_code_span(
                        code_text,
                        (group_start, group_end),
                        (base_offset + i + tick_count, base_offset + end_idx),
                        false,
                    ));
                    spans.push(ctx.make_code_span(
                        marker_str,
                        (group_start, group_end),
                        (base_offset + end_idx, group_end),
                        true,
                    ));

                    i = close_end;
                    normal_start = i;
                    continue;
                }
            }
        }

        // Inline images ![alt](url)
        if i + 1 < len
            && chars[i] == '!'
            && chars[i + 1] == '['
            && let Some(close_bracket) = chars[(i + 2)..]
                .iter()
                .position(|&c| c == ']')
                .map(|p| p + i + 2)
            && close_bracket + 1 < len
            && chars[close_bracket + 1] == '('
            && let Some(close_paren) = chars[(close_bracket + 2)..]
                .iter()
                .position(|&c| c == ')')
                .map(|p| p + close_bracket + 2)
        {
            flush_normal(&mut normal_buffer, normal_start, i, &mut spans);

            let alt_text: String = chars[(i + 2)..close_bracket].iter().collect();
            let close_str: String = chars[close_bracket..=close_paren].iter().collect();
            let group_start = base_offset + i;
            let group_end = base_offset + close_paren + 1;

            spans.push(InlineSpan {
                text: "![".to_string(),
                is_image: true,
                is_marker: true,
                group_range: (group_start, group_end),
                span_range: (group_start, base_offset + i + 2),
                ..Default::default()
            });

            if !alt_text.is_empty() {
                spans.push(InlineSpan {
                    text: alt_text,
                    is_image: true,
                    group_range: (group_start, group_end),
                    span_range: (base_offset + i + 2, base_offset + close_bracket),
                    ..Default::default()
                });
            }

            spans.push(InlineSpan {
                text: close_str,
                is_image: true,
                is_marker: true,
                group_range: (group_start, group_end),
                span_range: (base_offset + close_bracket, group_end),
                ..Default::default()
            });

            i = close_paren + 1;
            normal_start = i;
            continue;
        }

        // Links [text](url)
        if !ctx.is_link && chars[i] == '[' {
            let mut close_bracket = None;
            let mut j = i + 1;
            while j < len {
                if chars[j] == '`'
                    && let Some(next_j) = find_matching_code_span(&chars, j)
                {
                    j = next_j;
                    continue;
                }
                if chars[j] == ']' {
                    close_bracket = Some(j);
                    break;
                }
                j += 1;
            }

            if let Some(close_bracket) = close_bracket
                && close_bracket + 1 < len
                && chars[close_bracket + 1] == '('
                && let Some(close_paren) = chars[(close_bracket + 2)..]
                    .iter()
                    .position(|&c| c == ')')
                    .map(|p| p + close_bracket + 2)
            {
                flush_normal(&mut normal_buffer, normal_start, i, &mut spans);

                let link_text: String = chars[(i + 1)..close_bracket].iter().collect();
                let url: String = chars[(close_bracket + 2)..close_paren].iter().collect();
                let close_str: String = chars[close_bracket..=close_paren].iter().collect();

                push_delimited_with_nesting(
                    &mut spans,
                    DelimitedSpan {
                        open_marker: "[",
                        close_marker: &close_str,
                        inner_text: &link_text,
                        start_idx: i,
                        inner_start: i + 1,
                        inner_end: close_bracket,
                        end_idx: close_paren + 1,
                    },
                    base_offset,
                    ctx,
                    |c| {
                        c.is_link = true;
                        c.link_url = Some(&url);
                    },
                    depth,
                );

                i = close_paren + 1;
                normal_start = i;
                continue;
            }
        }

        // Autolinks <http://...> or <https://...>
        if chars[i] == '<'
            && let Some(close_bracket) = chars[(i + 1)..]
                .iter()
                .position(|&c| c == '>')
                .map(|p| p + i + 1)
        {
            let inner: String = chars[(i + 1)..close_bracket].iter().collect();
            if inner.starts_with("http://")
                || inner.starts_with("https://")
                || inner.starts_with("mailto:")
                || (inner.contains('@') && !inner.contains(' '))
            {
                flush_normal(&mut normal_buffer, normal_start, i, &mut spans);

                let group_start = base_offset + i;
                let group_end = base_offset + close_bracket + 1;
                let mut link_ctx = ctx;
                link_ctx.is_link = true;
                link_ctx.link_url = Some(&inner);

                spans.push(link_ctx.make_span(
                    "<",
                    (group_start, group_end),
                    (group_start, base_offset + i + 1),
                    true,
                ));
                spans.push(link_ctx.make_span(
                    inner.clone(),
                    (group_start, group_end),
                    (base_offset + i + 1, base_offset + close_bracket),
                    false,
                ));
                spans.push(link_ctx.make_span(
                    ">",
                    (group_start, group_end),
                    (base_offset + close_bracket, group_end),
                    true,
                ));

                i = close_bracket + 1;
                normal_start = i;
                continue;
            }
        }

        // Delimited inline styling: bold-italic (*** / ___), bold (** / __), strikethrough (~~), and italic (* / _)
        let delim_candidate = if (!ctx.is_bold || !ctx.is_italic)
            && i + 2 < len
            && ((chars[i] == '*' && chars[i + 1] == '*' && chars[i + 2] == '*')
                || (chars[i] == '_' && chars[i + 1] == '_' && chars[i + 2] == '_'))
        {
            Some((chars[i], 3, DelimiterStyle::BoldItalic))
        } else if !ctx.is_bold
            && i + 1 < len
            && ((chars[i] == '*' && chars[i + 1] == '*')
                || (chars[i] == '_' && chars[i + 1] == '_'))
        {
            Some((chars[i], 2, DelimiterStyle::Bold))
        } else if !ctx.is_strikethrough && i + 1 < len && chars[i] == '~' && chars[i + 1] == '~' {
            Some(('~', 2, DelimiterStyle::Strikethrough))
        } else if !ctx.is_italic && (chars[i] == '*' || chars[i] == '_') {
            Some((chars[i], 1, DelimiterStyle::Italic))
        } else {
            None
        };

        if let Some((ch, m_len, style)) = delim_candidate {
            let inner_start = i + m_len;
            if let Some(end_idx) = find_closing_delimiter(&chars, inner_start, ch, m_len)
                && end_idx > inner_start
            {
                flush_normal(&mut normal_buffer, normal_start, i, &mut spans);
                let marker_str: String = std::iter::repeat_n(ch, m_len).collect();
                let inner_text: String = chars[inner_start..end_idx].iter().collect();

                push_delimited_with_nesting(
                    &mut spans,
                    DelimitedSpan {
                        open_marker: &marker_str,
                        close_marker: &marker_str,
                        inner_text: &inner_text,
                        start_idx: i,
                        inner_start,
                        inner_end: end_idx,
                        end_idx: end_idx + m_len,
                    },
                    base_offset,
                    ctx,
                    |c| match style {
                        DelimiterStyle::BoldItalic => {
                            c.is_bold = true;
                            c.is_italic = true;
                        }
                        DelimiterStyle::Bold => c.is_bold = true,
                        DelimiterStyle::Strikethrough => c.is_strikethrough = true,
                        DelimiterStyle::Italic => c.is_italic = true,
                    },
                    depth,
                );

                i = end_idx + m_len;
                normal_start = i;
                continue;
            }
        }

        normal_buffer.push(chars[i]);
        i += 1;
    }

    flush_normal(&mut normal_buffer, normal_start, len, &mut spans);
    spans
}
