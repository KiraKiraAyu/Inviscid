use super::TextBuffer;
use super::char_col_to_byte_offset;
use super::movement::{is_closing_code_fence_lines, is_in_code_fence_lines};
use super::primitives::{Position, Selection};
use crate::markdown::{
    BlockPrefix, get_smart_continuation_prefix, is_thematic_break, parse_block_prefix,
    parse_heading_prefix,
};

fn ensure_min_empty_lines(lines: &mut Vec<String>) {
    if lines.is_empty() {
        lines.push(String::new());
        lines.push(String::new());
    } else if lines.len() == 1 && lines[0].is_empty() {
        lines.push(String::new());
    }
}

fn remove_char_at_col(line: &mut String, col: usize) {
    let b = char_col_to_byte_offset(line, col);
    if b < line.len() {
        line.remove(b);
    }
}

fn delete_range_internal(lines: &mut Vec<String>, (start, end): (Position, Position)) {
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
            lines.drain((start.line + 1)..=end.line);
        }
    }

    ensure_min_empty_lines(lines);
}

fn append_block_below(lines: &mut Vec<String>, current_line_idx: usize) -> usize {
    if current_line_idx + 1 < lines.len() && lines[current_line_idx + 1].trim().is_empty() {
        lines.insert(current_line_idx + 2, String::new());
        if current_line_idx + 3 >= lines.len() || !lines[current_line_idx + 3].trim().is_empty() {
            lines.insert(current_line_idx + 3, String::new());
        }
        current_line_idx + 2
    } else {
        lines.insert(current_line_idx + 1, String::new());
        lines.insert(current_line_idx + 2, String::new());
        current_line_idx + 2
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

        self.record_edit(|lines, sel| {
            let (start, end) = sel.range();
            if start.line == end.line {
                let line = &mut lines[start.line];
                let start_byte = char_col_to_byte_offset(line, start.col);
                let end_byte = char_col_to_byte_offset(line, end.col);
                let start_byte = start_byte.min(end_byte);

                line.insert_str(end_byte, close);
                line.insert_str(start_byte, open);

                let open_len = open.chars().count();
                let close_len = close.chars().count();
                *sel = Selection {
                    anchor: Position::new(start.line, start.col),
                    head: Position::new(end.line, end.col + open_len + close_len),
                };
            } else {
                let start_byte = char_col_to_byte_offset(&lines[start.line], start.col);
                let end_byte = char_col_to_byte_offset(&lines[end.line], end.col);

                lines[start.line].insert_str(start_byte, open);
                lines[end.line].insert_str(end_byte, close);

                let close_len = close.chars().count();
                *sel = Selection {
                    anchor: Position::new(start.line, start.col),
                    head: Position::new(end.line, end.col + close_len),
                };
            }
        });
    }

    pub fn backspace_auto_pair(&mut self) -> bool {
        if !self.selection.is_empty() {
            return false;
        }

        let pos = self.selection.head;
        if pos.line >= self.lines.len() || pos.col == 0 {
            return false;
        }
        let line = &self.lines[pos.line];
        let byte_left = char_col_to_byte_offset(line, pos.col - 1);
        let left_ch = line[byte_left..].chars().next();
        let byte_right = char_col_to_byte_offset(line, pos.col);
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
        if start.line == 0 {
            return;
        }

        self.record_edit(|lines, sel| {
            let line_above = lines.remove(start.line - 1);
            lines.insert(end.line, line_above);

            sel.anchor.line -= 1;
            sel.head.line -= 1;
        });
    }

    pub fn move_lines_down(&mut self) {
        let (start, end) = self.selection.range();
        if end.line + 1 >= self.lines.len() {
            return;
        }

        self.record_edit(|lines, sel| {
            let line_below = lines.remove(end.line + 1);
            lines.insert(start.line, line_below);

            sel.anchor.line += 1;
            sel.head.line += 1;
        });
    }

    pub fn duplicate_lines_down(&mut self) {
        let (start, end) = self.selection.range();
        self.record_edit(|lines, sel| {
            let block: Vec<String> = lines[start.line..=end.line].to_vec();
            let count = block.len();
            for (idx, line) in block.into_iter().enumerate() {
                lines.insert(end.line + 1 + idx, line);
            }
            sel.anchor.line += count;
            sel.head.line += count;
        });
    }

    pub fn duplicate_lines_up(&mut self) {
        let (start, end) = self.selection.range();
        self.record_edit(|lines, _sel| {
            let block: Vec<String> = lines[start.line..=end.line].to_vec();
            for (idx, line) in block.into_iter().enumerate() {
                lines.insert(start.line + idx, line);
            }
        });
    }

    pub fn delete_lines(&mut self) {
        let (start, end) = self.selection.range();
        self.record_edit(|lines, sel| {
            if lines.len() <= (end.line - start.line + 1) {
                *lines = vec![String::new(), String::new()];
                *sel = Selection::cursor(Position::new(0, 0));
                return;
            }

            lines.drain(start.line..=end.line);
            ensure_min_empty_lines(lines);
            let new_line = start.line.min(lines.len() - 1);
            let new_col = sel.head.col.min(lines[new_line].chars().count());
            *sel = Selection::cursor(Position::new(new_line, new_col));
        });
    }

    pub fn insert_newline_below(&mut self) {
        self.record_edit(|lines, sel| {
            let line_idx = sel.head.line;
            let current_line = &lines[line_idx];
            let trimmed_prefix = get_smart_continuation_prefix(current_line);

            let (new_content, new_col) = if let Some(prefix) = trimmed_prefix {
                let p_len = prefix.chars().count();
                (prefix, p_len)
            } else {
                (String::new(), 0)
            };

            lines.insert(line_idx + 1, new_content);
            *sel = Selection::cursor(Position::new(line_idx + 1, new_col));
        });
    }

    pub fn insert_newline_above(&mut self) {
        self.record_edit(|lines, sel| {
            let line_idx = sel.head.line;
            lines.insert(line_idx, String::new());
            *sel = Selection::cursor(Position::new(line_idx, 0));
        });
    }

    pub fn indent_lines(&mut self) {
        self.indent_lines_with_tab_size(4);
    }

    pub fn indent_lines_with_tab_size(&mut self, tab_size: usize) {
        let tab_size = tab_size.max(1);
        let tab_str: String = " ".repeat(tab_size);
        let (start, end) = self.selection.range();
        if start == end && self.selection.head.col > 0 {
            self.insert_text(&tab_str);
            return;
        }

        self.record_edit(|lines, sel| {
            for line in lines.iter_mut().take(end.line + 1).skip(start.line) {
                line.insert_str(0, &tab_str);
            }
            sel.anchor.col += tab_size;
            sel.head.col += tab_size;
        });
    }

    pub fn outdent_lines(&mut self) {
        self.outdent_lines_with_tab_size(4);
    }

    pub fn outdent_lines_with_tab_size(&mut self, tab_size: usize) {
        let tab_size = tab_size.max(1);
        let tab_str: String = " ".repeat(tab_size);
        let (start, end) = self.selection.range();
        self.record_edit(|lines, sel| {
            let mut anchor_removed = 0;
            let mut head_removed = 0;

            for (line_idx, line) in lines
                .iter_mut()
                .enumerate()
                .take(end.line + 1)
                .skip(start.line)
            {
                let removed = if let Some(stripped) = line.strip_prefix(&tab_str) {
                    *line = stripped.to_string();
                    tab_size
                } else if let Some(stripped) = line.strip_prefix('\t') {
                    *line = stripped.to_string();
                    1
                } else {
                    let spaces = line.chars().take_while(|c| *c == ' ').count().min(tab_size);
                    if spaces > 0 {
                        *line = line[spaces..].to_string();
                    }
                    spaces
                };

                if line_idx == sel.anchor.line {
                    anchor_removed = removed;
                }
                if line_idx == sel.head.line {
                    head_removed = removed;
                }
            }
            sel.anchor.col = sel.anchor.col.saturating_sub(anchor_removed);
            sel.head.col = sel.head.col.saturating_sub(head_removed);
        });
    }

    pub fn toggle_comment(&mut self) {
        let (start, end) = self.selection.range();
        self.record_edit(|lines, sel| {
            for line in lines.iter_mut().take(end.line + 1).skip(start.line) {
                let leading_len = line.chars().take_while(|c| c.is_whitespace()).count();
                let leading: String = line.chars().take(leading_len).collect();
                let trimmed = line.trim();

                if let Some(rest) = trimmed.strip_prefix("<!--")
                    && let Some(inner_trimmed) = rest.strip_suffix("-->")
                {
                    let inner = inner_trimmed.strip_prefix(' ').unwrap_or(inner_trimmed);
                    let inner = inner.strip_suffix(' ').unwrap_or(inner);
                    *line = format!("{}{}", leading, inner);
                    continue;
                }

                let content = &line[leading.len()..];
                *line = format!("{}<!-- {} -->", leading, content);
            }

            let anchor_line_len = lines[sel.anchor.line].chars().count();
            let head_line_len = lines[sel.head.line].chars().count();
            sel.anchor.col = sel.anchor.col.min(anchor_line_len);
            sel.head.col = sel.head.col.min(head_line_len);
        });
    }

    pub fn insert_text(&mut self, text: &str) {
        let is_single_line = self.selection.is_empty()
            && !text.contains('\n')
            && !text.contains('\r')
            && self.selection.head.line < self.lines.len();

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

        self.record_edit(|lines, sel| {
            if !sel.is_empty() {
                delete_range_internal(lines, sel.range());
                let (start, _) = sel.range();
                *sel = Selection::cursor(start);
            }

            let pos = sel.head;
            let byte_col = char_col_to_byte_offset(&lines[pos.line], pos.col);

            let normalized = text.replace("\r\n", "\n").replace('\r', "\n");
            let text_lines: Vec<&str> = normalized.split('\n').collect();
            if text_lines.len() == 1 {
                let insert_len = normalized.chars().count();
                lines[pos.line].insert_str(byte_col, &normalized);
                *sel = Selection::cursor(Position::new(pos.line, pos.col + insert_len));
            } else {
                let prefix = lines[pos.line][..byte_col].to_string();
                let suffix = lines[pos.line][byte_col..].to_string();

                let mut new_lines = Vec::with_capacity(text_lines.len());
                for (i, t_line) in text_lines.iter().enumerate() {
                    if i == 0 {
                        let mut first = prefix.clone();
                        first.push_str(t_line);
                        new_lines.push(first);
                    } else if i == text_lines.len() - 1 {
                        let last_col = t_line.chars().count();
                        let mut last = (*t_line).to_string();
                        last.push_str(&suffix);
                        new_lines.push(last);
                        *sel = Selection::cursor(Position::new(pos.line + i, last_col));
                    } else {
                        new_lines.push(t_line.to_string());
                    }
                }

                lines.splice(pos.line..=pos.line, new_lines);
            }
        });
    }

    #[cfg(test)]
    pub fn insert_newline(&mut self) {
        self.insert_newline_mode(false);
    }

    pub fn insert_newline_mode(&mut self, is_source: bool) {
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
                    || (pos.line + 1 < lines.len() && !lines[pos.line + 1].trim().is_empty())
                {
                    lines.insert(pos.line + 1, String::new());
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

            if is_in_code_fence_lines(lines, pos.line) || (is_source && !trimmed.starts_with("```"))
            {
                lines[pos.line] = left;
                lines.insert(pos.line + 1, right);
                *sel = Selection::cursor(Position::new(pos.line + 1, 0));
                return;
            }

            if trimmed.starts_with("```") {
                let has_closing = lines[(pos.line + 1)..]
                    .iter()
                    .any(|l| l.trim().starts_with("```"));
                if !has_closing {
                    lines.insert(pos.line + 1, String::new());
                    lines.insert(pos.line + 2, "```".to_string());
                    lines.insert(pos.line + 3, String::new());
                }
                *sel = Selection::cursor(Position::new(pos.line + 1, 0));
                return;
            }

            if is_thematic_break(trimmed) {
                let target = append_block_below(lines, pos.line);
                *sel = Selection::cursor(Position::new(target, 0));
                return;
            }

            if let Some((_level, prefix_len)) = parse_heading_prefix(trimmed) {
                let actual_prefix_len = leading_spaces + prefix_len;

                if col <= actual_prefix_len {
                    lines.insert(pos.line, String::new());
                    *sel = Selection::cursor(Position::new(pos.line + 1, actual_prefix_len));
                } else if col >= current_chars_len {
                    let target = append_block_below(lines, pos.line);
                    *sel = Selection::cursor(Position::new(target, 0));
                } else {
                    lines[pos.line] = left;
                    lines.insert(pos.line + 1, String::new());
                    lines.insert(pos.line + 2, right);
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
                lines.insert(pos.line + 1, String::new());
                if !is_source
                    && (pos.line + 2 >= lines.len() || !lines[pos.line + 2].trim().is_empty())
                {
                    lines.insert(pos.line + 2, String::new());
                }
                *sel = Selection::cursor(Position::new(pos.line + 1, 0));
                return;
            }

            if let Some(prefix) = get_smart_continuation_prefix(current_line) {
                let p_len = prefix.chars().count();
                lines[pos.line] = left;
                lines.insert(pos.line + 1, format!("{}{}", prefix, right));
                *sel = Selection::cursor(Position::new(pos.line + 1, p_len));
            } else if is_source {
                lines[pos.line] = left;
                lines.insert(pos.line + 1, right);
                *sel = Selection::cursor(Position::new(pos.line + 1, 0));
            } else if col == 0 {
                lines.insert(pos.line, String::new());
                lines.insert(pos.line + 1, String::new());
                *sel = Selection::cursor(Position::new(pos.line, 0));
            } else if col >= current_chars_len {
                let target = append_block_below(lines, pos.line);
                *sel = Selection::cursor(Position::new(target, 0));
            } else {
                lines[pos.line] = left;
                lines.insert(pos.line + 1, String::new());
                lines.insert(pos.line + 2, right);
                *sel = Selection::cursor(Position::new(pos.line + 2, 0));
            }
        });
    }

    pub fn insert_line_below_block(&mut self) {
        self.record_edit(|lines, sel| {
            let pos = sel.head;
            if pos.line >= lines.len() {
                lines.push(String::new());
                let last = lines.len() - 1;
                *sel = Selection::cursor(Position::new(last, 0));
                return;
            }

            let mut open_idx = None;
            let mut closing_idx = None;
            for (idx, line) in lines.iter().enumerate() {
                if line.trim_start().starts_with("```") {
                    if let Some(open) = open_idx.take() {
                        if open <= pos.line && pos.line <= idx {
                            closing_idx = Some(idx);
                            break;
                        }
                    } else {
                        open_idx = Some(idx);
                    }
                }
            }

            let anchor_line = closing_idx.unwrap_or(pos.line);
            let target = append_block_below(lines, anchor_line);
            *sel = Selection::cursor(Position::new(target, 0));
        });
    }

    pub fn backspace_mode(&mut self, is_source: bool) {
        if self.backspace_auto_pair() {
            return;
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
                } else if pos.line > 0 {
                    let current_line = lines.remove(pos.line);
                    let prev_len = lines[pos.line - 1].chars().count();
                    lines[pos.line - 1].push_str(&current_line);
                    *sel = Selection::cursor(Position::new(pos.line - 1, prev_len));
                }
                return;
            }

            if is_thematic_break(trimmed) {
                if pos.line > 0 && lines[pos.line - 1].trim().is_empty() {
                    lines.remove(pos.line);
                    if pos.line < lines.len() && lines[pos.line].trim().is_empty() {
                        lines.remove(pos.line);
                    }
                    *sel = Selection::cursor(Position::new(pos.line - 1, 0));
                } else if pos.line + 1 < lines.len() && lines[pos.line + 1].trim().is_empty() {
                    lines.remove(pos.line + 1);
                    lines[pos.line] = String::new();
                    *sel = Selection::cursor(Position::new(pos.line, 0));
                } else {
                    lines[pos.line] = String::new();
                    *sel = Selection::cursor(Position::new(pos.line, 0));
                }
                return;
            }

            if trimmed.starts_with("```") {
                let has_closing = lines[(pos.line + 1)..]
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

            if pos.line > 0
                && lines[pos.line].is_empty()
                && lines[pos.line - 1].trim().starts_with("```")
                && pos.line + 1 < lines.len()
                && lines[pos.line + 1].trim().starts_with("```")
            {
                let start_fence = pos.line - 1;
                let end_fence = pos.line + 1;
                let mut remove_end = end_fence;
                if remove_end + 1 < lines.len() && lines[remove_end + 1].trim().is_empty() {
                    remove_end += 1;
                }
                lines.drain(start_fence..=remove_end);
                lines.insert(start_fence, String::new());
                lines.insert(start_fence + 1, String::new());
                if start_fence > 0 && lines[start_fence - 1].trim().is_empty() {
                    lines.remove(start_fence);
                    *sel = Selection::cursor(Position::new(start_fence - 1, 0));
                } else {
                    *sel = Selection::cursor(Position::new(start_fence, 0));
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
                    && pos.line > 0
                    && !lines[pos.line - 1].trim().is_empty()
                {
                    lines.insert(pos.line, String::new());
                    lines[pos.line + 1] = format!("{}{}", leading, content);
                    if pos.line + 2 >= lines.len() || !lines[pos.line + 2].trim().is_empty() {
                        lines.insert(pos.line + 2, String::new());
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
            } else if pos.line > 0 {
                if is_closing_code_fence_lines(lines, pos.line - 1) {
                    let target_line = pos.line.saturating_sub(2);
                    let target_col = lines[target_line].chars().count();
                    if lines[pos.line].is_empty() {
                        lines.remove(pos.line);
                    }
                    *sel = Selection::cursor(Position::new(target_line, target_col));
                } else if pos.line >= 2
                    && lines[pos.line - 1].trim().is_empty()
                    && is_closing_code_fence_lines(lines, pos.line - 2)
                {
                    lines.remove(pos.line - 1);
                    let target_line = pos.line - 2;
                    let content_line = target_line.saturating_sub(1);
                    let target_col = lines[content_line].chars().count();
                    *sel = Selection::cursor(Position::new(content_line, target_col));
                } else if !is_source && pos.line >= 2 && lines[pos.line - 1].trim().is_empty() {
                    lines.remove(pos.line - 1);
                    let current_line = lines.remove(pos.line - 1);
                    let prev_line_idx = pos.line - 2;
                    let prev_line_chars = lines[prev_line_idx].chars().count();
                    lines[prev_line_idx].push_str(&current_line);
                    *sel = Selection::cursor(Position::new(prev_line_idx, prev_line_chars));
                } else {
                    let current_line = lines.remove(pos.line);
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
        self.record_edit(|lines, sel| {
            if !sel.is_empty() {
                delete_range_internal(lines, sel.range());
                let (start, _) = sel.range();
                *sel = Selection::cursor(start);
                return;
            }

            let pos = sel.head;
            let trimmed = lines[pos.line].trim();

            if !is_source && is_thematic_break(trimmed) {
                lines[pos.line] = String::new();
                *sel = Selection::cursor(Position::new(pos.line, 0));
                return;
            }

            let line_chars_count = lines[pos.line].chars().count();
            if pos.col < line_chars_count {
                remove_char_at_col(&mut lines[pos.line], pos.col);
            } else if pos.line + 1 < lines.len() {
                let next_line = lines.remove(pos.line + 1);
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
            let next_row = meta.delimiter_line + 1;
            if next_row < self.lines.len() && next_row <= meta.last_table_line {
                let next_cells =
                    crate::markdown::table::parse_table_row_cells(&self.lines[next_row]);
                let target_col = resolve_col(&next_cells);
                self.selection = Selection::cursor(Position::new(next_row, target_col));
                return;
            }
            let new_row = crate::markdown::table::build_empty_table_row(meta.num_columns);
            self.record_edit(|lines, sel| {
                lines.insert(meta.delimiter_line + 1, new_row);
                let cells =
                    crate::markdown::table::parse_table_row_cells(&lines[meta.delimiter_line + 1]);
                let col = resolve_col(&cells);
                *sel = Selection::cursor(Position::new(meta.delimiter_line + 1, col));
            });
            return;
        }

        if pos.line < meta.last_table_line {
            let next_row = pos.line + 1;
            let next_cells = crate::markdown::table::parse_table_row_cells(&self.lines[next_row]);
            let target_col = resolve_col(&next_cells);
            self.selection = Selection::cursor(Position::new(next_row, target_col));
            return;
        }

        let new_row = crate::markdown::table::build_empty_table_row(meta.num_columns);
        let insert_idx = meta.last_table_line + 1;
        self.record_edit(|lines, sel| {
            lines.insert(insert_idx, new_row);
            let cells = crate::markdown::table::parse_table_row_cells(&lines[insert_idx]);
            let col = resolve_col(&cells);
            *sel = Selection::cursor(Position::new(insert_idx, col));
        });
    }

    pub fn table_tab_forward(&mut self) -> bool {
        let pos = self.selection.head;
        let meta = match crate::markdown::table::get_table_metadata(&self.lines, pos.line) {
            Some(m) => m,
            None => return false,
        };

        let current_cells = crate::markdown::table::parse_table_row_cells(&self.lines[pos.line]);
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
        let meta = match crate::markdown::table::get_table_metadata(&self.lines, pos.line) {
            Some(m) => m,
            None => return false,
        };

        let current_cells = crate::markdown::table::parse_table_row_cells(&self.lines[pos.line]);
        let cell_idx = crate::markdown::table::get_cell_index_for_col(&current_cells, pos.col);

        if cell_idx > 0 {
            let target_col = current_cells[cell_idx - 1].col_range.1;
            self.selection = Selection::cursor(Position::new(pos.line, target_col));
            return true;
        }

        if pos.line == meta.delimiter_line + 1 {
            let header_cells =
                crate::markdown::table::parse_table_row_cells(&self.lines[meta.header_line]);
            let target_col = header_cells.last().map(|c| c.col_range.1).unwrap_or(2);
            self.selection = Selection::cursor(Position::new(meta.header_line, target_col));
            return true;
        }

        if pos.line > meta.delimiter_line + 1 {
            let prev_row = pos.line - 1;
            let prev_cells = crate::markdown::table::parse_table_row_cells(&self.lines[prev_row]);
            let target_col = prev_cells.last().map(|c| c.col_range.1).unwrap_or(2);
            self.selection = Selection::cursor(Position::new(prev_row, target_col));
            return true;
        }

        true
    }

    pub fn table_enter_next_row(&mut self) -> bool {
        let pos = self.selection.head;
        let meta = match crate::markdown::table::get_table_metadata(&self.lines, pos.line) {
            Some(m) => m,
            None => return false,
        };

        let current_cells = crate::markdown::table::parse_table_row_cells(&self.lines[pos.line]);
        let cell_idx = crate::markdown::table::get_cell_index_for_col(&current_cells, pos.col);

        self.table_navigate_or_insert_next_row(&meta, Some(cell_idx));
        true
    }

    pub fn table_backspace(&mut self) -> bool {
        let pos = self.selection.head;
        if crate::markdown::table::get_table_metadata(&self.lines, pos.line).is_none() {
            return false;
        }

        if !self.selection.is_empty() {
            self.insert_text("");
            return true;
        }

        let current_cells = crate::markdown::table::parse_table_row_cells(&self.lines[pos.line]);
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
        let last_idx = self.lines.len().saturating_sub(1);
        if self.is_in_code_fence(last_idx) || self.is_closing_code_fence(last_idx) {
            self.record_edit(|lines, sel| {
                let last = lines.len().saturating_sub(1);
                if is_closing_code_fence_lines(lines, last) {
                    lines.push(String::new());
                    lines.push(String::new());
                } else {
                    lines.push("```".to_string());
                    lines.push(String::new());
                }
                *sel = Selection::cursor(Position::new(lines.len() - 1, 0));
            });
            Some(self.selection.head)
        } else if last_idx > 0
            && (self.is_in_code_fence(last_idx - 1) || self.is_closing_code_fence(last_idx - 1))
            && self.lines[last_idx].trim().is_empty()
        {
            self.record_edit(|lines, sel| {
                lines.push(String::new());
                *sel = Selection::cursor(Position::new(lines.len() - 1, 0));
            });
            Some(self.selection.head)
        } else {
            None
        }
    }
}
