use super::indent::{quote_prefix_len, strip_quote_prefix};
use super::prefix::{
    BlockPrefix, fence_info, is_thematic_break, parse_block_prefix, parse_fence_prefix,
};
use super::scanner::local_separator;
use super::table::{get_table_metadata, is_table_delimiter_row, parse_table_row_cells};
use crate::buffer::{LineSource, Position, SourceLine, StructureIndex, TextBuffer};
use std::ops::Range;

/// Logical block abstraction shared by Live Preview layout, cursor navigation,
/// hit-testing, and structural editing.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ProjectedBlockKind {
    Paragraph,
    Heading { level: u8 },
    ListItem,
    Quote,
    ThematicBreak,
    CodeBlock { is_mermaid: bool },
    MathBlock,
    Table { delimiter_line: SourceLine },
    BlankSeparator,
}

/// Describes a source-backed Live Preview block and its editable interior.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProjectedBlock {
    pub kind: ProjectedBlockKind,
    /// Full source line range (exclusive end), including structural delimiters.
    pub source_range: Range<SourceLine>,
    /// Line range (exclusive end) allowed to host editable content in Live Preview.
    /// Tables also exclude `delimiter_line` within this range.
    pub editable_lines: Range<SourceLine>,
    /// True for delimited containers (paired code/mermaid fences, math blocks,
    /// and tables) whose trailing content must not pull following syntax or
    /// neighboring blocks into the container via forward-Delete.
    pub is_enclosed_container: bool,
}

impl ProjectedBlock {
    /// Returns true if `line` can host a Live Preview cursor or text edits.
    pub fn is_editable_line(&self, line: SourceLine) -> bool {
        if line < self.editable_lines.start || line >= self.editable_lines.end {
            return false;
        }
        match self.kind {
            ProjectedBlockKind::Table { delimiter_line } => line != delimiter_line,
            _ => true,
        }
    }

    /// Returns true if `line` is the non-editable closing delimiter of a paired fence.
    pub fn is_closing_fence(&self, line: SourceLine) -> bool {
        matches!(self.kind, ProjectedBlockKind::CodeBlock { .. })
            && self.is_enclosed_container
            && line.index() + 1 == self.source_range.end.index()
    }

    /// Returns true if `line` is an opening or closing `$$` line of a paired math block.
    pub fn is_math_delimiter(&self, line: SourceLine) -> bool {
        matches!(self.kind, ProjectedBlockKind::MathBlock)
            && (line == self.source_range.start
                || line.index() + 1 == self.source_range.end.index())
    }

    /// Returns the next editable line strictly inside this same block, if any.
    /// Tables intentionally return `None` so forward-Delete does not merge pipe rows.
    pub fn next_intra_block_editable_line(&self, line: SourceLine) -> Option<SourceLine> {
        if !self.is_editable_line(line) || matches!(self.kind, ProjectedBlockKind::Table { .. }) {
            return None;
        }
        let next = line + 1;
        self.is_editable_line(next).then_some(next)
    }

    /// Returns the minimum editable character column on `line` inside Live Preview.
    pub fn editable_min_col(&self, lines: &(impl LineSource + ?Sized), line: SourceLine) -> usize {
        if !self.is_editable_line(line) {
            return 0;
        }
        let raw = lines.source_line(line);
        let raw_chars = raw.chars().count();
        let quote_bytes = quote_prefix_len(&raw).min(raw.len());
        match self.kind {
            ProjectedBlockKind::CodeBlock { .. } => {
                if self.is_enclosed_container && line == self.source_range.start {
                    let inner = &raw[quote_bytes..];
                    if let Some((_, fence_len)) = parse_fence_prefix(inner.trim_start()) {
                        let leading = inner.len() - inner.trim_start().len();
                        return raw[..(quote_bytes + leading + fence_len)]
                            .chars()
                            .count()
                            .min(raw_chars);
                    }
                }
                0
            }
            ProjectedBlockKind::MathBlock => raw[..quote_bytes].chars().count().min(raw_chars),
            ProjectedBlockKind::Table { .. } => {
                let cells = parse_table_row_cells(&raw);
                cells
                    .first()
                    .map_or(0, |cell| cell.col_range.0)
                    .min(raw_chars)
            }
            ProjectedBlockKind::Heading { .. } | ProjectedBlockKind::ListItem => {
                let inner = &raw[quote_bytes..];
                let inner_prefix = parse_block_prefix(inner).map_or(0, |p| p.total_len);
                let total_bytes = (quote_bytes + inner_prefix).min(raw.len());
                raw[..total_bytes].chars().count().min(raw_chars)
            }
            ProjectedBlockKind::Quote => raw[..quote_bytes].chars().count().min(raw_chars),
            ProjectedBlockKind::Paragraph
            | ProjectedBlockKind::ThematicBreak
            | ProjectedBlockKind::BlankSeparator => 0,
        }
    }
}

fn is_mermaid_language(raw_fence_line: &str) -> bool {
    let stripped = strip_quote_prefix(raw_fence_line);
    fence_info(stripped).is_some_and(|info| {
        info.split_whitespace()
            .next()
            .is_some_and(|lang| lang.eq_ignore_ascii_case("mermaid"))
    })
}

/// Resolves the `ProjectedBlock` containing `line` in O(log N) via `StructureIndex`.
pub(crate) fn block_at(
    lines: &(impl LineSource + ?Sized),
    index: &StructureIndex,
    line: SourceLine,
) -> Option<ProjectedBlock> {
    if line.index() >= lines.line_count() {
        return None;
    }

    if let Some((start, end)) = index.projected_fence_pair(line) {
        let opener = lines.source_line(start);
        return Some(ProjectedBlock {
            kind: ProjectedBlockKind::CodeBlock {
                is_mermaid: is_mermaid_language(&opener),
            },
            source_range: start..(end + 1),
            editable_lines: start..end,
            is_enclosed_container: true,
        });
    }

    if let Some((start, end)) = index.math_pair(line) {
        let editable_start = start + 1;
        return Some(ProjectedBlock {
            kind: ProjectedBlockKind::MathBlock,
            source_range: start..(end + 1),
            editable_lines: editable_start..end,
            is_enclosed_container: true,
        });
    }

    let raw = lines.source_line(line);
    if raw.trim().is_empty() {
        if local_separator(lines, index, line.index()) {
            return Some(ProjectedBlock {
                kind: ProjectedBlockKind::BlankSeparator,
                source_range: line..(line + 1),
                editable_lines: line..line,
                is_enclosed_container: false,
            });
        }
        return Some(ProjectedBlock {
            kind: ProjectedBlockKind::Paragraph,
            source_range: line..(line + 1),
            editable_lines: line..(line + 1),
            is_enclosed_container: false,
        });
    }

    if let Some(meta) = get_table_metadata(lines, line) {
        return Some(ProjectedBlock {
            kind: ProjectedBlockKind::Table {
                delimiter_line: meta.delimiter_line,
            },
            source_range: meta.header_line..(meta.last_table_line + 1),
            editable_lines: meta.header_line..(meta.last_table_line + 1),
            is_enclosed_container: true,
        });
    }

    Some(classify_single_line_block(line, &raw))
}

fn classify_single_line_block(line: SourceLine, raw: &str) -> ProjectedBlock {
    let trimmed = raw.trim();
    let kind = if is_thematic_break(trimmed) {
        ProjectedBlockKind::ThematicBreak
    } else if let Some(prefix) = parse_block_prefix(strip_quote_prefix(raw)) {
        match prefix.kind {
            BlockPrefix::Heading { level } => ProjectedBlockKind::Heading { level },
            BlockPrefix::Task { .. } | BlockPrefix::Bullet { .. } | BlockPrefix::Ordered { .. } => {
                ProjectedBlockKind::ListItem
            }
            BlockPrefix::Quote => ProjectedBlockKind::Quote,
            BlockPrefix::Fence { .. } => ProjectedBlockKind::Paragraph,
        }
    } else if parse_block_prefix(raw).is_some_and(|p| matches!(p.kind, BlockPrefix::Quote)) {
        ProjectedBlockKind::Quote
    } else {
        ProjectedBlockKind::Paragraph
    };

    ProjectedBlock {
        kind,
        source_range: line..(line + 1),
        editable_lines: line..(line + 1),
        is_enclosed_container: false,
    }
}

/// Returns true when `line` can accept a cursor in Live Preview mode.
pub(crate) fn is_live_line_editable(
    lines: &(impl LineSource + ?Sized),
    index: &StructureIndex,
    line: SourceLine,
) -> bool {
    if line.index() >= lines.line_count() {
        return false;
    }
    let Some(block) = block_at(lines, index, line) else {
        return false;
    };
    if matches!(block.kind, ProjectedBlockKind::Paragraph) {
        let raw = lines.source_line(line);
        if is_table_delimiter_row(&raw) {
            return false;
        }
    }
    block.is_editable_line(line)
}

/// Finds the nearest Live Preview editable line strictly before or after `from_line`.
pub(crate) fn adjacent_live_editable_line(
    lines: &(impl LineSource + ?Sized),
    index: &StructureIndex,
    from_line: SourceLine,
    forward: bool,
) -> Option<SourceLine> {
    let count = lines.line_count();
    if forward {
        ((from_line.index() + 1)..count)
            .map(SourceLine::new)
            .find(|&line| is_live_line_editable(lines, index, line))
    } else {
        (0..from_line.index().min(count))
            .rev()
            .map(SourceLine::new)
            .find(|&line| is_live_line_editable(lines, index, line))
    }
}

/// Normalizes a cursor position into the closest valid Live Preview editable coordinate.
pub(crate) fn normalize_live_cursor(
    lines: &(impl LineSource + ?Sized),
    index: &StructureIndex,
    pos: Position,
) -> Position {
    let line_count = lines.line_count();
    if line_count == 0 {
        return Position::new(SourceLine::new(0), 0);
    }
    let line = SourceLine::new(pos.line.index().min(line_count - 1));
    let Some(block) = block_at(lines, index, line) else {
        return pos;
    };

    // Closing fence projects onto the previous editable line at its line end.
    if block.is_closing_fence(line)
        && let Some(prev) = line.checked_sub(1)
    {
        let len = lines.source_line(prev).chars().count();
        return Position::new(prev, len);
    }

    // Math delimiter projects inward if content lines exist; empty `$$\n$$` stays put.
    if block.is_math_delimiter(line) {
        if block.editable_lines.start >= block.editable_lines.end {
            return Position::new(line, pos.col);
        }
        let target_line = if line == block.source_range.start {
            block.editable_lines.start
        } else {
            block.editable_lines.end - 1
        };
        let target_block = block_at(lines, index, target_line).unwrap_or_else(|| block.clone());
        let col = if line == block.source_range.start {
            target_block.editable_min_col(lines, target_line)
        } else {
            lines.source_line(target_line).chars().count()
        };
        return Position::new(target_line, col);
    }

    // Separators and table delimiter rows project to nearest editable neighbor.
    let target_pos = if !block.is_editable_line(line) {
        if let Some(prev) = adjacent_live_editable_line(lines, index, line, false) {
            Position::new(prev, 0)
        } else if let Some(next) = adjacent_live_editable_line(lines, index, line, true) {
            Position::new(next, 0)
        } else {
            Position::new(line, pos.col)
        }
    } else {
        Position::new(line, pos.col)
    };

    let target_block = block_at(lines, index, target_pos.line).unwrap_or(block);
    let min_col = target_block.editable_min_col(lines, target_pos.line);
    let max_col = lines.source_line(target_pos.line).chars().count();
    Position::new(target_pos.line, target_pos.col.max(min_col).min(max_col))
}

/// Convenience wrapper resolving a `ProjectedBlock` directly from a `TextBuffer`'s cached index.
pub fn block_at_buffer(buffer: &TextBuffer, line: SourceLine) -> Option<ProjectedBlock> {
    let index = buffer.line_index();
    block_at(buffer, index.structure(), line)
}

/// Convenience wrapper checking Live Preview editability directly against a `TextBuffer`.
pub fn is_buffer_live_line_editable(buffer: &TextBuffer, line: SourceLine) -> bool {
    let index = buffer.line_index();
    is_live_line_editable(buffer, index.structure(), line)
}

/// Convenience wrapper finding the next/previous Live Preview editable line in a `TextBuffer`.
pub fn adjacent_buffer_live_editable_line(
    buffer: &TextBuffer,
    from_line: SourceLine,
    forward: bool,
) -> Option<SourceLine> {
    let index = buffer.line_index();
    adjacent_live_editable_line(buffer, index.structure(), from_line, forward)
}

/// Convenience wrapper normalizing a Live Preview cursor directly against a `TextBuffer`.
pub fn normalize_buffer_live_cursor(buffer: &TextBuffer, pos: Position) -> Position {
    let index = buffer.line_index();
    normalize_live_cursor(buffer, index.structure(), pos)
}
