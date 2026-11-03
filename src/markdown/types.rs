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
    MermaidBlock {
        is_fence_start: bool,
    },
    MermaidBlockContent,
    MathBlock {
        is_start: bool,
    },
    MathBlockContent,
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
        )
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct QuoteContext {
    pub depth: usize,
    pub content_indent: usize,
    pub is_start: bool,
    pub is_end: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ParsedLine {
    pub raw_text: String,
    pub kind: BlockKind,
    pub spans: Vec<InlineSpan>,
    pub quote: Option<QuoteContext>,
}

impl ParsedLine {
    #[inline]
    pub fn syntax_prefix_len(&self) -> usize {
        let quote_prefix_len = if self.quote.is_some() {
            crate::markdown::quote_prefix_len(&self.raw_text)
        } else {
            0
        };
        let inner_prefix_len = if self.kind.has_syntax_prefix() {
            crate::markdown::get_block_prefix_len(&self.raw_text[quote_prefix_len..])
        } else {
            0
        };
        quote_prefix_len + inner_prefix_len
    }

    pub fn visible_text(&self, is_active: bool, cursor_col: usize, is_source: bool) -> String {
        self.visible_text_for_selection(is_active, cursor_col, is_source, false)
    }

    pub fn visible_text_for_selection(
        &self,
        is_active: bool,
        cursor_col: usize,
        is_source: bool,
        is_selection_disclosure: bool,
    ) -> String {
        if is_source {
            return self.raw_text.clone();
        }
        let mut out = String::with_capacity(self.raw_text.len());
        for span in &self.spans {
            if span.is_visible_for_selection(is_active, cursor_col, false, is_selection_disclosure)
            {
                out.push_str(&span.rendered_text(is_active, cursor_col, false));
            }
        }
        out
    }
}
