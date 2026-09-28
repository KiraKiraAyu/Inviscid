use super::inline::{InlineSpan, parse_inline_spans};
use super::prefix::{BlockPrefix, is_thematic_break, parse_block_prefix};
use super::table::{is_table_delimiter_row, parse_table_alignments, parse_table_row_cells};
use super::types::{BlockKind, ParsedLine, TableAlignment};

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
        let mut result = Vec::with_capacity(range.len());
        let mut in_code_fence = initial_in_code_fence;
        let mut current_table_alignments: Option<Vec<TableAlignment>> = None;
        let mut next_closing_fence: Option<usize> = None;
        let mut no_more_closing_fences = false;

        for idx in range {
            let raw_line = &lines[idx];
            let trimmed = raw_line.trim_start();
            let leading_spaces = raw_line.len() - trimmed.len();

            // Code fence blocks (```)
            if let Some(stripped) = trimmed.strip_prefix("```") {
                if !in_code_fence {
                    if !no_more_closing_fences
                        && (next_closing_fence.is_none() || next_closing_fence.unwrap() <= idx)
                    {
                        next_closing_fence = lines[(idx + 1)..]
                            .iter()
                            .position(|l| l.trim_start().starts_with("```"))
                            .map(|p| p + idx + 1);
                        if next_closing_fence.is_none() {
                            no_more_closing_fences = true;
                        }
                    }

                    if next_closing_fence.is_some() {
                        in_code_fence = true;
                        let lang_str = stripped.trim().to_string();

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
                let char_count = raw_line.chars().count();
                result.push(ParsedLine {
                    raw_text: raw_line.clone(),
                    kind: BlockKind::CodeBlockContent,
                    spans: vec![InlineSpan::plain(raw_line.clone(), (0, char_count))],
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
        if line.trim_start().starts_with("```") {
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

pub fn is_syntactic_separator(lines: &[String], idx: usize) -> bool {
    if idx == 0 || idx >= lines.len() || !lines[idx].trim().is_empty() {
        return false;
    }

    let mut in_fence = false;
    let mut prev_is_separator = false;

    for (i, line) in lines.iter().enumerate().take(idx + 1) {
        if line.trim_start().starts_with("```") {
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
