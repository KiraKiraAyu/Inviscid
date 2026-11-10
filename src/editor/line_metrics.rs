use gpui::{Pixels, px};
use gpui_sum_tree::{Bias, ContextLessSummary, Item, SeekTarget, SumTree};
use std::{cmp::Ordering, ops::Range};

pub const DOCUMENT_TOP_PADDING: Pixels = px(24.0);
pub const DOCUMENT_BOTTOM_PADDING: Pixels = px(24.0);
pub const EMPTY_DOCUMENT_HEIGHT: Pixels = px(48.0);

#[cfg(test)]
thread_local! {
    pub(super) static SUMMARY_WORK: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct Fixed(i64);

impl Fixed {
    fn from_pixels(height: Pixels) -> Self {
        let height = f32::from(height);
        assert!(height.is_finite() && height >= 0.0, "invalid line height");
        Self((f64::from(height) * 1024.0).round() as i64)
    }

    fn pixels(self) -> Pixels {
        px((self.0 as f64 / 1024.0) as f32)
    }

    fn document_y(self) -> Pixels {
        Fixed(self.0 + Fixed::from_pixels(DOCUMENT_TOP_PADDING).0).pixels()
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct LineItem {
    count: usize,
    height: Fixed,
    measured: bool,
}

#[derive(Clone, Debug, Default)]
struct LineSummary {
    lines: usize,
    height: Fixed,
    visible_lines: usize,
    unmeasured: usize,
}

impl Item for LineItem {
    type Summary = LineSummary;

    fn summary(&self, (): ()) -> LineSummary {
        #[cfg(test)]
        SUMMARY_WORK.update(|work| work + 1);
        LineSummary {
            lines: self.count,
            height: Fixed(self.height.0 * self.count as i64),
            visible_lines: if self.height.0 > 0 { self.count } else { 0 },
            unmeasured: if self.measured { 0 } else { self.count },
        }
    }
}

impl ContextLessSummary for LineSummary {
    fn zero() -> Self {
        Self::default()
    }

    fn add_summary(&mut self, other: &Self) {
        #[cfg(test)]
        SUMMARY_WORK.update(|work| work + 1);
        self.lines += other.lines;
        self.height.0 += other.height.0;
        self.visible_lines += other.visible_lines;
        self.unmeasured += other.unmeasured;
    }
}

struct Lines(usize);
struct Height(Pixels);
struct PreciseHeight(f64);
struct VisibleLines(usize);
struct Unmeasured(usize);

impl<'a> SeekTarget<'a, LineSummary, LineSummary> for PreciseHeight {
    fn cmp(&self, location: &LineSummary, (): ()) -> Ordering {
        self.0
            .partial_cmp(&(24.0 + location.height.0 as f64 / 1024.0))
            .unwrap_or(Ordering::Less)
    }
}

impl<'a> SeekTarget<'a, LineSummary, LineSummary> for Unmeasured {
    fn cmp(&self, location: &LineSummary, (): ()) -> Ordering {
        Ord::cmp(&self.0, &location.unmeasured)
    }
}

impl<'a> SeekTarget<'a, LineSummary, LineSummary> for Lines {
    fn cmp(&self, location: &LineSummary, (): ()) -> Ordering {
        #[cfg(test)]
        SUMMARY_WORK.update(|work| work + 1);
        Ord::cmp(&self.0, &location.lines)
    }
}

impl<'a> SeekTarget<'a, LineSummary, LineSummary> for Height {
    fn cmp(&self, location: &LineSummary, (): ()) -> Ordering {
        #[cfg(test)]
        SUMMARY_WORK.update(|work| work + 1);
        // Compare the same f32 coordinates returned to GPUI, including at large y.
        self.0
            .partial_cmp(&location.height.document_y())
            .unwrap_or(Ordering::Less)
    }
}

impl<'a> SeekTarget<'a, LineSummary, LineSummary> for VisibleLines {
    fn cmp(&self, location: &LineSummary, (): ()) -> Ordering {
        #[cfg(test)]
        SUMMARY_WORK.update(|work| work + 1);
        Ord::cmp(&self.0, &location.visible_lines)
    }
}

/// Persistent geometry index. Clones share the tree; updates copy only tree paths.
#[derive(Clone, Debug)]
pub struct LineMetrics {
    tree: SumTree<LineItem>,
    estimate: Fixed,
}

impl Default for LineMetrics {
    fn default() -> Self {
        Self {
            tree: SumTree::new(()),
            estimate: Fixed::default(),
        }
    }
}

impl LineMetrics {
    pub(crate) fn top_precise(&self, line: usize) -> Option<f64> {
        if line >= self.len() {
            return None;
        }
        let mut cursor = self.tree.cursor::<LineSummary>(());
        cursor.seek(&Lines(line), Bias::Right);
        let height = cursor.start().height.0
            + (line - cursor.start().lines) as i64 * cursor.item()?.height.0;
        Some(24.0 + height as f64 / 1024.0)
    }

    pub(crate) fn total_height_precise(&self) -> f64 {
        48.0 + self.tree.summary().height.0 as f64 / 1024.0
    }

    pub(crate) fn top_relative(&self, line: usize, origin: Option<usize>) -> Option<Pixels> {
        match origin {
            Some(origin) => Some(px((self.top_precise(line)?
                - self.top_precise(origin.min(self.len().checked_sub(1)?))?)
                as f32)),
            None => self.top(line),
        }
    }

    pub(crate) fn line_at_precise_y(&self, y: f64) -> Option<usize> {
        if self.is_empty() {
            return None;
        }
        if y < 24.0 || y.is_nan() {
            return Some(0);
        }
        let mut cursor = self.tree.cursor::<LineSummary>(());
        cursor.seek(&PreciseHeight(y), Bias::Right);
        let offset = cursor
            .item()
            .map(|item| {
                let top = 24.0 + cursor.start().height.0 as f64 / 1024.0;
                if item.height.0 == 0 {
                    item.count - 1
                } else {
                    (((y - top) * 1024.0 / item.height.0 as f64).floor() as usize)
                        .min(item.count - 1)
                }
            })
            .unwrap_or(0);
        Some((cursor.start().lines + offset).min(self.len() - 1))
    }

    pub(crate) fn line_at_relative_y(&self, origin: Option<usize>, y: Pixels) -> Option<usize> {
        match origin {
            Some(origin) => self.line_at_precise_y(
                self.top_precise(origin.min(self.len().checked_sub(1)?))? + f64::from(f32::from(y)),
            ),
            None => self.line_at_y(y),
        }
    }

    pub(crate) fn visible_lines_relative(
        &self,
        origin: Option<usize>,
        top: Pixels,
        height: Pixels,
    ) -> Range<usize> {
        let Some(origin) = origin else {
            return self.visible_lines(top, height);
        };
        if height <= px(0.0) || self.is_empty() {
            return 0..0;
        }
        let origin_y = self.top_precise(origin.min(self.len() - 1)).unwrap();
        let top = origin_y + f64::from(f32::from(top));
        let bottom = top + f64::from(f32::from(height));
        let end = if bottom <= 24.0 || bottom.is_nan() {
            0
        } else {
            let mut cursor = self.tree.cursor::<LineSummary>(());
            cursor.seek(&PreciseHeight(bottom), Bias::Left);
            cursor.start().lines
                + cursor
                    .item()
                    .map(|item| {
                        let prefix = cursor.start().height.0;
                        let mut low = 0;
                        let mut high = item.count;
                        while low < high {
                            let mid = low + (high - low) / 2;
                            let y = 24.0 + (prefix + mid as i64 * item.height.0) as f64 / 1024.0;
                            if y < bottom {
                                low = mid + 1;
                            } else {
                                high = mid;
                            }
                        }
                        low
                    })
                    .unwrap_or(0)
        };
        let mut start = self.line_at_precise_y(top).unwrap();
        if self.top_precise(start).unwrap() + f64::from(f32::from(self.height(start).unwrap()))
            <= top
        {
            start += 1;
        }
        start.min(end)..end
    }

    pub fn from_heights(heights: Vec<Pixels>) -> Self {
        Self {
            tree: SumTree::from_iter(
                heights.into_iter().map(|height| LineItem {
                    count: 1,
                    height: Fixed::from_pixels(height),
                    measured: true,
                }),
                (),
            ),
            estimate: Fixed::default(),
        }
    }

    pub fn estimated(count: usize, height: Pixels) -> Self {
        let estimate = Fixed::from_pixels(height);
        Self {
            tree: SumTree::from_iter(
                (count > 0).then_some(LineItem {
                    count,
                    height: estimate,
                    measured: false,
                }),
                (),
            ),
            estimate,
        }
    }

    pub fn unmeasured_count(&self) -> usize {
        self.tree.summary().unmeasured
    }

    pub fn next_unmeasured(&self, from: usize) -> Option<usize> {
        if from >= self.len() {
            return None;
        }
        let mut cursor = self.tree.cursor::<LineSummary>(());
        cursor.seek(&Lines(from), Bias::Right);
        if !cursor.item()?.measured {
            return Some(from);
        }
        let pending = cursor.start().unmeasured;
        cursor.seek(&Unmeasured(pending), Bias::Right);
        cursor.item().map(|_| cursor.start().lines)
    }

    pub fn mark_unmeasured(&mut self, range: Range<usize>) {
        let count = range.len();
        self.replace_runs(
            range,
            (count > 0).then_some(LineItem {
                count,
                height: self.estimate,
                measured: false,
            }),
        );
    }

    pub fn set_estimate(&mut self, height: Pixels) {
        self.estimate = Fixed::from_pixels(height);
    }

    pub fn set_measured(&mut self, range: Range<usize>, heights: Vec<Pixels>) {
        assert_eq!(range.len(), heights.len());
        self.splice(range, heights);
    }

    pub fn set_uniform_measured(&mut self, range: Range<usize>, height: Pixels) {
        let count = range.len();
        self.replace_runs(
            range,
            (count > 0).then_some(LineItem {
                count,
                height: Fixed::from_pixels(height),
                measured: true,
            }),
        );
    }

    pub fn set_height(&mut self, line: usize, height: Pixels) {
        assert!(line < self.len(), "line out of bounds");
        self.splice(line..line + 1, vec![height]);
    }

    /// Replaces source rows and immediately updates all derived geometry.
    pub fn splice(&mut self, range: Range<usize>, heights: Vec<Pixels>) -> bool {
        assert!(
            range.start <= range.end && range.end <= self.len(),
            "invalid line range"
        );
        let items = heights
            .into_iter()
            .map(|height| LineItem {
                count: 1,
                height: Fixed::from_pixels(height),
                measured: true,
            })
            .collect::<Vec<_>>();
        let unchanged = range.len() == items.len()
            && items.iter().enumerate().all(|(offset, item)| {
                let mut cursor = self.tree.cursor::<LineSummary>(());
                cursor.seek(&Lines(range.start + offset), Bias::Right);
                cursor
                    .item()
                    .is_some_and(|old| old.height == item.height && old.measured)
            });
        if unchanged {
            return false;
        }
        self.replace_runs(range, items);
        true
    }

    fn replace_runs(&mut self, range: Range<usize>, items: impl IntoIterator<Item = LineItem>) {
        assert!(range.start <= range.end && range.end <= self.len());
        let mut cursor = self.tree.cursor::<LineSummary>(());
        let mut tree = cursor.slice(&Lines(range.start), Bias::Right);
        if let Some(item) = cursor.item() {
            let count = range.start - cursor.start().lines;
            if count > 0 {
                tree.push(
                    LineItem {
                        count,
                        ..item.clone()
                    },
                    (),
                );
            }
        }
        cursor.seek_forward(&Lines(range.end), Bias::Right);
        let tail = cursor.item().and_then(|item| {
            let skipped = range.end - cursor.start().lines;
            (skipped > 0).then(|| LineItem {
                count: item.count - skipped,
                ..item.clone()
            })
        });
        if tail.is_some() {
            cursor.next();
        }
        tree.extend(items, ());
        tree.extend(tail, ());
        tree.append(cursor.suffix(), ());
        drop(cursor);
        self.tree = tree;
    }

    pub fn len(&self) -> usize {
        self.tree.summary().lines
    }

    pub fn is_empty(&self) -> bool {
        self.tree.is_empty()
    }

    pub fn top(&self, line: usize) -> Option<Pixels> {
        if line >= self.len() {
            return None;
        }
        let mut cursor = self.tree.cursor::<LineSummary>(());
        cursor.seek(&Lines(line), Bias::Right);
        Some(
            Fixed(
                cursor.start().height.0
                    + (line - cursor.start().lines) as i64 * cursor.item()?.height.0,
            )
            .document_y(),
        )
    }

    pub fn height(&self, line: usize) -> Option<Pixels> {
        if line >= self.len() {
            return None;
        }
        let mut cursor = self.tree.cursor::<LineSummary>(());
        cursor.seek(&Lines(line), Bias::Right);
        cursor.item().map(|item| item.height.pixels())
    }

    pub fn y_range(&self, line: usize) -> Option<(Pixels, Pixels)> {
        let top = self.top(line)?;
        Some((top, top + self.height(line)?))
    }

    pub fn line_at_y(&self, y: Pixels) -> Option<usize> {
        if self.is_empty() {
            return None;
        }
        if y < DOCUMENT_TOP_PADDING || f32::from(y).is_nan() {
            return Some(0);
        }
        let mut cursor = self.tree.cursor::<LineSummary>(());
        cursor.seek(&Height(y), Bias::Right);
        let offset = cursor
            .item()
            .map(|item| {
                partition_run(item, cursor.start().height, |top| top <= y).saturating_sub(1)
            })
            .unwrap_or(0);
        Some((cursor.start().lines + offset).min(self.len() - 1))
    }

    pub fn visible_lines(&self, top: Pixels, height: Pixels) -> Range<usize> {
        if height <= px(0.0) || self.is_empty() {
            return 0..0;
        }
        let bottom = top + height;
        let end = if bottom <= DOCUMENT_TOP_PADDING || f32::from(bottom).is_nan() {
            0
        } else {
            let mut cursor = self.tree.cursor::<LineSummary>(());
            cursor.seek(&Height(bottom), Bias::Left);
            cursor.start().lines
                + cursor
                    .item()
                    .map(|item| partition_run(item, cursor.start().height, |y| y < bottom))
                    .unwrap_or(0)
        };
        let mut start = self.line_at_y(top).unwrap();
        if self.top(start).unwrap() + self.height(start).unwrap() <= top {
            start += 1;
        }
        start.min(end)..end
    }

    pub fn next_visible_line_after(&self, idx: usize) -> Option<usize> {
        let next = idx.saturating_add(1);
        if next >= self.len() {
            return None;
        }
        let mut cursor = self.tree.cursor::<LineSummary>(());
        cursor.seek(&Lines(next), Bias::Right);
        if cursor.item().is_some_and(|item| item.height.0 > 0) {
            return Some(next);
        }
        let visible = cursor.start().visible_lines;
        cursor.seek(&VisibleLines(visible), Bias::Right);
        cursor.item().map(|_| cursor.start().lines)
    }

    pub fn total_height(&self) -> Pixels {
        Fixed(
            self.tree.summary().height.0
                + Fixed::from_pixels(DOCUMENT_TOP_PADDING + DOCUMENT_BOTTOM_PADDING).0,
        )
        .pixels()
    }

    #[cfg(test)]
    pub(super) fn heights(&self) -> Vec<Pixels> {
        self.tree
            .iter()
            .flat_map(|item| std::iter::repeat_n(item.height.pixels(), item.count))
            .collect()
    }

    #[cfg(test)]
    pub(super) fn offsets(&self) -> Vec<Pixels> {
        let mut height = Fixed::default();
        self.tree
            .iter()
            .flat_map(|item| {
                let start = height;
                height.0 += item.height.0 * item.count as i64;
                (0..item.count)
                    .map(move |offset| Fixed(start.0 + offset as i64 * item.height.0).document_y())
            })
            .collect()
    }
}

#[cfg(test)]
impl PartialEq for LineMetrics {
    fn eq(&self, other: &Self) -> bool {
        self.heights() == other.heights()
    }
}

fn partition_run(item: &LineItem, prefix: Fixed, predicate: impl Fn(Pixels) -> bool) -> usize {
    let mut low = 0;
    let mut high = item.count;
    while low < high {
        let mid = low + (high - low) / 2;
        if predicate(Fixed(prefix.0 + mid as i64 * item.height.0).document_y()) {
            low = mid + 1;
        } else {
            high = mid;
        }
    }
    low
}

#[cfg(test)]
mod vec_metrics;

#[cfg(test)]
mod tests {
    use super::vec_metrics::VecMetrics;
    use super::*;

    #[test]
    fn estimated_runs_split_and_keep_old_snapshots() {
        let mut metrics = LineMetrics::estimated(1_000_000, px(24.0));
        assert_eq!(metrics.tree.iter().count(), 1);
        assert_eq!(metrics.unmeasured_count(), 1_000_000);
        let snapshot = metrics.clone();
        metrics.set_measured(123..126, vec![px(0.0), px(48.0), px(24.0)]);
        assert_eq!(metrics.next_unmeasured(123), Some(126));
        assert_eq!(metrics.next_unmeasured(122), Some(122));
        assert_eq!(metrics.unmeasured_count(), 999_997);
        assert_eq!(snapshot.unmeasured_count(), 1_000_000);
        assert_eq!(metrics.top(126), snapshot.top(126));
        metrics.mark_unmeasured(120..130);
        assert_eq!(metrics.unmeasured_count(), 1_000_000);
        assert_eq!(metrics.heights(), snapshot.heights());
        metrics.set_measured(0..1_000_000, vec![px(24.0); 1_000_000]);
        assert_eq!(metrics.next_unmeasured(0), None);
    }

    #[test]
    fn estimated_run_edits_match_quantized_vec_oracle() {
        let mut metrics = LineMetrics::estimated(5000, px(23.123));
        let mut oracle = VecMetrics::from_heights(vec![px(23.123); 5000]);
        let mut seed = 0x1234_5678_u64;
        for step in 0..10_000 {
            seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1);
            let start = (seed >> 32) as usize % 4990;
            let range = start..start + 10;
            if step % 3 == 0 {
                metrics.mark_unmeasured(range.clone());
                oracle.splice(range, vec![px(23.123); 10]);
            } else {
                let heights = vec![px((step % 7) as f32 * 10.25); 10];
                metrics.set_measured(range.clone(), heights.clone());
                oracle.splice(range, heights);
            }
            oracle.rebuild_from(start);
            let y = oracle.top(start).unwrap();
            assert_eq!(metrics.line_at_y(y), oracle.line_at_y(y));
            assert_eq!(
                metrics.visible_lines(y, px(700.0)),
                oracle.visible_lines(y, px(700.0))
            );
            assert_eq!(
                metrics.next_visible_line_after(start),
                oracle.next_visible_line_after(start)
            );
            if step % 1000 == 0 {
                assert_matches_oracle(&metrics, &oracle, y);
            }
        }
    }

    fn assert_matches_oracle(metrics: &LineMetrics, oracle: &VecMetrics, y: Pixels) {
        assert_eq!(metrics.len(), oracle.len());
        assert_eq!(metrics.is_empty(), oracle.is_empty());
        assert_eq!(metrics.total_height(), oracle.total_height());
        for line in 0..=metrics.len() {
            assert_eq!(metrics.top(line), oracle.top(line), "top at {line}");
            assert_eq!(
                metrics.height(line),
                oracle.height(line),
                "height at {line}"
            );
            assert_eq!(metrics.y_range(line), oracle.y_range(line));
        }
        for y in [y, px(-1.0), DOCUMENT_TOP_PADDING, metrics.total_height()] {
            assert_eq!(metrics.line_at_y(y), oracle.line_at_y(y), "y {y:?}");
            for height in [px(0.0), px(0.001), px(32.0), px(700.0)] {
                assert_eq!(
                    metrics.visible_lines(y, height),
                    oracle.visible_lines(y, height),
                    "viewport {y:?} + {height:?}"
                );
            }
        }
    }

    #[test]
    fn fixed_seed_edits_match_vec_oracle_for_ten_thousand_steps() {
        let mut seed = 0x5eed_cafe_u64;
        let mut random = || {
            seed = seed
                .wrapping_mul(6364136223846793005)
                .wrapping_add(1442695040888963407);
            (seed >> 32) as usize
        };
        let initial: Vec<_> = (0..5_000)
            .map(|line| {
                if line % 7 == 0 {
                    px(0.0)
                } else {
                    px((random() % 100_000) as f32 / 997.0)
                }
            })
            .collect();
        let mut metrics = LineMetrics::from_heights(initial.clone());
        let mut oracle = VecMetrics::from_heights(initial);
        for step in 0..10_000 {
            let len = metrics.len();
            let start = random() % (len + 1);
            if step % 3 == 0 && len > 0 {
                let line = start.min(len - 1);
                let height = px((random() % 65_537) as f32 / 1027.0);
                metrics.set_height(line, height);
                oracle.set_height(line, height);
                oracle.rebuild_from(line);
            } else {
                let removed = (random() % 9).min(len - start);
                let added = (random() % 9).min(5_000 - (len - removed));
                let heights: Vec<_> = (0..added)
                    .map(|_| {
                        let value = random();
                        if value % 3 == 0 {
                            px(0.0)
                        } else {
                            px((value % 65_537) as f32 / 997.0)
                        }
                    })
                    .collect();
                assert_eq!(
                    metrics.splice(start..start + removed, heights.clone()),
                    oracle.splice(start..start + removed, heights)
                );
                oracle.rebuild_from(start);
            }
            let line = random() % (metrics.len() + 1);
            let y = oracle.top(line).unwrap_or(px(0.0)) + px((random() % 5) as f32 - 2.0);
            assert_eq!(metrics.top(line), oracle.top(line), "step {step}");
            assert_eq!(metrics.height(line), oracle.height(line));
            assert_eq!(metrics.total_height(), oracle.total_height());
            assert_eq!(
                metrics.line_at_y(y),
                oracle.line_at_y(y),
                "step {step}, y {y:?}"
            );
            assert_eq!(
                metrics.visible_lines(y, px(700.0)),
                oracle.visible_lines(y, px(700.0)),
                "step {step}"
            );
            assert_eq!(
                metrics.next_visible_line_after(line),
                oracle.next_visible_line_after(line)
            );
            if step % 500 == 0 {
                assert_matches_oracle(&metrics, &oracle, y);
            }
        }
        assert_matches_oracle(&metrics, &oracle, px(24.0));
    }

    #[test]
    fn collapsed_runs_and_large_coordinates_match_vec_boundaries() {
        for heights in [
            vec![],
            vec![px(0.0); 150],
            [vec![px(0.0); 80], vec![px(32.0)], vec![px(0.0); 80]].concat(),
            [
                vec![px(20_000_000.0)],
                vec![px(0.001); 100],
                vec![px(0.0); 80],
            ]
            .concat(),
        ] {
            let metrics = LineMetrics::from_heights(heights.clone());
            let oracle = VecMetrics::from_heights(heights);
            for line in 0..=metrics.len() {
                let y = oracle.top(line).unwrap_or(px(0.0));
                for y in [y - px(0.001), y, y + px(0.001)] {
                    assert_matches_oracle(&metrics, &oracle, y);
                }
                assert_eq!(
                    metrics.next_visible_line_after(line),
                    oracle.next_visible_line_after(line)
                );
            }
            assert_eq!(metrics.next_visible_line_after(usize::MAX), None);
        }
    }

    #[test]
    fn cloned_geometry_shares_storage_and_preserves_old_snapshots() {
        let mut metrics = LineMetrics::from_heights(vec![px(32.0); 5_000]);
        let retained = metrics.clone();
        assert!(std::ptr::eq(
            metrics.tree.summary(),
            retained.tree.summary()
        ));
        metrics.set_height(2_500, px(0.0));
        assert_eq!(retained.height(2_500), Some(px(32.0)));
        assert_eq!(
            metrics.top(2_501),
            retained.top(2_501).map(|y| y - px(32.0))
        );
        metrics.splice(1_000..4_000, vec![]);
        assert_eq!(metrics.len(), 2_000);
        assert_eq!(retained.len(), 5_000);
    }

    #[test]
    fn middle_edits_and_collapsed_row_queries_have_bounded_tree_work() {
        for len in [2_000, 20_000, 200_000] {
            let mut heights = vec![px(0.0); len];
            heights[0] = px(32.0);
            heights[len - 1] = px(32.0);
            let mut metrics = LineMetrics::from_heights(heights);
            let retained = metrics.clone();
            SUMMARY_WORK.set(0);
            metrics.set_height(len / 2, px(24.0));
            metrics.splice(len / 2..len / 2, vec![px(0.0), px(16.0)]);
            assert_eq!(metrics.top(len - 1), Some(px(96.0)));
            assert_eq!(metrics.total_height(), px(152.0));
            assert_eq!(metrics.next_visible_line_after(len / 2 + 2), Some(len + 1));
            assert_eq!(retained.height(len / 2), Some(px(0.0)));
            let work = SUMMARY_WORK.get();
            assert!(
                work < 2_000,
                "{len} rows required {work} summary operations"
            );
        }
    }

    #[test]
    fn fractional_heights_use_fixed_precision_without_suffix_rebuilds() {
        let mut metrics = LineMetrics::from_heights(vec![px(0.0006), px(0.0006), px(0.0)]);
        assert_eq!(metrics.height(0), Some(px(1.0 / 1024.0)));
        metrics.set_height(1, px(32.0));
        assert_eq!(metrics.top(2), Some(px(56.0 + 1.0 / 1024.0)));
        metrics.splice(0..1, vec![px(12.0), px(0.0)]);
        assert_eq!(metrics.top(3), Some(px(68.0)));
        assert_eq!(metrics.total_height(), px(92.0));
    }

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
    fn height_updates_immediately_refresh_derived_offsets() {
        let mut metrics = LineMetrics::from_heights(vec![px(10.0), px(20.0), px(30.0)]);
        metrics.set_height(1, px(0.0));
        assert_eq!(
            metrics,
            LineMetrics::from_heights(vec![px(10.0), px(0.0), px(30.0)])
        );
        metrics.set_height(0, px(40.0));
        assert_eq!(metrics.top(2), Some(px(64.0)));
        assert_eq!(metrics.total_height(), px(118.0));
        let empty = LineMetrics::from_heights(Vec::new());
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
            assert!(metrics.splice(range, replacement));
            assert_eq!(metrics, LineMetrics::from_heights(expected));
        }
        assert!(!metrics.splice(0..1, vec![px(7.0)]));
    }
}
