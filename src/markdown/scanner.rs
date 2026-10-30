use super::inline::{InlineSpan, parse_inline_spans};
use super::prefix::{
    BlockPrefix, fence_info, is_fence_line, is_thematic_break, parse_block_prefix,
};
use super::table::{is_table_delimiter_row, parse_table_alignments, parse_table_row_cells};
use super::types::{BlockKind, ParsedLine, TableAlignment};
use crate::buffer::SourceLine;

fn find_next_fence_from(lines: &[String], from_idx: usize) -> Option<usize> {
    lines
        .get(from_idx..)?
        .iter()
        .position(|l| is_fence_line(l))
        .map(|p| p + from_idx)
}

fn is_math_delimiter(line: &str) -> bool {
    line.trim() == "$$"
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

pub(crate) fn math_block_pairs(lines: &[String]) -> Vec<(SourceLine, SourceLine)> {
    let mut pairs = Vec::new();
    let mut in_code_fence = false;
    let mut open_idx = None;

    for (idx, line) in lines.iter().enumerate() {
        if is_fence_line(line) {
            in_code_fence = !in_code_fence;
            continue;
        }
        if in_code_fence || !is_math_delimiter(line) {
            continue;
        }

        if let Some(open_idx) = open_idx.take() {
            pairs.push((SourceLine::new(open_idx), SourceLine::new(idx)));
        } else {
            open_idx = Some(idx);
        }
    }

    pairs
}

pub(crate) fn math_block_range(
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

fn math_block_state_before(lines: &[String], end_idx: usize) -> bool {
    math_block_range(lines, SourceLine::new(end_idx))
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

pub struct MarkdownScanner;

impl MarkdownScanner {
    pub fn scan_document(lines: &[String]) -> Vec<ParsedLine> {
        Self::scan_range(lines, 0..lines.len(), false)
    }

    pub fn scan_range(
        lines: &[String],
        range: std::ops::Range<usize>,
        initial_in_code_fence: bool,
    ) -> Vec<ParsedLine> {
        let initial_in_math_block = if initial_in_code_fence {
            false
        } else {
            math_block_state_before(lines, range.start)
        };
        Self::scan_range_with_state(lines, range, initial_in_code_fence, initial_in_math_block)
    }

    pub(crate) fn scan_range_with_state(
        lines: &[String],
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
        let mut current_code_block_highlights: Option<(usize, Vec<Vec<InlineSpan>>)> =
            if initial_in_code_fence && range.start < lines.len() {
                let open_fence_idx = (0..range.start).rev().find(|&i| is_fence_line(&lines[i]));
                let close_fence_idx =
                    find_next_fence_from(lines, range.start).unwrap_or(lines.len());
                Some(highlight_fenced_slice(
                    lines,
                    open_fence_idx,
                    close_fence_idx,
                ))
            } else {
                None
            };

        for idx in range {
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

                    if let Some(close_idx) = next_closing_fence {
                        in_code_fence = true;
                        let lang_str = stripped.trim().to_string();
                        current_code_block_highlights =
                            Some(highlight_fenced_slice(lines, Some(idx), close_idx));

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
                        result.push(ParsedLine {
                            raw_text: raw_line.clone(),
                            kind: BlockKind::CodeBlock {
                                is_fence_start: true,
                            },
                            spans,
                        });
                        continue;
                    }
                    // If there is no closing fence below, treat as normal paragraph text
                } else {
                    in_code_fence = false;
                    current_code_block_highlights = None;
                    let char_count = raw_line.chars().count();
                    result.push(ParsedLine {
                        raw_text: raw_line.clone(),
                        kind: BlockKind::CodeBlock {
                            is_fence_start: false,
                        },
                        spans: vec![InlineSpan::hidden_marker(
                            raw_line.clone(),
                            (0, char_count),
                            (0, char_count),
                        )],
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
                    kind: BlockKind::CodeBlockContent,
                    spans,
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
                });
                continue;
            }

            if let Some(prefix) = parse_block_prefix(raw_line) {
                let kind = match prefix.kind {
                    BlockPrefix::Heading { level } => Some(BlockKind::Heading { level }),
                    BlockPrefix::Task { checked, .. } => Some(BlockKind::TaskList { checked }),
                    BlockPrefix::Bullet { .. } => Some(BlockKind::BulletList),
                    BlockPrefix::Ordered { num } => Some(BlockKind::OrderedList { num }),
                    BlockPrefix::Quote => Some(BlockKind::BlockQuote),
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
                });
                continue;
            }

            current_table_alignments = None;
            let inline_spans = parse_inline_spans(raw_line, 0);
            result.push(ParsedLine {
                raw_text: raw_line.clone(),
                kind: BlockKind::Paragraph,
                spans: inline_spans,
            });
        }

        result
    }
}

pub fn compute_syntactic_separators(lines: &[String]) -> Vec<bool> {
    let mut separators = vec![false; lines.len()];
    if lines.is_empty() {
        return separators;
    }

    let mut in_fence = false;
    let mut prev_is_separator = false;

    for (i, line) in lines.iter().enumerate() {
        if is_fence_line(line) {
            in_fence = !in_fence;
        }

        let is_sep = if i == 0 || !line.trim().is_empty() || in_fence {
            false
        } else if !lines[i - 1].trim().is_empty() {
            true
        } else {
            !prev_is_separator
        };

        separators[i] = is_sep;
        prev_is_separator = is_sep;
    }

    separators
}

pub fn is_syntactic_separator(lines: &[String], source_line: SourceLine) -> bool {
    let idx = source_line.index();
    if idx == 0 || idx >= lines.len() || !lines[idx].trim().is_empty() {
        return false;
    }

    let mut in_fence = false;
    let mut prev_is_separator = false;

    for (i, line) in lines.iter().enumerate().take(idx + 1) {
        if is_fence_line(line) {
            in_fence = !in_fence;
        }

        prev_is_separator = if i == 0 || !line.trim().is_empty() || in_fence {
            false
        } else if !lines[i - 1].trim().is_empty() {
            true
        } else {
            !prev_is_separator
        };
    }

    prev_is_separator
}
