use super::*;

/// Geometry of source rows, including zero-height collapsed rows.
#[derive(Clone, Debug, Default, PartialEq)]
pub(super) struct VecMetrics {
    pub(super) heights: Vec<Pixels>,
    pub(super) offsets: Vec<Pixels>,
}

impl VecMetrics {
    pub fn from_heights(heights: Vec<Pixels>) -> Self {
        let heights = heights
            .into_iter()
            .map(|h| Fixed::from_pixels(h).pixels())
            .collect::<Vec<_>>();
        let mut metrics = Self {
            offsets: vec![px(0.0); heights.len()],
            heights,
        };
        metrics.rebuild_from(0);
        metrics
    }

    pub fn set_height(&mut self, line: usize, height: Pixels) {
        self.heights[line] = Fixed::from_pixels(height).pixels();
    }

    /// Returns whether geometry changed. Rebuild offsets after all height edits.
    pub fn splice(&mut self, range: Range<usize>, heights: Vec<Pixels>) -> bool {
        let heights = heights
            .into_iter()
            .map(|h| Fixed::from_pixels(h).pixels())
            .collect::<Vec<_>>();
        let changed = self.heights[range.clone()] != heights;
        if changed {
            self.offsets
                .splice(range.clone(), std::iter::repeat_n(px(0.0), heights.len()));
        }
        self.heights.splice(range, heights);
        changed
    }

    pub fn rebuild_from(&mut self, start: usize) {
        let _ = start;
        let mut y = Fixed::from_pixels(DOCUMENT_TOP_PADDING);
        for line in 0..self.len() {
            self.offsets[line] = y.pixels();
            y.0 += Fixed::from_pixels(self.heights[line]).0;
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
        let height = self
            .heights
            .iter()
            .map(|&h| Fixed::from_pixels(h).0)
            .sum::<i64>();
        Fixed(height + Fixed::from_pixels(EMPTY_DOCUMENT_HEIGHT).0).pixels()
    }
}
