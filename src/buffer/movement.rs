use super::TextBuffer;
use super::primitives::{Position, Selection};
use crate::markdown::{compute_syntactic_separators, get_block_prefix_len};
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

fn code_fence_status(lines: &[String], target_idx: usize) -> (bool, bool) {
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

pub fn is_closing_code_fence_lines(lines: &[String], target_idx: usize) -> bool {
    code_fence_status(lines, target_idx).1
}

pub fn is_in_code_fence_lines(lines: &[String], target_idx: usize) -> bool {
    code_fence_status(lines, target_idx).0
}

#[derive(Clone, Debug)]
pub(crate) struct LineIndex {
    version: usize,
    syntactic_separators: Vec<bool>,
    fence_pairs: Vec<(usize, usize)>,
    pub(crate) line_utf16_offsets: Vec<usize>,
}

impl LineIndex {
    fn build(version: usize, lines: &[String]) -> Self {
        let syntactic_separators = compute_syntactic_separators(lines);

        let mut fence_pairs = Vec::new();
        let mut open: Option<usize> = None;
        let mut line_utf16_offsets = Vec::with_capacity(lines.len());
        let mut current_offset = 0;

        for (i, line) in lines.iter().enumerate() {
            line_utf16_offsets.push(current_offset);
            let line_len: usize = line.chars().map(|c| c.len_utf16()).sum();
            current_offset += line_len + 1;

            if line.trim_start().starts_with("```") {
                match open {
                    Some(open_idx) => {
                        fence_pairs.push((open_idx, i));
                        open = None;
                    }
                    None => open = Some(i),
                }
            }
        }

        Self {
            version,
            syntactic_separators,
            fence_pairs,
            line_utf16_offsets,
        }
    }

    pub(crate) fn line_utf16_offset(&self, line_idx: usize) -> usize {
        self.line_utf16_offsets.get(line_idx).copied().unwrap_or(0)
    }

    pub(crate) fn utf16_offset_to_line(&self, offset: usize) -> usize {
        if self.line_utf16_offsets.is_empty() {
            return 0;
        }
        self.line_utf16_offsets
            .partition_point(|&o| o <= offset)
            .saturating_sub(1)
            .min(self.line_utf16_offsets.len() - 1)
    }

    fn fence_state(&self, target_idx: usize) -> (bool, bool) {
        for &(open_idx, close_idx) in &self.fence_pairs {
            if target_idx == open_idx {
                return (true, false);
            } else if target_idx == close_idx {
                return (false, true);
            } else if target_idx > open_idx && target_idx < close_idx {
                return (true, false);
            }
        }
        (false, false)
    }

    fn is_in_fence(&self, target_idx: usize) -> bool {
        self.fence_state(target_idx).0
    }

    fn is_closing_fence(&self, target_idx: usize) -> bool {
        self.fence_state(target_idx).1
    }
}

impl TextBuffer {
    pub(crate) fn line_index(&self) -> Ref<'_, LineIndex> {
        {
            let cache = self.line_index_cache.borrow();
            if let Some(index) = cache.as_ref()
                && index.version == self.version
            {
                return Ref::map(cache, |c| c.as_ref().unwrap());
            }
        }

        *self.line_index_cache.borrow_mut() = Some(LineIndex::build(self.version, &self.lines));

        let cache = self.line_index_cache.borrow();
        Ref::map(cache, |c| c.as_ref().unwrap())
    }

    pub fn is_closing_code_fence(&self, target_idx: usize) -> bool {
        self.line_index().is_closing_fence(target_idx)
    }

    pub fn is_in_code_fence(&self, target_idx: usize) -> bool {
        self.line_index().is_in_fence(target_idx)
    }

    fn is_line_editable(&self, index: &LineIndex, line: usize) -> bool {
        !crate::markdown::table::is_table_delimiter_row(&self.lines[line])
            && !index
                .syntactic_separators
                .get(line)
                .copied()
                .unwrap_or(false)
            && !index.is_closing_fence(line)
    }

    pub fn find_next_editable_line(&self, from_line: usize) -> Option<usize> {
        let index = self.line_index();
        ((from_line + 1)..self.lines.len()).find(|&line| self.is_line_editable(&index, line))
    }

    pub fn find_prev_editable_line(&self, from_line: usize) -> Option<usize> {
        let index = self.line_index();
        (0..from_line)
            .rev()
            .find(|&line| self.is_line_editable(&index, line))
    }

    pub fn move_left_mode(&mut self, select: bool, is_source: bool) {
        self.column_goal = None;

        if !select && !self.selection.is_empty() {
            self.selection = Selection::cursor(self.selection.start());
            return;
        }

        let pos = self.selection.head;
        let new_pos = if is_source {
            if pos.col > 0 {
                Position::new(pos.line, pos.col - 1)
            } else if pos.line > 0 {
                Position::new(pos.line - 1, self.line_len(pos.line - 1))
            } else {
                pos
            }
        } else if let Some(meta) = crate::markdown::table::get_table_metadata(&self.lines, pos.line)
        {
            let cells = crate::markdown::table::parse_table_row_cells(&self.lines[pos.line]);
            let cell_idx = crate::markdown::table::get_cell_index_for_col(&cells, pos.col);
            if let Some(cell) = cells.get(cell_idx) {
                if pos.col > cell.col_range.1 {
                    Position::new(pos.line, cell.col_range.1)
                } else if pos.col > cell.col_range.0 {
                    Position::new(pos.line, pos.col - 1)
                } else if cell_idx > 0 {
                    Position::new(pos.line, cells[cell_idx - 1].col_range.1)
                } else if pos.line == meta.delimiter_line + 1 {
                    let header_cells = crate::markdown::table::parse_table_row_cells(
                        &self.lines[meta.header_line],
                    );
                    let col = header_cells.last().map(|c| c.col_range.1).unwrap_or(2);
                    Position::new(meta.header_line, col)
                } else if pos.line > meta.delimiter_line + 1 {
                    let prev_row = pos.line - 1;
                    let prev_cells =
                        crate::markdown::table::parse_table_row_cells(&self.lines[prev_row]);
                    let col = prev_cells.last().map(|c| c.col_range.1).unwrap_or(2);
                    Position::new(prev_row, col)
                } else if let Some(prev_line) = self.find_prev_editable_line(meta.header_line) {
                    Position::new(prev_line, self.line_len(prev_line))
                } else {
                    pos
                }
            } else {
                pos
            }
        } else {
            let line = &self.lines[pos.line];
            let prefix_len = get_block_prefix_len(line);

            if pos.col <= prefix_len {
                if let Some(prev_line) = self.find_prev_editable_line(pos.line) {
                    Position::new(prev_line, self.line_len(prev_line))
                } else {
                    Position::new(pos.line, prefix_len)
                }
            } else if pos.col > 0 {
                Position::new(pos.line, pos.col - 1)
            } else if let Some(prev_line) = self.find_prev_editable_line(pos.line) {
                Position::new(prev_line, self.line_len(prev_line))
            } else {
                pos
            }
        };

        self.update_cursor(select, new_pos);
    }

    pub fn move_right_mode(&mut self, select: bool, is_source: bool) {
        self.column_goal = None;

        if !select && !self.selection.is_empty() {
            self.selection = Selection::cursor(self.selection.end());
            return;
        }

        let pos = self.selection.head;
        let line_len = self.line_len(pos.line);

        let new_pos = if is_source {
            if pos.col < line_len {
                Position::new(pos.line, pos.col + 1)
            } else if pos.line + 1 < self.lines.len() {
                Position::new(pos.line + 1, 0)
            } else {
                pos
            }
        } else if let Some(meta) = crate::markdown::table::get_table_metadata(&self.lines, pos.line)
        {
            let cells = crate::markdown::table::parse_table_row_cells(&self.lines[pos.line]);
            let cell_idx = crate::markdown::table::get_cell_index_for_col(&cells, pos.col);
            if let Some(cell) = cells.get(cell_idx) {
                if pos.col < cell.col_range.0 {
                    Position::new(pos.line, cell.col_range.0)
                } else if pos.col < cell.col_range.1 {
                    Position::new(pos.line, pos.col + 1)
                } else if cell_idx + 1 < cells.len() {
                    // Jump to start of next cell
                    Position::new(pos.line, cells[cell_idx + 1].col_range.0)
                } else if pos.line == meta.header_line {
                    let next_row = meta.delimiter_line + 1;
                    if next_row < self.lines.len() && next_row <= meta.last_table_line {
                        let next_cells =
                            crate::markdown::table::parse_table_row_cells(&self.lines[next_row]);
                        let col = next_cells.first().map(|c| c.col_range.0).unwrap_or(2);
                        Position::new(next_row, col)
                    } else {
                        pos
                    }
                } else if pos.line < meta.last_table_line {
                    let next_row = pos.line + 1;
                    let next_cells =
                        crate::markdown::table::parse_table_row_cells(&self.lines[next_row]);
                    let col = next_cells.first().map(|c| c.col_range.0).unwrap_or(2);
                    Position::new(next_row, col)
                } else if let Some(next_line) = self.find_next_editable_line(meta.last_table_line) {
                    Position::new(next_line, get_block_prefix_len(&self.lines[next_line]))
                } else {
                    pos
                }
            } else {
                pos
            }
        } else {
            let line = &self.lines[pos.line];
            let prefix_len = get_block_prefix_len(line);

            if pos.col < prefix_len {
                // Step over hidden prefix directly to right of first visible character
                Position::new(pos.line, (prefix_len + 1).min(line_len))
            } else if pos.col < line_len {
                Position::new(pos.line, pos.col + 1)
            } else if let Some(next_line) = self.find_next_editable_line(pos.line) {
                let next_prefix = get_block_prefix_len(&self.lines[next_line]);
                Position::new(next_line, next_prefix.min(self.line_len(next_line)))
            } else if !select
                && (self.is_in_code_fence(pos.line) || self.is_closing_code_fence(pos.line))
                && self.ensure_trailing_code_fence_newline().is_some()
            {
                return;
            } else {
                pos
            }
        };

        self.update_cursor(select, new_pos);
    }

    pub fn move_vertical_mode(&mut self, is_down: bool, select: bool, is_source: bool) {
        let pos = self.selection.head;
        let goal = match self.column_goal {
            Some(g) => g,
            None => {
                let g = pos.col;
                self.column_goal = Some(g);
                g
            }
        };

        let new_pos = if is_source {
            if is_down {
                if pos.line + 1 < self.lines.len() {
                    let next_line = pos.line + 1;
                    Position::new(next_line, goal.min(self.line_len(next_line)))
                } else {
                    Position::new(pos.line, self.line_len(pos.line))
                }
            } else if pos.line > 0 {
                let prev_line = pos.line - 1;
                Position::new(prev_line, goal.min(self.line_len(prev_line)))
            } else {
                Position::new(0, 0)
            }
        } else {
            let target_line = if is_down {
                self.find_next_editable_line(pos.line)
            } else {
                self.find_prev_editable_line(pos.line)
            };

            if let Some(target) = target_line {
                let prefix = get_block_prefix_len(&self.lines[target]);
                let col = if self.lines[target].trim_start().starts_with("```") {
                    self.line_len(target)
                } else {
                    goal.max(prefix).min(self.line_len(target))
                };
                Position::new(target, col)
            } else if is_down
                && !select
                && (self.is_in_code_fence(pos.line) || self.is_closing_code_fence(pos.line))
                && self.ensure_trailing_code_fence_newline().is_some()
            {
                return;
            } else if is_down {
                Position::new(pos.line, self.line_len(pos.line))
            } else {
                Position::new(0, 0)
            }
        };

        self.update_cursor(select, new_pos);
    }

    pub fn move_up_mode(&mut self, select: bool, is_source: bool) {
        self.move_vertical_mode(false, select, is_source);
    }

    pub fn move_down_mode(&mut self, select: bool, is_source: bool) {
        self.move_vertical_mode(true, select, is_source);
    }

    pub fn move_to_line_start_mode(&mut self, select: bool, is_source: bool) {
        self.column_goal = None;
        let pos = self.selection.head;
        let line = &self.lines[pos.line];

        let new_col = if is_source {
            let trimmed = line.trim_start();
            let leading_spaces = line.len() - trimmed.len();
            if pos.col > leading_spaces {
                leading_spaces
            } else {
                0
            }
        } else {
            get_block_prefix_len(line)
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
        let new_pos = Position::new(0, 0);
        self.update_cursor(select, new_pos);
    }

    pub fn move_to_doc_end(&mut self, select: bool) {
        self.column_goal = None;
        let last_line = self.lines.len().saturating_sub(1);
        let new_pos = Position::new(last_line, self.line_len(last_line));
        self.update_cursor(select, new_pos);
    }

    pub fn move_to_previous_word(&mut self, select: bool) {
        self.column_goal = None;
        let pos = self.selection.head;
        if pos.col == 0 {
            if pos.line > 0 {
                let prev_line = pos.line - 1;
                let new_pos = Position::new(prev_line, self.line_len(prev_line));
                self.update_cursor(select, new_pos);
            }
            return;
        }

        let chars: Vec<char> = self.lines[pos.line].chars().collect();
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
            if pos.line + 1 < self.lines.len() {
                let new_pos = Position::new(pos.line + 1, 0);
                self.update_cursor(select, new_pos);
            }
            return;
        }

        let chars: Vec<char> = self.lines[pos.line].chars().collect();
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
        if self.lines.is_empty() {
            return;
        }
        let last_line = self.lines.len() - 1;
        let last_col = self.line_len(last_line);
        self.selection = Selection {
            anchor: Position::new(0, 0),
            head: Position::new(last_line, last_col),
        };
    }

    pub fn select_word_at(&mut self, pos: Position) {
        self.column_goal = None;
        if pos.line >= self.lines.len() {
            return;
        }
        let line = &self.lines[pos.line];
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

    pub fn select_line_at(&mut self, line_idx: usize) {
        self.column_goal = None;
        if line_idx >= self.lines.len() {
            return;
        }
        let line_len = self.line_len(line_idx);
        self.selection = Selection {
            anchor: Position::new(line_idx, 0),
            head: Position::new(line_idx, line_len),
        };
    }

    pub fn drag_selection_to(&mut self, pos: Position) {
        self.column_goal = None;
        self.selection.head = self.clamp_position(pos);
    }
}
