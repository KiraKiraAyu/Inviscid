use std::fmt;
use std::ops::{Add, AddAssign, Sub, SubAssign};

/// Zero-based line number in source text.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct SourceLine(usize);

impl SourceLine {
    pub const fn new(index: usize) -> Self {
        Self(index)
    }

    pub const fn index(self) -> usize {
        self.0
    }

    pub const fn saturating_sub(self, offset: usize) -> Self {
        Self::new(self.0.saturating_sub(offset))
    }

    pub const fn checked_sub(self, offset: usize) -> Option<Self> {
        match self.0.checked_sub(offset) {
            Some(index) => Some(Self::new(index)),
            None => None,
        }
    }
}

impl fmt::Display for SourceLine {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.index().fmt(f)
    }
}

impl Add<usize> for SourceLine {
    type Output = Self;

    fn add(self, offset: usize) -> Self::Output {
        Self::new(self.index() + offset)
    }
}

impl AddAssign<usize> for SourceLine {
    fn add_assign(&mut self, offset: usize) {
        *self = *self + offset;
    }
}

impl Sub<usize> for SourceLine {
    type Output = Self;

    fn sub(self, offset: usize) -> Self::Output {
        Self::new(self.index() - offset)
    }
}

impl Sub<SourceLine> for SourceLine {
    type Output = usize;

    fn sub(self, other: SourceLine) -> Self::Output {
        self.index() - other.index()
    }
}

impl SubAssign<usize> for SourceLine {
    fn sub_assign(&mut self, offset: usize) {
        *self = *self - offset;
    }
}

/// Position expressed in the editable source document's coordinate space.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct SourcePosition {
    pub line: SourceLine,
    pub col: usize,
}

impl SourcePosition {
    pub const fn new(line: SourceLine, col: usize) -> Self {
        Self { line, col }
    }

    pub fn from_position(position: Position) -> Self {
        Self::new(position.line, position.col)
    }

    pub fn to_position(self) -> Position {
        Position::new(self.line, self.col)
    }
}

/// ```compile_fail
/// use inviscid::buffer::Position;
/// let _ = Position::new(0usize, 0);
/// ```
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Default, Hash)]
pub struct Position {
    pub line: SourceLine,
    pub col: usize,
}

impl Position {
    pub const fn new(line: SourceLine, col: usize) -> Self {
        Self { line, col }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Selection {
    pub anchor: Position,
    pub head: Position,
}

impl Selection {
    pub fn new(anchor: Position, head: Position) -> Self {
        Self { anchor, head }
    }

    pub fn cursor(pos: Position) -> Self {
        Self {
            anchor: pos,
            head: pos,
        }
    }

    pub fn is_empty(&self) -> bool {
        self.anchor == self.head
    }

    pub fn start(&self) -> Position {
        self.anchor.min(self.head)
    }

    pub fn end(&self) -> Position {
        self.anchor.max(self.head)
    }

    pub fn range(&self) -> (Position, Position) {
        (self.start(), self.end())
    }
}

pub const MAX_UNDO_STEPS: usize = 1000;

/// An owned line-range slice sharing Rope nodes, with an explicit empty-line extent.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UndoLines {
    pub(crate) text: ropey::Rope,
    line_count: usize,
}

impl UndoLines {
    pub(crate) fn capture(rope: &ropey::Rope, start: usize, count: usize, empty: bool) -> Self {
        let text = if count == 0 {
            ropey::Rope::new()
        } else if empty || rope.len_chars() == 0 || start >= rope.len_lines() {
            ropey::Rope::from_str(&"\n".repeat(count - 1))
        } else {
            let available = count.min(rope.len_lines() - start);
            let end_line = start + available;
            let end = if end_line < rope.len_lines() {
                rope.line_to_char(end_line) - 1
            } else {
                rope.len_chars()
            };
            let mut text = ropey::Rope::from(rope.slice(rope.line_to_char(start)..end));
            if available < count {
                text.insert(text.len_chars(), &"\n".repeat(count - available));
            }
            text
        };
        Self {
            text,
            line_count: count,
        }
    }

    pub fn len(&self) -> usize {
        self.line_count
    }

    pub fn is_empty(&self) -> bool {
        self.line_count == 0
    }

    pub(crate) fn word_count(&self) -> usize {
        let mut in_word = false;
        let mut words = 0;
        for character in self.text.chars() {
            if character.is_whitespace() {
                in_word = false;
            } else {
                words += usize::from(!in_word);
                in_word = true;
            }
        }
        words
    }

    #[cfg(test)]
    pub(crate) fn to_lines(&self) -> Vec<String> {
        if self.is_empty() {
            Vec::new()
        } else {
            self.text
                .to_string()
                .split('\n')
                .map(str::to_owned)
                .collect()
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UndoStep {
    pub start_line: SourceLine,
    pub old_lines: UndoLines,
    pub new_lines: UndoLines,
    pub old_empty_doc: bool,
    pub new_empty_doc: bool,
    pub old_document_line_count: usize,
    pub new_document_line_count: usize,
    pub cursor_before: Selection,
    pub cursor_after: Selection,
    pub version_before: usize,
    pub version_after: usize,
}

impl UndoStep {
    pub fn try_coalesce_single_char(
        &mut self,
        line_idx: SourceLine,
        new_line_content: UndoLines,
        new_cursor_after: Selection,
        new_version_after: usize,
    ) -> bool {
        if self.start_line == line_idx && self.old_lines.len() == 1 && self.new_lines.len() == 1 {
            self.new_lines = new_line_content;
            self.cursor_after = new_cursor_after;
            self.version_after = new_version_after;
            true
        } else {
            false
        }
    }
}
