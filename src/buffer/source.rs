use super::{SourceLine, TextBuffer};
use std::borrow::Cow;

/// Read-only access to logical source lines without materializing a document.
/// A line crossing Rope chunks is copied on demand; contiguous lines are borrowed.
pub trait LineSource {
    fn line_count(&self) -> usize;
    /// Returns a line without its trailing LF. Panics for an out-of-bounds line.
    fn source_line(&self, line: SourceLine) -> Cow<'_, str>;
}

impl LineSource for [String] {
    fn line_count(&self) -> usize {
        self.len()
    }

    fn source_line(&self, line: SourceLine) -> Cow<'_, str> {
        Cow::Borrowed(&self[line.index()])
    }
}

impl LineSource for Vec<String> {
    fn line_count(&self) -> usize {
        self.len()
    }

    fn source_line(&self, line: SourceLine) -> Cow<'_, str> {
        self.as_slice().source_line(line)
    }
}

impl<const N: usize> LineSource for [String; N] {
    fn line_count(&self) -> usize {
        N
    }

    fn source_line(&self, line: SourceLine) -> Cow<'_, str> {
        self.as_slice().source_line(line)
    }
}

impl LineSource for TextBuffer {
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
        let line = self.rope.line(line.index());
        let end = line.len_chars()
            - usize::from(line.len_chars() > 0 && line.char(line.len_chars() - 1) == '\n');
        let content = line.slice(..end);
        match content.as_str() {
            Some(text) => Cow::Borrowed(text),
            None => Cow::Owned(content.to_string()),
        }
    }
}
