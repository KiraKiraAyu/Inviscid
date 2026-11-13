use super::char_col_to_byte_offset;
use super::primitives::{Position, Selection};
use super::{LineSource, SourceLine, TextBuffer};
use crate::markdown::{
    BlockPrefix, current_indent_relative, get_smart_continuation_prefix, indent_col,
    is_thematic_break, list_marker, parse_block_prefix, parse_heading_prefix, quote_prefix_len,
};

fn list_ancestor_for_buffer(
    buffer: &TextBuffer,
    line_idx: usize,
    max_marker_col: Option<usize>,
) -> Option<(usize, usize)> {
    let line = buffer.line_text(crate::buffer::SourceLine::new(line_idx))?;
    let quote_len = quote_prefix_len(&line);
    for idx in (0..line_idx).rev() {
        let candidate = buffer.line_text(crate::buffer::SourceLine::new(idx))?;
        if candidate.trim().is_empty() || quote_prefix_len(&candidate) != quote_len {
            continue;
        }
        if let Some((parent_col, content_start)) = list_marker(&candidate)
            && max_marker_col.is_none_or(|max_col| parent_col < max_col)
        {
            return Some((parent_col, content_start));
        }
    }
    None
}

fn local_tab_plan(buffer: &TextBuffer, line_idx: usize, tab_size: usize) -> (usize, usize) {
    let line = buffer
        .line_text(crate::buffer::SourceLine::new(line_idx))
        .unwrap_or_default();
    let tab_size = tab_size.max(1);
    let quote_len = quote_prefix_len(&line);
    let current_rel = current_indent_relative(&line);
    let target_rel = (current_rel / tab_size + 1) * tab_size;
    let target_rel = list_ancestor_for_buffer(buffer, line_idx, None)
        .map_or(target_rel, |(_, content_start)| {
            target_rel.max(content_start.saturating_sub(quote_len))
        });
    let current_indent = quote_len + current_rel;
    (
        indent_col(&line),
        (quote_len + target_rel).saturating_sub(current_indent),
    )
}

fn local_shift_tab_plan(buffer: &TextBuffer, line_idx: usize, tab_size: usize) -> (usize, usize) {
    let line = buffer
        .line_text(crate::buffer::SourceLine::new(line_idx))
        .unwrap_or_default();
    let tab_size = tab_size.max(1);
    let quote_len = quote_prefix_len(&line);
    let current_rel = current_indent_relative(&line);
    let default_rel = if current_rel.is_multiple_of(tab_size) {
        current_rel.saturating_sub(tab_size)
    } else {
        (current_rel / tab_size) * tab_size
    };
    let Some((marker_col, _)) = list_marker(&line) else {
        let current_indent = quote_len + current_rel;
        let target = quote_len + default_rel;
        return (target, current_indent.saturating_sub(target));
    };
    let marker_rel = marker_col.saturating_sub(quote_len);
    let target_rel = list_ancestor_for_buffer(buffer, line_idx, Some(marker_col))
        .map(|(parent_col, _)| {
            let parent_rel = parent_col.saturating_sub(quote_len);
            if parent_rel >= default_rel || marker_rel.saturating_sub(parent_rel) <= tab_size.max(3)
            {
                parent_rel
            } else {
                default_rel
            }
        })
        .unwrap_or(default_rel);
    let target = quote_len + target_rel;
    let current_indent = quote_len + current_rel;
    (target, current_indent.saturating_sub(target))
}

fn remove_char_at_col(line: &mut String, col: usize) {
    let b = char_col_to_byte_offset(line, col);
    if b < line.len() {
        line.remove(b);
    }
}

/// Inserts an empty block before an unfenced block. Each empty block occupies
/// an editable source line followed by its own blank separator, regardless of
/// whether the following block is a heading or a paragraph. Enter at the start
/// of that block leaves the caret on `following_line`, the block the user was
/// already editing.
struct PrependBlockPlan {
    inserted_line_count: usize,
    following_line: SourceLine,
}

fn prepend_block_plan(
    lines: &(impl LineSource + ?Sized),
    block_line: SourceLine,
) -> PrependBlockPlan {
    let has_separator_before =
        crate::markdown::blank_run_before_ends_with_separator(lines, block_line);
    // Insert [empty block, separator] when the preceding source already has
    // its separator. Otherwise insert [separator, empty block, separator].
    let editable_offset = usize::from(block_line.index() > 0 && !has_separator_before);
    let inserted_line_count = editable_offset + 2;
    PrependBlockPlan {
        inserted_line_count,
        following_line: block_line + inserted_line_count,
    }
}

/// A newline edit expressed as a line-range replacement, so both the rope path
/// and the materialized-selection path apply one decision.
struct NewlineEdit {
    start: SourceLine,
    remove_count: usize,
    inserted: Vec<String>,
    cursor: Position,
}

fn newline_edit(
    lines: &(impl LineSource + ?Sized),
    fence_hint: FenceHint,
    line: SourceLine,
    col: usize,
    is_source: bool,
) -> NewlineEdit {
    let line_count = lines.line_count();
    let current_line = lines.source_line(line).into_owned();
    let cursor_below = || NewlineEdit {
        start: line + 1,
        remove_count: 0,
        inserted: vec![String::new()],
        cursor: Position::new(line + 1, 0),
    };

    if current_line.is_empty() {
        let next_has_content =
            line.index() + 1 < line_count && !lines.source_line(line + 1).trim().is_empty();
        return if is_source || next_has_content {
            cursor_below()
        } else {
            appended_block(lines, line)
        };
    }

    let current_chars_len = current_line.chars().count();
    let col = col.min(current_chars_len);
    let trimmed = current_line.trim_start();
    let leading_spaces = current_line.len() - trimmed.len();
    let split_byte = char_col_to_byte_offset(&current_line, col);
    let left = current_line[..split_byte].to_string();
    let right = current_line[split_byte..].to_string();
    let split_here = || NewlineEdit {
        start: line,
        remove_count: 1,
        inserted: vec![left.clone(), right.clone()],
        cursor: Position::new(line + 1, 0),
    };

    if is_source && !trimmed.starts_with("```") && trimmed != "$$" || fence_hint.in_paired_fence {
        return split_here();
    }

    if trimmed.starts_with("```") {
        let mut inserted = vec![current_line];
        if !fence_hint.has_fence_after {
            inserted.extend([String::new(), "```".to_string(), String::new()]);
        }
        return NewlineEdit {
            start: line,
            remove_count: 1,
            inserted,
            cursor: Position::new(line + 1, 0),
        };
    }

    if trimmed == "$$" && col == current_chars_len {
        return math_delimiter_edit(lines, line);
    }
    if is_thematic_break(trimmed) {
        return appended_block(lines, line);
    }

    if let Some((_level, prefix_len)) = parse_heading_prefix(trimmed) {
        let actual_prefix_len = leading_spaces + prefix_len;
        return if col <= actual_prefix_len {
            prepended_block(lines, line, actual_prefix_len)
        } else if col >= current_chars_len {
            appended_block(lines, line)
        } else {
            NewlineEdit {
                start: line,
                remove_count: 1,
                inserted: vec![left, String::new(), right],
                cursor: Position::new(line + 2, 0),
            }
        };
    }

    if let Some(prefix) = parse_block_prefix(&current_line)
        && !matches!(
            prefix.kind,
            BlockPrefix::Heading { .. } | BlockPrefix::Fence { .. }
        )
        && current_chars_len <= prefix.total_len
    {
        let next_is_blank =
            line.index() + 1 < line_count && lines.source_line(line + 1).trim().is_empty();
        let mut inserted = vec![String::new(), String::new()];
        if !is_source && !next_is_blank {
            inserted.push(String::new());
        }
        return NewlineEdit {
            start: line,
            remove_count: 1,
            inserted,
            cursor: Position::new(line + 1, 0),
        };
    }

    if let Some(prefix) = get_smart_continuation_prefix(&current_line) {
        let prefix_len = prefix.chars().count();
        return NewlineEdit {
            start: line,
            remove_count: 1,
            inserted: vec![left, format!("{prefix}{right}")],
            cursor: Position::new(line + 1, prefix_len),
        };
    }

    if is_source {
        return split_here();
    }
    if col == 0 {
        // Enter at the start of a block opens an empty block above it and
        // leaves the caret on the block the user was already editing.
        return prepended_block(lines, line, 0);
    }
    if col >= current_chars_len {
        return appended_block(lines, line);
    }
    NewlineEdit {
        start: line,
        remove_count: 1,
        inserted: vec![left, String::new(), right],
        cursor: Position::new(line + 2, 0),
    }
}

/// Enter at the end of a `$$` line. A closing delimiter of an existing math block
/// is ordinary text and falls through to the remaining rules.
fn math_delimiter_edit(lines: &(impl LineSource + ?Sized), line: SourceLine) -> NewlineEdit {
    let cursor_below = || NewlineEdit {
        start: line + 1,
        remove_count: 0,
        inserted: vec![String::new()],
        cursor: Position::new(line + 1, 0),
    };
    let Some((start, end)) = crate::markdown::math_block_range(lines, line) else {
        return NewlineEdit {
            start: line + 1,
            remove_count: 0,
            inserted: vec![String::new(), "$$".to_string(), String::new()],
            cursor: Position::new(line + 1, 0),
        };
    };
    if start != line {
        // This `$$` closes a block opened above it.
        return NewlineEdit {
            start: line,
            remove_count: 1,
            inserted: vec![lines.source_line(line).into_owned(), String::new()],
            cursor: Position::new(line + 1, 0),
        };
    }
    if end.index() == start.index() + 1 {
        cursor_below()
    } else {
        NewlineEdit {
            start: line,
            remove_count: 0,
            inserted: Vec::new(),
            cursor: Position::new(line + 1, 0),
        }
    }
}

fn prepended_block(
    lines: &(impl LineSource + ?Sized),
    line: SourceLine,
    cursor_col: usize,
) -> NewlineEdit {
    let plan = prepend_block_plan(lines, line);
    NewlineEdit {
        start: line,
        remove_count: 0,
        inserted: vec![String::new(); plan.inserted_line_count],
        cursor: Position::new(plan.following_line, cursor_col),
    }
}

fn appended_block(lines: &(impl LineSource + ?Sized), line: SourceLine) -> NewlineEdit {
    let line_count = lines.line_count();
    let blank_at = |offset: usize| {
        line.index() + offset < line_count && lines.source_line(line + offset).trim().is_empty()
    };
    let (start_offset, inserted_count) = if blank_at(1) {
        (2, usize::from(!blank_at(2)) + 1)
    } else {
        (1, 2)
    };
    NewlineEdit {
        start: line + start_offset,
        remove_count: 0,
        inserted: vec![String::new(); inserted_count],
        cursor: Position::new(line + 2, 0),
    }
}

#[derive(Clone, Copy)]
struct FenceHint {
    in_paired_fence: bool,
    has_fence_after: bool,
}

impl FenceHint {
    fn from_buffer(buffer: &TextBuffer, line: SourceLine) -> Self {
        let index = buffer.line_index();
        Self {
            in_paired_fence: index.paired_fence_range(line).is_some(),
            has_fence_after: index.has_fence_after(line),
        }
    }

    fn scan(lines: &(impl LineSource + ?Sized), line: SourceLine) -> Self {
        let mut open_fence = None;
        let mut in_paired_fence = false;
        let mut has_fence_after = false;
        let target = line.index();
        for idx in 0..lines.line_count() {
            if !lines
                .source_line(SourceLine::new(idx))
                .trim_start()
                .starts_with("```")
            {
                continue;
            }
            if idx > target {
                has_fence_after = true;
            }
            if let Some(start) = open_fence.take() {
                if (start..=idx).contains(&target) {
                    in_paired_fence = true;
                }
            } else {
                open_fence = Some(idx);
            }
        }
        Self {
            in_paired_fence,
            has_fence_after,
        }
    }
}

/// A document with a selection already removed, computed lazily so a newline
/// over a selection does not materialize the untouched lines.
struct SelectionDeleted<'a> {
    buffer: &'a TextBuffer,
    start: Position,
    end: Position,
    joined: String,
}

impl<'a> SelectionDeleted<'a> {
    fn new(buffer: &'a TextBuffer, start: Position, end: Position) -> Self {
        let start_line = buffer.line_text(start.line).unwrap_or_default();
        let start_byte = char_col_to_byte_offset(&start_line, start.col);
        let end_line = if start.line == end.line {
            start_line.clone()
        } else {
            buffer.line_text(end.line).unwrap_or_default()
        };
        let end_byte = char_col_to_byte_offset(&end_line, end.col);
        Self {
            buffer,
            start,
            end,
            joined: format!("{}{}", &start_line[..start_byte], &end_line[end_byte..]),
        }
    }

    /// Lines left after the deletion, before the empty-document sentinel padding.
    fn raw_count(&self) -> usize {
        self.buffer.line_count() - self.end.line.index() + self.start.line.index()
    }

    fn padded(&self) -> bool {
        self.raw_count() == 1 && self.joined.is_empty()
    }

    /// Folds the selection deletion and a following line edit into one range of
    /// the original document. The edit's coordinates are in this view.
    fn compose(
        &self,
        edit_start: SourceLine,
        remove_count: usize,
        mut inserted: Vec<String>,
    ) -> (SourceLine, usize, Vec<String>) {
        let join = self.start.line.index();
        let collapsed = self.end.line.index() - join;
        let edit_at = edit_start.index();
        let edit_end = edit_at + remove_count;
        let view_lo = edit_at.min(join);
        let view_hi = edit_end.max(join + 1);

        let mut lines = Vec::new();
        for idx in view_lo..edit_at {
            lines.push(self.source_line(SourceLine::new(idx)).into_owned());
        }
        lines.append(&mut inserted);
        for idx in edit_end..view_hi {
            lines.push(self.source_line(SourceLine::new(idx)).into_owned());
        }

        let map_end = |index: usize| {
            if index <= join {
                index
            } else {
                index + collapsed
            }
        };
        let original_end = map_end(view_hi.min(self.raw_count()));
        (SourceLine::new(view_lo), original_end - view_lo, lines)
    }
}

impl LineSource for SelectionDeleted<'_> {
    fn line_count(&self) -> usize {
        self.raw_count() + usize::from(self.padded())
    }

    fn source_line(&self, line: SourceLine) -> std::borrow::Cow<'_, str> {
        let idx = line.index();
        if self.padded() && idx == self.raw_count() {
            return std::borrow::Cow::Borrowed("");
        }
        if idx == self.start.line.index() {
            return std::borrow::Cow::Borrowed(&self.joined);
        }
        let original = if idx < self.start.line.index() {
            idx
        } else {
            idx + self.end.line.index() - self.start.line.index()
        };
        self.buffer.source_line(SourceLine::new(original))
    }
}

impl TextBuffer {
    pub fn surround_selection(&mut self, open: &str, close: &str) {
        if self.selection.is_empty() {
            let pos = self.selection.head;
            let pair = format!("{}{}", open, close);
            self.insert_text(&pair);
            let open_len = open.chars().count();
            self.selection = Selection::cursor(Position::new(pos.line, pos.col + open_len));
            return;
        }

        let (start, end) = self.selection.range();
        let mut replacement = (start.line.index()..=end.line.index())
            .map(|idx| {
                self.line_text(crate::buffer::SourceLine::new(idx))
                    .unwrap_or_default()
            })
            .collect::<Vec<_>>();
        let start_byte = char_col_to_byte_offset(&replacement[0], start.col);
        let end_offset = end.line.index() - start.line.index();
        let end_byte = char_col_to_byte_offset(&replacement[end_offset], end.col);
        if start.line == end.line {
            replacement[0].insert_str(end_byte, close);
            replacement[0].insert_str(start_byte, open);
        } else {
            replacement[0].insert_str(start_byte, open);
            replacement[end_offset].insert_str(end_byte, close);
        }
        let close_len = close.chars().count();
        let open_len = if start.line == end.line {
            open.chars().count()
        } else {
            0
        };
        self.replace_line_range(
            start.line,
            end_offset + 1,
            replacement,
            Selection {
                anchor: Position::new(start.line, start.col),
                head: Position::new(end.line, end.col + close_len + open_len),
            },
        );
    }

    pub fn backspace_auto_pair(&mut self) -> bool {
        if !self.selection.is_empty() {
            return false;
        }

        let pos = self.selection.head;
        if pos.line.index() >= self.line_count() || pos.col == 0 {
            return false;
        }
        let line = self.line_text(pos.line).unwrap_or_default();
        let byte_left = char_col_to_byte_offset(&line, pos.col - 1);
        let left_ch = line[byte_left..].chars().next();
        let byte_right = char_col_to_byte_offset(&line, pos.col);
        let right_ch = line[byte_right..].chars().next();

        if let (Some(left), Some(right)) = (left_ch, right_ch) {
            let is_pair = matches!(
                (left, right),
                ('(', ')') | ('[', ']') | ('{', '}') | ('"', '"') | ('\'', '\'') | ('`', '`')
            );

            if is_pair {
                let left_len = left.len_utf8();
                let right_len = right.len_utf8();
                self.record_single_line_edit(pos.line, |line, sel| {
                    let b = char_col_to_byte_offset(line, pos.col - 1);
                    line.drain(b..b + left_len + right_len);
                    *sel = Selection::cursor(Position::new(pos.line, pos.col - 1));
                });
                return true;
            }
        }

        false
    }

    pub fn move_lines_up(&mut self) {
        let (start, end) = self.selection.range();
        if start.line.index() == 0 {
            return;
        }
        let range_start = start.line - 1;
        let mut replacement: Vec<String> = (start.line.index()..=end.line.index())
            .map(|line| {
                self.line_text(crate::buffer::SourceLine::new(line))
                    .unwrap_or_default()
            })
            .collect();
        replacement.insert(
            replacement.len(),
            self.line_text(range_start).unwrap_or_default(),
        );
        let selection = Selection {
            anchor: Position::new(self.selection.anchor.line - 1, self.selection.anchor.col),
            head: Position::new(self.selection.head.line - 1, self.selection.head.col),
        };
        self.replace_line_range(
            range_start,
            end.line - range_start + 1,
            replacement,
            selection,
        );
    }

    pub fn move_lines_down(&mut self) {
        let (start, end) = self.selection.range();
        if (end.line + 1).index() >= self.line_count() {
            return;
        }

        let range_start = start.line;
        let mut replacement = vec![self.line_text(end.line + 1).unwrap_or_default()];
        replacement.extend((start.line.index()..=end.line.index()).map(|line| {
            self.line_text(crate::buffer::SourceLine::new(line))
                .unwrap_or_default()
        }));
        let selection = Selection {
            anchor: Position::new(self.selection.anchor.line + 1, self.selection.anchor.col),
            head: Position::new(self.selection.head.line + 1, self.selection.head.col),
        };
        self.replace_line_range(
            range_start,
            end.line - start.line + 2,
            replacement,
            selection,
        );
    }

    pub fn duplicate_lines_down(&mut self) {
        let (start, end) = self.selection.range();
        let count = end.line - start.line + 1;
        let block = (start.line.index()..=end.line.index())
            .map(|line| {
                self.line_text(crate::buffer::SourceLine::new(line))
                    .unwrap_or_default()
            })
            .collect::<Vec<_>>();
        let selection = Selection {
            anchor: Position::new(
                self.selection.anchor.line + count,
                self.selection.anchor.col,
            ),
            head: Position::new(self.selection.head.line + count, self.selection.head.col),
        };
        self.replace_line_range(end.line + 1, 0, block, selection);
    }

    pub fn duplicate_lines_up(&mut self) {
        let (start, end) = self.selection.range();
        let block = (start.line.index()..=end.line.index())
            .map(|line| {
                self.line_text(crate::buffer::SourceLine::new(line))
                    .unwrap_or_default()
            })
            .collect::<Vec<_>>();
        self.replace_line_range(start.line, 0, block, self.selection);
    }

    pub fn delete_lines(&mut self) {
        let (start, end) = self.selection.range();
        let count = end.line - start.line + 1;
        let line_count = self.line_count();
        if count >= line_count {
            self.replace_line_range(
                crate::buffer::SourceLine::new(0),
                line_count,
                vec![String::new(), String::new()],
                Selection::cursor(Position::default()),
            );
            return;
        }

        let remaining = line_count - count;
        let target_line = start.line.index().min(remaining - 1);
        let target_source_line = crate::buffer::SourceLine::new(target_line);
        let target_old_line = if target_line < start.line.index() {
            target_source_line
        } else {
            end.line + 1
        };
        let target_text = self.line_text(target_old_line).unwrap_or_default();
        if remaining == 1 && target_text.is_empty() {
            self.replace_line_range(
                crate::buffer::SourceLine::new(0),
                line_count,
                vec![String::new(), String::new()],
                Selection::cursor(Position::default()),
            );
        } else {
            let cursor = Position::new(
                target_source_line,
                self.selection.head.col.min(target_text.chars().count()),
            );
            self.replace_line_range(start.line, count, Vec::new(), Selection::cursor(cursor));
        }
    }

    pub fn insert_newline_below(&mut self) {
        let line_idx = self.selection.head.line;
        let current_line = self.line_text(line_idx).unwrap_or_default();
        let new_content = get_smart_continuation_prefix(&current_line).unwrap_or_default();
        let new_col = new_content.chars().count();
        self.replace_line_range(
            line_idx + 1,
            0,
            vec![new_content],
            Selection::cursor(Position::new(line_idx + 1, new_col)),
        );
    }

    pub fn insert_newline_above(&mut self) {
        let line_idx = self.selection.head.line;
        self.replace_line_range(
            line_idx,
            0,
            vec![String::new()],
            Selection::cursor(Position::new(line_idx, 0)),
        );
    }

    /// Indents the selected lines with smart tab size detection:
    /// - Inside code blocks: auto-detects 2 or 4 spaces (default 4)
    /// - Outside code blocks: always 4 spaces
    pub fn indent_lines(&mut self) {
        let tab_size = self.detect_smart_tab_size();
        self.indent_lines_with_tab_size(tab_size);
    }

    /// Indents the selected lines (or the cursor line):
    /// list items nest to their parent's content column, other
    /// lines step by `tab_size` spaces. A multi-line selection shifts
    /// uniformly by the smallest per-line step so the block stays coherent.
    ///
    /// With the cursor mid-line and no selection, Tab inserts `tab_size`
    /// spaces at the cursor.
    pub fn indent_lines_with_tab_size(&mut self, tab_size: usize) {
        let tab_size = tab_size.max(1);
        let (start, end) = self.selection.range();

        let is_list = self
            .line_text(start.line)
            .is_some_and(|l| list_marker(&l).is_some());

        if start == end && self.selection.head.col > 0 && !is_list {
            self.insert_text(&" ".repeat(tab_size));
            return;
        }

        let last = end.line.min(crate::buffer::SourceLine::new(
            self.line_count().saturating_sub(1),
        ));
        if start.line > last {
            return;
        }

        let plans: Vec<(usize, usize)> = (start.line.index()..=last.index())
            .map(|idx| local_tab_plan(self, idx, tab_size))
            .collect();

        let step = plans
            .iter()
            .enumerate()
            .filter(|(offset, _)| {
                !self
                    .line_text(start.line + *offset)
                    .unwrap_or_default()
                    .trim()
                    .is_empty()
            })
            .map(|(_, (_, len))| *len)
            .min()
            .or_else(|| plans.first().map(|(_, len)| *len))
            .unwrap_or(tab_size);
        if step == 0 {
            return;
        }

        let mut replacement = (start.line.index()..=last.index())
            .map(|idx| {
                self.line_text(crate::buffer::SourceLine::new(idx))
                    .unwrap_or_default()
            })
            .collect::<Vec<_>>();
        let spaces = " ".repeat(step);
        for (offset, (col, _)) in plans.iter().enumerate() {
            if start.line != last && replacement[offset].is_empty() {
                continue;
            }
            let byte = char_col_to_byte_offset(&replacement[offset], *col);
            replacement[offset].insert_str(byte, &spaces);
        }
        let mut selection = self.selection;
        for pos in [&mut selection.anchor, &mut selection.head] {
            if pos.line >= start.line && pos.line <= last {
                let insert_col = plans.get(pos.line - start.line).map_or(0, |(col, _)| *col);
                if pos.col >= insert_col {
                    pos.col += step;
                }
            }
        }
        self.replace_line_range(
            start.line,
            last.index() - start.line.index() + 1,
            replacement,
            selection,
        );
    }

    /// Outdents the selected lines with smart tab size detection:
    /// - Inside code blocks: auto-detects 2 or 4 spaces (default 4)
    /// - Outside code blocks: always 4 spaces
    pub fn outdent_lines(&mut self) {
        let tab_size = self.detect_smart_tab_size();
        self.outdent_lines_with_tab_size(tab_size);
    }

    /// Outdents the selected lines (or the cursor line):
    /// list items step out to the nearest enclosing list level,
    /// other lines remove up to `tab_size` leading spaces.
    pub fn outdent_lines_with_tab_size(&mut self, tab_size: usize) {
        let tab_size = tab_size.max(1);
        let (start, end) = self.selection.range();

        let last = end.line.min(crate::buffer::SourceLine::new(
            self.line_count().saturating_sub(1),
        ));
        if start.line > last {
            return;
        }

        let plans: Vec<(usize, usize)> = (start.line.index()..=last.index())
            .map(|idx| local_shift_tab_plan(self, idx, tab_size))
            .collect();
        let mut replacement = (start.line.index()..=last.index())
            .map(|idx| {
                self.line_text(crate::buffer::SourceLine::new(idx))
                    .unwrap_or_default()
            })
            .collect::<Vec<_>>();
        let mut selection = self.selection;
        for (offset, &(col, len)) in plans.iter().enumerate() {
            let start_byte = char_col_to_byte_offset(&replacement[offset], col);
            let max_end_byte = char_col_to_byte_offset(&replacement[offset], col + len);
            let safe_len = replacement[offset][start_byte..max_end_byte]
                .bytes()
                .take_while(|&byte| byte == b' ' || byte == b'\t')
                .count();
            if safe_len > 0 {
                replacement[offset].drain(start_byte..start_byte + safe_len);
                let line_idx = start.line + offset;
                for pos in [&mut selection.anchor, &mut selection.head] {
                    if pos.line == line_idx && pos.col > col {
                        pos.col = pos.col.saturating_sub(safe_len).max(col);
                    }
                }
            }
        }
        self.replace_line_range(
            start.line,
            last.index() - start.line.index() + 1,
            replacement,
            selection,
        );
    }

    /// Detects the appropriate tab size based on context:
    /// - Inside code blocks: analyzes existing indentation (2 or 4 spaces)
    /// - Outside code blocks: always returns 4
    fn detect_smart_tab_size(&self) -> usize {
        let cursor_line = self.selection.head.line;
        let Some((start, end)) = self.line_index().paired_fence_range(cursor_line) else {
            return 4;
        };

        let detect_start = start.index() + 1;
        let detect_end = end.index().min(detect_start + 50);
        let mut indent_2_count = 0;
        let mut indent_4_count = 0;
        for line_idx in detect_start..detect_end {
            let content = self
                .line_text(crate::buffer::SourceLine::new(line_idx))
                .unwrap_or_default();
            let spaces = crate::markdown::leading_indent_len(&content);
            if spaces == 0 || content.trim().is_empty() {
                continue;
            }
            if spaces % 2 == 0 && spaces % 4 != 0 {
                indent_2_count += 1;
            } else if spaces % 4 == 0 && spaces > 0 {
                indent_4_count += 1;
            } else if spaces % 2 == 0 {
                indent_2_count += 1;
            }
        }
        if indent_2_count + indent_4_count >= 3 {
            return if indent_2_count > indent_4_count {
                2
            } else {
                4
            };
        }
        4
    }

    pub fn toggle_comment(&mut self) {
        let (start, end) = self.selection.range();
        let mut replacement = (start.line.index()..=end.line.index())
            .map(|idx| {
                self.line_text(crate::buffer::SourceLine::new(idx))
                    .unwrap_or_default()
            })
            .collect::<Vec<_>>();
        for line in &mut replacement {
            let leading_len = line.chars().take_while(|c| c.is_whitespace()).count();
            let leading: String = line.chars().take(leading_len).collect();
            let trimmed = line.trim();
            if let Some(rest) = trimmed.strip_prefix("<!--")
                && let Some(inner_trimmed) = rest.strip_suffix("-->")
            {
                let inner = inner_trimmed.strip_prefix(' ').unwrap_or(inner_trimmed);
                let inner = inner.strip_suffix(' ').unwrap_or(inner);
                *line = format!("{}{}", leading, inner);
            } else {
                let content = &line[leading.len()..];
                *line = format!("{}<!-- {} -->", leading, content);
            }
        }
        self.replace_line_range(
            start.line,
            end.line.index() - start.line.index() + 1,
            replacement,
            self.selection,
        );
    }

    pub fn insert_text(&mut self, text: &str) {
        let is_single_line = self.selection.is_empty()
            && !text.contains('\n')
            && !text.contains('\r')
            && self.selection.head.line.index() < self.line_count();

        if is_single_line {
            let line_idx = self.selection.head.line;
            self.record_single_line_edit(line_idx, |line, sel| {
                let pos = sel.head;
                let byte_idx = char_col_to_byte_offset(line, pos.col);
                line.insert_str(byte_idx, text);
                let insert_len = text.chars().count();
                *sel = Selection::cursor(Position::new(pos.line, pos.col + insert_len));
            });
            return;
        }

        if !text.is_empty() || !self.selection.is_empty() {
            let range = self.selection.range();
            self.replace_range_in_rope(range, text);
            return;
        }

        return;
    }

    pub fn replace_range(&mut self, (start, end): (Position, Position), text: &str) -> Position {
        self.replace_range_in_rope((start, end), text)
    }

    pub fn replace_all_matches(
        &mut self,
        query: &crate::search::SearchQuery,
        replacement: &str,
    ) -> usize {
        let normalized_rep = replacement.replace("\r\n", "\n").replace('\r', "\n");
        let text = self.to_string_normalized();
        let (new_text, count) = query.replace_all(&text, &normalized_rep);
        if count > 0 {
            let mut lines: Vec<String> = new_text.split('\n').map(String::from).collect();
            if lines.len() == 1 && lines[0].is_empty() {
                lines.push(String::new());
            }
            self.replace_line_range(
                SourceLine::new(0),
                self.line_count(),
                lines,
                Selection::cursor(Position::default()),
            );
        }
        count
    }

    #[cfg(test)]
    pub fn insert_newline(&mut self) {
        self.insert_newline_mode(false);
    }

    pub fn insert_newline_mode(&mut self, is_source: bool) {
        if self.selection.is_empty() {
            let pos = self.selection.head;
            let fence_hint = FenceHint::from_buffer(self, pos.line);
            let edit = newline_edit(self, fence_hint, pos.line, pos.col, is_source);
            self.replace_line_range(
                edit.start,
                edit.remove_count,
                edit.inserted,
                Selection::cursor(edit.cursor),
            );
            return;
        }

        // The selection is deleted and the newline inserted as one undo step.
        let (start, end) = self.selection.range();
        let (start, end) = (self.clamp_position(start), self.clamp_position(end));
        let overlay = SelectionDeleted::new(self, start, end);
        let fence_hint = FenceHint::scan(&overlay, start.line);
        let edit = newline_edit(&overlay, fence_hint, start.line, start.col, is_source);
        let (from, remove_count, inserted) =
            overlay.compose(edit.start, edit.remove_count, edit.inserted);
        self.replace_line_range(from, remove_count, inserted, Selection::cursor(edit.cursor));
    }

    pub fn insert_line_below_block(&mut self) {
        let pos = self.selection.head;
        let closing_idx = self
            .line_index()
            .paired_fence_range(pos.line)
            .map(|(_, close)| close.index());

        let anchor = closing_idx.unwrap_or(pos.line.index().min(self.line_count() - 1));
        let has_blank_after = anchor + 1 < self.line_count()
            && self
                .line_text(crate::buffer::SourceLine::new(anchor + 1))
                .is_some_and(|line| line.trim().is_empty());
        let (insert_at, inserted_count) = if has_blank_after {
            let has_second_blank = anchor + 2 < self.line_count()
                && self
                    .line_text(crate::buffer::SourceLine::new(anchor + 2))
                    .is_some_and(|line| line.trim().is_empty());
            (
                crate::buffer::SourceLine::new(anchor + 2),
                usize::from(!has_second_blank) + 1,
            )
        } else {
            (crate::buffer::SourceLine::new(anchor + 1), 2)
        };
        self.replace_line_range(
            insert_at,
            0,
            vec![String::new(); inserted_count],
            Selection::cursor(Position::new(crate::buffer::SourceLine::new(anchor + 2), 0)),
        );
    }

    pub(crate) fn ensure_math_block_content_line(
        &mut self,
        start_line: crate::buffer::SourceLine,
    ) -> Option<Position> {
        let (start, end) = crate::markdown::math_block_range(self, start_line)?;
        if start != start_line {
            return None;
        }
        if end.index() > start.index() + 1 {
            return Some(Position::new(start + 1, 0));
        }

        let pos = Position::new(end, 0);
        self.replace_line_range(end, 0, vec![String::new()], Selection::cursor(pos));
        Some(pos)
    }

    // Ordinary text cannot be a delimiter, so it needs no structure lookup.
    fn paired_math_delimiter(&self, line: SourceLine) -> Option<(SourceLine, SourceLine)> {
        if crate::markdown::strip_quote_prefix(&self.source_line(line)).trim() != "$$" {
            return None;
        }
        self.line_index()
            .math_pair(line)
            .filter(|&(start, end)| line == start || line == end)
    }

    pub fn backspace_mode(&mut self, is_source: bool) {
        self.column_goal = None;
        if self.backspace_auto_pair() {
            return;
        }

        if !self.selection.is_empty() {
            self.replace_range(self.selection.range(), "");
            return;
        }

        let pos = self.selection.head;
        if is_source {
            if pos.col > 0 {
                self.replace_range((Position::new(pos.line, pos.col - 1), pos), "");
            } else if pos.line.index() > 0 {
                let previous_line = pos.line - 1;
                let previous_len = self.line_len(previous_line);
                self.replace_range((Position::new(previous_line, previous_len), pos), "");
            }
            return;
        }

        let line = self.line_text(pos.line).unwrap_or_default();
        if self.paired_math_delimiter(pos.line).is_some() {
            return;
        }
        if pos.col == 0 && pos.line.index() > 0 {
            let previous = pos.line - 1;
            if self.paired_math_delimiter(previous).is_some()
                || (previous.index() > 0
                    && self.source_line(previous).trim().is_empty()
                    && self
                        .paired_math_delimiter(previous - 1)
                        .is_some_and(|(_, end)| end == previous - 1))
            {
                return;
            }
        }

        if is_thematic_break(line.trim_start()) {
            let blank_after = (pos.line + 1).index() < self.line_count()
                && self.source_line(pos.line + 1).trim().is_empty();
            if pos.line.index() > 0 && self.source_line(pos.line - 1).trim().is_empty() {
                self.replace_line_range(
                    pos.line,
                    1 + usize::from(blank_after),
                    vec![],
                    Selection::cursor(Position::new(pos.line - 1, 0)),
                );
            } else {
                self.replace_line_range(
                    pos.line,
                    1 + usize::from(blank_after),
                    vec![String::new()],
                    Selection::cursor(Position::new(pos.line, 0)),
                );
            }
            return;
        }

        if line.trim_start().starts_with("```") && self.line_index().has_fence_after(pos.line) {
            let leading_spaces = line.chars().take_while(|c| *c == ' ').count();
            if pos.col > leading_spaces + 3 {
                self.replace_range((Position::new(pos.line, pos.col - 1), pos), "");
            }
            return;
        }

        let leading_spaces = line.chars().take_while(|c| *c == ' ').count();
        if pos.col >= 4 && pos.col <= leading_spaces {
            let spaces = if pos.col % 4 == 0 { 4 } else { pos.col % 4 };
            self.replace_range((Position::new(pos.line, pos.col - spaces), pos), "");
            return;
        }

        if pos.line.index() > 0
            && line.is_empty()
            && self.source_line(pos.line - 1).trim().starts_with("```")
            && (pos.line + 1).index() < self.line_count()
            && self.source_line(pos.line + 1).trim().starts_with("```")
        {
            let start = pos.line - 1;
            let blank_after = (pos.line + 2).index() < self.line_count()
                && self.source_line(pos.line + 2).trim().is_empty();
            let blank_before = start.index() > 0 && self.source_line(start - 1).trim().is_empty();
            self.replace_line_range(
                start,
                3 + usize::from(blank_after),
                vec![String::new(); if blank_before { 1 } else { 2 }],
                Selection::cursor(Position::new(
                    if blank_before { start - 1 } else { start },
                    0,
                )),
            );
            return;
        }

        if let Some(prefix) = parse_block_prefix(&line)
            && !matches!(prefix.kind, BlockPrefix::Fence { .. })
            && pos.col <= prefix.total_len
        {
            let split = char_col_to_byte_offset(&line, prefix.total_len);
            let leading = char_col_to_byte_offset(&line, prefix.leading_spaces);
            let content = format!("{}{}", &line[..leading], &line[split..]);
            let is_list_or_quote = matches!(
                prefix.kind,
                BlockPrefix::Task { .. }
                    | BlockPrefix::Bullet { .. }
                    | BlockPrefix::Ordered { .. }
                    | BlockPrefix::Quote
            );
            let (inserted, cursor_line) = if is_list_or_quote
                && pos.line.index() > 0
                && !self.source_line(pos.line - 1).trim().is_empty()
            {
                let mut inserted = vec![String::new(), content];
                if (pos.line + 1).index() >= self.line_count()
                    || !self.source_line(pos.line + 1).trim().is_empty()
                {
                    inserted.push(String::new());
                }
                (inserted, pos.line + 1)
            } else {
                (vec![content], pos.line)
            };
            self.replace_line_range(
                pos.line,
                1,
                inserted,
                Selection::cursor(Position::new(cursor_line, prefix.leading_spaces)),
            );
            return;
        }

        if pos.col > 0 {
            self.replace_range((Position::new(pos.line, pos.col - 1), pos), "");
        } else if pos.line.index() > 0 {
            if self
                .source_line(pos.line - 1)
                .trim_start()
                .starts_with("```")
                && self.is_closing_code_fence(pos.line - 1)
            {
                let target = pos.line.saturating_sub(2);
                // A boundary jump is undoable even when it changes only the selection.
                self.replace_line_range(
                    pos.line,
                    usize::from(line.is_empty()),
                    vec![],
                    Selection::cursor(Position::new(target, self.line_len(target))),
                );
            } else if pos.line.index() >= 2
                && self.source_line(pos.line - 1).trim().is_empty()
                && self
                    .source_line(pos.line - 2)
                    .trim_start()
                    .starts_with("```")
                && self.is_closing_code_fence(pos.line - 2)
            {
                let target = pos.line.saturating_sub(3);
                self.replace_line_range(
                    pos.line - 1,
                    1,
                    vec![],
                    Selection::cursor(Position::new(target, self.line_len(target))),
                );
            } else {
                let previous =
                    if pos.line.index() >= 2 && self.source_line(pos.line - 1).trim().is_empty() {
                        pos.line - 2
                    } else {
                        pos.line - 1
                    };
                self.replace_range((Position::new(previous, self.line_len(previous)), pos), "");
            }
        }
    }

    pub fn delete_forward_mode(&mut self, is_source: bool) {
        self.column_goal = None;
        if !self.selection.is_empty() {
            self.replace_range(self.selection.range(), "");
            return;
        }

        let pos = self.selection.head;
        let line = self.line_text(pos.line).unwrap_or_default();
        let line_len = line.chars().count();
        if is_source {
            if pos.col < line_len {
                self.replace_range((pos, Position::new(pos.line, pos.col + 1)), "");
            } else if (pos.line + 1).index() < self.line_count() {
                self.replace_range((pos, Position::new(pos.line + 1, 0)), "");
            }
            return;
        }

        if self.paired_math_delimiter(pos.line).is_some() {
            return;
        }
        if pos.col >= line_len && (pos.line + 1).index() < self.line_count() {
            let next = pos.line + 1;
            if self.paired_math_delimiter(next).is_some()
                || ((next + 1).index() < self.line_count()
                    && self.source_line(next).trim().is_empty()
                    && self
                        .paired_math_delimiter(next + 1)
                        .is_some_and(|(start, _)| start == next + 1))
            {
                return;
            }
        }
        if is_thematic_break(line.trim()) {
            self.replace_line_range(
                pos.line,
                1,
                vec![String::new()],
                Selection::cursor(Position::new(pos.line, 0)),
            );
        } else if pos.col < line_len {
            self.replace_range((pos, Position::new(pos.line, pos.col + 1)), "");
        } else if (pos.line + 1).index() < self.line_count() {
            self.replace_range((pos, Position::new(pos.line + 1, 0)), "");
        }
    }

    pub fn delete_to_previous_word(&mut self) {
        self.column_goal = None;
        if !self.selection.is_empty() {
            self.insert_text("");
            return;
        }
        let head = self.selection.head;
        self.move_to_previous_word(true);
        if self.selection.head != head {
            self.insert_text("");
        }
    }

    pub fn delete_to_next_word(&mut self) {
        self.column_goal = None;
        if !self.selection.is_empty() {
            self.insert_text("");
            return;
        }
        let head = self.selection.head;
        self.move_to_next_word(true);
        if self.selection.head != head {
            self.insert_text("");
        }
    }

    fn table_navigate_or_insert_next_row(
        &mut self,
        meta: &crate::markdown::table::TableMetadata,
        target_cell_idx: Option<usize>,
    ) {
        let pos = self.selection.head;
        let resolve_col = |cells: &[crate::markdown::TableCell]| match target_cell_idx {
            Some(idx) => cells.get(idx).map(|c| c.col_range.1).unwrap_or(2),
            None => cells.first().map(|c| c.col_range.1).unwrap_or(2),
        };

        if pos.line == meta.header_line {
            let next_row = meta.delimiter_line.index() + 1;
            if next_row < self.line_count() && next_row <= meta.last_table_line.index() {
                let next_cells = crate::markdown::table::parse_table_row_cells(
                    &self
                        .line_text(crate::buffer::SourceLine::new(next_row))
                        .unwrap_or_default(),
                );
                let target_col = resolve_col(&next_cells);
                self.selection = Selection::cursor(Position::new(
                    crate::buffer::SourceLine::new(next_row),
                    target_col,
                ));
                return;
            }
            let new_row = crate::markdown::table::build_empty_table_row(meta.num_columns);
            let cells = crate::markdown::table::parse_table_row_cells(&new_row);
            let col = resolve_col(&cells);
            self.replace_line_range(
                meta.delimiter_line + 1,
                0,
                vec![new_row],
                Selection::cursor(Position::new(meta.delimiter_line + 1, col)),
            );
            return;
        }

        if pos.line < meta.last_table_line {
            let next_row = pos.line + 1;
            let next_cells = crate::markdown::table::parse_table_row_cells(
                &self.line_text(next_row).unwrap_or_default(),
            );
            let target_col = resolve_col(&next_cells);
            self.selection = Selection::cursor(Position::new(next_row, target_col));
            return;
        }

        let new_row = crate::markdown::table::build_empty_table_row(meta.num_columns);
        let insert_idx = meta.last_table_line.index() + 1;
        let cells = crate::markdown::table::parse_table_row_cells(&new_row);
        let col = resolve_col(&cells);
        self.replace_line_range(
            crate::buffer::SourceLine::new(insert_idx),
            0,
            vec![new_row],
            Selection::cursor(Position::new(
                crate::buffer::SourceLine::new(insert_idx),
                col,
            )),
        );
    }

    pub fn table_tab_forward(&mut self) -> bool {
        let pos = self.selection.head;
        let meta = match crate::markdown::table::get_table_metadata(self, pos.line) {
            Some(m) => m,
            None => return false,
        };

        let current_cells = crate::markdown::table::parse_table_row_cells(
            &self.line_text(pos.line).unwrap_or_default(),
        );
        let cell_idx = crate::markdown::table::get_cell_index_for_col(&current_cells, pos.col);

        if cell_idx + 1 < current_cells.len() {
            let target_col = current_cells[cell_idx + 1].col_range.1;
            self.selection = Selection::cursor(Position::new(pos.line, target_col));
            return true;
        }

        self.table_navigate_or_insert_next_row(&meta, None);
        true
    }

    pub fn table_tab_backward(&mut self) -> bool {
        let pos = self.selection.head;
        let meta = match crate::markdown::table::get_table_metadata(self, pos.line) {
            Some(m) => m,
            None => return false,
        };

        let current_cells = crate::markdown::table::parse_table_row_cells(
            &self.line_text(pos.line).unwrap_or_default(),
        );
        let cell_idx = crate::markdown::table::get_cell_index_for_col(&current_cells, pos.col);

        if cell_idx > 0 {
            let target_col = current_cells[cell_idx - 1].col_range.1;
            self.selection = Selection::cursor(Position::new(pos.line, target_col));
            return true;
        }

        if pos.line == meta.delimiter_line + 1 {
            let header_cells = crate::markdown::table::parse_table_row_cells(
                &self.line_text(meta.header_line).unwrap_or_default(),
            );
            let target_col = header_cells.last().map(|c| c.col_range.1).unwrap_or(2);
            self.selection = Selection::cursor(Position::new(meta.header_line, target_col));
            return true;
        }

        if pos.line > meta.delimiter_line + 1 {
            let prev_row = pos.line - 1;
            let prev_cells = crate::markdown::table::parse_table_row_cells(
                &self.line_text(prev_row).unwrap_or_default(),
            );
            let target_col = prev_cells.last().map(|c| c.col_range.1).unwrap_or(2);
            self.selection = Selection::cursor(Position::new(prev_row, target_col));
            return true;
        }

        true
    }

    pub fn table_enter_next_row(&mut self) -> bool {
        let pos = self.selection.head;
        let meta = match crate::markdown::table::get_table_metadata(self, pos.line) {
            Some(m) => m,
            None => return false,
        };

        let current_cells = crate::markdown::table::parse_table_row_cells(
            &self.line_text(pos.line).unwrap_or_default(),
        );
        let cell_idx = crate::markdown::table::get_cell_index_for_col(&current_cells, pos.col);

        self.table_navigate_or_insert_next_row(&meta, Some(cell_idx));
        true
    }

    pub fn table_backspace(&mut self) -> bool {
        let pos = self.selection.head;
        if crate::markdown::table::get_table_metadata(self, pos.line).is_none() {
            return false;
        }

        if !self.selection.is_empty() {
            self.insert_text("");
            return true;
        }

        let current_cells = crate::markdown::table::parse_table_row_cells(
            &self.line_text(pos.line).unwrap_or_default(),
        );
        let cell_idx = crate::markdown::table::get_cell_index_for_col(&current_cells, pos.col);
        if let Some(cell) = current_cells.get(cell_idx) {
            if pos.col <= cell.col_range.0 {
                return true;
            }

            self.record_single_line_edit(pos.line, |line, sel| {
                if pos.col > 0 {
                    remove_char_at_col(line, pos.col - 1);
                    *sel = Selection::cursor(Position::new(pos.line, pos.col - 1));
                }
            });
            return true;
        }

        true
    }

    pub fn ensure_trailing_code_fence_newline(&mut self) -> Option<Position> {
        let last_idx = self.line_count().saturating_sub(1);
        if self.is_in_code_fence(crate::buffer::SourceLine::new(last_idx))
            || self.is_closing_code_fence(crate::buffer::SourceLine::new(last_idx))
        {
            let closing = self.is_closing_code_fence(crate::buffer::SourceLine::new(last_idx));
            let inserted = if closing {
                vec![String::new(), String::new()]
            } else {
                vec!["```".to_string(), String::new()]
            };
            self.replace_line_range(
                crate::buffer::SourceLine::new(self.line_count()),
                0,
                inserted,
                Selection::cursor(Position::new(
                    crate::buffer::SourceLine::new(self.line_count() + 1),
                    0,
                )),
            );
            Some(self.selection.head)
        } else if last_idx > 0
            && (self.is_in_code_fence(crate::buffer::SourceLine::new(last_idx - 1))
                || self.is_closing_code_fence(crate::buffer::SourceLine::new(last_idx - 1)))
            && self
                .line_text(crate::buffer::SourceLine::new(last_idx))
                .unwrap_or_default()
                .trim()
                .is_empty()
        {
            self.replace_line_range(
                crate::buffer::SourceLine::new(self.line_count()),
                0,
                vec![String::new()],
                Selection::cursor(Position::new(
                    crate::buffer::SourceLine::new(self.line_count()),
                    0,
                )),
            );
            Some(self.selection.head)
        } else {
            None
        }
    }
}
