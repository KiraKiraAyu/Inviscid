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
    if parsed.quote.is_some() || parsed.kind.has_syntax_prefix() {
        leading_indent_len(&parsed.raw_text)
    } else {
        0
    }
}

/// Total width of consecutive blockquote markers (`> `), including indentation
/// between markers. Returns 0 when the line does not start with a blockquote.
fn quote_prefix_metrics(line: &str) -> (usize, usize) {
    let mut cursor = 0;
    let mut depth = 0;

    loop {
        let rest = &line[cursor..];
        let trimmed = rest.trim_start_matches([' ', '\t']);
        let leading = rest.len() - trimmed.len();
        let Some(prefix_len) = parse_quote_prefix(trimmed) else {
            break;
        };
        depth += 1;
        cursor += leading + prefix_len;
        if cursor >= line.len() {
            break;
        }
    }

    if depth == 0 { (0, 0) } else { (cursor, depth) }
}

pub fn quote_prefix_len(line: &str) -> usize {
    quote_prefix_metrics(line).0
}

/// Number of consecutive blockquote containers at the start of a line.
#[inline]
pub fn quote_depth(line: &str) -> usize {
    quote_prefix_metrics(line).1
}

/// Returns the line content after all consecutive leading blockquote markers.
///
/// The returned slice borrows `line`, so callers that only need to inspect
/// block structure can avoid allocating a normalized copy.
#[inline]
pub fn strip_quote_prefix(line: &str) -> &str {
    &line[quote_prefix_len(line)..]
}

/// Parses `line` as a list item, looking through consecutive blockquote markers.
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
