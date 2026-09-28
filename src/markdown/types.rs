use super::inline::InlineSpan;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum TableAlignment {
    #[default]
    None,
    Left,
    Center,
    Right,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TableCell {
    pub raw_content: String,
    pub spans: Vec<InlineSpan>,
    pub col_range: (usize, usize),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum BlockKind {
    Heading {
        level: u8,
    },
    CodeBlock {
        is_fence_start: bool,
    },
    CodeBlockContent,
    BlockQuote,
    BulletList,
    OrderedList {
        num: usize,
    },
    TaskList {
        checked: bool,
    },
    ThematicBreak,

    Image {
        alt: String,
        url: String,
    },
    Table {
        is_header: bool,
        is_delimiter: bool,
        alignments: Vec<TableAlignment>,
        cells: Vec<TableCell>,
    },
    Paragraph,
}

impl BlockKind {
    #[inline]
    pub fn has_syntax_prefix(&self) -> bool {
        matches!(
            self,
            BlockKind::Heading { .. }
                | BlockKind::TaskList { .. }
                | BlockKind::BulletList
                | BlockKind::OrderedList { .. }
                | BlockKind::BlockQuote
        )
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ParsedLine {
    pub raw_text: String,
    pub kind: BlockKind,
    pub spans: Vec<InlineSpan>,
}

impl ParsedLine {
    #[inline]
    pub fn syntax_prefix_len(&self) -> usize {
        if self.kind.has_syntax_prefix() {
            crate::markdown::get_block_prefix_len(&self.raw_text)
        } else {
            0
        }
    }

    pub fn visible_text(&self, is_active: bool, cursor_col: usize, is_source: bool) -> String {
        if is_source {
            return self.raw_text.clone();
        }
        let mut out = String::with_capacity(self.raw_text.len());
        for span in &self.spans {
            if span.is_visible(is_active, cursor_col, false) {
                out.push_str(&span.text);
            }
        }
        out
    }
}
