use std::cell::{Cell, RefCell};
use std::collections::VecDeque;
use std::hash::{Hash, Hasher};
use std::ops::Range;
use std::path::{Path, PathBuf};
use std::time::{Instant, SystemTime};

pub mod movement;
pub mod operations;
pub mod primitives;
#[cfg(test)]
pub mod tests;

pub use primitives::*;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum LineEnding {
    #[default]
    Lf,
    CrLf,
}

impl LineEnding {
    #[inline]
    pub fn as_str(&self) -> &'static str {
        match self {
            LineEnding::Lf => "\n",
            LineEnding::CrLf => "\r\n",
        }
    }

    #[inline]
    pub fn detect(content: &str) -> Self {
        if content.contains("\r\n") {
            LineEnding::CrLf
        } else {
            LineEnding::Lf
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct EditDelta {
    pub start_line: usize,
    pub old_line_count: usize,
    pub new_line_count: usize,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SavedBaseline {
    pub content_hash: u64,
    pub revision: u64,
    pub line_count: usize,
    pub char_count: usize,
    pub is_empty_doc: bool,
}

#[derive(Clone, Debug)]
pub struct TextBuffer {
    lines: Vec<String>,
    pub(in crate::buffer) selection: Selection,
    pub(in crate::buffer) file_path: Option<PathBuf>,
    pub(in crate::buffer) line_ending: LineEnding,
    pub(in crate::buffer) last_saved_mtime: Option<SystemTime>,
    pub(in crate::buffer) is_dirty: bool,
    pub(in crate::buffer) column_goal: Option<usize>,

    undo_stack: VecDeque<UndoStep>,
    redo_stack: VecDeque<UndoStep>,

    saved_baseline: SavedBaseline,
    pub(in crate::buffer) version: usize,
    pub(in crate::buffer) revision: u64,
    total_char_count: usize,
    last_edit_time: Option<Instant>,
    is_empty_doc: bool,

    pub(in crate::buffer) line_index_cache: RefCell<Option<movement::LineIndex>>,
    pub(in crate::buffer) last_edit_delta: Option<EditDelta>,
    cached_word_count: Cell<Option<(u64, usize)>>,
}

impl Default for TextBuffer {
    fn default() -> Self {
        Self::new()
    }
}

pub fn compute_lines_hash(lines: &[String]) -> u64 {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    lines.hash(&mut hasher);
    hasher.finish()
}

#[inline]
pub(crate) fn char_col_to_byte_offset(s: &str, char_col: usize) -> usize {
    s.char_indices()
        .nth(char_col)
        .map(|(b, _)| b)
        .unwrap_or(s.len())
}

impl TextBuffer {
    pub fn new() -> Self {
        Self::from_str("", None)
    }

    pub fn from_str(content: &str, file_path: Option<PathBuf>) -> Self {
        let line_ending = LineEnding::detect(content);
        let normalized = content.replace("\r\n", "\n");
        let is_empty_doc = normalized.is_empty();
        let lines: Vec<String> = if is_empty_doc {
            vec![String::new(), String::new()]
        } else {
            normalized.split('\n').map(|s| s.to_string()).collect()
        };
        let saved_content_hash = compute_lines_hash(&lines);
        let saved_line_count = lines.len();
        let total_char_count =
            lines.iter().map(|l| l.chars().count()).sum::<usize>() + lines.len().saturating_sub(1);
        let baseline_char_count = if is_empty_doc { 0 } else { total_char_count };

        Self {
            lines,
            selection: Selection::cursor(Position::new(0, 0)),
            file_path,
            line_ending,
            last_saved_mtime: None,
            is_dirty: false,
            column_goal: None,
            undo_stack: VecDeque::new(),
            redo_stack: VecDeque::new(),
            saved_baseline: SavedBaseline {
                content_hash: saved_content_hash,
                revision: 0,
                line_count: saved_line_count,
                char_count: baseline_char_count,
                is_empty_doc,
            },
            version: 0,
            revision: 0,
            total_char_count,
            last_edit_time: None,
            is_empty_doc,
            line_index_cache: RefCell::new(None),
            last_edit_delta: None,
            cached_word_count: Cell::new(None),
        }
    }

    #[inline]
    pub fn lines(&self) -> &[String] {
        &self.lines
    }

    #[inline]
    pub fn line(&self, line_idx: usize) -> Option<&str> {
        self.lines.get(line_idx).map(|s| s.as_str())
    }

    #[inline]
    pub fn line_count(&self) -> usize {
        self.lines.len()
    }

    #[inline]
    fn has_only_empty_sentinel_lines(&self) -> bool {
        self.lines.is_empty()
            || (self.lines.len() == 1 && self.lines[0].is_empty())
            || (self.lines.len() == 2 && self.lines[0].is_empty() && self.lines[1].is_empty())
    }

    #[inline]
    pub fn is_empty(&self) -> bool {
        self.is_empty_doc && self.has_only_empty_sentinel_lines()
    }

    #[inline]
    pub fn selection(&self) -> Selection {
        self.selection
    }

    #[inline]
    pub fn cursor_pos(&self) -> Position {
        self.selection.head
    }

    #[inline]
    pub fn file_path(&self) -> Option<&Path> {
        self.file_path.as_deref()
    }

    #[inline]
    pub fn file_path_buf(&self) -> Option<PathBuf> {
        self.file_path.clone()
    }

    #[inline]
    pub fn set_file_path(&mut self, path: Option<PathBuf>) {
        self.file_path = path;
    }

    #[inline]
    pub fn line_ending(&self) -> LineEnding {
        self.line_ending
    }

    #[inline]
    pub fn set_line_ending(&mut self, line_ending: LineEnding) {
        self.line_ending = line_ending;
    }

    #[inline]
    pub fn last_saved_mtime(&self) -> Option<SystemTime> {
        self.last_saved_mtime
    }

    #[inline]
    pub fn set_last_saved_mtime(&mut self, mtime: Option<SystemTime>) {
        self.last_saved_mtime = mtime;
    }

    #[inline]
    pub fn is_dirty(&self) -> bool {
        self.is_dirty
    }

    #[inline]
    pub fn version(&self) -> usize {
        self.version
    }

    #[inline]
    pub fn last_edit_delta(&self) -> Option<EditDelta> {
        self.last_edit_delta
    }

    pub fn line_len(&self, line_idx: usize) -> usize {
        self.lines
            .get(line_idx)
            .map(|l| l.chars().count())
            .unwrap_or(0)
    }

    pub fn clamp_position(&self, pos: Position) -> Position {
        if self.lines.is_empty() {
            return Position::new(0, 0);
        }
        let line = pos.line.min(self.lines.len() - 1);
        let max_col = self.line_len(line);
        let col = pos.col.min(max_col);
        Position::new(line, col)
    }

    pub fn to_string_content(&self) -> String {
        if self.is_empty() {
            String::new()
        } else {
            self.lines.join(self.line_ending.as_str())
        }
    }

    pub fn to_string_normalized(&self) -> String {
        if self.is_empty() {
            String::new()
        } else {
            self.lines.join("\n")
        }
    }

    /// Yields chunks for each line and line ending.
    pub fn chunks_with_line_ending(&self) -> impl Iterator<Item = &str> {
        let line_ending_str = self.line_ending.as_str();
        let is_empty = self.is_empty();
        let total_lines = self.lines.len();
        self.lines
            .iter()
            .enumerate()
            .flat_map(move |(i, line)| {
                if is_empty {
                    ["", ""].into_iter()
                } else if i + 1 < total_lines {
                    [line.as_str(), line_ending_str].into_iter()
                } else {
                    [line.as_str(), ""].into_iter()
                }
            })
            .filter(|s| !s.is_empty())
    }

    pub fn word_count(&self) -> usize {
        if let Some((rev, count)) = self.cached_word_count.get()
            && rev == self.revision
        {
            return count;
        }
        let count = self
            .lines
            .iter()
            .map(|line| line.split_whitespace().count())
            .sum();
        self.cached_word_count.set(Some((self.revision, count)));
        count
    }

    #[inline]
    pub fn char_count(&self) -> usize {
        if self.is_empty() {
            0
        } else {
            self.total_char_count
        }
    }

    pub fn pos_to_utf16_offset(&self, pos: Position) -> usize {
        if self.lines.is_empty() {
            return 0;
        }
        let line_idx = pos.line.min(self.lines.len() - 1);
        let line_start = self.line_index().line_utf16_offset(line_idx);

        if let Some(line) = self.lines.get(line_idx) {
            let col_offset: usize = line.chars().take(pos.col).map(|c| c.len_utf16()).sum();
            line_start + col_offset
        } else {
            line_start
        }
    }

    pub fn utf16_offset_to_pos(&self, offset: usize) -> Position {
        if self.lines.is_empty() {
            return Position::new(0, 0);
        }
        let target_line = self.line_index().utf16_offset_to_line(offset);
        let line_start = self.line_index().line_utf16_offset(target_line);
        let rel_offset = offset.saturating_sub(line_start);

        if let Some(line) = self.lines.get(target_line) {
            let line_utf16_len: usize = line.chars().map(|c| c.len_utf16()).sum();
            if rel_offset <= line_utf16_len {
                let mut char_count = 0;
                let mut current_u16 = 0;
                for c in line.chars() {
                    let u16_len = c.len_utf16();
                    if current_u16 + u16_len > rel_offset {
                        break;
                    }
                    current_u16 += u16_len;
                    char_count += 1;
                }
                return Position::new(target_line, char_count);
            }
            if target_line == self.lines.len() - 1 {
                return Position::new(target_line, line.chars().count());
            }
        }
        Position::new(target_line, 0)
    }

    pub fn selected_utf16_range(&self) -> (Range<usize>, bool) {
        let head_offset = self.pos_to_utf16_offset(self.cursor_pos());
        let anchor_offset = self.pos_to_utf16_offset(self.selection.anchor);
        if anchor_offset <= head_offset {
            (anchor_offset..head_offset, false)
        } else {
            (head_offset..anchor_offset, true)
        }
    }

    pub fn text_for_utf16_range(&self, range_utf16: Range<usize>) -> (String, Range<usize>) {
        if self.lines.is_empty() {
            return (String::new(), 0..0);
        }

        let start_pos = self.utf16_offset_to_pos(range_utf16.start);
        let end_pos = self.utf16_offset_to_pos(range_utf16.end);
        let actual_start_utf16 = self.pos_to_utf16_offset(start_pos);
        let actual_end_utf16 = self.pos_to_utf16_offset(end_pos);
        let adjusted = actual_start_utf16..actual_end_utf16;

        if start_pos.line == end_pos.line {
            if start_pos.line >= self.lines.len() {
                return (String::new(), adjusted);
            }
            let line = self.line(start_pos.line).unwrap_or("");
            let char_count = end_pos.col.saturating_sub(start_pos.col);
            let text: String = line.chars().skip(start_pos.col).take(char_count).collect();
            (text, adjusted)
        } else {
            let mut result = Vec::new();
            let end_line = end_pos.line.min(self.lines.len().saturating_sub(1));
            for idx in start_pos.line..=end_line {
                let line = self.line(idx).unwrap_or("");
                if idx == start_pos.line {
                    result.push(line.chars().skip(start_pos.col).collect::<String>());
                } else if idx == end_pos.line {
                    result.push(line.chars().take(end_pos.col).collect::<String>());
                } else {
                    result.push(line.to_string());
                }
            }
            (result.join("\n"), adjusted)
        }
    }

    pub fn select_utf16_range(
        &mut self,
        range_utf16: Option<Range<usize>>,
        fallback_marked: Option<Range<usize>>,
    ) -> Range<usize> {
        let range = range_utf16
            .or(fallback_marked)
            .unwrap_or_else(|| self.selected_utf16_range().0);
        let start_pos = self.utf16_offset_to_pos(range.start);
        let end_pos = self.utf16_offset_to_pos(range.end);
        self.set_selection(Selection {
            anchor: start_pos,
            head: end_pos,
        });
        range
    }

    pub fn replace_and_mark_utf16(
        &mut self,
        range_utf16: Option<Range<usize>>,
        fallback_marked: Option<Range<usize>>,
        new_text: &str,
        new_selected_range: Option<Range<usize>>,
    ) -> Option<Range<usize>> {
        let range = self.select_utf16_range(range_utf16, fallback_marked);
        self.insert_text(new_text);
        let new_len_u16 = new_text.encode_utf16().count();
        let marked_range = (new_len_u16 > 0).then(|| range.start..(range.start + new_len_u16));

        if let Some(sel) = new_selected_range {
            let sel = sel.start.min(new_len_u16)..sel.end.min(new_len_u16);
            let sel_head = self.utf16_offset_to_pos(range.start + sel.end);
            let sel_anchor = self.utf16_offset_to_pos(range.start + sel.start);
            self.set_selection(Selection {
                anchor: sel_anchor,
                head: sel_head,
            });
        }

        marked_range
    }

    pub fn saved_baseline_snapshot(&self) -> SavedBaseline {
        SavedBaseline {
            content_hash: compute_lines_hash(&self.lines),
            revision: self.revision,
            line_count: self.lines.len(),
            char_count: self.char_count(),
            is_empty_doc: self.is_empty(),
        }
    }

    pub fn mark_saved_as(&mut self, baseline: SavedBaseline) {
        self.saved_baseline = baseline;
        self.update_dirty_state();
    }

    pub fn mark_as_saved(&mut self) {
        let baseline = self.saved_baseline_snapshot();
        self.mark_saved_as(baseline);
    }

    #[inline]
    fn update_empty_doc_state(&mut self) {
        self.is_empty_doc = self.has_only_empty_sentinel_lines();
    }

    pub fn update_dirty_state(&mut self) {
        if self.revision == self.saved_baseline.revision {
            self.is_dirty = false;
            return;
        }

        if self.lines.len() != self.saved_baseline.line_count
            || self.char_count() != self.saved_baseline.char_count
            || self.is_empty_doc != self.saved_baseline.is_empty_doc
        {
            self.is_dirty = true;
            return;
        }

        let current_hash = compute_lines_hash(&self.lines);
        self.is_dirty = current_hash != self.saved_baseline.content_hash;
    }

    fn apply_content_metrics(
        &mut self,
        start_line: usize,
        replaced_lines: &[String],
        inserted_lines: &[String],
    ) {
        self.revision = self.revision.wrapping_add(1);
        *self.line_index_cache.borrow_mut() = None;
        self.last_edit_delta = Some(EditDelta {
            start_line,
            old_line_count: replaced_lines.len(),
            new_line_count: inserted_lines.len(),
        });

        let old_chars: usize = replaced_lines.iter().map(|l| l.chars().count()).sum();
        let new_chars: usize = inserted_lines.iter().map(|l| l.chars().count()).sum();
        let net_delta = (new_chars as isize - old_chars as isize)
            + (inserted_lines.len() as isize - replaced_lines.len() as isize);
        self.total_char_count = (self.total_char_count as isize + net_delta) as usize;
    }

    fn push_undo_step(&mut self, step: UndoStep, is_single_char_insert: bool) {
        let now = Instant::now();
        let time_ok = self
            .last_edit_time
            .map(|t| now.duration_since(t).as_millis() < 800)
            .unwrap_or(false);

        let coalesced = if is_single_char_insert && time_ok {
            if let Some(top) = self.undo_stack.back_mut() {
                top.try_coalesce_single_char(
                    step.start_line,
                    step.new_lines[0].clone(),
                    step.cursor_after,
                    step.version_after,
                )
            } else {
                false
            }
        } else {
            false
        };

        if !coalesced {
            self.undo_stack.push_back(step);
            while self.undo_stack.len() > MAX_UNDO_STEPS {
                self.undo_stack.pop_front();
            }
        }

        self.last_edit_time = Some(now);
        self.redo_stack.clear();
        self.update_empty_doc_state();
        self.update_dirty_state();
    }

    pub fn record_single_line_edit<F>(&mut self, line_idx: usize, f: F)
    where
        F: FnOnce(&mut String, &mut Selection),
    {
        if line_idx >= self.lines.len() {
            return;
        }

        self.column_goal = None;
        let cursor_before = self.selection;
        let version_before = self.version;
        let old_line = self.lines[line_idx].clone();

        f(&mut self.lines[line_idx], &mut self.selection);

        self.selection.anchor = self.clamp_position(self.selection.anchor);
        self.selection.head = self.clamp_position(self.selection.head);

        if self.lines[line_idx] != old_line || self.selection != cursor_before {
            let new_lines = vec![self.lines[line_idx].clone()];
            let old_lines = vec![old_line];
            let content_changed = new_lines[0] != old_lines[0];

            let version_after = if content_changed {
                self.version = self.version.wrapping_add(1);
                self.apply_content_metrics(line_idx, &old_lines, &new_lines);
                self.version
            } else {
                version_before
            };

            let is_single_char_insert = content_changed
                && cursor_before.is_empty()
                && self.selection.is_empty()
                && self.selection.head.line == cursor_before.head.line
                && self.selection.head.col == cursor_before.head.col + 1
                && !new_lines[0].ends_with(' ')
                && !new_lines[0].ends_with('\t');

            let step = UndoStep {
                start_line: line_idx,
                old_lines,
                new_lines,
                cursor_before,
                cursor_after: self.selection,
                version_before,
                version_after,
            };

            self.push_undo_step(step, is_single_char_insert);
        }
    }

    pub fn record_edit<F>(&mut self, f: F)
    where
        F: FnOnce(&mut Vec<String>, &mut Selection),
    {
        self.column_goal = None;
        let cursor_before = self.selection;
        let version_before = self.version;
        let lines_before = self.lines.clone();

        f(&mut self.lines, &mut self.selection);

        self.selection.anchor = self.clamp_position(self.selection.anchor);
        self.selection.head = self.clamp_position(self.selection.head);

        if self.lines != lines_before || self.selection != cursor_before {
            let mut prefix = 0;
            while prefix < lines_before.len()
                && prefix < self.lines.len()
                && lines_before[prefix] == self.lines[prefix]
            {
                prefix += 1;
            }

            let mut suffix = 0;
            while suffix < (lines_before.len() - prefix)
                && suffix < (self.lines.len() - prefix)
                && lines_before[lines_before.len() - 1 - suffix]
                    == self.lines[self.lines.len() - 1 - suffix]
            {
                suffix += 1;
            }

            let old_lines = lines_before[prefix..(lines_before.len() - suffix)].to_vec();
            let new_lines = self.lines[prefix..(self.lines.len() - suffix)].to_vec();

            let content_changed = !old_lines.is_empty() || !new_lines.is_empty();
            let version_after = if content_changed {
                self.version = self.version.wrapping_add(1);
                self.apply_content_metrics(prefix, &old_lines, &new_lines);
                self.version
            } else {
                version_before
            };

            let is_single_char_insert = old_lines.len() == 1
                && new_lines.len() == 1
                && cursor_before.is_empty()
                && self.selection.is_empty()
                && self.selection.head.line == cursor_before.head.line
                && self.selection.head.col == cursor_before.head.col + 1
                && !new_lines[0].ends_with(' ')
                && !new_lines[0].ends_with('\t');

            let step = UndoStep {
                start_line: prefix,
                old_lines,
                new_lines,
                cursor_before,
                cursor_after: self.selection,
                version_before,
                version_after,
            };

            self.push_undo_step(step, is_single_char_insert);
        }
    }

    pub fn set_cursor(&mut self, pos: Position) {
        self.column_goal = None;
        self.selection = Selection::cursor(self.clamp_position(pos));
    }

    pub fn set_selection(&mut self, sel: Selection) {
        self.column_goal = None;
        self.selection = Selection {
            anchor: self.clamp_position(sel.anchor),
            head: self.clamp_position(sel.head),
        };
    }

    pub fn update_cursor(&mut self, select: bool, new_pos: Position) {
        if select {
            self.selection.head = new_pos;
        } else {
            self.selection = Selection::cursor(new_pos);
        }
    }

    pub fn selected_text(&self) -> Option<String> {
        if self.selection.is_empty() {
            return None;
        }

        let (start, end) = self.selection.range();
        if start.line == end.line {
            let line = &self.lines[start.line];
            let char_count = line.chars().count();
            let start_col = start.col.min(char_count);
            let end_col = end.col.min(char_count);
            if start_col >= end_col {
                return Some(String::new());
            }
            let selected: String = line
                .chars()
                .skip(start_col)
                .take(end_col - start_col)
                .collect();
            Some(selected)
        } else {
            let mut res = Vec::with_capacity(end.line - start.line + 1);
            for line_idx in start.line..=end.line {
                let line = &self.lines[line_idx];
                if line_idx == start.line {
                    res.push(line.chars().skip(start.col).collect::<String>());
                } else if line_idx == end.line {
                    res.push(line.chars().take(end.col).collect::<String>());
                } else {
                    res.push(line.clone());
                }
            }
            Some(res.join("\n"))
        }
    }

    pub fn undo(&mut self) {
        self.column_goal = None;
        self.last_edit_time = None;
        if let Some(step) = self.undo_stack.pop_back() {
            let replace_len = step.new_lines.len();
            self.lines.splice(
                step.start_line..(step.start_line + replace_len),
                step.old_lines.iter().cloned(),
            );
            self.selection = step.cursor_before;
            self.version = step.version_before;
            self.apply_content_metrics(step.start_line, &step.new_lines, &step.old_lines);

            self.redo_stack.push_back(step);
            self.update_empty_doc_state();
            self.update_dirty_state();
        }
    }

    pub fn redo(&mut self) {
        self.column_goal = None;
        self.last_edit_time = None;
        if let Some(step) = self.redo_stack.pop_back() {
            let replace_len = step.old_lines.len();
            self.lines.splice(
                step.start_line..(step.start_line + replace_len),
                step.new_lines.iter().cloned(),
            );
            self.selection = step.cursor_after;
            self.version = step.version_after;
            self.apply_content_metrics(step.start_line, &step.old_lines, &step.new_lines);

            self.undo_stack.push_back(step);
            self.update_empty_doc_state();
            self.update_dirty_state();
        }
    }
}
