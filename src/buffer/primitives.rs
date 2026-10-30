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

impl Add<ProjectedLine> for SourceLine {
    type Output = Self;

    fn add(self, offset: ProjectedLine) -> Self::Output {
        Self::new(self.index() + offset.index())
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

/// Zero-based visual block index in a rendered document.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ProjectedBlockIndex(usize);

impl ProjectedBlockIndex {
    pub const fn new(index: usize) -> Self {
        Self(index)
    }

    pub const fn index(self) -> usize {
        self.0
    }
}

impl Add<usize> for ProjectedBlockIndex {
    type Output = Self;

    fn add(self, offset: usize) -> Self::Output {
        Self::new(self.index() + offset)
    }
}

impl Sub<usize> for ProjectedBlockIndex {
    type Output = Self;

    fn sub(self, offset: usize) -> Self::Output {
        Self::new(self.index() - offset)
    }
}

/// Zero-based line within a projected visual block.
///
/// ```compile_fail
/// use inviscid::buffer::{ProjectedLine, SourceLine};
/// let source_line = SourceLine::new(0);
/// let visual_line = ProjectedLine::new(0);
/// let _ = source_line == visual_line;
/// ```
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ProjectedLine(usize);

impl ProjectedLine {
    pub const fn new(index: usize) -> Self {
        Self(index)
    }

    pub const fn index(self) -> usize {
        self.0
    }

    pub const fn saturating_sub(self, offset: usize) -> Self {
        Self::new(self.0.saturating_sub(offset))
    }
}

impl AddAssign<usize> for ProjectedLine {
    fn add_assign(&mut self, offset: usize) {
        *self = *self + offset;
    }
}

impl Add<usize> for ProjectedLine {
    type Output = Self;

    fn add(self, offset: usize) -> Self::Output {
        Self::new(self.index() + offset)
    }
}

impl Sub<usize> for ProjectedLine {
    type Output = Self;

    fn sub(self, offset: usize) -> Self::Output {
        Self::new(self.index() - offset)
    }
}

impl Add<ProjectedLine> for ProjectedLine {
    type Output = Self;

    fn add(self, offset: ProjectedLine) -> Self::Output {
        Self::new(self.index() + offset.index())
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
/// use inviscid::buffer::{Position, ProjectedLine};
/// let _ = Position::new(ProjectedLine::new(0), 0);
/// ```
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

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UndoStep {
    pub start_line: SourceLine,
    pub old_lines: Vec<String>,
    pub new_lines: Vec<String>,
    pub cursor_before: Selection,
    pub cursor_after: Selection,
    pub version_before: usize,
    pub version_after: usize,
}

impl UndoStep {
    pub fn try_coalesce_single_char(
        &mut self,
        line_idx: SourceLine,
        new_line_content: String,
        new_cursor_after: Selection,
        new_version_after: usize,
    ) -> bool {
        if self.start_line == line_idx && self.old_lines.len() == 1 && self.new_lines.len() == 1 {
            self.new_lines[0] = new_line_content;
            self.cursor_after = new_cursor_after;
            self.version_after = new_version_after;
            true
        } else {
            false
        }
    }
}
