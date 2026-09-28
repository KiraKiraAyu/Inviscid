use super::scanner::compute_syntactic_separators;
use super::types::{BlockKind, ParsedLine};
use crate::buffer::Position;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum VisualBlock {
    Heading {
        source_line: usize,
    },
    Paragraph {
        source_line: usize,
    },
    CodeBlock {
        code_line_count: usize,
        start_line: usize,
        end_line: usize,
    },
    Table {
        row_count: usize,
        start_line: usize,
        end_line: usize,
    },
    ListItem {
        source_line: usize,
    },
    BlockQuote {
        source_line: usize,
    },
    ThematicBreak {
        source_line: usize,
    },
    Image {
        source_line: usize,
    },
}

impl VisualBlock {
    pub fn source_range(&self) -> std::ops::Range<usize> {
        match self {
            VisualBlock::Heading { source_line }
            | VisualBlock::Paragraph { source_line }
            | VisualBlock::ListItem { source_line }
            | VisualBlock::BlockQuote { source_line }
            | VisualBlock::ThematicBreak { source_line }
            | VisualBlock::Image { source_line } => *source_line..*source_line + 1,
            VisualBlock::CodeBlock {
                start_line,
                end_line,
                ..
            }
            | VisualBlock::Table {
                start_line,
                end_line,
                ..
            } => *start_line..*end_line + 1,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct VisualPosition {
    pub block_idx: usize,
    pub line_in_block: usize,
    pub col: usize,
}

impl VisualPosition {
    pub fn new(block_idx: usize, line_in_block: usize, col: usize) -> Self {
        Self {
            block_idx,
            line_in_block,
            col,
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct VisualDocument {
    pub blocks: Vec<VisualBlock>,
}

impl VisualDocument {
    #[cfg(test)]
    pub fn from_lines(lines: &[String]) -> Self {
        let parsed = super::scanner::MarkdownScanner::scan_document(lines);
        Self::from_parsed(lines, &parsed)
    }

    pub fn from_parsed(lines: &[String], parsed_lines: &[ParsedLine]) -> Self {
        if lines.is_empty() || parsed_lines.is_empty() {
            return Self {
                blocks: vec![VisualBlock::Paragraph { source_line: 0 }],
            };
        }

        let mut blocks = Vec::new();
        let mut idx = 0;
        let separators = compute_syntactic_separators(lines);

        while idx < parsed_lines.len() {
            let parsed = &parsed_lines[idx];

            match &parsed.kind {
                BlockKind::CodeBlock { is_fence_start } => {
                    if *is_fence_start {
                        let start_line = idx;
                        let mut code_line_count = 0usize;
                        let mut end_line = start_line;
                        idx += 1;
                        while idx < parsed_lines.len() {
                            match &parsed_lines[idx].kind {
                                BlockKind::CodeBlockContent => {
                                    code_line_count += 1;
                                    end_line = idx;
                                    idx += 1;
                                }
                                BlockKind::CodeBlock {
                                    is_fence_start: false,
                                } => {
                                    end_line = idx;
                                    idx += 1;
                                    break;
                                }
                                _ => break,
                            }
                        }
                        blocks.push(VisualBlock::CodeBlock {
                            code_line_count,
                            start_line,
                            end_line,
                        });
                    } else {
                        idx += 1;
                    }
                }
                BlockKind::CodeBlockContent => {
                    idx += 1;
                }
                BlockKind::Table { is_header, .. } => {
                    if *is_header {
                        let start_line = idx;
                        let mut row_count = 0usize;
                        let mut end_line = idx;
                        idx += 1;
                        while idx < parsed_lines.len() {
                            if let BlockKind::Table {
                                is_header: false,
                                is_delimiter,
                                ..
                            } = &parsed_lines[idx].kind
                            {
                                if !is_delimiter {
                                    row_count += 1;
                                }
                                end_line = idx;
                                idx += 1;
                            } else {
                                break;
                            }
                        }
                        blocks.push(VisualBlock::Table {
                            row_count,
                            start_line,
                            end_line,
                        });
                    } else {
                        idx += 1;
                    }
                }
                BlockKind::Heading { .. } => {
                    blocks.push(VisualBlock::Heading { source_line: idx });
                    idx += 1;
                }
                BlockKind::BulletList
                | BlockKind::OrderedList { .. }
                | BlockKind::TaskList { .. } => {
                    blocks.push(VisualBlock::ListItem { source_line: idx });
                    idx += 1;
                }
                BlockKind::BlockQuote => {
                    blocks.push(VisualBlock::BlockQuote { source_line: idx });
                    idx += 1;
                }
                BlockKind::ThematicBreak => {
                    blocks.push(VisualBlock::ThematicBreak { source_line: idx });
                    idx += 1;
                }
                BlockKind::Image { .. } => {
                    blocks.push(VisualBlock::Image { source_line: idx });
                    idx += 1;
                }
                BlockKind::Paragraph => {
                    if separators.get(idx).copied().unwrap_or(false) {
                        idx += 1;
                        continue;
                    }
                    blocks.push(VisualBlock::Paragraph { source_line: idx });
                    idx += 1;
                }
            }
        }

        if blocks.is_empty() {
            blocks.push(VisualBlock::Paragraph { source_line: 0 });
        }

        Self { blocks }
    }

    pub fn source_to_visual(&self, pos: Position) -> VisualPosition {
        if self.blocks.is_empty() {
            return VisualPosition::new(0, 0, 0);
        }

        for (b_idx, block) in self.blocks.iter().enumerate() {
            let range = block.source_range();
            if pos.line >= range.start && pos.line < range.end {
                match block {
                    VisualBlock::CodeBlock { start_line, .. } => {
                        let line_in_block = pos.line - start_line;
                        return VisualPosition::new(b_idx, line_in_block, pos.col);
                    }
                    VisualBlock::Table { start_line, .. } => {
                        let line_in_block = if pos.line <= *start_line + 1 {
                            0
                        } else {
                            pos.line - start_line - 1
                        };
                        return VisualPosition::new(b_idx, line_in_block, pos.col);
                    }
                    _ => {
                        return VisualPosition::new(b_idx, 0, pos.col);
                    }
                }
            } else if pos.line < range.start {
                return VisualPosition::new(b_idx.saturating_sub(1), 0, pos.col);
            }
        }

        let last_idx = self.blocks.len() - 1;
        VisualPosition::new(last_idx, 0, pos.col)
    }

    pub fn block_line_count(&self, b_idx: usize) -> usize {
        if b_idx >= self.blocks.len() {
            return 0;
        }
        match &self.blocks[b_idx] {
            VisualBlock::CodeBlock {
                code_line_count, ..
            } => 1 + *code_line_count,
            VisualBlock::Table { row_count, .. } => 1 + *row_count,
            _ => 1,
        }
    }

    pub fn next_visual_line(&self, vpos: VisualPosition) -> Option<VisualPosition> {
        if self.blocks.is_empty() {
            return None;
        }
        let b_count = self.block_line_count(vpos.block_idx);
        if vpos.line_in_block + 1 < b_count {
            Some(VisualPosition::new(
                vpos.block_idx,
                vpos.line_in_block + 1,
                vpos.col,
            ))
        } else if vpos.block_idx + 1 < self.blocks.len() {
            Some(VisualPosition::new(vpos.block_idx + 1, 0, vpos.col))
        } else {
            None
        }
    }

    pub fn prev_visual_line(&self, vpos: VisualPosition) -> Option<VisualPosition> {
        if self.blocks.is_empty() {
            return None;
        }
        if vpos.line_in_block > 0 {
            Some(VisualPosition::new(
                vpos.block_idx,
                vpos.line_in_block - 1,
                vpos.col,
            ))
        } else if vpos.block_idx > 0 {
            let prev_idx = vpos.block_idx - 1;
            let prev_count = self.block_line_count(prev_idx);
            let prev_line = prev_count.saturating_sub(1);
            Some(VisualPosition::new(prev_idx, prev_line, vpos.col))
        } else {
            None
        }
    }

    pub fn visual_to_source(&self, vpos: VisualPosition) -> Position {
        if self.blocks.is_empty() {
            return Position::new(0, 0);
        }

        let b_idx = vpos.block_idx.min(self.blocks.len() - 1);
        match &self.blocks[b_idx] {
            VisualBlock::Heading { source_line, .. }
            | VisualBlock::Paragraph { source_line, .. }
            | VisualBlock::ListItem { source_line, .. }
            | VisualBlock::BlockQuote { source_line, .. }
            | VisualBlock::ThematicBreak { source_line }
            | VisualBlock::Image { source_line, .. } => Position::new(*source_line, vpos.col),
            VisualBlock::CodeBlock {
                start_line,
                end_line,
                ..
            } => {
                let target_line = (*start_line + vpos.line_in_block).min(*end_line);
                Position::new(target_line, vpos.col)
            }
            VisualBlock::Table {
                start_line,
                end_line,
                ..
            } => {
                let target_line = if vpos.line_in_block == 0 {
                    *start_line
                } else {
                    (*start_line + 1 + vpos.line_in_block).min(*end_line)
                };
                Position::new(target_line, vpos.col)
            }
        }
    }
}
