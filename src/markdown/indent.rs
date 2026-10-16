//! Markdown-aware indentation semantics.
//!
//! Nesting a list item under its parent requires indenting to the parent's
//! *content start column* (leading spaces + marker width) per CommonMark.
//! A fixed tab width cannot guarantee valid nesting — e.g. an `1. ` parent
//! needs 3 columns, so a 2-space tab would silently keep the item a loose
//! sibling. Indentation targets are therefore derived from the surrounding
//! lines, falling back to a plain `tab_size` step when no list parent exists.
//!
//! All columns are character columns. A list line's indent region starts at
//! its marker column (`  - x` → 2), looking through one level of blockquote
//! marker (`> - x` → 2). Plain lines indent at column 0.

use super::ParsedLine;
use super::prefix::{BlockPrefix, parse_block_prefix, parse_quote_prefix};

/// Count of leading ASCII indent characters (`' '` or `'\t'`).
/// Because both characters are single-byte ASCII, byte count equals character column count.
#[inline]
pub fn leading_indent_len(s: &str) -> usize {
    s.bytes().take_while(|&b| b == b' ' || b == b'\t').count()
}

/// Character columns of leading whitespace that Live Preview hides as part of a
/// block's syntax prefix (list markers, blockquote markers, heading hashes).
/// These columns are re-materialized as a layout offset so source nesting
/// stays visible in the rendered document.
#[inline]
pub fn line_visual_indent_chars(parsed: &ParsedLine) -> usize {
    if parsed.kind.has_syntax_prefix() {
        leading_indent_len(&parsed.raw_text)
    } else {
        0
    }
}

/// Width of a leading blockquote marker (`> `), including any leading spaces.
/// Returns 0 when the line does not start with a blockquote marker.
pub fn quote_prefix_len(line: &str) -> usize {
    let leading = leading_indent_len(line);
    parse_quote_prefix(&line[leading..]).map_or(0, |quote_len| leading + quote_len)
}

/// Parses `line` as a list item, looking through one level of blockquote marker.
/// Returns `(marker_col, content_start)`: the column where the list marker
/// starts and the column where its content begins (the child indent unit).
pub fn list_marker(line: &str) -> Option<(usize, usize)> {
    let quote_len = quote_prefix_len(line);
    let prefix = parse_block_prefix(&line[quote_len..])?;
    let marker_col = quote_len + prefix.leading_spaces;
    match prefix.kind {
        BlockPrefix::Bullet { .. } | BlockPrefix::Task { .. } => Some((marker_col, marker_col + 2)),
        BlockPrefix::Ordered { .. } => Some((
            marker_col,
            marker_col + (prefix.total_len - prefix.leading_spaces),
        )),
        _ => None,
    }
}

/// Column where this line's indent region begins: the list marker column for
/// list items, 0 for everything else.
pub fn indent_col(line: &str) -> usize {
    list_marker(line).map_or(0, |(marker_col, _)| marker_col)
}

/// Current indent level of the line relative to its container (after quote prefix):
/// marker column for list items, leading-whitespace count for plain lines.
pub fn current_indent_relative(line: &str) -> usize {
    let quote_len = quote_prefix_len(line);
    list_marker(line).map_or_else(
        || leading_indent_len(&line[quote_len..]),
        |(marker_col, _)| marker_col.saturating_sub(quote_len),
    )
}

/// Absolute indent level (including quote prefix).
fn current_indent(line: &str) -> usize {
    quote_prefix_len(line) + current_indent_relative(line)
}

/// Nearest list item above `line_idx` in the same container (matching blockquote
/// prefix) whose marker column is strictly less than `max_marker_col` (when given).
/// Blank lines are skipped so loose lists still nest.
fn find_list_ancestor(
    lines: &[String],
    line_idx: usize,
    max_marker_col: Option<usize>,
) -> Option<(usize, usize)> {
    let quote_len = quote_prefix_len(lines.get(line_idx)?);
    for idx in (0..line_idx).rev() {
        let candidate = &lines[idx];
        if candidate.trim().is_empty() || quote_prefix_len(candidate) != quote_len {
            continue;
        }
        if let Some((parent_col, content_start)) = list_marker(candidate)
            && max_marker_col.is_none_or(|max_col| parent_col < max_col)
        {
            return Some((parent_col, content_start));
        }
    }
    None
}

/// Target indent column for Tab on `lines[line_idx]`: steps to the next
/// `tab_size` stop, while ensuring it reaches at least the parent's content
/// start column (e.g. an `1. ` parent needs 3 columns even when `tab_size` is 2).
fn next_indent_target(lines: &[String], line_idx: usize, tab_size: usize) -> usize {
    let tab_size = tab_size.max(1);
    let line = &lines[line_idx];
    let quote_len = quote_prefix_len(line);
    let current_rel = current_indent_relative(line);
    let target_rel = (current_rel / tab_size + 1) * tab_size;

    let final_target_rel = match find_list_ancestor(lines, line_idx, None) {
        Some((_, content_start)) => target_rel.max(content_start.saturating_sub(quote_len)),
        None => target_rel,
    };

    quote_len + final_target_rel
}

/// Target indent column for Shift+Tab on `lines[line_idx]`: steps back by
/// `tab_size` (or to previous tab stop), or aligns to the nearest enclosing
/// list parent level when closer. Non-list lines step back by `tab_size`.
fn prev_indent_target(lines: &[String], line_idx: usize, tab_size: usize) -> usize {
    let tab_size = tab_size.max(1);
    let line = &lines[line_idx];
    let quote_len = quote_prefix_len(line);
    let current_rel = current_indent_relative(line);
    let default_rel = if current_rel.is_multiple_of(tab_size) {
        current_rel.saturating_sub(tab_size)
    } else {
        (current_rel / tab_size) * tab_size
    };
    let Some((marker_col, _)) = list_marker(line) else {
        return quote_len + default_rel;
    };
    let marker_rel = marker_col.saturating_sub(quote_len);
    if let Some((parent_col, _)) = find_list_ancestor(lines, line_idx, Some(marker_col)) {
        let parent_rel = parent_col.saturating_sub(quote_len);
        if parent_rel >= default_rel || marker_rel - parent_rel <= tab_size.max(3) {
            return quote_len + parent_rel;
        }
    }
    quote_len + default_rel
}

/// Tab plan for `lines[line_idx]`: `(col, insert_len)` — insert `insert_len`
/// spaces at character column `col`.
pub fn tab_plan(lines: &[String], line_idx: usize, tab_size: usize) -> (usize, usize) {
    let tab_size = tab_size.max(1);
    let line = &lines[line_idx];
    let col = indent_col(line);
    let target = next_indent_target(lines, line_idx, tab_size);
    (col, target.saturating_sub(current_indent(line)))
}

/// Shift+Tab plan for `lines[line_idx]`: `(col, remove_len)` — remove
/// `remove_len` characters starting at character column `col`.
pub fn shift_tab_plan(lines: &[String], line_idx: usize, tab_size: usize) -> (usize, usize) {
    let tab_size = tab_size.max(1);
    let line = &lines[line_idx];
    let target = prev_indent_target(lines, line_idx, tab_size);
    (target, current_indent(line).saturating_sub(target))
}

/// Detects the dominant indentation style in a code block by analyzing
/// existing indentation patterns. Returns 2 for 2-space indent, 4 for
/// 4-space indent, or the default (4) if no clear pattern is detected.
///
/// Scans up to 50 lines looking for leading spaces that are multiples of
/// 2 or 4. The style with more occurrences wins. Empty lines and lines
/// without indentation are ignored.
pub fn detect_code_block_indent(lines: &[String], start_line: usize, end_line: usize) -> usize {
    let mut indent_2_count = 0;
    let mut indent_4_count = 0;
    let scan_limit = 50;
    let actual_end = end_line.min(start_line + scan_limit);

    for line in lines.iter().skip(start_line).take(actual_end - start_line) {
        let spaces = leading_indent_len(line);
        if spaces == 0 || line.trim().is_empty() {
            continue;
        }

        // Count as 2-space if it's an odd multiple of 2 (2, 6, 10, ...)
        // or an even multiple that could be either (4, 8, 12, ...)
        if spaces % 2 == 0 && spaces % 4 != 0 {
            indent_2_count += 1;
        } else if spaces % 4 == 0 && spaces > 0 {
            // Multiples of 4 are ambiguous, but favor 4-space slightly
            indent_4_count += 1;
        } else if spaces % 2 == 0 {
            // Catch-all for even spacing
            indent_2_count += 1;
        }
    }

    // Require at least 3 samples to make a decision
    if indent_2_count + indent_4_count < 3 {
        return 4; // Default to 4-space
    }

    if indent_2_count > indent_4_count {
        2
    } else {
        4
    }
}
