use ropey::Rope;
#[cfg(test)]
use std::cell::Cell;
use std::cell::RefCell;
use std::collections::VecDeque;
use std::ops::{Index, IndexMut, Range, RangeFrom, RangeInclusive};
use std::path::{Path, PathBuf};
use std::time::{Instant, SystemTime};

mod fingerprint;
#[cfg(test)]
mod fingerprint_loading_tests;
mod loading;
pub(crate) use loading::StreamingBuffer;
pub mod movement;
pub mod operations;
mod parser_summary;
pub mod primitives;
mod source;
mod structure_index;
#[cfg(test)]
pub mod tests;

pub use fingerprint::{FingerprintJob, FingerprintResult};
pub use primitives::*;
pub use source::{LineSource, SourceSnapshot};
pub(crate) use structure_index::StructureIndex;

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
    /// Normalized document text; line strings are materialized only on demand.
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
    line_fingerprints: fingerprint::FingerprintState,
    pub(in crate::buffer) version: usize,
    pub(in crate::buffer) revision: u64,
    last_edit_time: Option<Instant>,
    is_empty_doc: bool,

    pub(in crate::buffer) line_index_cache: RefCell<Option<movement::LineIndex>>,
    pub(in crate::buffer) last_edit_delta: Option<EditDelta>,
    total_word_count: usize,
    #[cfg(test)]
    full_line_materializations: Cell<usize>,
}

impl Default for TextBuffer {
    fn default() -> Self {
        Self::new()
    }
}

pub fn compute_lines_hash(lines: &[String]) -> u64 {
    fingerprint::hash_lines(lines)
}

#[inline]
pub(crate) fn char_col_to_byte_offset(s: &str, char_col: usize) -> usize {
    s.char_indices()
        .nth(char_col)
        .map(|(b, _)| b)
        .unwrap_or(s.len())
}

impl TextBuffer {
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
        let mut buffer =
            Self::from_bytes(content.as_bytes(), file_path).expect("str is valid UTF-8");
        let result = buffer.fingerprint_job().unwrap().run();
        buffer.install_fingerprints(result, buffer.revision);
        buffer
    }

    fn from_rope(
        rope: Rope,
        line_ending: LineEnding,
        total_word_count: usize,
        file_path: Option<PathBuf>,
    ) -> Self {
        let is_empty_doc = rope.len_chars() == 0;
        let line_fingerprints = fingerprint::FingerprintState::Pending {
            baseline: Some(rope.clone()),
        };
        let saved_line_count = if is_empty_doc { 2 } else { rope.len_lines() };
        let baseline_char_count = rope.len_chars();

        Self {
            rope,
            selection: Selection::cursor(Position::new(SourceLine::new(0), 0)),
            file_path,
            line_ending,
            last_saved_mtime: None,
            is_dirty: false,
            column_goal: None,
            undo_stack: VecDeque::new(),
            redo_stack: VecDeque::new(),
            saved_baseline: SavedBaseline {
                content_hash: 0,
                revision: 0,
                line_count: saved_line_count,
                char_count: baseline_char_count,
                is_empty_doc,
            },
            line_fingerprints,
            version: 0,
            revision: 0,
            last_edit_time: None,
            is_empty_doc,
            line_index_cache: RefCell::new(None),
            last_edit_delta: None,
            total_word_count,
            #[cfg(test)]
            full_line_materializations: Cell::new(0),
        }
    }

    fn old_rope_lines(&self, start: usize, count: usize, empty_before: bool) -> Vec<String> {
        if empty_before || self.rope.len_chars() == 0 {
            return vec![String::new(); count];
        }
        (start..start + count)
            .map(|line_idx| {
                let mut line = self
                    .rope
                    .get_line(line_idx)
                    .map(|line| line.to_string())
                    .unwrap_or_default();
                if line.ends_with('\n') {
                    line.pop();
                }
                line
            })
            .collect()
    }

    fn position_to_rope_char(&self, pos: Position) -> usize {
        if self.is_empty_doc {
            return 0;
        }
        let line = pos
            .line
            .index()
            .min(self.rope.len_lines().saturating_sub(1));
        self.rope.line_to_char(line) + pos.col.min(self.line_len(SourceLine::new(line)))
    }

    fn rope_char_to_position(&self, char_idx: usize) -> Position {
        if self.rope.len_chars() == 0 {
            return Position::default();
        }
        let char_idx = char_idx.min(self.rope.len_chars());
        let line = self.rope.char_to_line(char_idx);
        let col = char_idx.saturating_sub(self.rope.line_to_char(line));
        Position::new(SourceLine::new(line), col)
    }

    fn replace_range_in_rope(&mut self, range: (Position, Position), text: &str) -> Position {
        let (start, end) = if range.0 <= range.1 {
            range
        } else {
            (range.1, range.0)
        };
        let start = self.clamp_position(start);
        let end = self.clamp_position(end);
        let normalized = text.replace("\r\n", "\n").replace('\r', "\n");
        if normalized.is_empty() && range.0 == range.1 {
            return self.clamp_position(range.0);
        }
        let cursor_before = self.selection;
        let version_before = self.version;
        let old_empty = self.is_empty_doc;
        let old_document_line_count = self.line_count();
        let old_count = if old_empty {
            2
        } else {
            end.line.index().saturating_sub(start.line.index()) + 1
        };
        let old_lines = self.old_rope_lines(start.line.index(), old_count, old_empty);
        let start_char = self.position_to_rope_char(start);
        let end_char = self.position_to_rope_char(end);
        let at_document_end = end_char == self.rope.len_chars();
        let old_has_trailing_newline = self
            .rope
            .len_chars()
            .checked_sub(1)
            .map(|idx| self.rope.char(idx) == '\n')
            .unwrap_or(false);
        self.rope.remove(start_char..end_char);

        let append_virtual_trailing_line = old_empty
            || (at_document_end && (old_has_trailing_newline || normalized.ends_with('\n')));
        let mut replacement = normalized.clone();
        if append_virtual_trailing_line && !replacement.ends_with('\n') && !replacement.is_empty() {
            replacement.push('\n');
        }
        self.rope.insert(start_char, &replacement);

        self.is_empty_doc = self.rope.len_chars() == 0;
        let new_cursor_char = start_char + normalized.chars().count();
        let new_cursor = self.rope_char_to_position(new_cursor_char);
        let new_count = if self.is_empty_doc {
            2
        } else {
            normalized.matches('\n').count()
                + 1
                + usize::from(append_virtual_trailing_line && !normalized.ends_with('\n'))
        };
        let new_lines = self.old_rope_lines(start.line.index(), new_count, false);

        self.version = self.version.wrapping_add(1);
        self.finish_content_metrics(SourceLine::new(start.line.index()), &old_lines, &new_lines);
        let step = UndoStep {
            start_line: start.line,
            old_lines,
            new_lines,
            old_empty_doc: old_empty,
            new_empty_doc: self.is_empty_doc,
            old_document_line_count,
            new_document_line_count: self.line_count(),
            cursor_before,
            cursor_after: Selection::cursor(new_cursor),
            version_before,
            version_after: self.version,
        };
        self.selection = Selection::cursor(new_cursor);
        self.push_undo_step(step, false);
        new_cursor
    }

    #[inline]
    pub fn lines(&self) -> Vec<String> {
        self.to_lines()
    }

    #[inline]
    pub fn line(&self, source_line: SourceLine) -> Option<String> {
        if self.is_empty_doc {
            return (source_line.index() < 2).then(String::new);
        }
        self.line_text(source_line)
    }

    pub fn line_text(&self, source_line: SourceLine) -> Option<String> {
        if self.is_empty_doc {
            return (source_line.index() < 2).then(String::new);
        }
        if source_line.index() >= self.rope.len_lines() {
            return None;
        }
        let mut line = self.rope.line(source_line.index()).to_string();
        if line.ends_with('\n') {
            line.pop();
        }
        Some(line)
    }

    pub fn to_lines(&self) -> Vec<String> {
        #[cfg(test)]
        self.full_line_materializations
            .set(self.full_line_materializations.get() + 1);
        if self.is_empty_doc {
            return vec![String::new(), String::new()];
        }
        (0..self.rope.len_lines())
            .map(|line| self.line_text(SourceLine::new(line)).unwrap_or_default())
            .collect()
    }

    #[inline]
    pub fn line_count(&self) -> usize {
        if self.is_empty_doc {
            2
        } else {
            self.rope.len_lines()
        }
    }

    #[cfg(test)]
    pub(crate) fn full_line_materializations(&self) -> usize {
        self.full_line_materializations.get()
    }

    #[inline]
    fn has_only_empty_sentinel_lines(&self) -> bool {
        self.rope.len_chars() == 0
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
        if self.is_empty_doc {
            return 0;
        }
        self.rope.get_line(source_line.index()).map_or(0, |line| {
            line.len_chars()
                - usize::from(line.len_chars() > 0 && line.char(line.len_chars() - 1) == '\n')
        })
    }

    pub fn clamp_position(&self, pos: Position) -> Position {
        if self.line_count() == 0 {
            return Position::new(SourceLine::new(0), 0);
        }
        let line = pos
            .line
            .min(crate::buffer::SourceLine::new(self.line_count() - 1));
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

    /// A cheap, immutable copy-on-write snapshot for background readers.
    pub fn text_snapshot(&self) -> Rope {
        self.rope.clone()
    }

    /// Yields chunks for each line and line ending.
    pub fn chunks_with_line_ending(&self) -> impl Iterator<Item = String> {
        let content = self.to_string_content();
        let ending = self.line_ending.as_str();
        let mut chunks = Vec::new();
        for segment in content.split_inclusive(ending) {
            if let Some(line) = segment.strip_suffix(ending) {
                if !line.is_empty() {
                    chunks.push(line.to_owned());
                }
                chunks.push(ending.to_owned());
            } else if !segment.is_empty() {
                chunks.push(segment.to_owned());
            }
        }
        chunks.into_iter()
    }

    pub fn word_count(&self) -> usize {
        self.total_word_count
    }

    #[inline]
    pub fn char_count(&self) -> usize {
        self.rope.len_chars()
    }

    pub fn pos_to_utf16_offset(&self, pos: SourcePosition) -> usize {
        if self.is_empty_doc {
            // Preserve the editor's two virtual lines in an empty document.
            return pos.line.index().min(1);
        }
        let char_idx = self.position_to_rope_char(pos.to_position());
        self.rope.char_to_utf16_cu(char_idx)
    }

    pub fn utf16_offset_to_pos(&self, offset: usize) -> SourcePosition {
        if self.is_empty_doc {
            return SourcePosition::new(SourceLine::new(usize::from(offset > 0)), 0);
        }
        let offset = offset.min(self.rope.len_utf16_cu());
        let char_idx = self.rope.utf16_cu_to_char(offset);
        SourcePosition::from_position(self.rope_char_to_position(char_idx))
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
        if self.line_count() == 0 {
            return (String::new(), 0..0);
        }

        let start_pos = self.utf16_offset_to_pos(range_utf16.start);
        let end_pos = self.utf16_offset_to_pos(range_utf16.end);
        let actual_start_utf16 = self.pos_to_utf16_offset(start_pos);
        let actual_end_utf16 = self.pos_to_utf16_offset(end_pos);
        let adjusted = actual_start_utf16..actual_end_utf16;

        if start_pos.line == end_pos.line {
            if start_pos.line.index() >= self.line_count() {
                return (String::new(), adjusted);
            }
            let line = self.line(start_pos.line).unwrap_or_default();
            let char_count = end_pos.col.saturating_sub(start_pos.col);
            let text: String = line.chars().skip(start_pos.col).take(char_count).collect();
            (text, adjusted)
        } else {
            let mut result = Vec::new();
            let start_line = start_pos.line.index();
            let end_line = end_pos
                .line
                .index()
                .min(self.line_count().saturating_sub(1));
            for idx in start_line..=end_line {
                let line = self
                    .line(crate::buffer::SourceLine::new(idx))
                    .unwrap_or_default();
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
            content_hash: self.compute_content_hash(),
            revision: self.revision,
            line_count: self.line_count(),
            char_count: self.char_count(),
            is_empty_doc: self.is_empty(),
        }
    }

    pub fn mark_saved_as(&mut self, baseline: SavedBaseline) {
        if let fingerprint::FingerprintState::Pending { baseline } = &mut self.line_fingerprints {
            *baseline = None;
        }
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

        if matches!(
            self.line_fingerprints,
            fingerprint::FingerprintState::Pending { .. }
        ) {
            self.is_dirty = true;
            return;
        }

        if self.line_count() != self.saved_baseline.line_count
            || self.char_count() != self.saved_baseline.char_count
            || self.is_empty_doc != self.saved_baseline.is_empty_doc
        {
            self.is_dirty = true;
            return;
        }

        let current_hash = self.compute_content_hash();
        self.is_dirty = current_hash != self.saved_baseline.content_hash;
    }

    fn compute_content_hash(&self) -> u64 {
        match &self.line_fingerprints {
            fingerprint::FingerprintState::Ready(fingerprints) => fingerprints.content_hash(),
            fingerprint::FingerprintState::Pending { .. } => {
                fingerprint::LineFingerprints::new(&self.rope).content_hash()
            }
        }
    }

    fn apply_content_metrics(
        &mut self,
        start_line: SourceLine,
        replaced_lines: &[String],
        inserted_lines: &[String],
    ) {
        if self.is_empty_doc {
            self.rope = Rope::new();
        } else {
            self.replace_rope_lines(start_line, replaced_lines.len(), inserted_lines);
        }

        self.finish_content_metrics(start_line, replaced_lines, inserted_lines);
    }

    fn apply_history_content(
        &mut self,
        start_line: SourceLine,
        replaced_lines: &[String],
        inserted_lines: &[String],
        target_empty_doc: bool,
        target_document_line_count: usize,
    ) {
        if target_empty_doc {
            self.rope = Rope::new();
        } else if self.is_empty_doc {
            // A history range can cover only part of the former document.
            // Its saved extent distinguishes a virtual trailing line from a
            // file that originally had no trailing newline.
            let mut lines = vec![""; target_document_line_count];
            let start = start_line.index();
            for (target, text) in lines[start..start + inserted_lines.len()]
                .iter_mut()
                .zip(inserted_lines)
            {
                *target = text.as_str();
            }
            self.rope = Rope::from_str(&lines.join("\n"));
        } else {
            self.replace_rope_lines(start_line, replaced_lines.len(), inserted_lines);
        }
        self.is_empty_doc = target_empty_doc;
        self.finish_content_metrics(start_line, replaced_lines, inserted_lines);
    }

    /// Replaces a contiguous range of logical lines without materializing the document.
    pub fn replace_line_range(
        &mut self,
        start_line: SourceLine,
        old_line_count: usize,
        inserted_lines: Vec<String>,
        selection_after: Selection,
    ) {
        self.column_goal = None;
        let cursor_before = self.selection;
        let version_before = self.version;
        let old_empty_doc = self.is_empty_doc;
        let line_count_before = self.line_count();
        let mut start = start_line.index().min(line_count_before);
        let old_count = old_line_count.min(line_count_before.saturating_sub(start));
        debug_assert!(
            inserted_lines
                .iter()
                .all(|line| !line.contains('\n') && !line.contains('\r'))
        );

        let mut old_lines = self.old_rope_lines(start, old_count, old_empty_doc);
        let mut new_lines = inserted_lines;
        let new_empty_doc;

        if old_empty_doc {
            let mut resulting_lines = self.to_lines();
            resulting_lines.splice(start..start + old_count, new_lines);
            if resulting_lines.is_empty()
                || (resulting_lines.len() == 1 && resulting_lines[0].is_empty())
            {
                resulting_lines = vec![String::new(), String::new()];
            }
            new_empty_doc =
                resulting_lines.len() == 2 && resulting_lines.iter().all(String::is_empty);
            old_lines = self.to_lines();
            new_lines = resulting_lines;
            start = 0;
        } else {
            new_empty_doc = start == 0
                && old_count == line_count_before
                && new_lines.len() == 2
                && new_lines.iter().all(String::is_empty);
        }

        let content_changed = old_lines != new_lines;
        if !content_changed && cursor_before == selection_after {
            return;
        }
        if !content_changed {
            old_lines.clear();
            new_lines.clear();
        }

        if content_changed {
            self.version = self.version.wrapping_add(1);
            if new_empty_doc {
                self.rope = Rope::new();
            } else if old_empty_doc {
                self.rope = Rope::from_str(&new_lines.join("\n"));
            } else {
                self.replace_rope_lines(SourceLine::new(start), old_count, &new_lines);
            }
            self.is_empty_doc = new_empty_doc;
            self.finish_content_metrics(SourceLine::new(start), &old_lines, &new_lines);
        }

        self.selection = Selection {
            anchor: self.clamp_position(selection_after.anchor),
            head: self.clamp_position(selection_after.head),
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
            start_line: SourceLine::new(start),
            old_lines,
            new_lines,
            old_empty_doc,
            new_empty_doc: self.is_empty_doc,
            old_document_line_count: line_count_before,
            new_document_line_count: self.line_count(),
            cursor_before,
            cursor_after: self.selection,
            version_before,
            version_after: self.version,
        };
        self.push_undo_step(step, is_single_char_insert);
    }

    fn finish_content_metrics(
        &mut self,
        start_line: SourceLine,
        replaced_lines: &[String],
        inserted_lines: &[String],
    ) {
        let removed_words: usize = replaced_lines
            .iter()
            .map(|line| line.split_whitespace().count())
            .sum();
        let inserted_words: usize = inserted_lines
            .iter()
            .map(|line| line.split_whitespace().count())
            .sum();
        self.total_word_count = self.total_word_count - removed_words + inserted_words;
        if let fingerprint::FingerprintState::Ready(fingerprints) = &mut self.line_fingerprints {
            fingerprints.update(&self.rope, start_line.index(), replaced_lines.len());
        }
        self.revision = self.revision.wrapping_add(1);
        self.last_edit_delta = Some(EditDelta {
            start_line,
            old_line_count: replaced_lines.len(),
            new_line_count: inserted_lines.len(),
        });

        self.update_empty_doc_state();
    }

    fn push_undo_step(&mut self, step: UndoStep, is_single_char_insert: bool) {
        let now = Instant::now();
        let time_ok = self
            .last_edit_time
            .map(|t| now.duration_since(t).as_millis() < 800)
            .unwrap_or(false);

        // A new edit after clearing the document starts a distinct history step.
        let coalesced = if is_single_char_insert && time_ok && !step.old_empty_doc {
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
        if line_idx.index() >= self.line_count() {
            return;
        }

        self.column_goal = None;
        let cursor_before = self.selection;
        let version_before = self.version;
        let was_empty_doc = self.is_empty_doc;
        let old_document_line_count = self.line_count();
        let old_line = self.line_text(line_idx).unwrap_or_default();
        let mut new_line = old_line.clone();
        f(&mut new_line, &mut self.selection);

        if was_empty_doc && !new_line.is_empty() {
            self.is_empty_doc = false;
        }

        let new_line_len = new_line.chars().count();
        let clamp_after_edit = |pos: Position| {
            if pos.line == line_idx {
                Position::new(line_idx, pos.col.min(new_line_len))
            } else {
                self.clamp_position(pos)
            }
        };
        let anchor = clamp_after_edit(self.selection.anchor);
        let head = clamp_after_edit(self.selection.head);
        self.selection.anchor = anchor;
        self.selection.head = head;

        if new_line != old_line || self.selection != cursor_before {
            let new_lines = vec![new_line];
            let old_lines = vec![old_line];
            let content_changed = new_lines[0] != old_lines[0];

            let version_after = if content_changed {
                self.version = self.version.wrapping_add(1);
                if was_empty_doc {
                    self.rope = Rope::from_str("\n");
                    self.replace_rope_lines(line_idx, old_lines.len(), &new_lines);
                    self.finish_content_metrics(line_idx, &old_lines, &new_lines);
                } else {
                    self.apply_content_metrics(line_idx, &old_lines, &new_lines);
                }
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
                old_empty_doc: was_empty_doc,
                new_empty_doc: self.is_empty_doc,
                old_document_line_count,
                new_document_line_count: self.line_count(),
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
        let line_count_before = self.line_count();
        let empty_before = self.is_empty_doc;
        let mut lines = SourceLines::from(self.to_lines());
        f(&mut lines, &mut self.selection);

        let clamp_to_lines = |pos: Position| {
            let line = pos.line.min(SourceLine::new(lines.len().saturating_sub(1)));
            let col = pos.col.min(lines[line].chars().count());
            Position::new(line, col)
        };
        self.selection.anchor = clamp_to_lines(self.selection.anchor);
        self.selection.head = clamp_to_lines(self.selection.head);

        let dirty = lines
            .take_dirty_range()
            .or_else(|| (lines.len() != line_count_before).then_some(0..lines.len()));
        if dirty.is_some() || self.selection != cursor_before {
            let mut prefix = dirty.as_ref().map_or(0, |range| range.start);
            let new_end = dirty
                .as_ref()
                .map_or(prefix, |range| range.end.min(lines.len()));
            let new_count = new_end.saturating_sub(prefix);
            let line_delta = lines.len() as isize - line_count_before as isize;
            let old_count = (new_count as isize - line_delta).max(0) as usize;
            let mut old_lines = self.old_rope_lines(prefix, old_count, empty_before);
            let mut new_lines = lines.as_slice()[prefix..new_end].to_vec();

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
                self.is_empty_doc = lines.len() == 2 && lines.iter().all(String::is_empty);
                if empty_before {
                    self.rope = if self.is_empty_doc {
                        Rope::new()
                    } else {
                        Rope::from_str(&lines.join("\n"))
                    };
                    self.finish_content_metrics(SourceLine::new(prefix), &old_lines, &new_lines);
                } else {
                    self.apply_content_metrics(SourceLine::new(prefix), &old_lines, &new_lines);
                }
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
                old_empty_doc: empty_before,
                new_empty_doc: self.is_empty_doc,
                old_document_line_count: line_count_before,
                new_document_line_count: self.line_count(),
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
            self.selection = step.cursor_before;
            self.version = step.version_before;
            if !step.new_lines.is_empty() || !step.old_lines.is_empty() {
                self.apply_history_content(
                    step.start_line,
                    &step.new_lines,
                    &step.old_lines,
                    step.old_empty_doc,
                    step.old_document_line_count,
                );
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
            self.selection = step.cursor_after;
            self.version = step.version_after;
            if !step.old_lines.is_empty() || !step.new_lines.is_empty() {
                self.apply_history_content(
                    step.start_line,
                    &step.old_lines,
                    &step.new_lines,
                    step.new_empty_doc,
                    step.new_document_line_count,
                );
            }

            self.undo_stack.push_back(step);
            self.update_empty_doc_state();
            self.update_dirty_state();
        }
    }
}
