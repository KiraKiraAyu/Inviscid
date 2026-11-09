use gpui::{Pixels, px};
use std::ops::Range;

pub const DOCUMENT_TOP_PADDING: Pixels = px(24.0);
pub const DOCUMENT_BOTTOM_PADDING: Pixels = px(24.0);
pub const EMPTY_DOCUMENT_HEIGHT: Pixels = px(48.0);

/// Geometry of source rows, including zero-height collapsed rows.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct LineMetrics {
    pub(super) heights: Vec<Pixels>,
    pub(super) offsets: Vec<Pixels>,
}

impl LineMetrics {
    pub fn from_heights(heights: Vec<Pixels>) -> Self {
        let mut metrics = Self {
            offsets: vec![px(0.0); heights.len()],
            heights,
        };
        metrics.rebuild_from(0);
        metrics
    }

    pub fn set_height(&mut self, line: usize, height: Pixels) {
        self.heights[line] = height;
    }

    /// Returns whether geometry changed. Rebuild offsets after all height edits.
    pub fn splice(&mut self, range: Range<usize>, heights: Vec<Pixels>) -> bool {
        let changed = self.heights[range.clone()] != heights;
        if changed {
            self.offsets
                .splice(range.clone(), std::iter::repeat_n(px(0.0), heights.len()));
        }
        self.heights.splice(range, heights);
        changed
    }

    pub fn rebuild_from(&mut self, start: usize) {
        let mut y = if start == 0 {
            DOCUMENT_TOP_PADDING
        } else {
            self.offsets[start - 1] + self.heights[start - 1]
        };
        for line in start..self.len() {
            self.offsets[line] = y;
            y += self.heights[line];
        }
    }

    pub fn len(&self) -> usize {
        self.heights.len()
    }

    pub fn is_empty(&self) -> bool {
        self.heights.is_empty()
    }

    pub fn top(&self, line: usize) -> Option<Pixels> {
        self.offsets.get(line).copied()
    }

    pub fn height(&self, line: usize) -> Option<Pixels> {
        self.heights.get(line).copied()
    }

    pub fn y_range(&self, line: usize) -> Option<(Pixels, Pixels)> {
        let top = self.top(line)?;
        Some((top, top + self.height(line)?))
    }

    pub fn line_at_y(&self, y: Pixels) -> Option<usize> {
        if self.is_empty() {
            return None;
        }
        Some(
            self.offsets
                .partition_point(|&top| top <= y)
                .saturating_sub(1)
                .min(self.len() - 1),
        )
    }

    pub fn visible_lines(&self, top: Pixels, height: Pixels) -> Range<usize> {
        if height <= px(0.0) || self.is_empty() {
            return 0..0;
        }
        let end = self.offsets.partition_point(|&y| y < top + height);
        let mut start = self.line_at_y(top).unwrap();
        if self.offsets[start] + self.heights[start] <= top {
            start += 1;
        }
        start.min(end)..end
    }

    pub fn next_visible_line_after(&self, idx: usize) -> Option<usize> {
        (idx.saturating_add(1)..self.len()).find(|&line| self.heights[line] > px(0.0))
    }

    pub fn total_height(&self) -> Pixels {
        match self.heights.last().zip(self.offsets.last()) {
            Some((height, top)) => *top + *height + DOCUMENT_BOTTOM_PADDING,
            None => EMPTY_DOCUMENT_HEIGHT,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_and_collapsed_line_boundaries_match_offsets() {
        let empty = LineMetrics::from_heights(Vec::new());
        assert_eq!(empty.len(), 0);
        assert_eq!(empty.total_height(), px(48.0));
        assert_eq!(empty.line_at_y(px(0.0)), None);
        assert_eq!(empty.y_range(0), None);
        assert_eq!(empty.visible_lines(px(0.0), px(700.0)), 0..0);
        let metrics =
            LineMetrics::from_heights(vec![px(0.0), px(32.0), px(0.0), px(24.0), px(0.0)]);
        let offsets = [24.0, 24.0, 56.0, 56.0, 80.0].map(px);
        assert_eq!(metrics.total_height(), px(104.0));
        assert_eq!(metrics.next_visible_line_after(0), Some(1));
        assert_eq!(metrics.next_visible_line_after(1), Some(3));
        assert_eq!(metrics.next_visible_line_after(3), None);
        for y in -20..120 {
            let y = px(y as f32);
            assert_eq!(
                metrics.line_at_y(y),
                Some(offsets.partition_point(|&top| top <= y).saturating_sub(1))
            );
        }
        assert_eq!(metrics.top(2), Some(px(56.0)));
        assert_eq!(metrics.height(2), Some(px(0.0)));
        assert_eq!(metrics.y_range(3), Some((px(56.0), px(80.0))));
    }

    #[test]
    fn viewport_intersections_include_tall_rows_and_skip_collapsed_rows() {
        let metrics =
            LineMetrics::from_heights([0.0, 32.0, 0.0, 1000.0, 0.0, 24.0, 0.0].map(px).to_vec());
        for top in (-20..1120).step_by(17) {
            for height in [0.0, 1.0, 23.0, 100.0, 999.0] {
                let top = px(top as f32);
                let height = px(height);
                let expected: Vec<_> = (0..metrics.len())
                    .filter(|&line| {
                        let (start, end) = metrics.y_range(line).unwrap();
                        height > px(0.0) && end > start && start < top + height && end > top
                    })
                    .collect();
                let actual: Vec<_> = metrics
                    .visible_lines(top, height)
                    .filter(|&line| metrics.height(line).unwrap() > px(0.0))
                    .collect();
                assert_eq!(actual, expected, "viewport {top:?} + {height:?}");
            }
        }
    }

    #[test]
    fn height_updates_rebuild_only_the_requested_suffix() {
        let mut metrics = LineMetrics::from_heights(vec![px(10.0), px(20.0), px(30.0)]);
        metrics.set_height(1, px(0.0));
        metrics.rebuild_from(1);
        assert_eq!(
            metrics,
            LineMetrics::from_heights(vec![px(10.0), px(0.0), px(30.0)])
        );
        metrics.set_height(0, px(40.0));
        metrics.rebuild_from(0);
        assert_eq!(metrics.top(2), Some(px(64.0)));
        assert_eq!(metrics.total_height(), px(118.0));
        let mut empty = LineMetrics::from_heights(Vec::new());
        empty.rebuild_from(0);
        assert_eq!(empty.total_height(), px(48.0));
    }

    #[test]
    fn splices_preserve_geometry_across_insert_delete_and_replace() {
        let mut metrics = LineMetrics::from_heights(vec![px(10.0), px(20.0), px(30.0)]);
        for (range, replacement, expected) in [
            (
                1..2,
                vec![px(0.0), px(5.0)],
                vec![px(10.0), px(0.0), px(5.0), px(30.0)],
            ),
            (0..2, vec![], vec![px(5.0), px(30.0)]),
            (2..2, vec![px(40.0)], vec![px(5.0), px(30.0), px(40.0)]),
            (0..3, vec![], vec![]),
            (0..0, vec![px(7.0)], vec![px(7.0)]),
        ] {
            let start = range.start;
            assert!(metrics.splice(range, replacement));
            metrics.rebuild_from(start);
            assert_eq!(metrics, LineMetrics::from_heights(expected));
        }
        assert!(!metrics.splice(0..1, vec![px(7.0)]));
    }
}
