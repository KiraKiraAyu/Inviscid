use ropey::Rope;
use std::cell::{Cell, RefCell};
use std::collections::VecDeque;
use std::hash::{Hash, Hasher};
use std::ops::{Index, IndexMut, Range, RangeFrom, RangeInclusive};
use std::path::{Path, PathBuf};
use std::time::{Instant, SystemTime};

pub mod movement;
pub mod operations;
pub mod primitives;
#[cfg(test)]
pub mod tests;

pub use primitives::*;

/// A document's source lines. Indexing requires a source-coordinate value.
///
/// ```compile_fail
/// use inviscid::buffer::SourceLines;
/// let lines = SourceLines::from(vec![String::from("text")]);
/// let _ = &lines[0];
/// ```
#[derive(Clone, Debug)]
pub struct SourceLines {
    lines: Vec<String>,
    dirty_range: Option<Range<usize>>,
}

impl PartialEq for SourceLines {
    fn eq(&self, other: &Self) -> bool {
        self.lines == other.lines
    }
}

impl Eq for SourceLines {}

impl SourceLines {
    fn track_mutation(&mut self, range: Range<usize>) {
        let Some(dirty) = &mut self.dirty_range else {
            self.dirty_range = Some(range);
            return;
        };
        dirty.start = dirty.start.min(range.start);
        dirty.end = dirty.end.max(range.end);
    }

    fn track_splice(&mut self, start: usize, removed: usize, inserted: usize) {
        let splice_end = start + removed;
        if let Some(dirty) = &mut self.dirty_range {
            // Keep earlier dirty coordinates aligned with the post-splice lines.
            let delta = inserted as isize - removed as isize;
            let map_start = |index: usize| {
                if index <= start {
                    index
                } else if index >= splice_end {
                    (index as isize + delta) as usize
                } else {
                    start
                }
            };
            let map_end = |index: usize| {
                if index < start {
                    index
                } else if index >= splice_end {
                    (index as isize + delta) as usize
                } else {
                    start + inserted
                }
            };
            let old_start = dirty.start;
            let old_end = dirty.end;
            dirty.start = map_start(old_start).min(start);
            dirty.end = map_end(old_end).max(start + inserted);
        } else {
            self.dirty_range = Some(start..start + inserted);
        }
    }

    fn take_dirty_range(&mut self) -> Option<Range<usize>> {
        self.dirty_range.take()
    }

    fn clear_dirty_range(&mut self) {
        self.dirty_range = None;
    }
}

impl From<Vec<String>> for SourceLines {
    fn from(lines: Vec<String>) -> Self {
        Self {
            lines,
            dirty_range: None,
        }
    }
}

impl PartialEq<Vec<&str>> for SourceLines {
    fn eq(&self, other: &Vec<&str>) -> bool {
        self.lines
            .iter()
            .map(String::as_str)
            .eq(other.iter().copied())
    }
}

impl PartialEq<Vec<&str>> for &SourceLines {
    fn eq(&self, other: &Vec<&str>) -> bool {
        self.lines
            .iter()
            .map(String::as_str)
            .eq(other.iter().copied())
    }
}

impl FromIterator<String> for SourceLines {
    fn from_iter<T: IntoIterator<Item = String>>(iter: T) -> Self {
        Self::from(Vec::from_iter(iter))
    }
}

impl SourceLines {
    pub fn as_slice(&self) -> &[String] {
        &self.lines
    }

    pub fn len(&self) -> usize {
        self.lines.len()
    }

    pub fn is_empty(&self) -> bool {
        self.lines.is_empty()
    }

    pub fn iter(&self) -> std::slice::Iter<'_, String> {
        self.lines.iter()
    }

    pub fn iter_mut(&mut self) -> std::slice::IterMut<'_, String> {
        self.track_mutation(0..self.lines.len());
        self.lines.iter_mut()
    }

    pub fn push(&mut self, line: String) {
        let index = self.lines.len();
        self.lines.push(line);
        self.track_splice(index, 0, 1);
    }

    pub fn clear(&mut self) {
        let old_len = self.lines.len();
        self.lines.clear();
        self.track_splice(0, old_len, 0);
    }

    pub fn join(&self, separator: &str) -> String {
        self.lines.join(separator)
    }

    pub fn to_vec(&self) -> Vec<String> {
        self.lines.clone()
    }

    pub fn get_source_line(&self, line: SourceLine) -> Option<&String> {
        self.lines.get(line.index())
    }

    pub fn get_source_line_mut(&mut self, line: SourceLine) -> Option<&mut String> {
        let index = line.index();
        self.track_mutation(index..index + 1);
        self.lines.get_mut(index)
    }

    pub fn drain_source_lines(
        &mut self,
        lines: RangeInclusive<SourceLine>,
    ) -> std::vec::Drain<'_, String> {
        let start = lines.start().index();
        let removed = lines.end().index().saturating_sub(start) + 1;
        self.track_splice(start, removed, 0);
        self.lines.drain(start..start + removed)
    }

    pub fn replace_source_lines(
        &mut self,
        lines: RangeInclusive<SourceLine>,
        replacements: impl IntoIterator<Item = String>,
    ) {
        let start = lines.start().index();
        let end = lines.end().index().saturating_add(1);
        let replacements = Vec::from_iter(replacements);
        let replacement_len = replacements.len();
        self.lines.splice(start..end, replacements);
        self.track_splice(start, end - start, replacement_len);
    }

    pub fn replace_source_lines_at(
        &mut self,
        start: SourceLine,
        remove_count: usize,
        replacements: impl IntoIterator<Item = String>,
    ) {
        let start = start.index();
        let replacements = Vec::from_iter(replacements);
        let replacement_len = replacements.len();
        self.lines.splice(start..start + remove_count, replacements);
        self.track_splice(start, remove_count, replacement_len);
    }

    pub fn insert_source_line(&mut self, line: SourceLine, text: String) {
        let index = line.index();
        self.lines.insert(index, text);
        self.track_splice(index, 0, 1);
    }

    pub fn remove_source_line(&mut self, line: SourceLine) -> String {
        let index = line.index();
        let removed = self.lines.remove(index);
        self.track_splice(index, 1, 0);
        removed
    }

    pub fn replace_all(&mut self, replacement: Vec<String>) {
        let old_len = self.lines.len();
        let new_len = replacement.len();
        self.lines = replacement;
        self.track_splice(0, old_len, new_len);
    }
}

impl Index<SourceLine> for SourceLines {
    type Output = String;

    fn index(&self, line: SourceLine) -> &Self::Output {
        &self.lines[line.index()]
    }
}

impl IndexMut<SourceLine> for SourceLines {
    fn index_mut(&mut self, line: SourceLine) -> &mut Self::Output {
        let index = line.index();
        self.track_mutation(index..index + 1);
        &mut self.lines[index]
    }
}

impl Index<RangeInclusive<SourceLine>> for SourceLines {
    type Output = [String];

    fn index(&self, lines: RangeInclusive<SourceLine>) -> &Self::Output {
        &self.lines[lines.start().index()..=lines.end().index()]
    }
}

impl Index<RangeFrom<SourceLine>> for SourceLines {
    type Output = [String];

    fn index(&self, lines: RangeFrom<SourceLine>) -> &Self::Output {
        &self.lines[lines.start.index()..]
    }
}

impl IndexMut<RangeInclusive<SourceLine>> for SourceLines {
    fn index_mut(&mut self, lines: RangeInclusive<SourceLine>) -> &mut Self::Output {
        let start = lines.start().index();
        let end = lines.end().index().saturating_add(1);
        self.track_mutation(start..end);
        &mut self.lines[start..end]
    }
}

impl IndexMut<RangeFrom<SourceLine>> for SourceLines {
    fn index_mut(&mut self, lines: RangeFrom<SourceLine>) -> &mut Self::Output {
        let start = lines.start.index();
        self.track_mutation(start..self.lines.len());
        &mut self.lines[start..]
    }
}

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
    pub start_line: SourceLine,
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
    lines: SourceLines,
    /// Normalized document text. The line vector remains a compatibility view
    /// for layout and markdown code while callers migrate to range-based access.
    pub(in crate::buffer) rope: Rope,
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
    fn rope_from_lines(lines: &SourceLines, is_empty_doc: bool) -> Rope {
        if is_empty_doc {
            Rope::new()
        } else {
            Rope::from_str(&lines.join("\n"))
        }
    }

    fn replace_rope_lines(
        &mut self,
        start_line: SourceLine,
        old_line_count: usize,
        inserted_lines: &[String],
    ) {
        let start = start_line.index().min(self.rope.len_lines());
        let end_line = start.saturating_add(old_line_count);
        let mut start_char = self.rope.line_to_char(start);
        let end_char = if end_line < self.rope.len_lines() {
            self.rope.line_to_char(end_line)
        } else {
            self.rope.len_chars()
        };
        if end_line >= self.rope.len_lines() && inserted_lines.is_empty() && start > 0 {
            start_char -= 1;
        }
        let mut replacement = inserted_lines.join("\n");
        if end_line < self.rope.len_lines() && !inserted_lines.is_empty() {
            replacement.push('\n');
        } else if start == self.rope.len_lines()
            && old_line_count == 0
            && start > 0
            && !inserted_lines.is_empty()
        {
            replacement.insert(0, '\n');
        }
        self.rope.remove(start_char..end_char);
        self.rope.insert(start_char, &replacement);
    }

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
            lines: SourceLines::from(lines),
            rope: Rope::from_str(if is_empty_doc { "" } else { &normalized }),
            selection: Selection::cursor(Position::new(SourceLine::new(0), 0)),
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

    fn old_rope_lines(&self, start: usize, count: usize, empty_before: bool) -> Vec<String> {
        if empty_before {
            return vec![String::new(); count];
        }
        (start..start + count)
            .map(|line_idx| {
                let mut line = self.rope.line(line_idx).to_string();
                if line.ends_with('\n') {
                    line.pop();
                }
                line
            })
            .collect()
    }

    #[inline]
    pub fn lines(&self) -> &[String] {
        self.lines.as_slice()
    }

    #[inline]
    pub fn line(&self, source_line: SourceLine) -> Option<&str> {
        self.lines.get_source_line(source_line).map(|s| s.as_str())
    }

    #[inline]
    pub fn line_count(&self) -> usize {
        self.lines.len()
    }

    #[inline]
    fn has_only_empty_sentinel_lines(&self) -> bool {
        self.lines.is_empty()
            || (self.lines.len() == 1 && self.lines[SourceLine::new(0)].is_empty())
            || (self.lines.len() == 2
                && self.lines[SourceLine::new(0)].is_empty()
                && self.lines[SourceLine::new(1)].is_empty())
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
    pub fn revision(&self) -> u64 {
        self.revision
    }

    #[inline]
    pub fn last_edit_delta(&self) -> Option<EditDelta> {
        self.last_edit_delta
    }

    pub fn line_len(&self, source_line: SourceLine) -> usize {
        self.lines
            .get_source_line(source_line)
            .map(|l| l.chars().count())
            .unwrap_or(0)
    }

    pub fn clamp_position(&self, pos: Position) -> Position {
        if self.lines.is_empty() {
            return Position::new(SourceLine::new(0), 0);
        }
        let line = pos
            .line
            .min(crate::buffer::SourceLine::new(self.lines.len() - 1));
        let max_col = self.line_len(line);
        let col = pos.col.min(max_col);
        Position::new(line, col)
    }

    pub fn to_string_content(&self) -> String {
        if self.is_empty() {
            String::new()
        } else {
            self.rope
                .to_string()
                .replace('\n', self.line_ending.as_str())
        }
    }

    pub fn to_string_normalized(&self) -> String {
        if self.is_empty() {
            String::new()
        } else {
            self.rope.to_string()
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

    pub fn pos_to_utf16_offset(&self, pos: SourcePosition) -> usize {
        if self.lines.is_empty() {
            return 0;
        }
        let line_idx = pos.line.index().min(self.lines.len() - 1);
        let line_start = self.line_index().line_utf16_offset(line_idx);

        if let Some(line) = self.lines.get_source_line(SourceLine::new(line_idx)) {
            let col_offset: usize = line.chars().take(pos.col).map(|c| c.len_utf16()).sum();
            line_start + col_offset
        } else {
            line_start
        }
    }

    pub fn utf16_offset_to_pos(&self, offset: usize) -> SourcePosition {
        if self.lines.is_empty() {
            return SourcePosition::new(SourceLine::new(0), 0);
        }
        let target_line_idx = self.line_index().utf16_offset_to_line(offset);
        let target_line = SourceLine::new(target_line_idx);
        let line_start = self.line_index().line_utf16_offset(target_line_idx);
        let rel_offset = offset.saturating_sub(line_start);

        if let Some(line) = self.lines.get_source_line(SourceLine::new(target_line_idx)) {
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
                return SourcePosition::new(target_line, char_count);
            }
            if target_line_idx == self.lines.len() - 1 {
                return SourcePosition::new(target_line, line.chars().count());
            }
        }
        SourcePosition::new(target_line, 0)
    }

    pub fn selected_utf16_range(&self) -> (Range<usize>, bool) {
        let head_offset =
            self.pos_to_utf16_offset(SourcePosition::from_position(self.cursor_pos()));
        let anchor_offset =
            self.pos_to_utf16_offset(SourcePosition::from_position(self.selection.anchor));
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
            if start_pos.line.index() >= self.lines.len() {
                return (String::new(), adjusted);
            }
            let line = self.line(start_pos.line).unwrap_or("");
            let char_count = end_pos.col.saturating_sub(start_pos.col);
            let text: String = line.chars().skip(start_pos.col).take(char_count).collect();
            (text, adjusted)
        } else {
            let mut result = Vec::new();
            let start_line = start_pos.line.index();
            let end_line = end_pos.line.index().min(self.lines.len().saturating_sub(1));
            for idx in start_line..=end_line {
                let line = self.line(crate::buffer::SourceLine::new(idx)).unwrap_or("");
                if idx == start_line {
                    result.push(line.chars().skip(start_pos.col).collect::<String>());
                } else if idx == end_pos.line.index() {
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
            anchor: start_pos.to_position(),
            head: end_pos.to_position(),
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
                anchor: sel_anchor.to_position(),
                head: sel_head.to_position(),
            });
        }

        marked_range
    }

    pub fn saved_baseline_snapshot(&self) -> SavedBaseline {
        SavedBaseline {
            content_hash: compute_lines_hash(self.lines.as_slice()),
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

        let current_hash = compute_lines_hash(self.lines.as_slice());
        self.is_dirty = current_hash != self.saved_baseline.content_hash;
    }

    fn apply_content_metrics(
        &mut self,
        start_line: SourceLine,
        replaced_lines: &[String],
        inserted_lines: &[String],
    ) {
        self.is_empty_doc = self.has_only_empty_sentinel_lines();
        if self.rope.len_chars() == 0 || self.is_empty_doc {
            self.rope = Self::rope_from_lines(&self.lines, self.is_empty_doc);
        } else {
            self.replace_rope_lines(start_line, replaced_lines.len(), inserted_lines);
        }

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

    pub fn record_single_line_edit<F>(&mut self, line_idx: SourceLine, f: F)
    where
        F: FnOnce(&mut String, &mut Selection),
    {
        if line_idx.index() >= self.lines.len() {
            return;
        }

        self.column_goal = None;
        let cursor_before = self.selection;
        let version_before = self.version;
        let old_line = self.lines[line_idx].clone();

        f(&mut self.lines[line_idx], &mut self.selection);
        self.lines.clear_dirty_range();

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
        F: FnOnce(&mut SourceLines, &mut Selection),
    {
        self.column_goal = None;
        let cursor_before = self.selection;
        let version_before = self.version;
        let line_count_before = self.lines.len();
        let empty_before = self.is_empty_doc;
        self.lines.clear_dirty_range();

        f(&mut self.lines, &mut self.selection);

        self.selection.anchor = self.clamp_position(self.selection.anchor);
        self.selection.head = self.clamp_position(self.selection.head);

        let dirty = self
            .lines
            .take_dirty_range()
            .or_else(|| (self.lines.len() != line_count_before).then_some(0..self.lines.len()));
        if dirty.is_some() || self.selection != cursor_before {
            let mut prefix = dirty.as_ref().map_or(0, |range| range.start);
            let new_end = dirty
                .as_ref()
                .map_or(prefix, |range| range.end.min(self.lines.len()));
            let new_count = new_end.saturating_sub(prefix);
            let line_delta = self.lines.len() as isize - line_count_before as isize;
            let old_count = (new_count as isize - line_delta).max(0) as usize;
            let mut old_lines = self.old_rope_lines(prefix, old_count, empty_before);
            let mut new_lines = self.lines.as_slice()[prefix..new_end].to_vec();

            let mut unchanged_prefix = 0;
            while unchanged_prefix < old_lines.len().min(new_lines.len())
                && old_lines[unchanged_prefix] == new_lines[unchanged_prefix]
            {
                unchanged_prefix += 1;
            }
            if unchanged_prefix > 0 {
                old_lines.drain(..unchanged_prefix);
                new_lines.drain(..unchanged_prefix);
                prefix += unchanged_prefix;
            }

            let mut unchanged_suffix = 0;
            while unchanged_suffix < old_lines.len().min(new_lines.len())
                && old_lines[old_lines.len() - 1 - unchanged_suffix]
                    == new_lines[new_lines.len() - 1 - unchanged_suffix]
            {
                unchanged_suffix += 1;
            }
            if unchanged_suffix > 0 {
                old_lines.truncate(old_lines.len() - unchanged_suffix);
                new_lines.truncate(new_lines.len() - unchanged_suffix);
            }

            let content_changed = !old_lines.is_empty() || !new_lines.is_empty();
            if !content_changed && self.selection == cursor_before {
                return;
            }
            let version_after = if content_changed {
                self.version = self.version.wrapping_add(1);
                self.apply_content_metrics(SourceLine::new(prefix), &old_lines, &new_lines);
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
                start_line: SourceLine::new(prefix),
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

    pub fn text_in_range(&self, (start, end): (Position, Position)) -> String {
        if self.is_empty_doc {
            return String::new();
        }
        let (start, end) = if start <= end {
            (start, end)
        } else {
            (end, start)
        };
        let start = self.clamp_position(start);
        let end = self.clamp_position(end);
        let start_char = self.rope.line_to_char(start.line.index()) + start.col;
        let end_char = self.rope.line_to_char(end.line.index()) + end.col;
        self.rope.slice(start_char..end_char).to_string()
    }

    pub fn selected_text(&self) -> Option<String> {
        if self.selection.is_empty() {
            None
        } else {
            Some(self.text_in_range(self.selection.range()))
        }
    }

    pub fn undo(&mut self) {
        self.column_goal = None;
        self.last_edit_time = None;
        if let Some(step) = self.undo_stack.pop_back() {
            let replace_len = step.new_lines.len();
            self.lines.replace_source_lines_at(
                step.start_line,
                replace_len,
                step.old_lines.iter().cloned(),
            );
            self.selection = step.cursor_before;
            self.version = step.version_before;
            if !step.new_lines.is_empty() || !step.old_lines.is_empty() {
                self.apply_content_metrics(step.start_line, &step.new_lines, &step.old_lines);
            }

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
            self.lines.replace_source_lines_at(
                step.start_line,
                replace_len,
                step.new_lines.iter().cloned(),
            );
            self.selection = step.cursor_after;
            self.version = step.version_after;
            if !step.old_lines.is_empty() || !step.new_lines.is_empty() {
                self.apply_content_metrics(step.start_line, &step.old_lines, &step.new_lines);
            }

            self.undo_stack.push_back(step);
            self.update_empty_doc_state();
            self.update_dirty_state();
        }
    }
}
