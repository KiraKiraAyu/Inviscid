use super::prefix::{
    BlockPrefix, get_block_prefix_len, get_smart_continuation_prefix, is_thematic_break,
    parse_block_prefix, parse_heading_prefix,
};
use super::projection::{
    ProjectedBlock, ProjectedBlockKind, adjacent_buffer_live_editable_line, block_at_buffer,
};
use super::table::{
    TableMetadata, build_empty_table_row, get_cell_index_for_col, get_table_metadata,
    parse_table_row_cells,
};
use crate::buffer::{
    LineSource, Position, Selection, SourceLine, TextBuffer, char_col_to_byte_offset,
    remove_char_at_col,
};

/// Live Preview editing and structural cursor policy over a [`TextBuffer`].
///
/// Separates Markdown block-boundary rules (delimiter protection, block
/// prefix degeneration, fenced/math/table containment, smart continuation) from
/// pure Rope source primitives in `buffer`.
pub struct LiveBuffer<'a> {
    buffer: &'a mut TextBuffer,
}

impl<'a> LiveBuffer<'a> {
    pub fn new(buffer: &'a mut TextBuffer) -> Self {
        Self { buffer }
    }

    fn block_at(&self, line: SourceLine) -> Option<ProjectedBlock> {
        block_at_buffer(self.buffer, line)
    }

    fn adjacent_editable(&self, from_line: SourceLine, forward: bool) -> Option<SourceLine> {
        adjacent_buffer_live_editable_line(self.buffer, from_line, forward)
    }

    fn is_next_math_boundary(&self, line: SourceLine) -> bool {
        let line_count = self.buffer.line_count();
        let next = line + 1;
        if next.index() >= line_count {
            return false;
        }
        let Some(next_block) = self.block_at(next) else {
            return false;
        };
        if next_block.is_math_delimiter(next) {
            return true;
        }
        matches!(next_block.kind, ProjectedBlockKind::BlankSeparator)
            && (next + 1).index() < line_count
            && self
                .block_at(next + 1)
                .is_some_and(|after| after.is_math_delimiter(next + 1))
    }

    fn is_prev_math_boundary(&self, line: SourceLine) -> bool {
        if line.index() == 0 {
            return false;
        }
        let prev = line - 1;
        let Some(prev_block) = self.block_at(prev) else {
            return false;
        };
        if prev_block.is_math_delimiter(prev) {
            return true;
        }
        matches!(prev_block.kind, ProjectedBlockKind::BlankSeparator)
            && prev.index() > 0
            && self
                .block_at(prev - 1)
                .is_some_and(|before| before.is_math_delimiter(prev - 1))
    }

    pub fn move_left(&mut self, select: bool) {
        self.buffer.set_column_goal(None);

        if !select && !self.buffer.selection().is_empty() {
            self.buffer
                .set_selection_direct(Selection::cursor(self.buffer.selection().start()));
            return;
        }

        let pos = self.buffer.selection().head;
        if let Some(block) = self.block_at(pos.line)
            && block.is_math_delimiter(pos.line)
        {
            let start = block.source_range.start;
            let end = block.source_range.end - 1;
            let target = if pos.line == start {
                self.adjacent_editable(start, false)
                    .map(|line| Position::new(line, self.buffer.line_len(line)))
                    .unwrap_or(Position::new(start + 1, 0))
            } else if end.index() > start.index() + 1 {
                Position::new(end - 1, self.buffer.line_len(end - 1))
            } else {
                self.adjacent_editable(start, false)
                    .map(|line| Position::new(line, self.buffer.line_len(line)))
                    .unwrap_or(Position::new(start, 0))
            };
            self.buffer.update_cursor(select, target);
            return;
        }

        let new_pos = if let Some(meta) = get_table_metadata(self.buffer, pos.line) {
            let cells = parse_table_row_cells(&self.buffer.line_text(pos.line).unwrap_or_default());
            let cell_idx = get_cell_index_for_col(&cells, pos.col);
            if let Some(cell) = cells.get(cell_idx) {
                if pos.col > cell.col_range.1 {
                    Position::new(pos.line, cell.col_range.1)
                } else if pos.col > cell.col_range.0 {
                    Position::new(pos.line, pos.col - 1)
                } else if cell_idx > 0 {
                    Position::new(pos.line, cells[cell_idx - 1].col_range.1)
                } else if pos.line.index() == meta.delimiter_line.index() + 1 {
                    let header_cells = parse_table_row_cells(
                        &self.buffer.line_text(meta.header_line).unwrap_or_default(),
                    );
                    let col = header_cells.last().map(|c| c.col_range.1).unwrap_or(2);
                    Position::new(meta.header_line, col)
                } else if pos.line.index() > meta.delimiter_line.index() + 1 {
                    let prev_row = pos.line - 1;
                    let prev_cells =
                        parse_table_row_cells(&self.buffer.line_text(prev_row).unwrap_or_default());
                    let col = prev_cells.last().map(|c| c.col_range.1).unwrap_or(2);
                    Position::new(prev_row, col)
                } else if let Some(prev_line) = self.adjacent_editable(meta.header_line, false) {
                    Position::new(prev_line, self.buffer.line_len(prev_line))
                } else {
                    pos
                }
            } else {
                pos
            }
        } else {
            let line = self.buffer.line_text(pos.line).unwrap_or_default();
            let prefix_len = get_block_prefix_len(&line);

            if pos.col <= prefix_len {
                if let Some(prev_line) = self.adjacent_editable(pos.line, false) {
                    Position::new(prev_line, self.buffer.line_len(prev_line))
                } else {
                    Position::new(pos.line, prefix_len)
                }
            } else if pos.col > 0 {
                Position::new(pos.line, pos.col - 1)
            } else if let Some(prev_line) = self.adjacent_editable(pos.line, false) {
                Position::new(prev_line, self.buffer.line_len(prev_line))
            } else {
                pos
            }
        };

        self.buffer.update_cursor(select, new_pos);
    }

    pub fn move_right(&mut self, select: bool) {
        self.buffer.set_column_goal(None);

        if !select && !self.buffer.selection().is_empty() {
            self.buffer
                .set_selection_direct(Selection::cursor(self.buffer.selection().end()));
            return;
        }

        let pos = self.buffer.selection().head;
        if let Some(block) = self.block_at(pos.line)
            && block.is_math_delimiter(pos.line)
        {
            let start = block.source_range.start;
            let end = block.source_range.end - 1;
            let target = if pos.line == start {
                if end.index() > start.index() + 1 {
                    Position::new(start + 1, 0)
                } else {
                    self.adjacent_editable(end, true)
                        .map(|line| {
                            Position::new(
                                line,
                                get_block_prefix_len(
                                    &self.buffer.line_text(line).unwrap_or_default(),
                                ),
                            )
                        })
                        .unwrap_or(Position::new(end, 0))
                }
            } else {
                self.adjacent_editable(end, true)
                    .map(|line| {
                        Position::new(
                            line,
                            get_block_prefix_len(&self.buffer.line_text(line).unwrap_or_default()),
                        )
                    })
                    .unwrap_or(Position::new(end, 0))
            };
            self.buffer.update_cursor(select, target);
            return;
        }

        let line_len = self.buffer.line_len(pos.line);
        let new_pos = if let Some(meta) = get_table_metadata(self.buffer, pos.line) {
            let cells = parse_table_row_cells(&self.buffer.line_text(pos.line).unwrap_or_default());
            let cell_idx = get_cell_index_for_col(&cells, pos.col);
            if let Some(cell) = cells.get(cell_idx) {
                if pos.col < cell.col_range.0 {
                    Position::new(pos.line, cell.col_range.0)
                } else if pos.col < cell.col_range.1 {
                    Position::new(pos.line, pos.col + 1)
                } else if cell_idx + 1 < cells.len() {
                    Position::new(pos.line, cells[cell_idx + 1].col_range.0)
                } else if pos.line == meta.header_line {
                    let next_row = meta.delimiter_line.index() + 1;
                    if next_row < self.buffer.line_count()
                        && next_row <= meta.last_table_line.index()
                    {
                        let next_cells = parse_table_row_cells(
                            &self
                                .buffer
                                .line_text(SourceLine::new(next_row))
                                .unwrap_or_default(),
                        );
                        let col = next_cells.first().map(|c| c.col_range.0).unwrap_or(2);
                        Position::new(SourceLine::new(next_row), col)
                    } else {
                        pos
                    }
                } else if pos.line < meta.last_table_line {
                    let next_row = pos.line + 1;
                    let next_cells =
                        parse_table_row_cells(&self.buffer.line_text(next_row).unwrap_or_default());
                    let col = next_cells.first().map(|c| c.col_range.0).unwrap_or(2);
                    Position::new(next_row, col)
                } else if let Some(next_line) = self.adjacent_editable(meta.last_table_line, true) {
                    Position::new(
                        next_line,
                        get_block_prefix_len(&self.buffer.line_text(next_line).unwrap_or_default()),
                    )
                } else {
                    pos
                }
            } else {
                pos
            }
        } else {
            let line = self.buffer.line_text(pos.line).unwrap_or_default();
            let prefix_len = get_block_prefix_len(&line);

            if pos.col < prefix_len {
                // Step over hidden prefix directly to right of first visible character
                Position::new(pos.line, (prefix_len + 1).min(line_len))
            } else if pos.col < line_len {
                Position::new(pos.line, pos.col + 1)
            } else if let Some(next_line) = self.adjacent_editable(pos.line, true) {
                let next_prefix =
                    get_block_prefix_len(&self.buffer.line_text(next_line).unwrap_or_default());
                Position::new(next_line, next_prefix.min(self.buffer.line_len(next_line)))
            } else if !select
                && (self.buffer.is_in_code_fence(pos.line)
                    || self.buffer.is_closing_code_fence(pos.line))
                && self.ensure_trailing_code_fence_newline().is_some()
            {
                return;
            } else {
                pos
            }
        };

        self.buffer.update_cursor(select, new_pos);
    }

    pub fn move_vertical(&mut self, is_down: bool, select: bool) {
        let pos = self.buffer.selection().head;
        let goal = match self.buffer.column_goal() {
            Some(g) => g,
            None => {
                let g = pos.col;
                self.buffer.set_column_goal(Some(g));
                g
            }
        };

        let target_line = self.adjacent_editable(pos.line, is_down);
        let new_pos = if let Some(target) = target_line {
            let target_text = self.buffer.line_text(target).unwrap_or_default();
            let prefix = get_block_prefix_len(&target_text);
            let col = if target_text.trim_start().starts_with("```") {
                self.buffer.line_len(target)
            } else {
                goal.max(prefix).min(self.buffer.line_len(target))
            };
            Position::new(target, col)
        } else if is_down
            && !select
            && (self.buffer.is_in_code_fence(pos.line)
                || self.buffer.is_closing_code_fence(pos.line))
            && self.ensure_trailing_code_fence_newline().is_some()
        {
            return;
        } else if is_down {
            Position::new(pos.line, self.buffer.line_len(pos.line))
        } else {
            Position::new(SourceLine::new(0), 0)
        };

        self.buffer.update_cursor(select, new_pos);
    }

    pub fn move_up(&mut self, select: bool) {
        self.move_vertical(false, select);
    }

    pub fn move_down(&mut self, select: bool) {
        self.move_vertical(true, select);
    }

    pub fn move_to_line_start(&mut self, select: bool) {
        self.buffer.set_column_goal(None);
        let pos = self.buffer.selection().head;
        let line = self.buffer.line_text(pos.line).unwrap_or_default();
        let new_col = get_block_prefix_len(&line);
        self.buffer
            .update_cursor(select, Position::new(pos.line, new_col));
    }

    pub fn insert_newline(&mut self) {
        if self.buffer.selection().is_empty() {
            let pos = self.buffer.selection().head;
            let fence_hint = FenceHint::from_buffer(self.buffer, pos.line);
            let edit = newline_edit(self.buffer, fence_hint, pos.line, pos.col);
            self.buffer.replace_line_range(
                edit.start,
                edit.remove_count,
                edit.inserted,
                Selection::cursor(edit.cursor),
            );
            return;
        }

        // The selection is deleted and the newline inserted as one undo step.
        let (start, end) = self.buffer.selection().range();
        let (start, end) = (
            self.buffer.clamp_position(start),
            self.buffer.clamp_position(end),
        );
        let overlay = SelectionDeleted::new(self.buffer, start, end);
        let fence_hint = FenceHint::scan(&overlay, start.line);
        let edit = newline_edit(&overlay, fence_hint, start.line, start.col);
        let (from, remove_count, inserted) =
            overlay.compose(edit.start, edit.remove_count, edit.inserted);
        self.buffer.replace_line_range(
            from,
            remove_count,
            inserted,
            Selection::cursor(edit.cursor),
        );
    }

    pub fn insert_line_below_block(&mut self) {
        let pos = self.buffer.selection().head;
        let closing_idx = self
            .buffer
            .line_index()
            .paired_fence_range(pos.line)
            .map(|(_, close)| close.index());

        let anchor = closing_idx.unwrap_or(pos.line.index().min(self.buffer.line_count() - 1));
        let has_blank_after = anchor + 1 < self.buffer.line_count()
            && self
                .buffer
                .line_text(SourceLine::new(anchor + 1))
                .is_some_and(|line| line.trim().is_empty());
        let (insert_at, inserted_count) = if has_blank_after {
            let has_second_blank = anchor + 2 < self.buffer.line_count()
                && self
                    .buffer
                    .line_text(SourceLine::new(anchor + 2))
                    .is_some_and(|line| line.trim().is_empty());
            (
                SourceLine::new(anchor + 2),
                usize::from(!has_second_blank) + 1,
            )
        } else {
            (SourceLine::new(anchor + 1), 2)
        };
        self.buffer.replace_line_range(
            insert_at,
            0,
            vec![String::new(); inserted_count],
            Selection::cursor(Position::new(SourceLine::new(anchor + 2), 0)),
        );
    }

    pub(crate) fn ensure_math_block_content_line(
        &mut self,
        start_line: SourceLine,
    ) -> Option<Position> {
        let (start, end) = super::math_block_range(self.buffer, start_line)?;
        if start != start_line {
            return None;
        }
        if end.index() > start.index() + 1 {
            return Some(Position::new(start + 1, 0));
        }

        let pos = Position::new(end, 0);
        self.buffer
            .replace_line_range(end, 0, vec![String::new()], Selection::cursor(pos));
        Some(pos)
    }

    pub fn backspace(&mut self) {
        self.buffer.set_column_goal(None);
        if self.buffer.backspace_auto_pair() {
            return;
        }

        if !self.buffer.selection().is_empty() {
            self.buffer
                .replace_range(self.buffer.selection().range(), "");
            return;
        }

        let pos = self.buffer.selection().head;
        let block = self.block_at(pos.line);
        if block
            .as_ref()
            .is_some_and(|b| b.is_math_delimiter(pos.line))
        {
            return;
        }
        if pos.col == 0 && self.is_prev_math_boundary(pos.line) {
            return;
        }

        let line = self.buffer.line_text(pos.line).unwrap_or_default();
        if block
            .as_ref()
            .is_some_and(|b| matches!(b.kind, ProjectedBlockKind::ThematicBreak))
        {
            let blank_after = (pos.line + 1).index() < self.buffer.line_count()
                && self.buffer.source_line(pos.line + 1).trim().is_empty();
            if pos.line.index() > 0 && self.buffer.source_line(pos.line - 1).trim().is_empty() {
                self.buffer.replace_line_range(
                    pos.line,
                    1 + usize::from(blank_after),
                    vec![],
                    Selection::cursor(Position::new(pos.line - 1, 0)),
                );
            } else {
                self.buffer.replace_line_range(
                    pos.line,
                    1 + usize::from(blank_after),
                    vec![String::new()],
                    Selection::cursor(Position::new(pos.line, 0)),
                );
            }
            return;
        }

        if let Some(ref b) = block
            && matches!(b.kind, ProjectedBlockKind::CodeBlock { .. })
            && b.is_enclosed_container
            && pos.line == b.source_range.start
        {
            let min_col = b.editable_min_col(self.buffer, pos.line);
            if pos.col > min_col {
                self.buffer
                    .replace_range((Position::new(pos.line, pos.col - 1), pos), "");
            }
            return;
        }

        let leading_spaces = line.chars().take_while(|c| *c == ' ').count();
        if pos.col >= 4 && pos.col <= leading_spaces {
            let spaces = if pos.col % 4 == 0 { 4 } else { pos.col % 4 };
            self.buffer
                .replace_range((Position::new(pos.line, pos.col - spaces), pos), "");
            return;
        }

        if let Some(ref b) = block
            && matches!(b.kind, ProjectedBlockKind::CodeBlock { .. })
            && b.is_enclosed_container
            && b.source_range.end.index() - b.source_range.start.index() == 3
            && pos.line == b.source_range.start + 1
            && line.is_empty()
        {
            let start = pos.line - 1;
            let blank_after = (pos.line + 2).index() < self.buffer.line_count()
                && self.buffer.source_line(pos.line + 2).trim().is_empty();
            let blank_before =
                start.index() > 0 && self.buffer.source_line(start - 1).trim().is_empty();
            self.buffer.replace_line_range(
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
                && !self.buffer.source_line(pos.line - 1).trim().is_empty()
            {
                let mut inserted = vec![String::new(), content];
                if (pos.line + 1).index() >= self.buffer.line_count()
                    || !self.buffer.source_line(pos.line + 1).trim().is_empty()
                {
                    inserted.push(String::new());
                }
                (inserted, pos.line + 1)
            } else {
                (vec![content], pos.line)
            };
            self.buffer.replace_line_range(
                pos.line,
                1,
                inserted,
                Selection::cursor(Position::new(cursor_line, prefix.leading_spaces)),
            );
            return;
        }

        if pos.col > 0 {
            self.buffer
                .replace_range((Position::new(pos.line, pos.col - 1), pos), "");
        } else if pos.line.index() > 0 {
            if self
                .block_at(pos.line - 1)
                .is_some_and(|b| b.is_closing_fence(pos.line - 1))
            {
                let target = pos.line.saturating_sub(2);
                // A boundary jump is undoable even when it changes only the selection.
                self.buffer.replace_line_range(
                    pos.line,
                    usize::from(line.is_empty()),
                    vec![],
                    Selection::cursor(Position::new(target, self.buffer.line_len(target))),
                );
            } else if pos.line.index() >= 2
                && self.buffer.source_line(pos.line - 1).trim().is_empty()
                && self
                    .block_at(pos.line - 2)
                    .is_some_and(|b| b.is_closing_fence(pos.line - 2))
            {
                let target = pos.line.saturating_sub(3);
                self.buffer.replace_line_range(
                    pos.line - 1,
                    1,
                    vec![],
                    Selection::cursor(Position::new(target, self.buffer.line_len(target))),
                );
            } else {
                let previous = if pos.line.index() >= 2
                    && self.buffer.source_line(pos.line - 1).trim().is_empty()
                    && self
                        .block_at(pos.line - 2)
                        .is_some_and(|b| b.is_editable_line(pos.line - 2))
                {
                    pos.line - 2
                } else {
                    pos.line - 1
                };
                self.buffer.replace_range(
                    (Position::new(previous, self.buffer.line_len(previous)), pos),
                    "",
                );
            }
        }
    }

    pub fn delete_forward(&mut self) {
        self.buffer.set_column_goal(None);
        if !self.buffer.selection().is_empty() {
            self.buffer
                .replace_range(self.buffer.selection().range(), "");
            return;
        }

        let pos = self.buffer.selection().head;
        let Some(block) = self.block_at(pos.line) else {
            return;
        };
        if block.is_math_delimiter(pos.line)
            || (block.is_enclosed_container && !block.is_editable_line(pos.line))
        {
            return;
        }

        if matches!(block.kind, ProjectedBlockKind::ThematicBreak) {
            self.buffer.replace_line_range(
                pos.line,
                1,
                vec![String::new()],
                Selection::cursor(Position::new(pos.line, 0)),
            );
            return;
        }

        let line_len = self.buffer.line_len(pos.line);
        if pos.col < line_len {
            self.buffer
                .replace_range((pos, Position::new(pos.line, pos.col + 1)), "");
            return;
        }

        if (pos.line + 1).index() >= self.buffer.line_count() {
            return;
        }

        let allow_merge = if block.is_enclosed_container {
            block.next_intra_block_editable_line(pos.line).is_some()
        } else {
            !self.is_next_math_boundary(pos.line)
        };
        if allow_merge {
            self.buffer
                .replace_range((pos, Position::new(pos.line + 1, 0)), "");
        }
    }

    fn table_navigate_or_insert_next_row(
        &mut self,
        meta: &TableMetadata,
        target_cell_idx: Option<usize>,
    ) {
        let pos = self.buffer.selection().head;
        let resolve_col = |cells: &[super::TableCell]| match target_cell_idx {
            Some(idx) => cells.get(idx).map(|c| c.col_range.1).unwrap_or(2),
            None => cells.first().map(|c| c.col_range.1).unwrap_or(2),
        };

        if pos.line == meta.header_line {
            let next_row = meta.delimiter_line.index() + 1;
            if next_row < self.buffer.line_count() && next_row <= meta.last_table_line.index() {
                let next_cells = parse_table_row_cells(
                    &self
                        .buffer
                        .line_text(SourceLine::new(next_row))
                        .unwrap_or_default(),
                );
                let target_col = resolve_col(&next_cells);
                self.buffer
                    .set_selection_direct(Selection::cursor(Position::new(
                        SourceLine::new(next_row),
                        target_col,
                    )));
                return;
            }
            let new_row = build_empty_table_row(meta.num_columns);
            let cells = parse_table_row_cells(&new_row);
            let col = resolve_col(&cells);
            self.buffer.replace_line_range(
                meta.delimiter_line + 1,
                0,
                vec![new_row],
                Selection::cursor(Position::new(meta.delimiter_line + 1, col)),
            );
            return;
        }

        if pos.line < meta.last_table_line {
            let next_row = pos.line + 1;
            let next_cells =
                parse_table_row_cells(&self.buffer.line_text(next_row).unwrap_or_default());
            let target_col = resolve_col(&next_cells);
            self.buffer
                .set_selection_direct(Selection::cursor(Position::new(next_row, target_col)));
            return;
        }

        let new_row = build_empty_table_row(meta.num_columns);
        let insert_idx = meta.last_table_line.index() + 1;
        let cells = parse_table_row_cells(&new_row);
        let col = resolve_col(&cells);
        self.buffer.replace_line_range(
            SourceLine::new(insert_idx),
            0,
            vec![new_row],
            Selection::cursor(Position::new(SourceLine::new(insert_idx), col)),
        );
    }

    pub fn table_tab_forward(&mut self) -> bool {
        let pos = self.buffer.selection().head;
        let Some(meta) = get_table_metadata(self.buffer, pos.line) else {
            return false;
        };

        let current_cells =
            parse_table_row_cells(&self.buffer.line_text(pos.line).unwrap_or_default());
        let cell_idx = get_cell_index_for_col(&current_cells, pos.col);

        if cell_idx + 1 < current_cells.len() {
            let target_col = current_cells[cell_idx + 1].col_range.1;
            self.buffer
                .set_selection_direct(Selection::cursor(Position::new(pos.line, target_col)));
            return true;
        }

        self.table_navigate_or_insert_next_row(&meta, None);
        true
    }

    pub fn table_tab_backward(&mut self) -> bool {
        let pos = self.buffer.selection().head;
        let Some(meta) = get_table_metadata(self.buffer, pos.line) else {
            return false;
        };

        let current_cells =
            parse_table_row_cells(&self.buffer.line_text(pos.line).unwrap_or_default());
        let cell_idx = get_cell_index_for_col(&current_cells, pos.col);

        if cell_idx > 0 {
            let target_col = current_cells[cell_idx - 1].col_range.1;
            self.buffer
                .set_selection_direct(Selection::cursor(Position::new(pos.line, target_col)));
            return true;
        }

        if pos.line == meta.delimiter_line + 1 {
            let header_cells =
                parse_table_row_cells(&self.buffer.line_text(meta.header_line).unwrap_or_default());
            let target_col = header_cells.last().map(|c| c.col_range.1).unwrap_or(2);
            self.buffer
                .set_selection_direct(Selection::cursor(Position::new(
                    meta.header_line,
                    target_col,
                )));
            return true;
        }

        if pos.line > meta.delimiter_line + 1 {
            let prev_row = pos.line - 1;
            let prev_cells =
                parse_table_row_cells(&self.buffer.line_text(prev_row).unwrap_or_default());
            let target_col = prev_cells.last().map(|c| c.col_range.1).unwrap_or(2);
            self.buffer
                .set_selection_direct(Selection::cursor(Position::new(prev_row, target_col)));
            return true;
        }

        true
    }

    pub fn table_enter_next_row(&mut self) -> bool {
        let pos = self.buffer.selection().head;
        let Some(meta) = get_table_metadata(self.buffer, pos.line) else {
            return false;
        };

        let current_cells =
            parse_table_row_cells(&self.buffer.line_text(pos.line).unwrap_or_default());
        let cell_idx = get_cell_index_for_col(&current_cells, pos.col);

        self.table_navigate_or_insert_next_row(&meta, Some(cell_idx));
        true
    }

    pub fn table_backspace(&mut self) -> bool {
        let pos = self.buffer.selection().head;
        if get_table_metadata(self.buffer, pos.line).is_none() {
            return false;
        }

        if !self.buffer.selection().is_empty() {
            self.buffer.insert_text("");
            return true;
        }

        let current_cells =
            parse_table_row_cells(&self.buffer.line_text(pos.line).unwrap_or_default());
        let cell_idx = get_cell_index_for_col(&current_cells, pos.col);
        if let Some(cell) = current_cells.get(cell_idx) {
            if pos.col <= cell.col_range.0 {
                return true;
            }

            self.buffer.record_single_line_edit(pos.line, |line, sel| {
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
        let last_idx = self.buffer.line_count().saturating_sub(1);
        let last_line = SourceLine::new(last_idx);
        if self.buffer.is_in_code_fence(last_line) || self.buffer.is_closing_code_fence(last_line) {
            let closing = self.buffer.is_closing_code_fence(last_line);
            let inserted = if closing {
                vec![String::new(), String::new()]
            } else {
                vec!["```".to_string(), String::new()]
            };
            self.buffer.replace_line_range(
                SourceLine::new(self.buffer.line_count()),
                0,
                inserted,
                Selection::cursor(Position::new(
                    SourceLine::new(self.buffer.line_count() + 1),
                    0,
                )),
            );
            Some(self.buffer.selection().head)
        } else if last_idx > 0
            && (self.buffer.is_in_code_fence(SourceLine::new(last_idx - 1))
                || self
                    .buffer
                    .is_closing_code_fence(SourceLine::new(last_idx - 1)))
            && self
                .buffer
                .line_text(last_line)
                .unwrap_or_default()
                .trim()
                .is_empty()
        {
            self.buffer.replace_line_range(
                SourceLine::new(self.buffer.line_count()),
                0,
                vec![String::new()],
                Selection::cursor(Position::new(SourceLine::new(self.buffer.line_count()), 0)),
            );
            Some(self.buffer.selection().head)
        } else {
            None
        }
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
    let has_separator_before = super::blank_run_before_ends_with_separator(lines, block_line);
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

    fn raw_count(&self) -> usize {
        self.buffer.line_count() - self.end.line.index() + self.start.line.index()
    }

    fn padded(&self) -> bool {
        self.raw_count() == 1 && self.joined.is_empty()
    }

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

fn newline_edit(
    lines: &(impl LineSource + ?Sized),
    fence_hint: FenceHint,
    line: SourceLine,
    col: usize,
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
        return if next_has_content {
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

    if fence_hint.in_paired_fence {
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
        if !next_is_blank {
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
    let Some((start, end)) = super::math_block_range(lines, line) else {
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
