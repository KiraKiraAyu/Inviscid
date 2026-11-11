use std::cell::{Cell, Ref, RefCell};

use crate::buffer::{EditDelta, SourceLine, TextBuffer};

/// Collapsed range of source lines `[start_line..=end_line]`.
/// `start_line` remains visible as the fold header; `start_line + 1 ..= end_line` are hidden.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FoldRegion {
    pub start_line: SourceLine,
    pub end_line: SourceLine,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct FoldMap {
    regions: Vec<FoldRegion>,
    version: usize,
}

impl FoldMap {
    pub fn new() -> Self {
        Self::default()
    }

    #[inline]
    pub fn version(&self) -> usize {
        self.version
    }

    /// Folds lines `start_line..=end_line`, keeping `start_line` visible as the header.
    pub fn fold(&mut self, start_line: SourceLine, end_line: SourceLine) {
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
        self.version = self.version.wrapping_add(1);
    }

    /// Unfolds the region starting at `start_line`. Returns `true` if a region was removed.
    pub fn unfold(&mut self, start_line: SourceLine) -> bool {
        let initial_len = self.regions.len();
        self.regions.retain(|r| r.start_line != start_line);
        let changed = self.regions.len() < initial_len;
        if changed {
            self.version = self.version.wrapping_add(1);
        }
        changed
    }

    pub fn unfold_all(&mut self) {
        if !self.regions.is_empty() {
            self.regions.clear();
            self.version = self.version.wrapping_add(1);
        }
    }

    /// Returns `true` if `line` is hidden inside a folded region.
    #[inline]
    pub fn is_line_folded(&self, line: SourceLine) -> bool {
        self.region_hiding(line).is_some()
    }

    fn region_hiding(&self, line: SourceLine) -> Option<&FoldRegion> {
        let index = self
            .regions
            .partition_point(|region| region.start_line < line);
        let region = self.regions.get(index.checked_sub(1)?)?;
        (line <= region.end_line).then_some(region)
    }

    /// Returns `true` if `line` is the visible header of a folded region.
    #[inline]
    pub fn is_fold_header(&self, line: SourceLine) -> bool {
        let index = self
            .regions
            .partition_point(|region| region.start_line < line);
        self.regions
            .get(index)
            .is_some_and(|region| region.start_line == line)
    }

    /// Returns the next visible line after `from_line`, skipping folded lines.
    pub fn next_visible_line(
        &self,
        from_line: SourceLine,
        total_lines: usize,
    ) -> Option<SourceLine> {
        let line = SourceLine::new(from_line.index().checked_add(1)?);
        let line = self
            .region_hiding(line)
            .map_or(line, |region| region.end_line + 1);
        (line.index() < total_lines).then_some(line)
    }

    /// Returns the previous visible line before `from_line`, skipping folded lines.
    pub fn prev_visible_line(&self, from_line: SourceLine) -> Option<SourceLine> {
        let line = from_line.checked_sub(1)?;
        Some(
            self.region_hiding(line)
                .map_or(line, |region| region.start_line),
        )
    }

    pub fn folded_regions(&self) -> &[FoldRegion] {
        &self.regions
    }

    fn apply_edit(&mut self, delta: EditDelta) {
        let start = delta.start_line.index();
        let end = start + delta.old_line_count;
        let mut changed = false;
        self.regions.retain_mut(|region| {
            if end <= region.start_line.index() {
                let shift = |line: SourceLine| {
                    SourceLine::new(line.index() - delta.old_line_count + delta.new_line_count)
                };
                changed |= delta.old_line_count != delta.new_line_count;
                region.start_line = shift(region.start_line);
                region.end_line = shift(region.end_line);
                true
            } else if start > region.end_line.index() {
                true
            } else {
                changed = true;
                false
            }
        });
        if changed {
            self.version = self.version.wrapping_add(1);
        }
    }
}

#[derive(Clone, Debug, Default)]
pub struct DisplayMap {
    fold_map: RefCell<FoldMap>,
    buffer_revision: Cell<Option<u64>>,
}

impl DisplayMap {
    pub fn new() -> Self {
        Self::default()
    }

    #[inline]
    pub fn version(&self) -> usize {
        self.fold_map.borrow().version()
    }

    pub fn fold_map(&self) -> Ref<'_, FoldMap> {
        self.fold_map.borrow()
    }

    pub fn fold_map_mut(&mut self) -> &mut FoldMap {
        self.fold_map.get_mut()
    }

    pub(crate) fn synchronize(&self, buffer: &TextBuffer) {
        let revision = buffer.revision();
        let previous = self.buffer_revision.replace(Some(revision));
        if previous == Some(revision) {
            return;
        }
        let mut folds = self.fold_map.borrow_mut();
        if previous.is_some_and(|previous| previous.wrapping_add(1) == revision)
            && let Some(delta) = buffer.last_edit_delta()
        {
            folds.apply_edit(delta);
        } else {
            // The buffer exposes only its last edit, so skipped revisions cannot be mapped.
            folds.unfold_all();
        }
    }

    #[inline]
    pub fn next_visible_line(
        &self,
        from_line: SourceLine,
        total_lines: usize,
    ) -> Option<SourceLine> {
        self.fold_map
            .borrow()
            .next_visible_line(from_line, total_lines)
    }

    #[inline]
    pub fn prev_visible_line(&self, from_line: SourceLine) -> Option<SourceLine> {
        self.fold_map.borrow().prev_visible_line(from_line)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Original linear queries remain the oracle for the ordered-region index.
    fn linear_next(regions: &[FoldRegion], from: SourceLine, total: usize) -> Option<SourceLine> {
        let mut line = from.index() + 1;
        while line < total {
            if let Some(region) = regions
                .iter()
                .find(|r| line > r.start_line.index() && line <= r.end_line.index())
            {
                line = region.end_line.index() + 1;
            } else {
                return Some(SourceLine::new(line));
            }
        }
        None
    }

    fn linear_prev(regions: &[FoldRegion], from: SourceLine) -> Option<SourceLine> {
        let line = from.index().checked_sub(1)?;
        Some(
            regions
                .iter()
                .find(|r| line > r.start_line.index() && line <= r.end_line.index())
                .map_or(SourceLine::new(line), |r| r.start_line),
        )
    }

    #[test]
    fn fold_queries_match_linear_oracle_after_random_updates() {
        let mut folds = FoldMap::new();
        let mut seed = 0x85ad_19c2_43b7_6001_u64;
        let mut random = || {
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            seed as usize
        };
        for step in 0..10_000 {
            let start = random() % 4096;
            let end = start + random() % 12;
            match step % 11 {
                0 => {
                    folds.unfold(SourceLine::new(start));
                }
                _ => folds.fold(SourceLine::new(start), SourceLine::new(end)),
            }
            for line in [0, start, start + 1, end, end + 1, random() % 4110, 4110] {
                let line = SourceLine::new(line);
                assert_eq!(
                    folds.is_line_folded(line),
                    folds
                        .regions
                        .iter()
                        .any(|r| line > r.start_line && line <= r.end_line)
                );
                assert_eq!(
                    folds.is_fold_header(line),
                    folds.regions.iter().any(|r| line == r.start_line)
                );
                assert_eq!(
                    folds.next_visible_line(line, 4110),
                    linear_next(&folds.regions, line, 4110)
                );
                assert_eq!(
                    folds.prev_visible_line(line),
                    linear_prev(&folds.regions, line)
                );
            }
        }
    }

    #[test]
    fn skipped_edit_revisions_expand_stale_folds() {
        let mut buffer = TextBuffer::from_str("before\nheader\na\nb\nafter", None);
        let mut map = DisplayMap::new();
        map.synchronize(&buffer);
        map.fold_map_mut()
            .fold(SourceLine::new(1), SourceLine::new(3));
        buffer.insert_text("one\n");
        buffer.insert_text("two\n");
        map.synchronize(&buffer);
        assert!(map.fold_map().folded_regions().is_empty());
    }
}
