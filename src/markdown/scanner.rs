use super::indent::{leading_indent_len, quote_depth};
use super::inline::{InlineSpan, parse_inline_spans};
use super::prefix::{
    BlockPrefix, fence_info, is_fence_line, is_thematic_break, parse_block_prefix,
};
use super::table::{is_table_delimiter_row, parse_table_alignments, parse_table_row_cells};
use super::types::{BlockKind, ParsedLine, QuoteContext, TableAlignment};
#[cfg(test)]
use crate::buffer::EditDelta;
use crate::buffer::{LineSource, SourceLine};

mod window;

fn find_next_fence_from(lines: &[String], from_idx: usize) -> Option<usize> {
    lines
        .get(from_idx..)?
        .iter()
        .position(|l| is_fence_line(l))
        .map(|p| p + from_idx)
}

fn is_math_delimiter(line: &str) -> bool {
    super::structure::classify(line).is_math_delim
}

fn is_mermaid_info(info: &str) -> bool {
    info.split_whitespace()
        .next()
        .is_some_and(|language| language.eq_ignore_ascii_case("mermaid"))
}

fn is_mermaid_fence(line: &str) -> bool {
    fence_info(line).is_some_and(is_mermaid_info)
}

fn find_closing_math_delimiter(lines: &[String], from_idx: usize) -> Option<usize> {
    let mut in_code_fence = false;
    for (offset, line) in lines.get(from_idx..)?.iter().enumerate() {
        if is_fence_line(line) {
            in_code_fence = !in_code_fence;
        } else if !in_code_fence && is_math_delimiter(line) {
            return Some(from_idx + offset);
        }
    }
    None
}

/// Immutable source projection; normalized text is borrowed or produced one row at a time.
#[derive(Clone, Debug)]
pub struct QuoteProjection {
    source: crate::buffer::SourceSnapshot,
    index: crate::buffer::StructureIndex,
}

impl Default for QuoteProjection {
    fn default() -> Self {
        Self::from_lines(&[])
    }
}

impl QuoteProjection {
    pub(crate) fn context_range(
        &self,
        mut range: std::ops::Range<usize>,
    ) -> std::ops::Range<usize> {
        if range.is_empty() {
            return range;
        }
        if let Some(blank) = self.index.blank_range(range.start) {
            range.start = blank.start;
        }
        if let Some(blank) = self.index.blank_range(range.end - 1) {
            range.end = blank.end;
        }
        for group in self.quote_ranges(range.clone()) {
            range.start = range.start.min(group.start);
            range.end = range.end.max(group.end);
        }
        while range.start > 0
            && self.normalized_line(range.start).contains('|')
            && self.normalized_line(range.start - 1).contains('|')
        {
            range.start -= 1;
        }
        while range.end < self.len()
            && self.normalized_line(range.end - 1).contains('|')
            && self.normalized_line(range.end).contains('|')
        {
            range.end += 1;
        }
        range.start.saturating_sub(1)..(range.end + 1).min(self.len())
    }

    pub(crate) fn block_pair(&self, row: usize) -> Option<(SourceLine, SourceLine)> {
        let line = SourceLine::new(row);
        self.index
            .fence_range(line)
            .or_else(|| self.index.math_range(line))
            .map(|(start, end)| {
                (
                    start,
                    end.unwrap_or_else(|| SourceLine::new(self.len().saturating_sub(1))),
                )
            })
    }

    pub(crate) fn quote_ranges(
        &self,
        requested: std::ops::Range<usize>,
    ) -> Vec<std::ops::Range<usize>> {
        let mut ranges = Vec::new();
        let mut row = requested.start;
        while row < requested.end {
            if self.quote_prefix(row).is_none() {
                row += 1;
                continue;
            }
            let mut start = row;
            while start > 0 && self.quote_prefix(start - 1).is_some() {
                start -= 1;
            }
            row += 1;
            while row < self.len() && self.quote_prefix(row).is_some() {
                row += 1;
            }
            ranges.push(start..row);
        }
        ranges
    }
    pub fn separator(&self, row: usize) -> bool {
        local_separator(&self.source, &self.index, row)
    }
    pub fn from_lines(lines: &[String]) -> Self {
        Self::from_source(lines)
    }

    pub fn from_source(source: &(impl LineSource + ?Sized)) -> Self {
        let snapshot = source.source_snapshot();
        Self {
            index: snapshot.structure_index(),
            source: snapshot,
        }
    }

    pub fn len(&self) -> usize {
        self.source.line_count()
    }
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    pub fn quote_prefix(&self, row: usize) -> Option<usize> {
        let prefix = self.index.quote_prefix(SourceLine::new(row));
        (prefix > 0).then_some(prefix)
    }

    pub fn normalized_line(&self, row: usize) -> std::borrow::Cow<'_, str> {
        let line = self.source.source_line(SourceLine::new(row));
        let prefix = self.quote_prefix(row).unwrap_or(0);
        match line {
            std::borrow::Cow::Borrowed(text) => std::borrow::Cow::Borrowed(&text[prefix..]),
            std::borrow::Cow::Owned(text) if prefix == 0 => std::borrow::Cow::Owned(text),
            std::borrow::Cow::Owned(text) => std::borrow::Cow::Owned(text[prefix..].to_owned()),
        }
    }

    #[cfg(test)]
    pub fn normalized_lines(&self) -> Vec<String> {
        (0..self.len())
            .map(|row| self.normalized_line(row).into_owned())
            .collect()
    }
    #[cfg(test)]
    pub fn quote_prefixes(&self) -> Vec<Option<usize>> {
        (0..self.len()).map(|row| self.quote_prefix(row)).collect()
    }
    #[cfg(test)]
    pub(crate) fn try_update_source(
        &mut self,
        source: &(impl LineSource + ?Sized),
        delta: EditDelta,
    ) -> bool {
        let start = delta.start_line.index();
        let Some(end) = start.checked_add(delta.old_line_count) else {
            return false;
        };
        if delta.old_line_count != delta.new_line_count
            || source.line_count() != self.len()
            || end > self.len()
        {
            return false;
        }
        if (start..end).any(|idx| {
            self.quote_prefix(idx).is_none() && is_fence_line(&self.normalized_line(idx))
        }) {
            return false;
        }
        let changed: Vec<_> = (start..end)
            .map(|idx| source.source_line(SourceLine::new(idx)))
            .collect();
        if changed.iter().any(|line| is_fence_line(line)) {
            return false;
        }
        let changes: Vec<(usize, String)> = (start..end)
            .zip(changed)
            .map(|(idx, line)| (idx, line.to_string()))
            .collect();
        self.source.replace_rows(&changes);
        self.index.splice(
            start..start + delta.old_line_count,
            &self.source,
            start..start + delta.new_line_count,
        );
        true
    }
}

pub(crate) fn math_block_range_with_projection(
    projection: &QuoteProjection,
    target_line: SourceLine,
) -> Option<(SourceLine, SourceLine)> {
    let (start, Some(end)) = projection.index.math_range(target_line)? else {
        return None;
    };
    Some((start, end))
}

#[cfg(test)]
fn math_block_range_normalized(
    lines: &[String],
    target_line: SourceLine,
) -> Option<(SourceLine, SourceLine)> {
    let target_idx = target_line.index();
    let mut in_code_fence = false;
    let mut open_idx = None;

    for (idx, line) in lines.iter().enumerate() {
        if is_fence_line(line) {
            in_code_fence = !in_code_fence;
        } else if !in_code_fence && is_math_delimiter(line) {
            if let Some(start) = open_idx.take() {
                if start <= target_idx && target_idx <= idx {
                    return Some((SourceLine::new(start), SourceLine::new(idx)));
                }
                if idx >= target_idx {
                    return None;
                }
            } else {
                if idx > target_idx {
                    return None;
                }
                open_idx = Some(idx);
            }
        }

        if idx >= target_idx && open_idx.is_none() {
            return None;
        }
    }

    None
}

pub(crate) fn mermaid_block_range_with_projection(
    projection: &QuoteProjection,
    target_line: SourceLine,
) -> Option<(SourceLine, SourceLine)> {
    let (start, Some(end)) = projection.index.fence_range(target_line)? else {
        return None;
    };
    is_mermaid_fence(&projection.normalized_line(start.index())).then_some((start, end))
}

#[cfg(test)]
fn math_block_state_before(projection: &QuoteProjection, end_idx: usize) -> bool {
    math_block_range_with_projection(projection, SourceLine::new(end_idx))
        .is_some_and(|(start, _)| start.index() < end_idx)
}

fn highlight_fenced_slice(
    lines: &[String],
    open_fence_idx: Option<usize>,
    close_fence_idx: usize,
) -> (usize, Vec<Vec<InlineSpan>>) {
    let content_start = open_fence_idx.map(|i| i + 1).unwrap_or(0);
    let lang = open_fence_idx
        .and_then(|i| fence_info(&lines[i]))
        .unwrap_or("");
    let spans = if content_start < close_fence_idx {
        crate::syntax::highlight_code_block(lang, &lines[content_start..close_fence_idx])
    } else {
        Vec::new()
    };
    (content_start, spans)
}

#[cfg(test)]
thread_local! {
    pub(crate) static FULL_FENCE_SCANS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

fn shift_spans(spans: &mut [InlineSpan], offset: usize) {
    for span in spans {
        span.group_range.0 += offset;
        span.group_range.1 += offset;
        span.span_range.0 += offset;
        span.span_range.1 += offset;
    }
}

pub struct MarkdownScanner;

#[cfg(test)]
mod window_tests {
    use super::*;
    use crate::buffer::TextBuffer;

    #[test]
    fn separator_queries_do_not_walk_long_blank_runs() {
        struct Counted {
            rows: Vec<String>,
            reads: std::cell::Cell<usize>,
        }
        impl LineSource for Counted {
            fn line_count(&self) -> usize {
                self.rows.len()
            }
            fn source_line(&self, line: SourceLine) -> std::borrow::Cow<'_, str> {
                self.reads.set(self.reads.get() + 1);
                std::borrow::Cow::Borrowed(&self.rows[line.index()])
            }
        }
        let mut rows = vec![String::new(); 100_000];
        rows[0] = "paragraph".into();
        let index = crate::buffer::StructureIndex::new(&rows);
        let source = Counted {
            rows,
            reads: std::cell::Cell::new(0),
        };
        assert!(local_separator(&source, &index, 99_999));
        assert!(
            source.reads.get() <= 2,
            "{} source reads",
            source.reads.get()
        );
    }

    #[test]
    fn local_separators_match_full_scan_after_fixed_seed_edits() {
        let mut seed = 0x9134_7182_u64;
        let tokens = [
            "",
            " ",
            "text",
            "> ",
            "> text",
            "```",
            "> ```",
            "$$",
            "# heading",
        ];
        let mut lines = vec![String::new(); 80];
        for _ in 0..200 {
            seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1);
            let row = (seed >> 32) as usize % lines.len();
            lines[row] = tokens[seed as usize % tokens.len()].to_owned();
            let projection = QuoteProjection::from_lines(&lines);
            let expected = compute_syntactic_separators(&lines);
            for (row, expected) in expected.into_iter().enumerate() {
                assert_eq!(projection.separator(row), expected, "{lines:?}, {row}");
            }
        }
    }

    #[test]
    fn source_projection_reads_rows_without_storing_normalized_strings() {
        let mut buffer = TextBuffer::from_str("before\n```\n> literal\n```\n> quoted", None);
        let projection = QuoteProjection::from_source(&buffer);
        assert_eq!(projection.normalized_line(2), "> literal");
        assert_eq!(projection.normalized_line(4), "quoted");
        assert_eq!(projection.quote_prefix(2), None);
        assert_eq!(projection.quote_prefix(4), Some(2));
        buffer.record_single_line_edit(SourceLine::new(3), |line, _| line.clear());
        let edited = QuoteProjection::from_source(&buffer);
        assert_eq!(edited.normalized_line(2), "> literal");
        assert_eq!(projection.normalized_line(2), "> literal");
        assert_eq!(edited.normalized_line(4), "> quoted");
        for row in 0..edited.len() {
            let line = SourceLine::new(row);
            assert_eq!(
                math_block_range_with_projection(&edited, line),
                math_block_range_normalized(&edited.normalized_lines(), line)
            );
        }
    }

    #[test]
    fn projection_updates_empty_document_logical_rows() {
        let mut buffer = TextBuffer::new();
        let mut projection = QuoteProjection::from_source(&buffer);
        buffer.set_cursor(crate::buffer::Position::new(SourceLine::new(1), 0));
        buffer.insert_text("text");
        assert!(projection.try_update_source(&buffer, buffer.last_edit_delta().unwrap()));
        assert_eq!(projection.normalized_line(1), "text");
    }

    #[test]
    fn indexed_windows_match_whole_document_at_every_block_boundary() {
        for text in [
            "before\nplain\nafter\n",
            "before\n```rust\nlet a = 1;\n> literal\n```\nafter",
            "before\n> ```rust\n> let a = 1;\n> ```\n> tail\nafter",
            "before\n$$\nx + y\n$$\nafter",
            "before\n> $$\n> x + y\n> $$\nafter",
            "before\n| a | b |\n| --- | --- |\n| c | d |\n| e | f |\nafter",
            "before\n> | a | b |\n> | --- | --- |\n> | c | d |\nafter",
            "before\n```mermaid\ngraph TD\nA --> B\n```\nafter",
            "before\n> outer\n> > inner\n> \n> tail\nafter",
            "before\n```\n> unmatched\n$$\nx\n$$",
            "before\n$$\n```\nx\n$$\n```\n$$\nafter",
        ] {
            let buffer = TextBuffer::from_str(text, None);
            let whole = MarkdownScanner::scan_document(&buffer.lines());
            for start in 0..buffer.line_count() {
                for width in [1, 2, 4] {
                    let requested = start..(start + width).min(buffer.line_count());
                    let (range, parsed) = MarkdownScanner::scan_window(&buffer, requested.clone());
                    assert!(range.start <= requested.start && range.end >= requested.end);
                    assert_eq!(
                        parsed,
                        whole[range.clone()],
                        "{text:?}, {requested:?} => {range:?}"
                    );
                }
            }
        }
    }

    #[test]
    fn ordinary_paragraph_window_does_not_expand_to_document_edges() {
        let buffer = TextBuffer::from_str(&"paragraph\n".repeat(20_000), None);
        let (range, parsed) = MarkdownScanner::scan_window(&buffer, 10_000..10_064);
        assert_eq!(range, 10_000..10_064);
        assert_eq!(parsed.len(), 64);
    }

    #[test]
    fn indexed_windows_match_fixed_seed_mixed_documents() {
        let mut seed = 0x7193_4a62_u64;
        let tokens = [
            "text",
            "",
            "```",
            "```rust",
            "$$",
            "x",
            "> text",
            "> ```",
            "> $$",
            "| a | b |",
            "| --- | --- |",
            "# title",
            "- item",
            "---",
        ];
        for _ in 0..40 {
            let lines: Vec<String> = (0..30)
                .map(|_| {
                    seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1);
                    tokens[(seed >> 32) as usize % tokens.len()].to_owned()
                })
                .collect();
            let buffer = TextBuffer::from_str(&lines.join("\n"), None);
            let whole = MarkdownScanner::scan_document(&lines);
            for row in 0..lines.len() {
                let (range, parsed) = MarkdownScanner::scan_window(&buffer, row..row + 1);
                for (offset, actual) in parsed.iter().enumerate() {
                    assert!(
                        actual == &whole[range.start + offset],
                        "source {lines:?}, request {row}, window {range:?}, mismatch {}: {:?} vs {:?}",
                        range.start + offset,
                        actual,
                        whole[range.start + offset]
                    );
                }
            }
        }
    }
}

impl MarkdownScanner {
    pub fn scan_window(
        buffer: &crate::buffer::TextBuffer,
        range: std::ops::Range<usize>,
    ) -> (std::ops::Range<usize>, Vec<ParsedLine>) {
        let index = buffer.line_index().structure_snapshot();
        window::scan(buffer, &index, range)
    }

    pub(crate) fn scan_snapshot_window(
        source: &crate::buffer::SourceSnapshot,
        index: &crate::buffer::StructureIndex,
        range: std::ops::Range<usize>,
    ) -> (std::ops::Range<usize>, Vec<ParsedLine>) {
        window::scan(source, index, range)
    }

    #[cfg(test)]
    pub fn scan_document(lines: &[String]) -> Vec<ParsedLine> {
        let projection = QuoteProjection::from_lines(lines);
        Self::scan_document_with_projection(lines, &projection)
    }

    #[cfg(test)]
    pub(crate) fn scan_document_with_projection(
        lines: &(impl LineSource + ?Sized),
        projection: &QuoteProjection,
    ) -> Vec<ParsedLine> {
        Self::scan_range_with_projection_with_state(
            lines,
            projection,
            0..lines.line_count(),
            false,
            false,
        )
    }

    #[cfg(test)]
    pub fn scan_range(
        lines: &[String],
        range: std::ops::Range<usize>,
        initial_in_code_fence: bool,
    ) -> Vec<ParsedLine> {
        let projection = QuoteProjection::from_lines(lines);
        let initial_in_math_block = if initial_in_code_fence {
            false
        } else {
            math_block_state_before(&projection, range.start)
        };
        Self::scan_range_with_projection_with_state(
            lines,
            &projection,
            range,
            initial_in_code_fence,
            initial_in_math_block,
        )
    }

    #[cfg(test)]
    pub(crate) fn scan_range_with_projection_with_state(
        lines: &(impl LineSource + ?Sized),
        projection: &QuoteProjection,
        range: std::ops::Range<usize>,
        initial_in_code_fence: bool,
        initial_in_math_block: bool,
    ) -> Vec<ParsedLine> {
        let normalized: Vec<String> = (0..projection.len())
            .map(|row| projection.normalized_line(row).into_owned())
            .collect();
        let prefixes: Vec<Option<usize>> = (0..projection.len())
            .map(|row| projection.quote_prefix(row))
            .collect();
        Self::scan_normalized_range_with_state(
            lines,
            &normalized,
            &prefixes,
            range,
            initial_in_code_fence,
            initial_in_math_block,
        )
    }

    fn scan_normalized_range_with_state(
        source_lines: &(impl LineSource + ?Sized),
        lines: &[String],
        quote_prefixes: &[Option<usize>],
        range: std::ops::Range<usize>,
        initial_in_code_fence: bool,
        initial_in_math_block: bool,
    ) -> Vec<ParsedLine> {
        let mut result = Vec::with_capacity(range.len());
        let mut in_code_fence = initial_in_code_fence;
        let mut in_math_block = initial_in_math_block;
        let mut current_table_alignments: Option<Vec<TableAlignment>> = None;
        let mut next_closing_fence: Option<usize> = None;
        let mut no_more_closing_fences = false;
        let (mut current_code_block_highlights, mut current_mermaid_preview) =
            if initial_in_code_fence && range.start < lines.len() {
                let open_fence_idx = (0..range.start).rev().find(|&i| is_fence_line(&lines[i]));
                let close_fence_idx =
                    find_next_fence_from(lines, range.start).unwrap_or(lines.len());
                let preview = open_fence_idx
                    .filter(|&open_idx| is_mermaid_fence(&lines[open_idx]))
                    .filter(|_| close_fence_idx < lines.len())
                    .and_then(|open_idx| {
                        crate::mermaid::prepare_mermaid_lines(&lines[open_idx + 1..close_fence_idx])
                    });
                (
                    Some(highlight_fenced_slice(
                        lines,
                        open_fence_idx,
                        close_fence_idx,
                    )),
                    preview,
                )
            } else {
                (None, None)
            };

        for idx in range.clone() {
            let raw_line = &lines[idx];
            let trimmed = raw_line.trim_start();
            let leading_spaces = raw_line.len() - trimmed.len();

            if in_math_block {
                current_table_alignments = None;
                let is_close = is_math_delimiter(raw_line);
                result.push(ParsedLine {
                    raw_text: raw_line.clone(),
                    kind: if is_close {
                        in_math_block = false;
                        BlockKind::MathBlock { is_start: false }
                    } else {
                        BlockKind::MathBlockContent
                    },
                    spans: vec![InlineSpan::plain(
                        raw_line.clone(),
                        (0, raw_line.chars().count()),
                    )],
                    quote: None,
                });
                continue;
            }

            // Code fence blocks (```)
            if let Some(stripped) = trimmed.strip_prefix("```") {
                if !in_code_fence {
                    if !no_more_closing_fences
                        && (next_closing_fence.is_none() || next_closing_fence.unwrap() <= idx)
                    {
                        next_closing_fence = find_next_fence_from(lines, idx + 1);
                        if next_closing_fence.is_none() {
                            no_more_closing_fences = true;
                        }
                    }

                    {
                        let close_idx = next_closing_fence.unwrap_or(lines.len());
                        in_code_fence = true;
                        let lang_str = stripped.trim().to_string();
                        let mermaid_preview = (is_mermaid_fence(raw_line)
                            && close_idx < lines.len())
                        .then(|| crate::mermaid::prepare_mermaid_lines(&lines[idx + 1..close_idx]))
                        .flatten();
                        let is_mermaid = mermaid_preview.is_some();
                        current_mermaid_preview = mermaid_preview.clone();
                        current_code_block_highlights = (close_idx < lines.len())
                            .then(|| highlight_fenced_slice(lines, Some(idx), close_idx));

                        let mut spans = Vec::new();
                        // Opening backticks are hidden in Live Preview mode
                        spans.push(InlineSpan::hidden_marker("```", (0, 3), (0, 3)));
                        if !lang_str.is_empty() {
                            let actual_lang_col = leading_spaces + 3;
                            spans.push(InlineSpan::plain(
                                raw_line[actual_lang_col..].to_string(),
                                (actual_lang_col, raw_line.chars().count()),
                            ));
                        }
                        if let Some(rendered) = mermaid_preview {
                            spans.push(InlineSpan::mermaid_diagram(rendered));
                        }
                        result.push(ParsedLine {
                            raw_text: raw_line.clone(),
                            kind: if is_mermaid {
                                BlockKind::MermaidBlock {
                                    is_fence_start: true,
                                }
                            } else {
                                BlockKind::CodeBlock {
                                    is_fence_start: true,
                                }
                            },
                            spans,
                            quote: None,
                        });
                        continue;
                    }
                } else {
                    in_code_fence = false;
                    current_code_block_highlights = None;
                    let is_mermaid = current_mermaid_preview.take().is_some();
                    let char_count = raw_line.chars().count();
                    result.push(ParsedLine {
                        raw_text: raw_line.clone(),
                        kind: if is_mermaid {
                            BlockKind::MermaidBlock {
                                is_fence_start: false,
                            }
                        } else {
                            BlockKind::CodeBlock {
                                is_fence_start: false,
                            }
                        },
                        spans: vec![InlineSpan::hidden_marker(
                            raw_line.clone(),
                            (0, char_count),
                            (0, char_count),
                        )],
                        quote: None,
                    });
                    continue;
                }
            }

            if in_code_fence {
                let spans = current_code_block_highlights
                    .as_ref()
                    .and_then(|(content_start, block_spans)| {
                        idx.checked_sub(*content_start)
                            .and_then(|rel_idx| block_spans.get(rel_idx).cloned())
                    })
                    .unwrap_or_else(|| {
                        let char_count = raw_line.chars().count();
                        vec![InlineSpan::plain(raw_line.clone(), (0, char_count))]
                    });
                result.push(ParsedLine {
                    raw_text: raw_line.clone(),
                    kind: if current_mermaid_preview.is_some() {
                        BlockKind::MermaidBlockContent
                    } else {
                        BlockKind::CodeBlockContent
                    },
                    spans,
                    quote: None,
                });
                continue;
            }

            if is_math_delimiter(raw_line)
                && let Some(close_idx) = find_closing_math_delimiter(lines, idx + 1)
            {
                let tex = lines[idx + 1..close_idx].join("\n");
                current_table_alignments = None;
                in_math_block = true;
                let open_start = leading_spaces;
                let open_end = open_start + 2;
                let mut spans = Vec::new();
                if open_start > 0 {
                    spans.push(InlineSpan::plain(
                        raw_line[..open_start].to_string(),
                        (0, open_start),
                    ));
                }
                if let Some(rendered) = crate::math::prepare_latex(&tex, true) {
                    spans.push(InlineSpan::math(
                        "$$",
                        tex,
                        (open_start, open_end),
                        rendered,
                        true,
                    ));
                } else {
                    spans.push(InlineSpan::hidden_marker(
                        "$$",
                        (open_start, open_end),
                        (open_start, open_end),
                    ));
                }
                if raw_line.len() > open_end {
                    spans.push(InlineSpan::plain(
                        raw_line[open_end..].to_string(),
                        (open_end, raw_line.chars().count()),
                    ));
                }
                result.push(ParsedLine {
                    raw_text: raw_line.clone(),
                    kind: BlockKind::MathBlock { is_start: true },
                    spans,
                    quote: None,
                });
                continue;
            }

            // Blank lines
            if trimmed.is_empty() {
                current_table_alignments = None;
                result.push(ParsedLine {
                    raw_text: raw_line.clone(),
                    kind: BlockKind::Paragraph,
                    spans: vec![InlineSpan::plain(raw_line.clone(), (0, raw_line.len()))],
                    quote: None,
                });
                continue;
            }

            // Thematic break (--- / *** / ___)
            if is_thematic_break(trimmed) {
                let char_count = raw_line.chars().count();
                result.push(ParsedLine {
                    raw_text: raw_line.clone(),
                    kind: BlockKind::ThematicBreak,
                    spans: vec![InlineSpan::hidden_marker(
                        raw_line.clone(),
                        (0, char_count),
                        (0, char_count),
                    )],
                    quote: None,
                });
                continue;
            }

            if let Some(prefix) = parse_block_prefix(raw_line) {
                let kind = match prefix.kind {
                    BlockPrefix::Heading { level } => Some(BlockKind::Heading { level }),
                    BlockPrefix::Task { checked, .. } => Some(BlockKind::TaskList { checked }),
                    BlockPrefix::Bullet { .. } => Some(BlockKind::BulletList),
                    BlockPrefix::Ordered { num } => Some(BlockKind::OrderedList { num }),
                    BlockPrefix::Quote => None,
                    BlockPrefix::Fence { .. } => None,
                };

                if let Some(kind) = kind {
                    let actual_prefix_len = prefix.total_len;
                    let content = &raw_line[actual_prefix_len..];
                    let prefix_str = &raw_line[..actual_prefix_len];

                    let mut marker_span = InlineSpan::hidden_marker(
                        prefix_str,
                        (0, actual_prefix_len),
                        (0, actual_prefix_len),
                    );
                    if matches!(kind, BlockKind::Heading { .. }) {
                        marker_span.is_bold = true;
                    }
                    let mut inline_spans = vec![marker_span];
                    inline_spans.extend(parse_inline_spans(content, actual_prefix_len));

                    result.push(ParsedLine {
                        raw_text: raw_line.clone(),
                        kind,
                        spans: inline_spans,
                        quote: None,
                    });
                    continue;
                }
            }

            // Standalone image (![alt](url))
            if trimmed.starts_with("![")
                && trimmed.ends_with(')')
                && let Some(bracket_end) = trimmed.find("](")
            {
                current_table_alignments = None;
                let alt = trimmed[2..bracket_end].to_string();
                let url = trimmed[(bracket_end + 2)..(trimmed.len() - 1)].to_string();
                let inline_spans = parse_inline_spans(raw_line, 0);
                result.push(ParsedLine {
                    raw_text: raw_line.clone(),
                    kind: BlockKind::Image { alt, url },
                    spans: inline_spans,
                    quote: None,
                });
                continue;
            }

            // Table row or delimiter (GFM Table)
            if let Some(alignments) = &current_table_alignments
                && trimmed.contains('|')
            {
                let cells = parse_table_row_cells(raw_line);
                let inline_spans = parse_inline_spans(raw_line, 0);
                result.push(ParsedLine {
                    raw_text: raw_line.clone(),
                    kind: BlockKind::Table {
                        is_header: false,
                        is_delimiter: is_table_delimiter_row(trimmed),
                        alignments: alignments.clone(),
                        cells,
                    },
                    spans: inline_spans,
                    quote: None,
                });
                continue;
            }

            if trimmed.contains('|')
                && idx + 1 < lines.len()
                && is_table_delimiter_row(&lines[idx + 1])
            {
                let alignments = parse_table_alignments(&lines[idx + 1]);
                let cells = parse_table_row_cells(raw_line);
                let inline_spans = parse_inline_spans(raw_line, 0);
                current_table_alignments = Some(alignments.clone());
                result.push(ParsedLine {
                    raw_text: raw_line.clone(),
                    kind: BlockKind::Table {
                        is_header: true,
                        is_delimiter: false,
                        alignments,
                        cells,
                    },
                    spans: inline_spans,
                    quote: None,
                });
                continue;
            }

            current_table_alignments = None;
            let inline_spans = parse_inline_spans(raw_line, 0);
            result.push(ParsedLine {
                raw_text: raw_line.clone(),
                kind: BlockKind::Paragraph,
                spans: inline_spans,
                quote: None,
            });
        }

        for (parsed, source_idx) in result.iter_mut().zip(range) {
            if let Some(prefix_bytes) = quote_prefixes.get(source_idx).copied().flatten() {
                let source = source_lines.source_line(SourceLine::new(source_idx));
                let prefix_chars = source[..prefix_bytes].chars().count();
                parsed.quote = Some(QuoteContext {
                    depth: quote_depth(&source),
                    content_indent: leading_indent_len(&lines[source_idx]),
                    is_start: source_idx == 0 || quote_prefixes[source_idx - 1].is_none(),
                    is_end: source_idx + 1 == source_lines.line_count()
                        || quote_prefixes[source_idx + 1].is_none(),
                });
                shift_spans(&mut parsed.spans, prefix_chars);
                parsed.spans.insert(
                    0,
                    InlineSpan::hidden_marker(
                        source[..prefix_bytes].to_string(),
                        (0, prefix_chars),
                        (0, prefix_chars),
                    ),
                );
                parsed.raw_text = source.into_owned();
            }
        }

        result
    }
}

fn step_separator_state(
    index: usize,
    raw_line: &str,
    state: &mut crate::buffer::BlockState,
    previous_nonempty: &mut bool,
    previous_separator: &mut bool,
) -> bool {
    let raw = super::structure::classify(raw_line);
    let projected = super::structure::classify(&raw_line[raw.quote_prefix..]);
    let is_blank = if state.is_raw_code() {
        raw.is_blank
    } else {
        projected.is_blank
    };
    *state = crate::buffer::PrefixSummary::line(raw, projected).0[*state as usize].exit;
    let is_sep = is_separator_line(
        index,
        is_blank,
        state.is_code(),
        *previous_nonempty,
        *previous_separator,
    );
    *previous_separator = is_sep;
    *previous_nonempty = !is_blank;
    is_sep
}

#[cfg(test)]
pub fn compute_syntactic_separators(lines: &(impl LineSource + ?Sized)) -> Vec<bool> {
    let mut separators = vec![false; lines.line_count()];
    if lines.line_count() == 0 {
        return separators;
    }

    let mut state = crate::buffer::BlockState::Outside;
    let mut previous_nonempty = false;
    let mut prev_is_separator = false;

    for (i, separator) in separators.iter_mut().enumerate() {
        let raw_line = lines.source_line(SourceLine::new(i));
        *separator = step_separator_state(
            i,
            &raw_line,
            &mut state,
            &mut previous_nonempty,
            &mut prev_is_separator,
        );
    }

    separators
}

/// Returns whether the blank run directly before `block_line` ends with a
/// syntactic separator according to the source projection's alternating rule.
pub(crate) fn blank_run_before_ends_with_separator(
    lines: &(impl LineSource + ?Sized),
    block_line: SourceLine,
) -> bool {
    let mut run_start = block_line.index();
    while run_start > 0 {
        let line = lines.source_line(SourceLine::new(run_start - 1));
        if !super::indent::strip_quote_prefix(&line).trim().is_empty() {
            break;
        }
        run_start -= 1;
    }

    let mut previous_nonempty = run_start > 0;
    let mut previous_separator = false;
    for index in run_start..block_line.index() {
        previous_separator =
            is_separator_line(index, true, false, previous_nonempty, previous_separator);
        previous_nonempty = false;
    }
    previous_separator
}

fn is_separator_line(
    index: usize,
    is_blank: bool,
    in_fence: bool,
    previous_nonempty: bool,
    previous_separator: bool,
) -> bool {
    index != 0 && is_blank && !in_fence && (previous_nonempty || !previous_separator)
}

pub fn is_syntactic_separator(lines: &(impl LineSource + ?Sized), source_line: SourceLine) -> bool {
    let idx = source_line.index();
    if idx == 0 || idx >= lines.line_count() {
        return false;
    }

    let mut state = crate::buffer::BlockState::Outside;
    let mut previous_nonempty = false;
    let mut prev_is_separator = false;

    for i in 0..=idx {
        let raw_line = lines.source_line(SourceLine::new(i));
        step_separator_state(
            i,
            &raw_line,
            &mut state,
            &mut previous_nonempty,
            &mut prev_is_separator,
        );
    }

    prev_is_separator
}

pub(crate) fn local_separator(
    source: &(impl LineSource + ?Sized),
    index: &crate::buffer::StructureIndex,
    row: usize,
) -> bool {
    if row == 0 || row >= source.line_count() {
        return false;
    }
    let Some(range) = index.blank_range(row) else {
        return false;
    };
    if index.is_in_code_block_after(SourceLine::new(row)) {
        return false;
    }
    let start = range.start;
    if start == 0 {
        row % 2 == 1
    } else {
        (row - start) % 2 == 0
    }
}
