use super::{MarkdownScanner, ParsedLine};
use crate::buffer::{LineSource, SourceLine, StructureIndex};
use std::ops::Range;

const CODE_HIGHLIGHT_CONTEXT_LINES: usize = 32;
const MAX_PREVIEW_BLOCK_EXPANSION: usize = 1024;

pub(super) fn scan(
    source: &(impl LineSource + ?Sized),
    index: &StructureIndex,
    requested: Range<usize>,
) -> (Range<usize>, Vec<ParsedLine>) {
    assert!(requested.start <= requested.end && requested.end <= source.line_count());
    if requested.is_empty() {
        return (requested, Vec::new());
    }
    let bounded_code = |row: usize| -> Option<(usize, Option<usize>)> {
        let (start, end) = index.fence_range(SourceLine::new(row))?;
        let is_small_mermaid = end.is_some_and(|close| {
            close.index() - start.index() <= MAX_PREVIEW_BLOCK_EXPANSION && {
                let line = source.source_line(start);
                let prefix = index.quote_prefix(start);
                super::is_mermaid_fence(&line[prefix..])
            }
        });
        (!is_small_mermaid).then_some((start.index(), end.map(|e| e.index())))
    };
    if requested.clone().any(|row| bounded_code(row).is_some()) {
        let mut rows = Vec::new();
        let mut row = requested.start;
        while row < requested.end {
            if let Some((open, close)) = bounded_code(row) {
                let stop = requested
                    .end
                    .min(close.map_or(requested.end, |end| end + 1));
                let mut parsed = if row == open {
                    scan_normalized(source, index, row..stop, false)
                } else {
                    let ctx_start = row
                        .saturating_sub(CODE_HIGHLIGHT_CONTEXT_LINES)
                        .max(open + 1);
                    let skip = 1 + (row - ctx_start);
                    let indices = std::iter::once(open).chain(ctx_start..stop);
                    scan_normalized(source, index, indices, false)
                        .into_iter()
                        .skip(skip)
                        .collect()
                };
                apply_quote_edges(source, index, row, &mut parsed);
                rows.extend(parsed);
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
            if let Some((start, Some(end))) = index
                .fence_range(line)
                .or_else(|| index.math_range(line))
                .filter(|(s, e)| {
                    e.is_some_and(|e| e.index() - s.index() <= MAX_PREVIEW_BLOCK_EXPANSION)
                })
            {
                range.start = range.start.min(start.index());
                range.end = range.end.max(end.index() + 1);
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
    let initial_in_math_block = index
        .math_range(SourceLine::new(range.start))
        .is_some_and(|(start, _)| start.index() < range.start);
    let mut parsed = scan_normalized(source, index, range.clone(), initial_in_math_block);
    apply_quote_edges(source, index, range.start, &mut parsed);
    (range, parsed)
}

fn scan_normalized(
    source: &(impl LineSource + ?Sized),
    index: &StructureIndex,
    source_rows: impl Iterator<Item = usize> + Clone,
    initial_in_math_block: bool,
) -> Vec<ParsedLine> {
    let raw: Vec<String> = source_rows
        .clone()
        .map(|row| source.source_line(SourceLine::new(row)).into_owned())
        .collect();
    let prefixes: Vec<Option<usize>> = source_rows
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
    MarkdownScanner::scan_normalized_range_with_state(
        &raw,
        &normalized,
        &prefixes,
        0..raw.len(),
        false,
        initial_in_math_block,
    )
}

fn apply_quote_edges(
    source: &(impl LineSource + ?Sized),
    index: &StructureIndex,
    range_start: usize,
    parsed: &mut [ParsedLine],
) {
    // Group edges use document coordinates even when a window starts at an edge.
    for (offset, line) in parsed.iter_mut().enumerate() {
        let row = range_start + offset;
        if let Some(quote) = line.quote.as_mut() {
            quote.is_start = row == 0 || index.quote_prefix(SourceLine::new(row - 1)) == 0;
            quote.is_end =
                row + 1 == source.line_count() || index.quote_prefix(SourceLine::new(row + 1)) == 0;
        }
    }
}

fn connected(
    source: &(impl LineSource + ?Sized),
    index: &StructureIndex,
    left: usize,
    right: usize,
) -> bool {
    if index.fence_range(SourceLine::new(left)).is_some()
        || index.fence_range(SourceLine::new(right)).is_some()
    {
        return false;
    }
    let left_prefix = index.quote_prefix(SourceLine::new(left));
    let right_prefix = index.quote_prefix(SourceLine::new(right));
    if left_prefix > 0 && right_prefix > 0 {
        return true;
    }
    let left = source.source_line(SourceLine::new(left));
    let right = source.source_line(SourceLine::new(right));
    left[left_prefix..].contains('|') && right[right_prefix..].contains('|')
}
