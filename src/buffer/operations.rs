use super::char_col_to_byte_offset;
use super::primitives::{Position, Selection};
use super::{SourceLine, TextBuffer};
use crate::markdown::{
    current_indent_relative, get_smart_continuation_prefix, indent_col, list_marker,
    quote_prefix_len,
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

    pub fn insert_newline(&mut self) {
        let (start, end) = self.selection.range();
        let (start, end) = (self.clamp_position(start), self.clamp_position(end));
        let start_line = self.line_text(start.line).unwrap_or_default();
        let start_byte = char_col_to_byte_offset(&start_line, start.col);
        let end_line = if start.line == end.line {
            start_line.clone()
        } else {
            self.line_text(end.line).unwrap_or_default()
        };
        let end_byte = char_col_to_byte_offset(&end_line, end.col);
        let left = start_line[..start_byte].to_string();
        let right = end_line[end_byte..].to_string();
        self.replace_line_range(
            start.line,
            end.line.index() - start.line.index() + 1,
            vec![left, right],
            Selection::cursor(Position::new(start.line + 1, 0)),
        );
    }

    pub fn backspace(&mut self) {
        self.column_goal = None;
        if self.backspace_auto_pair() {
            return;
        }

        if !self.selection.is_empty() {
            self.replace_range(self.selection.range(), "");
            return;
        }

        let pos = self.selection.head;
        if pos.col > 0 {
            self.replace_range((Position::new(pos.line, pos.col - 1), pos), "");
        } else if pos.line.index() > 0 {
            let previous_line = pos.line - 1;
            let previous_len = self.line_len(previous_line);
            self.replace_range((Position::new(previous_line, previous_len), pos), "");
        }
    }

    pub fn delete_forward(&mut self) {
        self.column_goal = None;
        if !self.selection.is_empty() {
            self.replace_range(self.selection.range(), "");
            return;
        }

        let pos = self.selection.head;
        let line_len = self.line_len(pos.line);
        if pos.col < line_len {
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
}
