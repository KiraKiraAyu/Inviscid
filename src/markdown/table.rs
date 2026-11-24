use super::inline::{parse_inline_spans, parse_inline_spans_with_render};
use super::prefix::is_fence_line;
use super::types::{TableAlignment, TableCell};
use crate::buffer::{LineSource, SourceLine};

pub fn is_table_delimiter_row(line: &str) -> bool {
    let trimmed = line.trim();
    if !trimmed.contains('|') {
        return false;
    }

    let cells = split_raw_cells(trimmed);
    if cells.is_empty() {
        return false;
    }

    cells.iter().all(|cell| {
        let c = cell.trim();
        if c.is_empty() {
            return false;
        }
        let stripped = c.strip_prefix(':').unwrap_or(c);
        let stripped = stripped.strip_suffix(':').unwrap_or(stripped);
        !stripped.is_empty() && stripped.chars().all(|ch| ch == '-')
    })
}

pub fn parse_table_alignments(delimiter_line: &str) -> Vec<TableAlignment> {
    split_raw_cells(delimiter_line.trim())
        .into_iter()
        .map(|cell| {
            let c = cell.trim();
            match (c.starts_with(':'), c.ends_with(':')) {
                (true, true) => TableAlignment::Center,
                (true, false) => TableAlignment::Left,
                (false, true) => TableAlignment::Right,
                (false, false) => TableAlignment::None,
            }
        })
        .collect()
}

fn find_matching_backtick_span_end(content: &str, start: usize) -> Option<usize> {
    let tick_count = content[start..].bytes().take_while(|&b| b == b'`').count();
    if tick_count == 0 || tick_count >= 3 {
        return None;
    }
    let bytes = content.as_bytes();
    let mut j = start + tick_count;
    while j + tick_count <= bytes.len() {
        if bytes[j..(j + tick_count)].iter().all(|&b| b == b'`')
            && (j + tick_count == bytes.len() || bytes[j + tick_count] != b'`')
            && (j == 0 || bytes[j - 1] != b'`')
        {
            return Some(j + tick_count);
        }
        j += 1;
    }
    None
}

pub fn split_raw_cells(line: &str) -> Vec<&str> {
    let trimmed = line.trim();
    let content = trimmed.strip_prefix('|').unwrap_or(trimmed);
    let content = content.strip_suffix('|').unwrap_or(content);

    let mut cells = Vec::new();
    let mut last_idx = 0;
    let mut in_escape = false;
    let mut skip_until = 0;

    for (idx, ch) in content.char_indices() {
        if idx < skip_until {
            continue;
        }
        if in_escape {
            in_escape = false;
            continue;
        }
        if ch == '\\' {
            in_escape = true;
            continue;
        }
        if ch == '`'
            && let Some(end) = find_matching_backtick_span_end(content, idx)
        {
            skip_until = end;
            continue;
        }
        if ch == '|' {
            cells.push(&content[last_idx..idx]);
            last_idx = idx + 1;
        }
    }
    cells.push(&content[last_idx..]);
    cells
}

pub fn parse_table_row_cells(row_line: &str) -> Vec<TableCell> {
    parse_table_row_cells_with_render(row_line, true)
}

pub(crate) fn parse_table_row_cells_with_render(
    row_line: &str,
    allow_math_render: bool,
) -> Vec<TableCell> {
    let leading_spaces = row_line.chars().take_while(|c| c.is_whitespace()).count();
    let trimmed = row_line.trim();

    let content_start = if trimmed.starts_with('|') {
        leading_spaces + 1
    } else {
        leading_spaces
    };

    let cells_raw = split_raw_cells(trimmed);
    let mut result = Vec::with_capacity(cells_raw.len());
    let mut current_offset = content_start;

    for raw_cell in cells_raw {
        let cell_trimmed = raw_cell.trim();
        let cell_leading = raw_cell.chars().take_while(|c| c.is_whitespace()).count();
        let cell_start_col = current_offset + cell_leading;
        let cell_end_col = cell_start_col + cell_trimmed.chars().count();

        let spans = if cell_trimmed.is_empty() {
            Vec::new()
        } else if allow_math_render {
            parse_inline_spans(cell_trimmed, cell_start_col)
        } else {
            parse_inline_spans_with_render(cell_trimmed, cell_start_col, false)
        };

        result.push(TableCell {
            raw_content: cell_trimmed.to_string(),
            spans,
            col_range: (cell_start_col, cell_end_col),
        });

        current_offset += raw_cell.chars().count() + 1;
    }

    result
}

pub(crate) fn parse_source_table_row_cells(raw_line: &str) -> Vec<TableCell> {
    let quote_bytes = super::indent::quote_prefix_len(raw_line);
    if quote_bytes == 0 {
        return parse_table_row_cells(raw_line);
    }
    let quote_chars = raw_line[..quote_bytes].chars().count();
    let mut cells = parse_table_row_cells(&raw_line[quote_bytes..]);
    for cell in &mut cells {
        cell.col_range.0 += quote_chars;
        cell.col_range.1 += quote_chars;
        for span in &mut cell.spans {
            span.group_range.0 += quote_chars;
            span.group_range.1 += quote_chars;
            span.span_range.0 += quote_chars;
            span.span_range.1 += quote_chars;
        }
    }
    cells
}

pub fn get_cell_index_for_col(cells: &[TableCell], col: usize) -> usize {
    cells
        .iter()
        .position(|cell| col <= cell.col_range.1)
        .unwrap_or_else(|| cells.len().saturating_sub(1))
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TableMetadata {
    pub header_line: SourceLine,
    pub delimiter_line: SourceLine,
    pub last_table_line: SourceLine,
    pub num_columns: usize,
    pub alignments: Vec<TableAlignment>,
}

fn stripped_for_depth(line: &str, target_depth: usize) -> Option<&str> {
    if super::indent::quote_depth(line) != target_depth {
        return None;
    }
    Some(&line[super::indent::quote_prefix_len(line)..])
}

pub fn get_table_metadata(
    lines: &(impl LineSource + ?Sized),
    target_line: SourceLine,
) -> Option<TableMetadata> {
    let target_idx = target_line.index();
    let line_count = lines.line_count();
    if target_idx >= line_count {
        return None;
    }

    let target_raw = lines.source_line(target_line);
    let target_depth = super::indent::quote_depth(&target_raw);
    let target_inner = stripped_for_depth(&target_raw, target_depth)?;
    if !target_inner.contains('|') {
        return None;
    }

    let mut header_idx = None;
    for cur in (0..=target_idx).rev() {
        let cur_line = lines.source_line(SourceLine::new(cur));
        let Some(cur_inner) = stripped_for_depth(&cur_line, target_depth) else {
            break;
        };
        let trimmed = cur_inner.trim();
        if cur + 1 < line_count
            && cur_inner.contains('|')
            && stripped_for_depth(&lines.source_line(SourceLine::new(cur + 1)), target_depth)
                .is_some_and(is_table_delimiter_row)
        {
            header_idx = Some(cur);
            break;
        }

        if trimmed.is_empty() || trimmed.starts_with('#') || is_fence_line(trimmed) {
            break;
        }
    }

    let header_line = header_idx?;
    let delimiter_line = header_line + 1;
    let delim_raw = lines.source_line(SourceLine::new(delimiter_line));
    let delim_inner = stripped_for_depth(&delim_raw, target_depth)?;
    let alignments = parse_table_alignments(delim_inner);
    let num_columns = alignments.len().max(1);

    let mut last_table_line = delimiter_line;
    for idx in (delimiter_line + 1)..line_count {
        let line = lines.source_line(SourceLine::new(idx));
        let Some(inner) = stripped_for_depth(&line, target_depth) else {
            break;
        };
        let trimmed = inner.trim();
        if trimmed.is_empty()
            || !trimmed.contains('|')
            || trimmed.starts_with('#')
            || is_fence_line(trimmed)
        {
            break;
        }
        last_table_line = idx;
    }

    if target_idx > last_table_line {
        return None;
    }

    Some(TableMetadata {
        header_line: SourceLine::new(header_line),
        delimiter_line: SourceLine::new(delimiter_line),
        last_table_line: SourceLine::new(last_table_line),
        num_columns,
        alignments,
    })
}

pub fn build_empty_table_row(num_columns: usize) -> String {
    format!("|{}", "  |".repeat(num_columns.max(1)))
}
