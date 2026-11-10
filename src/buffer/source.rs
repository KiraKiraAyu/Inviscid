use super::{SourceLine, TextBuffer};
use ropey::{Rope, RopeBuilder, RopeSlice};
use std::borrow::Cow;

/// Read-only access to logical source lines without materializing a document.
/// A line crossing Rope chunks is copied on demand; contiguous lines are borrowed.
pub trait LineSource {
    fn line_count(&self) -> usize;
    /// Returns a line without its trailing LF. Panics for an out-of-bounds line.
    fn source_line(&self, line: SourceLine) -> Cow<'_, str>;

    fn source_snapshot(&self) -> SourceSnapshot {
        let mut builder = RopeBuilder::new();
        for (index, line) in self.source_lines().enumerate() {
            if index > 0 {
                builder.append("\n");
            }
            builder.append(&line);
        }
        SourceSnapshot {
            rope: builder.finish(),
            lines: self.line_count(),
            structure: None,
        }
    }

    /// Visits logical lines in order, without repeating indexed lookups.
    fn source_lines(&self) -> impl Iterator<Item = Cow<'_, str>> {
        (0..self.line_count()).map(|index| self.source_line(SourceLine::new(index)))
    }
}

impl LineSource for [String] {
    fn line_count(&self) -> usize {
        self.len()
    }

    fn source_line(&self, line: SourceLine) -> Cow<'_, str> {
        Cow::Borrowed(&self[line.index()])
    }

    fn source_lines(&self) -> impl Iterator<Item = Cow<'_, str>> {
        self.iter().map(|line| Cow::Borrowed(line.as_str()))
    }
}

impl LineSource for Vec<String> {
    fn line_count(&self) -> usize {
        self.len()
    }

    fn source_line(&self, line: SourceLine) -> Cow<'_, str> {
        self.as_slice().source_line(line)
    }

    fn source_lines(&self) -> impl Iterator<Item = Cow<'_, str>> {
        self.as_slice().source_lines()
    }
}

impl<const N: usize> LineSource for [String; N] {
    fn line_count(&self) -> usize {
        N
    }

    fn source_line(&self, line: SourceLine) -> Cow<'_, str> {
        self.as_slice().source_line(line)
    }

    fn source_lines(&self) -> impl Iterator<Item = Cow<'_, str>> {
        self.as_slice().source_lines()
    }
}

impl LineSource for TextBuffer {
    fn source_snapshot(&self) -> SourceSnapshot {
        SourceSnapshot {
            rope: self.rope.clone(),
            lines: self.line_count(),
            structure: Some(self.line_index().structure_snapshot()),
        }
    }
    fn line_count(&self) -> usize {
        self.line_count()
    }

    fn source_line(&self, line: SourceLine) -> Cow<'_, str> {
        assert!(
            line.index() < self.line_count(),
            "source line out of bounds"
        );
        if self.is_empty_doc {
            return Cow::Borrowed("");
        }
        rope_line_content(self.rope.line(line.index()))
    }

    fn source_lines(&self) -> impl Iterator<Item = Cow<'_, str>> {
        let empty_lines =
            std::iter::repeat_n(Cow::Borrowed(""), if self.is_empty_doc { 2 } else { 0 });
        let rope_lines = self
            .rope
            .lines()
            .take(if self.is_empty_doc {
                0
            } else {
                self.rope.len_lines()
            })
            .map(rope_line_content);
        empty_lines.chain(rope_lines)
    }
}

#[derive(Clone, Debug)]
pub struct SourceSnapshot {
    rope: Rope,
    lines: usize,
    structure: Option<super::StructureIndex>,
}

impl SourceSnapshot {
    #[cfg(test)]
    pub(crate) fn replace_rows(&mut self, changes: &[(usize, String)]) {
        while self.rope.len_lines() < self.lines {
            self.rope.insert(self.rope.len_chars(), "\n");
        }
        for (row, text) in changes.iter().rev() {
            let start = self.rope.line_to_char(*row);
            let end = if row + 1 < self.rope.len_lines() {
                self.rope.line_to_char(row + 1) - 1
            } else {
                self.rope.len_chars()
            };
            self.rope.remove(start..end);
            self.rope.insert(start, text);
        }
        self.structure = None;
    }

    pub(crate) fn structure_index(&self) -> super::StructureIndex {
        self.structure
            .clone()
            .unwrap_or_else(|| super::StructureIndex::new(self))
    }
}

impl LineSource for SourceSnapshot {
    fn line_count(&self) -> usize {
        self.lines
    }

    fn source_line(&self, line: SourceLine) -> Cow<'_, str> {
        assert!(line.index() < self.lines);
        if self.rope.len_chars() == 0 {
            return Cow::Borrowed("");
        }
        rope_line_content(self.rope.line(line.index()))
    }
}

fn rope_line_content(line: RopeSlice<'_>) -> Cow<'_, str> {
    if let Some(text) = line.as_str() {
        Cow::Borrowed(text.strip_suffix('\n').unwrap_or(text))
    } else {
        let mut text = line.to_string();
        if text.ends_with('\n') {
            text.pop();
        }
        Cow::Owned(text)
    }
}
