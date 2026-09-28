#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Default, Hash)]
pub struct Position {
    pub line: usize,
    pub col: usize,
}

impl Position {
    pub fn new(line: usize, col: usize) -> Self {
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
    pub start_line: usize,
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
        line_idx: usize,
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
