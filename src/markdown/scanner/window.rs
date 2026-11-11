use super::{MarkdownScanner, ParsedLine};
use crate::buffer::{LineSource, SourceLine, StructureIndex};
use std::ops::Range;

pub(super) fn scan(
    source: &(impl LineSource + ?Sized),
    index: &StructureIndex,
    requested: Range<usize>,
) -> (Range<usize>, Vec<ParsedLine>) {
    // Long and unfinished code blocks are consumed as rows, not expanded to their end.
    let bounded_code = |row: usize| {
        index
            .streaming_fence(SourceLine::new(row))
            .filter(|_| index.quote_prefix(SourceLine::new(row)) == 0)
            .filter(|(start, end)| end.is_none_or(|end| end - start > 1024))
            .filter(|(start, _)| {
                index
                    .parser_pair(SourceLine::new(row))
                    .is_some_and(|(open, _)| open.index() == *start)
            })
    };
    if requested.clone().any(|row| bounded_code(row).is_some()) {
        let mut rows = Vec::new();
        let mut row = requested.start;
        while row < requested.end {
            if let Some((start, end)) = bounded_code(row) {
                let stop = requested.end.min(end.map_or(requested.end, |end| end + 1));
                let mut raw = Vec::new();
                let prefix = usize::from(row > start);
                if prefix > 0 {
                    raw.push(source.source_line(SourceLine::new(start)).into_owned());
                }
                raw.extend(
                    (row..stop).map(|row| source.source_line(SourceLine::new(row)).into_owned()),
                );
                let parsed = MarkdownScanner::scan_normalized_range_with_state(
                    &raw,
                    &raw,
                    &vec![None; raw.len()],
                    0..raw.len(),
                    false,
                    false,
                );
                rows.extend(parsed.into_iter().skip(prefix));
                row = stop;
            } else {
                let start = row;
                while row < requested.end && bounded_code(row).is_none() {
                    row += 1;
                }
                let (range, parsed) = scan_complete(source, index, start..row);
                rows.extend(
                    parsed
                        .into_iter()
                        .skip(start - range.start)
                        .take(row - start),
                );
            }
        }
        return (requested, rows);
    }
    scan_complete(source, index, requested)
}

fn scan_complete(
    source: &(impl LineSource + ?Sized),
    index: &StructureIndex,
    requested: Range<usize>,
) -> (Range<usize>, Vec<ParsedLine>) {
    assert!(requested.start <= requested.end && requested.end <= source.line_count());
    if requested.is_empty() {
        return (requested, Vec::new());
    }
    let mut range = requested;
    loop {
        let old = range.clone();
        let mut row = old.start;
        while row < old.end {
            let line = SourceLine::new(row);
            if let Some((start, end)) = index.parser_pair(line) {
                range.start = range.start.min(start.index());
                range.end = range.end.max(end.index() + 1);
                let opening = source.source_line(start);
                let prefix = index.quote_prefix(start);
                if crate::markdown::structure::classify(&opening[prefix..]).is_math_delim
                    && let Some(lookahead) = index.math_lookahead_end(start.index() + 1)
                {
                    range.end = range.end.max(lookahead + 1);
                }
                row = end.index() + 1;
            } else {
                row += 1;
            }
        }
        // Tables and quote groups have local, contiguous source boundaries.
        while range.start > 0 && connected(source, index, range.start - 1, range.start) {
            range.start -= 1;
        }
        while range.end < source.line_count() && connected(source, index, range.end - 1, range.end)
        {
            range.end += 1;
        }
        if range == old {
            break;
        }
    }
    let raw: Vec<String> = range
        .clone()
        .map(|row| source.source_line(SourceLine::new(row)).into_owned())
        .collect();
    let prefixes: Vec<Option<usize>> = range
        .clone()
        .map(|row| {
            let prefix = index.quote_prefix(SourceLine::new(row));
            (prefix > 0).then_some(prefix)
        })
        .collect();
    let normalized: Vec<String> = raw
        .iter()
        .zip(&prefixes)
        .map(|(line, prefix)| line[prefix.unwrap_or(0)..].to_owned())
        .collect();
    let mut parsed = MarkdownScanner::scan_normalized_range_with_state(
        &raw,
        &normalized,
        &prefixes,
        0..raw.len(),
        false,
        false,
    );
    // Group edges use document coordinates even when a window starts at an edge.
    for (offset, line) in parsed.iter_mut().enumerate() {
        let row = range.start + offset;
        if let Some(quote) = line.quote.as_mut() {
            quote.is_start = row == 0 || index.quote_prefix(SourceLine::new(row - 1)) == 0;
            quote.is_end =
                row + 1 == source.line_count() || index.quote_prefix(SourceLine::new(row + 1)) == 0;
        }
    }
    (range, parsed)
}

fn connected(
    source: &(impl LineSource + ?Sized),
    index: &StructureIndex,
    left: usize,
    right: usize,
) -> bool {
    let left_prefix = index.quote_prefix(SourceLine::new(left));
    let right_prefix = index.quote_prefix(SourceLine::new(right));
    if left_prefix > 0 && right_prefix > 0 {
        return true;
    }
    let left = source.source_line(SourceLine::new(left));
    let right = source.source_line(SourceLine::new(right));
    left[left_prefix..].contains('|') && right[right_prefix..].contains('|')
}
