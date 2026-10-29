}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
struct FoldMap {
    regions: Vec<FoldRegion>,
    version: usize,
}

impl FoldMap {
    #[inline]
    fn version(&self) -> usize {
        self.version
    }

    /// Folds lines `start_line..=end_line`, keeping `start_line` visible as the header.
    fn fold(&mut self, start_line: SourceLine, end_line: SourceLine) {
        if start_line >= end_line {
            return;
        }
        self.regions
            .retain(|r| r.end_line < start_line || r.start_line > end_line);
        self.regions.push(FoldRegion {
            start_line,
            end_line,
        });
        self.regions.sort_by_key(|r| r.start_line);
        
        if changed {
            self.version = self.version.wrapping_add(1);
        }
        changed
        
        }
    }

    
    }

    fn folded_regions(&self) -> &[FoldRegion] {
        &self.regions
    }
}

#[derive(Clone, Debug, Default)]
pub struct DisplayMap {
    fold_map: FoldMap,
}

impl DisplayMap {
    pub fn new() -> Self {
        Self::default()
    }

    #[inline]
    pub fn unfold(&mut self, start_line: SourceLine) -> bool {
        self.fold_map.unfold(start_line)
    }

    pub fn unfold_all(&mut self) {
        self.fold_map.unfold_all();
    }

    pub fn is_line_folded(&self, line: SourceLine) -> bool {
        self.fold_map.is_line_folded(line)
    }

    pub fn is_fold_header(&self, line: SourceLine) -> bool {
        self.fold_map.is_fold_header(line)
    }

    pub fn normalize_position(
        &self,
        projection: &EditorProjection,
        pos: Position,
    ) -> Option<Position> {
        let source_line = pos.source_line();
        let line = self
            .fold_header_for_line(source_line)
            .unwrap_or(source_line)
            .index();
        let normalized = projection.normalize_position(Position::new(line, pos.col));
        if self.is_visible_caret_line(projection, normalized.source_line()) {
            return Some(normalized);
        }

        self.nearest_visible_caret_line(projection, normalized.source_line())
            .map(|target_line| {
                projection.normalize_position(Position::new(target_line.index(), pos.col))
            })
    }

    pub fn resolve_hit_target(
        &self,
        projection: &EditorProjection,
        source_line: SourceLine,
    ) -> Option<CaretTarget> {
        let source_line = self
            .fold_header_for_line(source_line)
            .unwrap_or(source_line);
        let target = projection.hit_test_target(source_line)?;
        if self.is_visible_hit_target(projection, target) {
            return Some(target);
        }

        self.nearest_visible_caret_line(projection, self.target_source_line(target))
            .map(|line| CaretTarget::Position(SourcePosition::new(line, 0)))
    }

    pub fn hit_test_anchor_line(
        &self,
        projection: &EditorProjection,
        source_line: SourceLine,
    ) -> Option<SourceLine> {
        let source_line = self
            .fold_header_for_line(source_line)
            .unwrap_or(source_line);
        let anchor_line = projection.hit_test_anchor_line(source_line)?;
        Some(
            self.fold_header_for_line(anchor_line)
                .unwrap_or(anchor_line),
        )
    }

    pub fn adjacent_caret_line(
        &self,
        projection: &EditorProjection,
        pos: Position,
        is_down: bool,
    ) -> Option<SourceLine> {
        let pos = self.normalize_position(projection, pos)?;
        projection.adjacent_caret_line(pos, is_down, |line| !self.fold_map.is_line_folded(line))
    }

    pub fn page_caret_line(
        &self,
        projection: &EditorProjection,
        pos: Position,
        is_down: bool,
        visual_rows: usize,
    ) -> Option<SourceLine> {
        let mut line = self.normalize_position(projection, pos)?.source_line();
        for _ in 0..visual_rows {
            let Some(next) =
                self.adjacent_caret_line(projection, Position::new(line.index(), pos.col), is_down)
            else {
                break;
            };
            line = next;
        }
        Some(line)
    }

    pub fn fold_header_for_line(&self, line: SourceLine) -> Option<SourceLine> {
        self.fold_map
            .folded_regions()
            .iter()
            .find(|region| line > region.start_line && line <= region.end_line)
            .map(|region| region.start_line)
    }

    fn is_visible_caret_line(&self, projection: &EditorProjection, line: SourceLine) -> bool {
        projection.is_caret_line(line) && !self.fold_map.is_line_folded(line)
    }

    fn is_visible_hit_target(&self, projection: &EditorProjection, target: CaretTarget) -> bool {
        match target {
            CaretTarget::Position(position) => {
                self.is_visible_caret_line(projection, position.line)
            }
            CaretTarget::CreateMathContent { start_line } => projection
                .math_range(start_line)
                .is_some_and(|(start, end)| {
                    start < end
                        && !self.fold_map.is_line_folded(start)
                        && !self
                            .fold_map
                            .is_line_folded(SourceLine::new(start.index() + 1))
                }),
        }
    }

    fn nearest_visible_caret_line(
        &self,
        projection: &EditorProjection,
        from_line: SourceLine,
    ) -> Option<SourceLine> {
        let line_count = projection.source_line_count();
        if line_count == 0 {
            return None;
        }

        let from_line = from_line.index().min(line_count - 1);
        for distance in 0..line_count {
            if let Some(line) = from_line.checked_sub(distance)
                && self.is_visible_caret_line(projection, SourceLine::new(line))
            {
                return Some(SourceLine::new(line));
            }
            if distance > 0
                && let Some(line) = from_line.checked_add(distance)
                && line < line_count
                && self.is_visible_caret_line(projection, SourceLine::new(line))
            {
                return Some(SourceLine::new(line));
            }
        }

        None
    }

    fn target_source_line(&self, target: CaretTarget) -> SourceLine {
        match target {
            CaretTarget::Position(position) => position.line,
            CaretTarget::CreateMathContent { start_line } => start_line,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::DisplayMap;
    use crate::buffer::{Position, SourceLine, SourcePosition};
    use crate::markdown::{CaretTarget, EditorProjection, MarkdownScanner, ProjectionMode};

    #[test]
    fn display_map_is_the_single_entry_point_for_fold_and_markdown_caret_mapping() {
        let lines = [
            "Before",
            "$$",
            "x + y",
            "$$",
            "Fold header",
            "Folded text",
            "After",
        ]
        .map(str::to_string);
        let parsed = MarkdownScanner::scan_document(&lines);
        let projection = EditorProjection::from_parsed_with_mode(
            &lines,
            &parsed,
            ProjectionMode::live_preview(None),
        );
        let mut display_map = DisplayMap::new();
        display_map.fold(SourceLine::new(4), SourceLine::new(5));

        assert_eq!(
            display_map.resolve_hit_target(&projection, SourceLine::new(1)),
            Some(CaretTarget::Position(SourcePosition::new(SourceLine::new(2), 0)))
        );
        assert_eq!(
            display_map.resolve_hit_target(&projection, SourceLine::new(5)),
            Some(CaretTarget::Position(SourcePosition::new(SourceLine::new(4), 0)))
        );
        assert_eq!(
            display_map.adjacent_caret_line(&projection, Position::new(2, 0), true),
            Some(SourceLine::new(4))
        );
        assert_eq!(
            display_map.adjacent_caret_line(&projection, Position::new(4, 0), true),
            Some(SourceLine::new(6))
        );
        assert_eq!(
            display_map.page_caret_line(&projection, Position::new(0, 0), true, 3),
            Some(SourceLine::new(6))
        );
    }

    #[test]
    fn every_live_preview_hit_and_vertical_step_resolves_to_a_visible_caret() {
        let lines = [
            "# Heading",
            "",
            "Paragraph",
            "```rust",
            "fn main() {}",
            "```",
            "$$",
            "x +",
            "y",
            "$$",
            "| A | B |",
            "|---|---|",
            "| 1 | 2 |",
            "Fold header",
            "Folded paragraph",
            "After",
        ]
        .map(str::to_string);
        let parsed = MarkdownScanner::scan_document(&lines);
        let projection = EditorProjection::from_parsed_with_mode(
            &lines,
            &parsed,
            ProjectionMode::live_preview(None),
        );
        let mut display_map = DisplayMap::new();
        display_map.fold(SourceLine::new(13), SourceLine::new(14));

        for source_line in 0..lines.len() {
            match display_map.resolve_hit_target(&projection, SourceLine::new(source_line)) {
                Some(CaretTarget::Position(position)) => {
                    assert!(projection.is_caret_line(position.line));
                    assert!(!display_map.is_line_folded(position.line));
                }
                Some(CaretTarget::CreateMathContent { start_line }) => {
                    assert_eq!(
                        projection.math_range(start_line),
                        Some((start_line, SourceLine::new(start_line.index() + 1)))
                    );
                }
                None => panic!("source line {source_line} must resolve to a caret target"),
            }
        }

        for source_line in 0..lines.len() {
            if !projection.is_caret_line(SourceLine::new(source_line))
                || display_map.is_line_folded(SourceLine::new(source_line))
            {
                continue;
            }

            for is_down in [false, true] {
                if let Some(target) = display_map.adjacent_caret_line(
                    &projection,
                    Position::new(source_line, 0),
                    is_down,
                ) {
                    assert_ne!(target.index(), source_line);
                    assert_eq!(target.index() > source_line, is_down);
                    assert!(projection.is_caret_line(target));
                    assert!(!display_map.is_line_folded(target));
                }
            }
        }
    }

    #[test]
    fn folded_markdown_projection_never_resolves_back_into_the_fold() {
        let lines = ["Before", "$$", "x + y", "$$", "After"].map(str::to_string);
        let parsed = MarkdownScanner::scan_document(&lines);
        let projection = EditorProjection::from_parsed_with_mode(
            &lines,
            &parsed,
            ProjectionMode::live_preview(None),
        );
        let mut display_map = DisplayMap::new();
        display_map.fold(SourceLine::new(1), SourceLine::new(3));

        assert_eq!(
            display_map.resolve_hit_target(&projection, SourceLine::new(1)),
            Some(CaretTarget::Position(SourcePosition::new(SourceLine::new(0), 0)))
        );
        assert_eq!(
            display_map.hit_test_anchor_line(&projection, SourceLine::new(1)),
            Some(SourceLine::new(1))
        );
        assert_eq!(
            display_map.normalize_position(&projection, Position::new(2, 0)),
            Some(Position::new(0, 0))
        );
    }

    #[test]
    fn mapping_returns_no_target_when_the_document_has_no_editable_line() {
        let lines = ["|---|"].map(str::to_string);
        let parsed = MarkdownScanner::scan_document(&lines);
        let projection = EditorProjection::from_parsed_with_mode(
            &lines,
            &parsed,
            ProjectionMode::live_preview(None),
        );
        let display_map = DisplayMap::new();

        assert_eq!(
            display_map.resolve_hit_target(&projection, SourceLine::new(0)),
            None
        );
        assert_eq!(
            display_map.normalize_position(&projection, Position::new(0, 0)),
            None
        );
        assert_eq!(
            display_map.adjacent_caret_line(&projection, Position::new(0, 0), true),
            None
        );
        assert_eq!(
            display_map.page_caret_line(&projection, Position::new(0, 0), true, 15),
            None
        );
    }

    #[test]
    fn fold_queries_use_source_line_types_without_exposing_fold_map() {
        let mut display_map = DisplayMap::new();
        display_map.fold(SourceLine::new(2), SourceLine::new(4));

        assert!(display_map.is_line_folded(SourceLine::new(3)));
        assert!(display_map.is_fold_header(SourceLine::new(2)));
        assert_eq!(
            display_map.fold_header_for_line(SourceLine::new(3)),
            Some(SourceLine::new(2))
        );
    }
}
