use super::inline::parse_inline_spans;
use super::prefix::is_fence_line;
use super::types::{TableAlignment, TableCell};

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

pub fn split_raw_cells(line: &str) -> Vec<&str> {
    let trimmed = line.trim();
    let content = trimmed.strip_prefix('|').unwrap_or(trimmed);
    let content = content.strip_suffix('|').unwrap_or(content);

    let mut cells = Vec::new();
    let mut last_idx = 0;
    let mut in_escape = false;

    for (idx, ch) in content.char_indices() {
        if in_escape {
            in_escape = false;
            continue;
        }
        if ch == '\\' {
            in_escape = true;
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
        } else {
            parse_inline_spans(cell_trimmed, cell_start_col)
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

pub fn get_cell_index_for_col(cells: &[TableCell], col: usize) -> usize {
    cells
        .iter()
        .position(|cell| col <= cell.col_range.1)
        .unwrap_or_else(|| cells.len().saturating_sub(1))
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TableMetadata {
    pub header_line: usize,
    pub delimiter_line: usize,
    pub last_table_line: usize,
    pub num_columns: usize,
    pub alignments: Vec<TableAlignment>,
}

pub fn get_table_metadata(lines: &[String], target_line: usize) -> Option<TableMetadata> {
    if target_line >= lines.len() {
        return None;
    }

    let mut header_idx = None;
    for cur in (0..=target_line).rev() {
        let trimmed = lines[cur].trim();
        if cur + 1 < lines.len()
            && is_table_delimiter_row(&lines[cur + 1])
            && lines[cur].contains('|')
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
    let alignments = parse_table_alignments(&lines[delimiter_line]);
    let num_columns = alignments.len().max(1);

    let mut last_table_line = delimiter_line;
    for (idx, line) in lines.iter().enumerate().skip(delimiter_line + 1) {
        let trimmed = line.trim();
        if trimmed.is_empty()
            || !trimmed.contains('|')
            || trimmed.starts_with('#')
            || is_fence_line(trimmed)
        {
            break;
        }
        last_table_line = idx;
    }

    if target_line > last_table_line {
        return None;
    }

    Some(TableMetadata {
        header_line,
        delimiter_line,
        last_table_line,
        num_columns,
        alignments,
    })
}

pub fn build_empty_table_row(num_columns: usize) -> String {
    format!("|{}", "  |".repeat(num_columns.max(1)))
}
