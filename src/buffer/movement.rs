
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
    
pub fn is_in_code_fence_lines(lines: &[String], target_idx: usize) -> bool {
    code_fence_status(lines, target_idx).0
}
let caret_map = Arc::new(CaretMap::from_lines(lines));

        let mut line_utf16_offsets = Vec::with_capacity(lines.len());
        let mut current_offset = 0;

        for line in lines {
            line_utf16_offsets.push(current_offset);
            let line_len: usize = line.chars().map(|c| c.len_utf16()).sum();
            current_offset += line_len + 1;
        }

        Self {
            version,
            caret_map,
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
            self.caret_map.fence_state(target_idx).0
    }

    fn is_closing_fence(&self, target_idx: usize) -> bool {
        self.caret_map.fence_state(target_idx).1
    }

    fn math_pair(&self, line: SourceLine) -> Option<(SourceLine, SourceLine)> {
        self.caret_map.math_pair(line)
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

    self.find_next_editable_source_line(SourceLine::new(from_line), is_source)
            .map(SourceLine::index)
    }

    pub fn find_prev_editable_line(&self, from_line: usize, is_source: bool) -> Option<usize> {
        self.find_prev_editable_source_line(SourceLine::new(from_line), is_source)
            .map(SourceLine::index)
    }

    pub fn move_left_mode(&mut self, select: bool, is_source: bool) {
        self.column_goal = None;

        if !select && !self.selection.is_empty() {
            self.selection = Selection::cursor(self.selection.start());
            return;
        }
        let (start_idx, end_idx) = (start.index(), end.index());
                let target = if pos_line == start {
                    self.find_prev_editable_source_line(start, false)
                        .map(|line| Position::new(line.index(), self.line_len(line.index())))
                        .unwrap_or(Position::new(start_idx + 1, 0))
                } else if end_idx > start_idx + 1 {
                    Position::new(end_idx - 1, self.line_len(end_idx - 1))
                } else {
                    self.find_prev_editable_source_line(start, false)
                        .map(|line| Position::new(line.index(), self.line_len(line.index())))
                        .unwrap_or(Position::new(start_idx, 0))
                };
                drop(index);
                self.update_cursor(select, target);
                return;
            }
        }
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
                    Position::new(prev_line.index(), self.line_len(prev_line.index()))
                } else {
                    pos
                }
            } else {
                pos
            }
        } else {
        } else if let Some(prev_line) =
                self.find_prev_editable_source_line(pos.source_line(), is_source)
            {
                Position::new(prev_line.index(), self.line_len(prev_line.index()))
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
        let (start_idx, end_idx) = (start.index(), end.index());
                let target = if pos_line == start {
                    if end_idx > start_idx + 1 {
                        Position::new(start_idx + 1, 0)
                    } else {
                        self.find_next_editable_source_line(end, false)
                            .map(|line| {
                                let line_idx = line.index();
                                Position::new(line_idx, get_block_prefix_len(&self.lines[line_idx]))
                            })
                            .unwrap_or(Position::new(end_idx, 0))
                    }
                } else {
                    self.find_next_editable_source_line(end, false)
                        .map(|line| {
                            let line_idx = line.index();
                            Position::new(line_idx, get_block_prefix_len(&self.lines[line_idx]))
                        })
                        .unwrap_or(Position::new(end_idx, 0))
                };
                drop(index);
                self.update_cursor(select, target);
                return;
            }
        }
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
                    
            } else {
                pos
            }
        } else {
            let line = &self.lines[pos.line];
            let prefix_len = get_block_prefix_len(line);

            if pos.col < prefix_len {
                
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
                self.find_prev_editable_source_line(pos.source_line(), is_source)
            };

            if let Some(target) = target_line {
                let target_idx = target.index();
                let prefix = get_block_prefix_len(&self.lines[target_idx]);
                let col = if self.lines[target_idx].trim_start().starts_with("```") {
                    self.line_len(target_idx)
                } else {
                    goal.max(prefix).min(self.line_len(target_idx))
                };
                Position::new(target_idx, col)
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
