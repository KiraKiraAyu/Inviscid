use super::char_col_to_byte_offset;
use super::movement::{is_closing_code_fence_lines, is_in_code_fence_lines};
use super::primitives::{Position, Selection};
use super::{SourceLines, TextBuffer};
use crate::markdown::{
    BlockPrefix, current_indent_relative, get_smart_continuation_prefix, indent_col,
    is_thematic_break, list_marker, parse_block_prefix, parse_heading_prefix, quote_prefix_len,
};

fn ensure_min_empty_lines(lines: &mut SourceLines) {
    if lines.is_empty() {
        lines.push(String::new());
        lines.push(String::new());
    } else if lines.len() == 1 && lines[crate::buffer::SourceLine::new(0)].is_empty() {
        lines.push(String::new());
    }
}

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

fn delete_range_internal(lines: &mut SourceLines, (start, end): (Position, Position)) {
    if start == end {
        return;
    }
    if start.line == end.line {
        let line = &mut lines[start.line];
        let start_byte = char_col_to_byte_offset(line, start.col);
        let end_byte = char_col_to_byte_offset(line, end.col);
        let start_byte = start_byte.min(end_byte);
        line.drain(start_byte..end_byte);
    } else {
        let start_byte = char_col_to_byte_offset(&lines[start.line], start.col);
        let end_byte = char_col_to_byte_offset(&lines[end.line], end.col);
        let suffix = lines[end.line][end_byte..].to_string();
        lines[start.line].truncate(start_byte);
        lines[start.line].push_str(&suffix);

        if end.line > start.line {
            lines.drain_source_lines((start.line + 1)..=end.line);
        }
    }

    ensure_min_empty_lines(lines);
}

fn append_block_below_buffer(
    buffer: &mut TextBuffer,
    current_line: crate::buffer::SourceLine,
) -> crate::buffer::SourceLine {
    let line_count = buffer.line_count();
    let has_blank_after = current_line.index() + 1 < line_count
        && buffer
            .line_text(current_line + 1)
            .is_some_and(|line| line.trim().is_empty());
    let (insert_at, inserted_count) = if has_blank_after {
        let has_second_blank = current_line.index() + 2 < line_count
            && buffer
                .line_text(current_line + 2)
                .is_some_and(|line| line.trim().is_empty());
        (current_line + 2, usize::from(!has_second_blank) + 1)
    } else {
        (current_line + 1, 2)
    };
    let target = current_line + 2;
    buffer.replace_line_range(
        insert_at,
        0,
        vec![String::new(); inserted_count],
        Selection::cursor(Position::new(target, 0)),
    );
    target
}

fn append_block_below(
    lines: &mut SourceLines,
    current_line: crate::buffer::SourceLine,
) -> crate::buffer::SourceLine {
    if current_line + 1 < crate::buffer::SourceLine::new(lines.len())
        && lines[current_line + 1].trim().is_empty()
    {
        lines.insert_source_line(current_line + 2, String::new());
        if (current_line + 3).index() >= lines.len() || !lines[current_line + 3].trim().is_empty() {
            lines.insert_source_line(current_line + 3, String::new());
        }
        current_line + 2
    } else {
        lines.insert_source_line(current_line + 1, String::new());
        lines.insert_source_line(current_line + 2, String::new());
        current_line + 2
    }
}

fn is_in_paired_fence(buffer: &TextBuffer, target_line: crate::buffer::SourceLine) -> bool {
    let mut open_fence = None;
    for idx in 0..buffer.line_count() {
        let line = buffer
            .line_text(crate::buffer::SourceLine::new(idx))
            .unwrap_or_default();
        if !line.trim_start().starts_with("```") {
            continue;
        }
        if let Some(start) = open_fence.take() {
            if (start..=idx).contains(&target_line.index()) {
                return true;
            }
        } else {
            open_fence = Some(idx);
        }
    }
    false
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
        let mut open_fence = None;
        for idx in 0..self.line_count() {
            let line = self
                .line_text(crate::buffer::SourceLine::new(idx))
                .unwrap_or_default();
            if !line.trim_start().starts_with("```") {
                continue;
            }
            if let Some(start) = open_fence.take() {
                if (start..=idx).contains(&cursor_line.index()) {
                    let detect_start = start + 1;
                    let detect_end = idx.min(detect_start + 50);
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
                    return 4;
                }
            } else {
                open_fence = Some(idx);
            }
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
            self.record_edit(|lines, sel| {
                lines.replace_all(new_text.split('\n').map(String::from).collect());
                ensure_min_empty_lines(lines);
                *sel = Selection::cursor(Position::default());
            });
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
            let current_line = self.line_text(pos.line).unwrap_or_default();
            let line_count = self.line_count();
            if current_line.is_empty() {
                if is_source
                    || (pos.line.index() + 1 < line_count
                        && self
                            .line_text(pos.line + 1)
                            .is_some_and(|line| !line.trim().is_empty()))
                {
                    self.replace_line_range(
                        pos.line + 1,
                        0,
                        vec![String::new()],
                        Selection::cursor(Position::new(pos.line + 1, 0)),
                    );
                } else {
                    append_block_below_buffer(self, pos.line);
                }
                return;
            }

            let current_chars_len = current_line.chars().count();
            let col = pos.col.min(current_chars_len);
            let trimmed = current_line.trim_start();
            let leading_spaces = current_line.len() - trimmed.len();
            let split_byte = char_col_to_byte_offset(&current_line, col);
            let left = current_line[..split_byte].to_string();
            let right = current_line[split_byte..].to_string();

            if is_source && !trimmed.starts_with("```") && trimmed != "$$"
                || !is_source && is_in_paired_fence(self, pos.line)
            {
                self.replace_line_range(
                    pos.line,
                    1,
                    vec![left, right],
                    Selection::cursor(Position::new(pos.line + 1, 0)),
                );
                return;
            }

            if trimmed.starts_with("```") {
                let has_closing = (pos.line.index() + 1..line_count).any(|idx| {
                    self.line_text(crate::buffer::SourceLine::new(idx))
                        .is_some_and(|line| line.trim().starts_with("```"))
                });
                let inserted = if has_closing {
                    vec![current_line]
                } else {
                    vec![
                        current_line,
                        String::new(),
                        "```".to_string(),
                        String::new(),
                    ]
                };
                self.replace_line_range(
                    pos.line,
                    1,
                    inserted,
                    Selection::cursor(Position::new(pos.line + 1, 0)),
                );
                return;
            }

            if trimmed == "$$" && col == current_chars_len {
                // Math delimiter pairing is context-sensitive; keep its established path.
            } else if is_thematic_break(trimmed) {
                append_block_below_buffer(self, pos.line);
                return;
            } else if let Some((_level, prefix_len)) = parse_heading_prefix(trimmed) {
                let actual_prefix_len = leading_spaces + prefix_len;
                if col <= actual_prefix_len {
                    self.replace_line_range(
                        pos.line,
                        0,
                        vec![String::new()],
                        Selection::cursor(Position::new(pos.line + 1, actual_prefix_len)),
                    );
                } else if col >= current_chars_len {
                    append_block_below_buffer(self, pos.line);
                } else {
                    self.replace_line_range(
                        pos.line,
                        1,
                        vec![left, String::new(), right],
                        Selection::cursor(Position::new(pos.line + 2, 0)),
                    );
                }
                return;
            } else if let Some(prefix) = parse_block_prefix(&current_line)
                && !matches!(
                    prefix.kind,
                    BlockPrefix::Heading { .. } | BlockPrefix::Fence { .. }
                )
                && current_chars_len <= prefix.total_len
            {
                let next_line_is_blank = pos.line.index() + 1 < line_count
                    && self
                        .line_text(pos.line + 1)
                        .is_some_and(|line| line.trim().is_empty());
                let mut inserted = vec![String::new(), String::new()];
                if !is_source && !next_line_is_blank {
                    inserted.push(String::new());
                }
                self.replace_line_range(
                    pos.line,
                    1,
                    inserted,
                    Selection::cursor(Position::new(pos.line + 1, 0)),
                );
                return;
            } else if let Some(prefix) = get_smart_continuation_prefix(&current_line) {
                let prefix_len = prefix.chars().count();
                self.replace_line_range(
                    pos.line,
                    1,
                    vec![left, format!("{}{}", prefix, right)],
                    Selection::cursor(Position::new(pos.line + 1, prefix_len)),
                );
                return;
            } else if is_source {
                self.replace_line_range(
                    pos.line,
                    1,
                    vec![left, right],
                    Selection::cursor(Position::new(pos.line + 1, 0)),
                );
                return;
            } else if col == 0 {
                self.replace_line_range(
                    pos.line,
                    0,
                    vec![String::new(), String::new()],
                    Selection::cursor(Position::new(pos.line, 0)),
                );
                return;
            } else if col >= current_chars_len {
                append_block_below_buffer(self, pos.line);
                return;
            } else {
                self.replace_line_range(
                    pos.line,
                    1,
                    vec![left, String::new(), right],
                    Selection::cursor(Position::new(pos.line + 2, 0)),
                );
                return;
            }
        }

        self.record_edit(|lines, sel| {
            if !sel.is_empty() {
                delete_range_internal(lines, sel.range());
                let (start, _) = sel.range();
                *sel = Selection::cursor(start);
            }

            let pos = sel.head;
            let current_line = &lines[pos.line];
            let current_chars_len = current_line.chars().count();
            let col = pos.col.min(current_chars_len);
            let trimmed = current_line.trim_start();
            let leading_spaces = current_line.len() - trimmed.len();

            if current_line.is_empty() {
                if is_source
                    || ((pos.line + 1).index() < lines.len()
                        && !lines[pos.line + 1].trim().is_empty())
                {
                    lines.insert_source_line(pos.line + 1, String::new());
                    *sel = Selection::cursor(Position::new(pos.line + 1, 0));
                } else {
                    let target = append_block_below(lines, pos.line);
                    *sel = Selection::cursor(Position::new(target, 0));
                }
                return;
            }

            let split_b = char_col_to_byte_offset(current_line, col);
            let left = current_line[..split_b].to_string();
            let right = current_line[split_b..].to_string();

            if is_in_code_fence_lines(lines.as_slice(), pos.line)
                || (is_source && !trimmed.starts_with("```") && trimmed != "$$")
            {
                lines[pos.line] = left;
                lines.insert_source_line(pos.line + 1, right);
                *sel = Selection::cursor(Position::new(pos.line + 1, 0));
                return;
            }

            if trimmed.starts_with("```") {
                let has_closing = lines.as_slice()[(pos.line + 1).index()..]
                    .iter()
                    .any(|l| l.trim().starts_with("```"));
                if !has_closing {
                    lines.insert_source_line(pos.line + 1, String::new());
                    lines.insert_source_line(pos.line + 2, "```".to_string());
                    lines.insert_source_line(pos.line + 3, String::new());
                }
                *sel = Selection::cursor(Position::new(pos.line + 1, 0));
                return;
            }

            if trimmed == "$$" && col == current_chars_len {
                let pair = crate::markdown::math_block_pairs(lines.as_slice())
                    .into_iter()
                    .find(|&(start, end)| start <= pos.line && pos.line <= end);
                if pair.is_none_or(|(start, _)| start == pos.line) {
                    if pair.is_some_and(|(start, end)| end.index() == start.index() + 1) {
                        lines.insert_source_line(pos.line + 1, String::new());
                        *sel = Selection::cursor(Position::new(pos.line + 1, 0));
                    } else if pair.is_some() {
                        *sel = Selection::cursor(Position::new(pos.line + 1, 0));
                    } else {
                        lines.insert_source_line(pos.line + 1, String::new());
                        lines.insert_source_line(pos.line + 2, "$$".to_string());
                        lines.insert_source_line(pos.line + 3, String::new());
                        *sel = Selection::cursor(Position::new(pos.line + 1, 0));
                    }
                    return;
                }
            }

            if is_thematic_break(trimmed) {
                let target = append_block_below(lines, pos.line);
                *sel = Selection::cursor(Position::new(target, 0));
                return;
            }

            if let Some((_level, prefix_len)) = parse_heading_prefix(trimmed) {
                let actual_prefix_len = leading_spaces + prefix_len;

                if col <= actual_prefix_len {
                    lines.insert_source_line(pos.line, String::new());
                    *sel = Selection::cursor(Position::new(pos.line + 1, actual_prefix_len));
                } else if col >= current_chars_len {
                    let target = append_block_below(lines, pos.line);
                    *sel = Selection::cursor(Position::new(target, 0));
                } else {
                    lines[pos.line] = left;
                    lines.insert_source_line(pos.line + 1, String::new());
                    lines.insert_source_line(pos.line + 2, right);
                    *sel = Selection::cursor(Position::new(pos.line + 2, 0));
                }
                return;
            }

            if let Some(parsed) = parse_block_prefix(current_line)
                && !matches!(
                    parsed.kind,
                    BlockPrefix::Heading { .. } | BlockPrefix::Fence { .. }
                )
                && current_chars_len <= parsed.total_len
            {
                lines[pos.line] = String::new();
                lines.insert_source_line(pos.line + 1, String::new());
                if !is_source
                    && ((pos.line + 2).index() >= lines.len()
                        || !lines[pos.line + 2].trim().is_empty())
                {
                    lines.insert_source_line(pos.line + 2, String::new());
                }
                *sel = Selection::cursor(Position::new(pos.line + 1, 0));
                return;
            }

            if let Some(prefix) = get_smart_continuation_prefix(current_line) {
                let p_len = prefix.chars().count();
                lines[pos.line] = left;
                lines.insert_source_line(pos.line + 1, format!("{}{}", prefix, right));
                *sel = Selection::cursor(Position::new(pos.line + 1, p_len));
            } else if is_source {
                lines[pos.line] = left;
                lines.insert_source_line(pos.line + 1, right);
                *sel = Selection::cursor(Position::new(pos.line + 1, 0));
            } else if col == 0 {
                lines.insert_source_line(pos.line, String::new());
                lines.insert_source_line(pos.line + 1, String::new());
                *sel = Selection::cursor(Position::new(pos.line, 0));
            } else if col >= current_chars_len {
                let target = append_block_below(lines, pos.line);
                *sel = Selection::cursor(Position::new(target, 0));
            } else {
                lines[pos.line] = left;
                lines.insert_source_line(pos.line + 1, String::new());
                lines.insert_source_line(pos.line + 2, right);
                *sel = Selection::cursor(Position::new(pos.line + 2, 0));
            }
        });
    }

    pub fn insert_line_below_block(&mut self) {
        let pos = self.selection.head;
        let mut open_idx = None;
        let mut closing_idx = None;
        for idx in 0..self.line_count() {
            let line = self
                .line_text(crate::buffer::SourceLine::new(idx))
                .unwrap_or_default();
            if !line.trim_start().starts_with("```") {
                continue;
            }
            if let Some(open) = open_idx.take() {
                if open <= pos.line.index() && pos.line.index() <= idx {
                    closing_idx = Some(idx);
                    break;
                }
            } else {
                open_idx = Some(idx);
            }
        }

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
        let (start, end) = crate::markdown::math_block_range(&self.to_lines(), start_line)?;
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

    pub fn backspace_mode(&mut self, is_source: bool) {
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

        if pos.col > 0 {
            let line = self.line_text(pos.line).unwrap_or_default();
            let trimmed = line.trim_start();
            let leading_spaces = line.chars().take_while(|c| *c == ' ').count();
            if !is_thematic_break(trimmed) {
                if trimmed.starts_with("```") {
                    let has_closing = (pos.line.index() + 1..self.line_count()).any(|idx| {
                        self.line_text(crate::buffer::SourceLine::new(idx))
                            .is_some_and(|line| line.trim().starts_with("```"))
                    });
                    if has_closing {
                        if pos.col > leading_spaces + 3 {
                            self.replace_range((Position::new(pos.line, pos.col - 1), pos), "");
                        }
                        return;
                    }
                }

                if pos.col >= 4 && pos.col <= leading_spaces {
                    let spaces_to_remove = if pos.col % 4 == 0 { 4 } else { pos.col % 4 };
                    let start_col = pos.col - spaces_to_remove;
                    self.replace_range((Position::new(pos.line, start_col), pos), "");
                    return;
                }

                let has_block_prefix = parse_block_prefix(&line).is_some_and(|prefix| {
                    !matches!(prefix.kind, BlockPrefix::Fence { .. }) && pos.col <= prefix.total_len
                });
                if !has_block_prefix && line.trim() != "$$" {
                    self.replace_range((Position::new(pos.line, pos.col - 1), pos), "");
                    return;
                }
            }
        }

        if pos.col == 0 && pos.line.index() > 0 {
            let current_line = self.line_text(pos.line).unwrap_or_default();
            let previous_line = self.line_text(pos.line - 1).unwrap_or_default();
            let previous_previous_line = if pos.line.index() > 1 {
                self.line_text(pos.line - 2).unwrap_or_default()
            } else {
                String::new()
            };
            let is_special_line = is_thematic_break(current_line.trim_start())
                || current_line.trim() == "$$"
                || current_line.trim_start().starts_with("```")
                || parse_block_prefix(&current_line).is_some()
                || previous_line.trim() == "$$"
                || previous_line.trim_start().starts_with("```")
                || previous_previous_line.trim() == "$$"
                || previous_previous_line.trim_start().starts_with("```");

            if !is_special_line {
                if !is_source && previous_line.trim().is_empty() && pos.line.index() > 1 {
                    let previous_previous = pos.line - 2;
                    let join_col = self.line_len(previous_previous);
                    self.replace_range((Position::new(previous_previous, join_col), pos), "");
                } else {
                    let previous = pos.line - 1;
                    let join_col = self.line_len(previous);
                    self.replace_range((Position::new(previous, join_col), pos), "");
                }
                return;
            }
        }

        self.record_edit(|lines, sel| {
            if !sel.is_empty() {
                delete_range_internal(lines, sel.range());
                let (start, _) = sel.range();
                *sel = Selection::cursor(start);
                return;
            }

            let pos = sel.head;
            let line_str = lines[pos.line].clone();
            let trimmed = line_str.trim_start();
            let leading_spaces = line_str.chars().take_while(|c| *c == ' ').count();

            if is_source {
                if pos.col > 0 {
                    remove_char_at_col(&mut lines[pos.line], pos.col - 1);
                    *sel = Selection::cursor(Position::new(pos.line, pos.col - 1));
                } else if pos.line.index() > 0 {
                    let current_line = lines.remove_source_line(pos.line);
                    let prev_len = lines[pos.line - 1].chars().count();
                    lines[pos.line - 1].push_str(&current_line);
                    *sel = Selection::cursor(Position::new(pos.line - 1, prev_len));
                }
                return;
            }

            if pos.col == 0
                && (pos.line.index() > 0
                    && crate::markdown::math_block_range(lines.as_slice(), pos.line - 1)
                        .is_some_and(|(_, end)| end == pos.line - 1)
                    || pos.line.index() > 1
                        && lines[pos.line - 1].trim().is_empty()
                        && crate::markdown::math_block_range(lines.as_slice(), pos.line - 2)
                            .is_some_and(|(_, end)| end == pos.line - 2))
            {
                return;
            }

            if crate::markdown::math_block_range(lines.as_slice(), pos.line)
                .is_some_and(|(start, end)| pos.line == start || pos.line == end)
            {
                return;
            }

            if pos.col == 0
                && crate::markdown::math_block_range(lines.as_slice(), pos.line)
                    .is_some_and(|(start, _)| pos.line == start + 1)
            {
                return;
            }

            if is_thematic_break(trimmed) {
                if pos.line.index() > 0 && lines[pos.line - 1].trim().is_empty() {
                    lines.remove_source_line(pos.line);
                    if pos.line.index() < lines.len() && lines[pos.line].trim().is_empty() {
                        lines.remove_source_line(pos.line);
                    }
                    *sel = Selection::cursor(Position::new(pos.line - 1, 0));
                } else if (pos.line + 1).index() < lines.len()
                    && lines[pos.line + 1].trim().is_empty()
                {
                    lines.remove_source_line(pos.line + 1);
                    lines[pos.line] = String::new();
                    *sel = Selection::cursor(Position::new(pos.line, 0));
                } else {
                    lines[pos.line] = String::new();
                    *sel = Selection::cursor(Position::new(pos.line, 0));
                }
                return;
            }

            if trimmed.starts_with("```") {
                let has_closing = lines.as_slice()[(pos.line + 1).index()..]
                    .iter()
                    .any(|l| l.trim().starts_with("```"));
                if has_closing {
                    if pos.col > leading_spaces + 3 {
                        remove_char_at_col(&mut lines[pos.line], pos.col - 1);
                        *sel = Selection::cursor(Position::new(pos.line, pos.col - 1));
                    }
                    return;
                }
            }

            if pos.col >= 4 && pos.col <= leading_spaces {
                let spaces_to_remove = if pos.col % 4 == 0 { 4 } else { pos.col % 4 };
                let start_col = pos.col - spaces_to_remove;
                lines[pos.line].drain(start_col..pos.col);
                *sel = Selection::cursor(Position::new(pos.line, start_col));
                return;
            }

            if pos.line.index() > 0
                && lines[pos.line].is_empty()
                && lines[pos.line - 1].trim().starts_with("```")
                && (pos.line + 1).index() < lines.len()
                && lines[pos.line + 1].trim().starts_with("```")
            {
                let start_fence = (pos.line - 1).index();
                let end_fence = (pos.line + 1).index();
                let mut remove_end = end_fence;
                if remove_end + 1 < lines.len()
                    && lines[crate::buffer::SourceLine::new(remove_end + 1)]
                        .trim()
                        .is_empty()
                {
                    remove_end += 1;
                }
                lines.drain_source_lines(
                    crate::buffer::SourceLine::new(start_fence)
                        ..=crate::buffer::SourceLine::new(remove_end),
                );
                lines
                    .insert_source_line(crate::buffer::SourceLine::new(start_fence), String::new());
                lines.insert_source_line(
                    crate::buffer::SourceLine::new(start_fence + 1),
                    String::new(),
                );
                if start_fence > 0
                    && lines[crate::buffer::SourceLine::new(start_fence - 1)]
                        .trim()
                        .is_empty()
                {
                    lines.remove_source_line(crate::buffer::SourceLine::new(start_fence));
                    *sel = Selection::cursor(Position::new(
                        crate::buffer::SourceLine::new(start_fence - 1),
                        0,
                    ));
                } else {
                    *sel = Selection::cursor(Position::new(
                        crate::buffer::SourceLine::new(start_fence),
                        0,
                    ));
                }
                return;
            }

            if let Some(prefix) = parse_block_prefix(&line_str)
                && !matches!(prefix.kind, BlockPrefix::Fence { .. })
                && pos.col <= prefix.total_len
            {
                let split_b = char_col_to_byte_offset(&line_str, prefix.total_len);
                let content = &line_str[split_b..];
                let leading_b = char_col_to_byte_offset(&line_str, prefix.leading_spaces);
                let leading = &line_str[..leading_b];
                let is_list_or_quote = matches!(
                    prefix.kind,
                    BlockPrefix::Task { .. }
                        | BlockPrefix::Bullet { .. }
                        | BlockPrefix::Ordered { .. }
                        | BlockPrefix::Quote
                );

                if !is_source
                    && is_list_or_quote
                    && pos.line.index() > 0
                    && !lines[pos.line - 1].trim().is_empty()
                {
                    lines.insert_source_line(pos.line, String::new());
                    lines[pos.line + 1] = format!("{}{}", leading, content);
                    if (pos.line + 2).index() >= lines.len()
                        || !lines[pos.line + 2].trim().is_empty()
                    {
                        lines.insert_source_line(pos.line + 2, String::new());
                    }
                    *sel = Selection::cursor(Position::new(pos.line + 1, prefix.leading_spaces));
                } else {
                    lines[pos.line] = format!("{}{}", leading, content);
                    *sel = Selection::cursor(Position::new(pos.line, prefix.leading_spaces));
                }
                return;
            }

            if pos.col > 0 {
                remove_char_at_col(&mut lines[pos.line], pos.col - 1);
                *sel = Selection::cursor(Position::new(pos.line, pos.col - 1));
            } else if pos.line > crate::buffer::SourceLine::new(0) {
                if is_closing_code_fence_lines(lines.as_slice(), pos.line - 1) {
                    let target_line = pos.line.saturating_sub(2);
                    let target_col = lines[target_line].chars().count();
                    if lines[pos.line].is_empty() {
                        lines.remove_source_line(pos.line);
                    }
                    *sel = Selection::cursor(Position::new(target_line, target_col));
                } else if pos.line >= crate::buffer::SourceLine::new(2)
                    && lines[pos.line - 1].trim().is_empty()
                    && is_closing_code_fence_lines(lines.as_slice(), pos.line - 2)
                {
                    lines.remove_source_line(pos.line - 1);
                    let target_line = pos.line - 2;
                    let content_line = target_line.saturating_sub(1);
                    let target_col = lines[content_line].chars().count();
                    *sel = Selection::cursor(Position::new(content_line, target_col));
                } else if !is_source
                    && pos.line >= crate::buffer::SourceLine::new(2)
                    && lines[pos.line - 1].trim().is_empty()
                {
                    lines.remove_source_line(pos.line - 1);
                    let current_line = lines.remove_source_line(pos.line - 1);
                    let prev_line_idx = pos.line - 2;
                    let prev_line_chars = lines[prev_line_idx].chars().count();
                    lines[prev_line_idx].push_str(&current_line);
                    *sel = Selection::cursor(Position::new(prev_line_idx, prev_line_chars));
                } else {
                    let current_line = lines.remove_source_line(pos.line);
                    let prev_line_idx = pos.line - 1;
                    let prev_line_chars = lines[prev_line_idx].chars().count();
                    lines[prev_line_idx].push_str(&current_line);
                    *sel = Selection::cursor(Position::new(prev_line_idx, prev_line_chars));
                }
            }

            ensure_min_empty_lines(lines);
        });
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

        if !is_thematic_break(line.trim()) && line.trim() != "$$" {
            if pos.col < line_len {
                self.replace_range((pos, Position::new(pos.line, pos.col + 1)), "");
                return;
            }
            if (pos.line + 1).index() < self.line_count() {
                let next_line = self.line_text(pos.line + 1).unwrap_or_default();
                let next_next_line = if (pos.line + 2).index() < self.line_count() {
                    self.line_text(pos.line + 2).unwrap_or_default()
                } else {
                    String::new()
                };
                if next_line.trim() != "$$"
                    && !(next_line.trim().is_empty() && next_next_line.trim() == "$$")
                {
                    self.replace_range((pos, Position::new(pos.line + 1, 0)), "");
                    return;
                }
            }
        }

        self.record_edit(|lines, sel| {
            if !sel.is_empty() {
                delete_range_internal(lines, sel.range());
                let (start, _) = sel.range();
                *sel = Selection::cursor(start);
                return;
            }

            let pos = sel.head;
            let trimmed = lines[pos.line].trim();

            if !is_source
                && crate::markdown::math_block_range(lines.as_slice(), pos.line)
                    .is_some_and(|(start, end)| pos.line == start || pos.line == end)
            {
                return;
            }

            if !is_source
                && pos.col >= lines[pos.line].chars().count()
                && (pos.line + 1).index() < lines.len()
                && (crate::markdown::math_block_range(lines.as_slice(), pos.line + 1)
                    .is_some_and(|(start, _)| start == pos.line + 1)
                    || (pos.line + 2).index() < lines.len()
                        && lines[pos.line + 1].trim().is_empty()
                        && crate::markdown::math_block_range(lines.as_slice(), pos.line + 2)
                            .is_some_and(|(start, _)| start == pos.line + 2))
            {
                return;
            }

            if !is_source && is_thematic_break(trimmed) {
                lines[pos.line] = String::new();
                *sel = Selection::cursor(Position::new(pos.line, 0));
                return;
            }

            let line_chars_count = lines[pos.line].chars().count();
            if pos.col < line_chars_count {
                remove_char_at_col(&mut lines[pos.line], pos.col);
            } else if !is_source
                && crate::markdown::math_block_range(lines.as_slice(), pos.line)
                    .is_some_and(|(_, end)| pos.line + 1 == end)
            {
                return;
            } else if (pos.line + 1).index() < lines.len() {
                let next_line = lines.remove_source_line(pos.line + 1);
                lines[pos.line].push_str(&next_line);
            }

            ensure_min_empty_lines(lines);
        });
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
        let meta = match crate::markdown::table::get_table_metadata(&self.to_lines(), pos.line) {
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
        let meta = match crate::markdown::table::get_table_metadata(&self.to_lines(), pos.line) {
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
        let meta = match crate::markdown::table::get_table_metadata(&self.to_lines(), pos.line) {
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
        if crate::markdown::table::get_table_metadata(&self.to_lines(), pos.line).is_none() {
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
