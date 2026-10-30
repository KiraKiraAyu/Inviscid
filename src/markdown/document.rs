use super::scanner::compute_syntactic_separators;
use super::types::{BlockKind, ParsedLine};
use crate::buffer::{ProjectedBlockIndex, ProjectedLine, SourceLine, SourcePosition};
use std::ops::Index;

/// Projected blocks can only be addressed with a projected block coordinate.
///
/// ```compile_fail
/// use inviscid::buffer::ProjectedBlockIndex;
/// use inviscid::markdown::document::{VisualBlock, VisualBlocks};
/// let blocks = VisualBlocks::default();
/// let _ = &blocks[0usize];
/// ```
/// ```compile_fail
/// use inviscid::buffer::SourceLine;
/// use inviscid::markdown::document::VisualBlocks;
/// let blocks = VisualBlocks::default();
/// let _ = &blocks[SourceLine::new(0)];
/// ```
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct VisualBlocks(Vec<VisualBlock>);

impl VisualBlocks {
    pub fn len(&self) -> usize {
        self.0.len()
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    pub fn iter(&self) -> std::slice::Iter<'_, VisualBlock> {
        self.0.iter()
    }

    fn push(&mut self, block: VisualBlock) {
        self.0.push(block);
    }
}

impl Index<ProjectedBlockIndex> for VisualBlocks {
    type Output = VisualBlock;

    fn index(&self, index: ProjectedBlockIndex) -> &Self::Output {
        &self.0[index.index()]
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum VisualBlock {
    Heading {
        source_line: SourceLine,
    },
    Paragraph {
        source_line: SourceLine,
    },
    CodeBlock {
        code_line_count: ProjectedLine,
        start_line: SourceLine,
        end_line: SourceLine,
    },
    Table {
        row_count: ProjectedLine,
        start_line: SourceLine,
        end_line: SourceLine,
    },
    ListItem {
        source_line: SourceLine,
    },
    BlockQuote {
        source_line: SourceLine,
    },
    ThematicBreak {
        source_line: SourceLine,
    },
    Image {
        source_line: SourceLine,
    },
}

impl VisualBlock {
    pub fn source_range(&self) -> std::ops::Range<SourceLine> {
        match self {
            VisualBlock::Heading { source_line }
            | VisualBlock::Paragraph { source_line }
            | VisualBlock::ListItem { source_line }
            | VisualBlock::BlockQuote { source_line }
            | VisualBlock::ThematicBreak { source_line }
            | VisualBlock::Image { source_line } => {
                *source_line..SourceLine::new(source_line.index() + 1)
            }
            VisualBlock::CodeBlock {
                start_line,
                end_line,
                ..
            }
            | VisualBlock::Table {
                start_line,
                end_line,
                ..
            } => *start_line..SourceLine::new(end_line.index() + 1),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct VisualPosition {
    pub block_idx: ProjectedBlockIndex,
    pub line_in_block: ProjectedLine,
    pub col: usize,
}

impl VisualPosition {
    pub fn new(block_idx: ProjectedBlockIndex, line_in_block: ProjectedLine, col: usize) -> Self {
        Self {
            block_idx,
            line_in_block,
            col,
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct VisualDocument {
    pub blocks: VisualBlocks,
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
                blocks: VisualBlocks(vec![VisualBlock::Paragraph {
                    source_line: SourceLine::new(0),
                }]),
            };
        }

        let mut blocks = VisualBlocks::default();
        let mut idx = 0;
        let separators = compute_syntactic_separators(lines);

        while idx < parsed_lines.len() {
            let parsed = &parsed_lines[idx];

            match &parsed.kind {
                BlockKind::CodeBlock { is_fence_start } => {
                    if *is_fence_start {
                        let start_line = idx;
                        let mut code_line_count = ProjectedLine::new(0);
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
                            start_line: SourceLine::new(start_line),
                            end_line: SourceLine::new(end_line),
                        });
                    } else {
                        idx += 1;
                    }
                }
                BlockKind::CodeBlockContent => {
                    idx += 1;
                }
                BlockKind::MathBlock { .. } | BlockKind::MathBlockContent => {
                    blocks.push(VisualBlock::Paragraph {
                        source_line: SourceLine::new(idx),
                    });
                    idx += 1;
                }
                BlockKind::Table { is_header, .. } => {
                    if *is_header {
                        let start_line = idx;
                        let mut row_count = ProjectedLine::new(0);
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
                            start_line: SourceLine::new(start_line),
                            end_line: SourceLine::new(end_line),
                        });
                    } else {
                        idx += 1;
                    }
                }
                BlockKind::Heading { .. } => {
                    blocks.push(VisualBlock::Heading {
                        source_line: SourceLine::new(idx),
                    });
                    idx += 1;
                }
                BlockKind::BulletList
                | BlockKind::OrderedList { .. }
                | BlockKind::TaskList { .. } => {
                    blocks.push(VisualBlock::ListItem {
                        source_line: SourceLine::new(idx),
                    });
                    idx += 1;
                }
                BlockKind::BlockQuote => {
                    blocks.push(VisualBlock::BlockQuote {
                        source_line: SourceLine::new(idx),
                    });
                    idx += 1;
                }
                BlockKind::ThematicBreak => {
                    blocks.push(VisualBlock::ThematicBreak {
                        source_line: SourceLine::new(idx),
                    });
                    idx += 1;
                }
                BlockKind::Image { .. } => {
                    blocks.push(VisualBlock::Image {
                        source_line: SourceLine::new(idx),
                    });
                    idx += 1;
                }
                BlockKind::Paragraph => {
                    if separators.get(idx).copied().unwrap_or(false) {
                        idx += 1;
                        continue;
                    }
                    blocks.push(VisualBlock::Paragraph {
                        source_line: SourceLine::new(idx),
                    });
                    idx += 1;
                }
            }
        }

        if blocks.is_empty() {
            blocks.push(VisualBlock::Paragraph {
                source_line: SourceLine::new(0),
            });
        }

        Self { blocks }
    }

    pub fn source_to_visual(&self, pos: SourcePosition) -> VisualPosition {
        if self.blocks.is_empty() {
            return VisualPosition::new(ProjectedBlockIndex::new(0), ProjectedLine::new(0), 0);
        }

        for (b_idx, block) in self.blocks.iter().enumerate() {
            let range = block.source_range();
            if range.contains(&pos.line) {
                match block {
                    VisualBlock::CodeBlock { start_line, .. } => {
                        let line_in_block = pos.line - *start_line;
                        return VisualPosition::new(
                            ProjectedBlockIndex::new(b_idx),
                            ProjectedLine::new(line_in_block),
                            pos.col,
                        );
                    }
                    VisualBlock::Table { start_line, .. } => {
                        let line_in_block = if pos.line <= *start_line + 1 {
                            0
                        } else {
                            pos.line - *start_line - 1
                        };
                        return VisualPosition::new(
                            ProjectedBlockIndex::new(b_idx),
                            ProjectedLine::new(line_in_block),
                            pos.col,
                        );
                    }
                    _ => {
                        return VisualPosition::new(
                            ProjectedBlockIndex::new(b_idx),
                            ProjectedLine::new(0),
                            pos.col,
                        );
                    }
                }
            } else if pos.line < range.start {
                return VisualPosition::new(
                    ProjectedBlockIndex::new(b_idx.saturating_sub(1)),
                    ProjectedLine::new(0),
                    pos.col,
                );
            }
        }

        let last_idx = self.blocks.len() - 1;
        VisualPosition::new(
            ProjectedBlockIndex::new(last_idx),
            ProjectedLine::new(0),
            pos.col,
        )
    }

    pub fn block_line_count(&self, b_idx: ProjectedBlockIndex) -> ProjectedLine {
        if b_idx.index() >= self.blocks.len() {
            return ProjectedLine::new(0);
        }
        match &self.blocks[b_idx] {
            VisualBlock::CodeBlock {
                code_line_count, ..
            } => ProjectedLine::new(1) + *code_line_count,
            VisualBlock::Table { row_count, .. } => ProjectedLine::new(1) + *row_count,
            _ => ProjectedLine::new(1),
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
        } else if (vpos.block_idx + 1).index() < self.blocks.len() {
            Some(VisualPosition::new(
                vpos.block_idx + 1,
                ProjectedLine::new(0),
                vpos.col,
            ))
        } else {
            None
        }
    }

    pub fn prev_visual_line(&self, vpos: VisualPosition) -> Option<VisualPosition> {
        if self.blocks.is_empty() {
            return None;
        }
        if vpos.line_in_block > ProjectedLine::new(0) {
            Some(VisualPosition::new(
                vpos.block_idx,
                vpos.line_in_block - 1,
                vpos.col,
            ))
        } else if vpos.block_idx.index() > 0 {
            let prev_idx = vpos.block_idx - 1;
            let prev_count = self.block_line_count(prev_idx);
            let prev_line = prev_count.saturating_sub(1);
            Some(VisualPosition::new(prev_idx, prev_line, vpos.col))
        } else {
            None
        }
    }

    pub fn visual_to_source(&self, vpos: VisualPosition) -> SourcePosition {
        if self.blocks.is_empty() {
            return SourcePosition::new(SourceLine::new(0), 0);
        }

        let b_idx = ProjectedBlockIndex::new(vpos.block_idx.index().min(self.blocks.len() - 1));
        let line_in_block = vpos.line_in_block;
        match &self.blocks[b_idx] {
            VisualBlock::Heading { source_line, .. }
            | VisualBlock::Paragraph { source_line, .. }
            | VisualBlock::ListItem { source_line, .. }
            | VisualBlock::BlockQuote { source_line, .. }
            | VisualBlock::ThematicBreak { source_line }
            | VisualBlock::Image { source_line, .. } => SourcePosition::new(*source_line, vpos.col),
            VisualBlock::CodeBlock {
                start_line,
                end_line,
                ..
            } => {
                let target_line = (*start_line + line_in_block).min(*end_line);
                SourcePosition::new(target_line, vpos.col)
            }
            VisualBlock::Table {
                start_line,
                end_line,
                ..
            } => {
                let target_line = if line_in_block == ProjectedLine::new(0) {
                    *start_line
                } else {
                    (*start_line + 1 + line_in_block).min(*end_line)
                };
                SourcePosition::new(target_line, vpos.col)
            }
        }
    }
}
