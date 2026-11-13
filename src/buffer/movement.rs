use super::TextBuffer;
use super::primitives::{Position, Selection};
#[cfg(test)]
use crate::markdown::compute_syntactic_separators;
use std::cell::Ref;

#[derive(PartialEq, Eq, Clone, Copy)]
pub enum CharKind {
    Whitespace,
    Word,
    Punctuation,
}

fn char_kind(c: char) -> CharKind {
    if c.is_whitespace() {
        CharKind::Whitespace
    } else if c.is_alphanumeric() || c == '_' {
        CharKind::Word
    } else {
        CharKind::Punctuation
    }
}

fn code_fence_status(lines: &[String], target_line: crate::buffer::SourceLine) -> (bool, bool) {
    let target_idx = target_line.index();
    if target_idx >= lines.len() {
        return (false, false);
    }
    let mut open_idx: Option<usize> = None;
    for (i, line) in lines.iter().enumerate() {
        if line.trim_start().starts_with("```") {
            match open_idx {
                Some(open) => {
                    if target_idx == open {
                        return (true, false);
                    } else if target_idx == i {
                        return (false, true);
                    } else if target_idx > open && target_idx < i {
                        return (true, false);
                    }
                    open_idx = None;
                }
                None => open_idx = Some(i),
            }
        }
    }
    (false, false)
}

/// Returns the range `(start_line, end_line)` of the code fence containing
/// `target_idx`, or `None` if the line is not inside a code fence.
/// The range includes the opening and closing fence lines.
pub fn code_fence_range(
    lines: &[String],
    target_line: crate::buffer::SourceLine,
) -> Option<(crate::buffer::SourceLine, crate::buffer::SourceLine)> {
    let target_idx = target_line.index();
    if target_idx >= lines.len() {
        return None;
    }
    let mut open_idx: Option<usize> = None;
    for (i, line) in lines.iter().enumerate() {
        if line.trim_start().starts_with("```") {
            match open_idx {
                Some(open) => {
                    if (target_idx >= open && target_idx <= i) || target_idx == open {
                        return Some((
                            crate::buffer::SourceLine::new(open),
                            crate::buffer::SourceLine::new(i),
                        ));
                    }
                    open_idx = None;
                }
                None => open_idx = Some(i),
            }
        }
    }
    None
}

pub fn is_closing_code_fence_lines(
    lines: &[String],
    target_line: crate::buffer::SourceLine,
) -> bool {
    code_fence_status(lines, target_line).1
}

pub fn is_in_code_fence_lines(lines: &[String], target_line: crate::buffer::SourceLine) -> bool {
    code_fence_status(lines, target_line).0
}

#[derive(Clone, Debug)]
pub(crate) struct LineIndex {
    version: usize,
    revision: u64,
    structure: super::structure_index::StructureIndex,
}

impl LineIndex {
    pub(crate) fn structure_snapshot(&self) -> super::StructureIndex {
        self.structure.clone()
    }

    pub(crate) fn structure(&self) -> &super::StructureIndex {
        &self.structure
    }

    fn build(buffer: &TextBuffer) -> Self {
        Self {
            version: buffer.version,
            revision: buffer.revision,
            structure: super::structure_index::StructureIndex::new(buffer),
        }
    }

    fn update(&mut self, buffer: &TextBuffer) {
        if self.version.checked_add(1) == Some(buffer.version)
            && self.revision.checked_add(1) == Some(buffer.revision)
            && let Some(delta) = buffer.last_edit_delta
            && delta.start_line.index() + delta.old_line_count <= self.structure.len()
            && self.structure.len() - delta.old_line_count + delta.new_line_count
                == buffer.line_count()
        {
            let start = delta.start_line.index();
            self.structure.splice(
                start..start + delta.old_line_count,
                buffer,
                start..start + delta.new_line_count,
            );
            self.version = buffer.version;
            self.revision = buffer.revision;
        } else {
            *self = Self::build(buffer);
        }
    }

    #[cfg(test)]
    pub(super) fn classified_lines(&self) -> usize {
        self.structure.classified_lines
    }

    fn is_in_fence(&self, target_line: crate::buffer::SourceLine) -> bool {
        self.structure.fence_state(target_line).0
    }

    fn is_closing_fence(&self, target_line: crate::buffer::SourceLine) -> bool {
        self.structure.fence_state(target_line).1
    }

    pub(crate) fn has_fence_after(&self, line: crate::buffer::SourceLine) -> bool {
        self.structure.has_fence_after(line)
    }

    pub(crate) fn paired_fence_range(
        &self,
        line: crate::buffer::SourceLine,
    ) -> Option<(crate::buffer::SourceLine, crate::buffer::SourceLine)> {
        self.structure.paired_fence_range(line)
    }

    #[cfg(test)]
    fn is_math_delimiter(&self, target_line: crate::buffer::SourceLine) -> bool {
        self.structure.is_math_delimiter(target_line)
    }

    #[cfg(test)]
    fn is_separator(&self, buffer: &TextBuffer, line: usize) -> bool {
        crate::markdown::local_separator(buffer, &self.structure, line)
    }
}

#[cfg(test)]
mod structure_cache_tests {
    use super::*;
    use crate::buffer::{LineSource, SourceLine};

    fn assert_matches_full_scan(buffer: &TextBuffer) {
        let lines: Vec<_> = buffer
            .source_lines()
            .map(|line| line.into_owned())
            .collect();
        let separators = compute_syntactic_separators(&lines);
        let index = buffer.line_index();
        for (row, separator) in separators.into_iter().enumerate() {
            let line = SourceLine::new(row);
            let math = crate::markdown::math_block_range(&lines, line);
            let fence = code_fence_status(&lines, line);
            assert_eq!(index.is_in_fence(line), fence.0);
            assert_eq!(index.is_closing_fence(line), fence.1);
            assert_eq!(index.structure.math_pair(line), math);
            assert_eq!(
                index.is_math_delimiter(line),
                math.is_some_and(|(start, end)| line == start || line == end)
            );
            assert_eq!(index.is_separator(buffer, row), separator);
        }
    }

    #[test]
    fn undo_branch_with_reused_version_rejects_stale_index() {
        let mut buffer = TextBuffer::from_str("```\nx\n```", None);
        buffer.record_single_line_edit(SourceLine::new(0), |line, _| *line = "text".into());
        assert!(!buffer.is_in_code_fence(SourceLine::new(1)));
        let cached_version = buffer.version;
        buffer.undo();
        buffer.record_single_line_edit(SourceLine::new(1), |line, _| *line = "$$".into());
        assert_eq!(buffer.version, cached_version);
        assert!(buffer.is_in_code_fence(SourceLine::new(1)));
        assert_matches_full_scan(&buffer);
    }

    #[test]
    fn history_skipped_versions_and_empty_transitions_match_full_scan() {
        let mut buffer = TextBuffer::from_str("", None);
        assert_matches_full_scan(&buffer);
        buffer.insert_text("$$\nx\n$$\n> ```\n> $$\n> ```");
        assert_matches_full_scan(&buffer);
        let snapshot = buffer.clone();
        buffer.record_single_line_edit(SourceLine::new(0), |line, _| *line = "```".into());
        buffer.record_single_line_edit(SourceLine::new(1), |line, _| *line = "```".into());
        assert_matches_full_scan(&buffer);
        assert_matches_full_scan(&snapshot);
        buffer.undo();
        assert_matches_full_scan(&buffer);
        buffer.redo();
        assert_matches_full_scan(&buffer);
        buffer.replace_line_range(
            SourceLine::new(0),
            buffer.line_count(),
            vec![String::new(), String::new()],
            Selection::cursor(Position::new(SourceLine::new(0), 0)),
        );
        assert_matches_full_scan(&buffer);
        buffer.undo();
        assert_matches_full_scan(&buffer);
        buffer.redo();
        assert_matches_full_scan(&buffer);
    }
}

impl TextBuffer {
    pub(crate) fn line_index(&self) -> Ref<'_, LineIndex> {
        {
            let cache = self.line_index_cache.borrow();
            if let Some(index) = cache.as_ref()
                && index.version == self.version
                && index.revision == self.revision
            {
                return Ref::map(cache, |c| c.as_ref().unwrap());
            }
        }

        {
            let mut cache = self.line_index_cache.borrow_mut();
            if let Some(index) = cache.as_mut() {
                index.update(self);
            } else {
                *cache = Some(LineIndex::build(self));
            }
        }

        let cache = self.line_index_cache.borrow();
        Ref::map(cache, |c| c.as_ref().unwrap())
    }

    pub fn is_closing_code_fence(&self, source_line: crate::buffer::SourceLine) -> bool {
        self.line_index().is_closing_fence(source_line)
    }

    pub fn is_in_code_fence(&self, source_line: crate::buffer::SourceLine) -> bool {
        self.line_index().is_in_fence(source_line)
    }

    pub fn move_left(&mut self, select: bool) {
        self.column_goal = None;

        if !select && !self.selection.is_empty() {
            self.selection = Selection::cursor(self.selection.start());
            return;
        }

        let pos = self.selection.head;
        let new_pos = if pos.col > 0 {
            Position::new(pos.line, pos.col - 1)
        } else if pos.line.index() > 0 {
            Position::new(pos.line - 1, self.line_len(pos.line - 1))
        } else {
            pos
        };

        self.update_cursor(select, new_pos);
    }

    pub fn move_right(&mut self, select: bool) {
        self.column_goal = None;

        if !select && !self.selection.is_empty() {
            self.selection = Selection::cursor(self.selection.end());
            return;
        }

        let pos = self.selection.head;
        let line_len = self.line_len(pos.line);
        let new_pos = if pos.col < line_len {
            Position::new(pos.line, pos.col + 1)
        } else if (pos.line + 1).index() < self.line_count() {
            Position::new(pos.line + 1, 0)
        } else {
            pos
        };

        self.update_cursor(select, new_pos);
    }

    pub fn move_vertical(&mut self, is_down: bool, select: bool) {
        let pos = self.selection.head;
        let goal = match self.column_goal {
            Some(g) => g,
            None => {
                let g = pos.col;
                self.column_goal = Some(g);
                g
            }
        };

        let new_pos = if is_down {
            if (pos.line + 1).index() < self.line_count() {
                let next_line = pos.line + 1;
                Position::new(next_line, goal.min(self.line_len(next_line)))
            } else {
                Position::new(pos.line, self.line_len(pos.line))
            }
        } else if pos.line.index() > 0 {
            let prev_line = pos.line - 1;
            Position::new(prev_line, goal.min(self.line_len(prev_line)))
        } else {
            Position::new(crate::buffer::SourceLine::new(0), 0)
        };

        self.update_cursor(select, new_pos);
    }

    pub fn move_up(&mut self, select: bool) {
        self.move_vertical(false, select);
    }

    pub fn move_down(&mut self, select: bool) {
        self.move_vertical(true, select);
    }

    pub fn move_to_line_start(&mut self, select: bool) {
        self.column_goal = None;
        let pos = self.selection.head;
        let line = self.line_text(pos.line).unwrap_or_default();
        let trimmed = line.trim_start();
        let leading_spaces = line.len() - trimmed.len();
        let new_col = if pos.col > leading_spaces {
            leading_spaces
        } else {
            0
        };
        let new_pos = Position::new(pos.line, new_col);
        self.update_cursor(select, new_pos);
    }

    pub fn move_to_line_end(&mut self, select: bool) {
        self.column_goal = None;
        let line = self.selection.head.line;
        let new_pos = Position::new(line, self.line_len(line));
        self.update_cursor(select, new_pos);
    }

    pub fn move_to_doc_start(&mut self, select: bool) {
        self.column_goal = None;
        let new_pos = Position::new(crate::buffer::SourceLine::new(0), 0);
        self.update_cursor(select, new_pos);
    }

    pub fn move_to_doc_end(&mut self, select: bool) {
        self.column_goal = None;
        let last_line = self.line_count().saturating_sub(1);
        let new_pos = Position::new(
            crate::buffer::SourceLine::new(last_line),
            self.line_len(crate::buffer::SourceLine::new(last_line)),
        );
        self.update_cursor(select, new_pos);
    }

    pub fn move_to_previous_word(&mut self, select: bool) {
        self.column_goal = None;
        let pos = self.selection.head;
        if pos.col == 0 {
            if pos.line.index() > 0 {
                let prev_line = pos.line - 1;
                let new_pos = Position::new(prev_line, self.line_len(prev_line));
                self.update_cursor(select, new_pos);
            }
            return;
        }

        let chars: Vec<char> = self
            .line_text(pos.line)
            .unwrap_or_default()
            .chars()
            .collect();
        let mut idx = pos.col;

        // Skip left whitespace
        while idx > 0 && chars[idx - 1].is_whitespace() {
            idx -= 1;
        }

        if idx > 0 {
            let kind = char_kind(chars[idx - 1]);
            while idx > 0 && char_kind(chars[idx - 1]) == kind {
                idx -= 1;
            }
        }

        let new_pos = Position::new(pos.line, idx);
        self.update_cursor(select, new_pos);
    }

    pub fn move_to_next_word(&mut self, select: bool) {
        self.column_goal = None;
        let pos = self.selection.head;
        let line_len = self.line_len(pos.line);

        if pos.col >= line_len {
            if (pos.line + 1).index() < self.line_count() {
                let new_pos = Position::new(pos.line + 1, 0);
                self.update_cursor(select, new_pos);
            }
            return;
        }

        let chars: Vec<char> = self
            .line_text(pos.line)
            .unwrap_or_default()
            .chars()
            .collect();
        let mut idx = pos.col;

        let kind = char_kind(chars[idx]);
        while idx < chars.len() && char_kind(chars[idx]) == kind {
            idx += 1;
        }

        // Skip following whitespace
        while idx < chars.len() && chars[idx].is_whitespace() {
            idx += 1;
        }

        let new_pos = Position::new(pos.line, idx);
        self.update_cursor(select, new_pos);
    }

    pub fn select_all(&mut self) {
        self.column_goal = None;
        if self.line_count() == 0 {
            return;
        }
        let last_line = crate::buffer::SourceLine::new(self.line_count() - 1);
        let last_col = self.line_len(last_line);
        self.selection = Selection {
            anchor: Position::new(crate::buffer::SourceLine::new(0), 0),
            head: Position::new(last_line, last_col),
        };
    }

    pub fn select_word_at(&mut self, pos: Position) {
        self.column_goal = None;
        if pos.line.index() >= self.line_count() {
            return;
        }
        let line = self.line_text(pos.line).unwrap_or_default();
        let chars: Vec<char> = line.chars().collect();
        if chars.is_empty() {
            self.selection = Selection::cursor(Position::new(pos.line, 0));
            return;
        }
        let col = pos.col.min(chars.len().saturating_sub(1));
        let kind = char_kind(chars[col]);

        let mut start = col;
        while start > 0 && char_kind(chars[start - 1]) == kind {
            start -= 1;
        }
        let mut end = col;
        while end < chars.len() && char_kind(chars[end]) == kind {
            end += 1;
        }

        self.selection = Selection {
            anchor: Position::new(pos.line, start),
            head: Position::new(pos.line, end),
        };
    }

    pub fn select_line_at(&mut self, source_line: crate::buffer::SourceLine) {
        self.column_goal = None;
        let line_idx = source_line.index();
        if line_idx >= self.line_count() {
            return;
        }
        let line_len = self.line_len(crate::buffer::SourceLine::new(line_idx));
        self.selection = Selection {
            anchor: Position::new(source_line, 0),
            head: Position::new(source_line, line_len),
        };
    }

    pub fn drag_selection_to(&mut self, pos: Position) {
        self.column_goal = None;
        self.selection.head = self.clamp_position(pos);
    }
}
