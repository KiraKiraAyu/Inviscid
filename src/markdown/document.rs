    },
    Paragraph {
        source_line: SourceLine,
    },
    CodeBlock {
        code_line_count: usize,
        start_line: SourceLine,
        end_line: SourceLine,
    },
    MathBlock {
        start_line: SourceLine,
        end_line: SourceLine,
        is_active: bool,
    },
    Table {
        row_count: usize,
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
    fn source_range(&self) -> std::ops::Range<SourceLine> {
        match self {
            VisualBlock::Heading { source_line }
            | VisualBlock::Paragraph { source_line }
            | VisualBlock::ListItem { source_line }
            | VisualBlock::MathBlock {
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
pub struct ProjectedPosition {
    pub block_idx: usize,
    pub line_in_block: usize,
    pub col: usize,
}

impl ProjectedPosition {
    pub fn new(block_idx: usize, line_in_block: usize, col: usize) -> Self {
        Self {
            block_idx,
            line_in_block,
            col,
            #[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct EditorProjection {
    blocks: Vec<VisualBlock>,
    mode: ProjectionMode,
    caret_map: Arc<CaretMap>,
}

impl EditorProjection {
    #[cfg(test)]
    pub fn from_lines(lines: &[String]) -> Self {
        let parsed = super::scanner::MarkdownScanner::scan_document(lines);
        Self::from_parsed(lines, &parsed)
    }
    blocks: vec![VisualBlock::Paragraph {
                    source_line: SourceLine::new(0),
                }],
                mode,
                caret_map,
            };
        }

        if mode.is_source() {
            return Self {
                blocks: (0..lines.len())
                    .map(|source_line| VisualBlock::Paragraph {
                        source_line: SourceLine::new(source_line),
                    })
                    .collect(),
                mode,
                caret_map,
            };
        }

        let mut blocks = Vec::new();
        let mut idx = 0;
        let separators = caret_map.syntactic_separators();

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
                BlockKind::CodeBlockContent => {
                    
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
                        
        }
        Self {
            blocks,
            mode,
            caret_map,
        }
    }

    pub fn blocks(&self) -> &[VisualBlock] {
        &self.blocks
    }

    pub fn source_line_count(&self) -> usize {
        self.caret_map.line_count()
    }

    pub fn is_caret_line(&self, line: SourceLine) -> bool {
        self.mode.is_source() || self.caret_map.is_caret_line(line)
    }

    pub fn math_range(&self, line: SourceLine) -> Option<(SourceLine, SourceLine)> {
        self.caret_map.math_pair(line)
    }

    pub fn hit_test_target(&self, source_line: SourceLine) -> Option<CaretTarget> {
        let line = source_line.index();
        if line >= self.caret_map.caret_lines.len() {
            return None;
        }
        if self.mode.is_source() {
            return Some(CaretTarget::Position(SourcePosition::new(source_line, 0)));
        }
        if let Some((start, end)) = self.caret_map.math_pair(source_line)
            && end.index() == start.index() + 1
        {
            return Some(CaretTarget::CreateMathContent { start_line: start });
        }

        Some(CaretTarget::Position(SourcePosition::from_position(
            self.normalize_position(Position::new(line, 0)),
        )))
    }

    pub fn hit_test_anchor_line(&self, source_line: SourceLine) -> Option<SourceLine> {
        let line = source_line.index();
        if self.mode.is_source() {
            return (line < self.caret_map.caret_lines.len()).then_some(source_line);
        }
        if line >= self.caret_map.caret_lines.len() {
            return None;
        }
        if let Some((start, end)) = self.caret_map.math_pair(source_line)
            && self.mode != ProjectionMode::live_preview(Some((start, end)))
        {
            return Some(start);
        }
        match self.hit_test_target(source_line)? {
            CaretTarget::Position(position) => Some(position.line),
            CaretTarget::CreateMathContent { start_line } => Some(start_line),
        }
    }

    pub fn normalize_position(&self, pos: Position) -> Position {
        if self.mode.is_source() {
            return self.caret_map.normalize_source_position(pos);
        }
        self.caret_map.normalize_position(pos)
    }

    pub fn adjacent_caret_line<F>(
        &self,
        pos: Position,
        is_down: bool,
        mut is_allowed: F,
    ) -> Option<SourceLine>
    where
        F: FnMut(SourceLine) -> bool,
    {
        let mut candidate = self.source_to_projected(pos);
        loop {
            candidate = if is_down {
                self.next_projected_line(candidate)?
            } else {
                self.prev_projected_line(candidate)?
            };
            let source_line = self.navigation_source_line(candidate, is_down);
            if self.is_caret_line(source_line) && is_allowed(source_line) {
                return Some(source_line);
            }
        }
    }

    fn navigation_source_line(&self, pos: ProjectedPosition, is_down: bool) -> SourceLine {
        if let Some(VisualBlock::MathBlock {
            start_line,
            end_line,
            is_active: false,
        }) = self.blocks.get(pos.block_idx)
            && start_line.index() + 1 < end_line.index()
        {
            return if is_down {
                SourceLine::new(start_line.index() + 1)
            } else {
                SourceLine::new(end_line.index() - 1)
            };
        }
        self.projected_to_source(pos).source_line()
    }

    pub fn source_to_projected(&self, pos: Position) -> ProjectedPosition {
        if self.blocks.is_empty() {
            return ProjectedPosition::new(0, 0, 0);
        }

        for (b_idx, block) in self.blocks.iter().enumerate() {
            let range = block.source_range();
            let source_line = pos.source_line();
            if source_line >= range.start && source_line < range.end {
                match block {
                    VisualBlock::CodeBlock { start_line, .. } => {
                        let line_in_block = pos.line - start_line.index();
                        return ProjectedPosition::new(b_idx, line_in_block, pos.col);
                    }
                    VisualBlock::Table { start_line, .. } => {
                        let start_line = start_line.index();
                        let line_in_block = if pos.line <= start_line + 1 {
                            0
                        } else {
                            pos.line - start_line - 1
                        };
                        return ProjectedPosition::new(b_idx, line_in_block, pos.col);
                    }
                    VisualBlock::MathBlock {
                        start_line,
                        end_line,
                        is_active,
                    } => {
                        let (start_line, end_line) = (start_line.index(), end_line.index());
                        let line_in_block = if *is_active && pos.line > start_line {
                            (pos.line - start_line - 1).min(end_line.saturating_sub(start_line + 2))
                        } else {
                            0
                        };
                        return ProjectedPosition::new(b_idx, line_in_block, pos.col);
                    }
                    _ => {
                        return ProjectedPosition::new(b_idx, 0, pos.col);
                    }
                }
            } else if source_line < range.start {
                return ProjectedPosition::new(b_idx.saturating_sub(1), 0, pos.col);
            }
        }

        let last_idx = self.blocks.len() - 1;
        ProjectedPosition::new(last_idx, 0, pos.col)
    }

    fn block_line_count(&self, b_idx: usize) -> usize {
        if b_idx >= self.blocks.len() {
            return 0;
        }
        match &self.blocks[b_idx] {
        fn next_projected_line(&self, vpos: ProjectedPosition) -> Option<ProjectedPosition> {
        if self.blocks.is_empty() {
            return None;
        }
        let b_count = self.block_line_count(vpos.block_idx);
        if vpos.line_in_block + 1 < b_count {
            Some(ProjectedPosition::new(
                vpos.block_idx,
                vpos.line_in_block + 1,
                vpos.col,
            ))
        } else if vpos.block_idx + 1 < self.blocks.len() {
            Some(ProjectedPosition::new(vpos.block_idx + 1, 0, vpos.col))
        } else {
            None
        }
    }

    fn prev_projected_line(&self, vpos: ProjectedPosition) -> Option<ProjectedPosition> {
        if self.blocks.is_empty() {
            return None;
        }
        if vpos.line_in_block > 0 {
            Some(ProjectedPosition::new(
                vpos.block_idx,
                vpos.line_in_block - 1,
                vpos.col,
            ))
            pub fn projected_to_source(&self, vpos: ProjectedPosition) -> Position {
        if self.blocks.is_empty() {
            return Position::new(0, 0);
        }

        let b_idx = vpos.block_idx.min(self.blocks.len() - 1);
        match &self.blocks[b_idx] {
            VisualBlock::Heading { source_line, .. }
            let target_line = (start_line.index() + vpos.line_in_block).min(end_line.index());
                Position::new(target_line, vpos.col)
            }
            VisualBlock::Table {
                start_line,
                (start_line.index() + 1 + vpos.line_in_block).min(end_line.index())
                };
                Position::new(target_line, vpos.col)
            }
            VisualBlock::MathBlock {
                start_line,
                end_line,
                is_active,
            } => {
                if *is_active {
                    let target_line =
                        (start_line.index() + 1 + vpos.line_in_block).min(end_line.index() - 1);
                    Position::new(target_line, vpos.col)
                } else if start_line.index() + 1 < end_line.index() {
                    Position::new(start_line.index() + 1, vpos.col)
                } else {
                    Position::new(start_line.index(), vpos.col)
                }
            }
        }
    }
}
