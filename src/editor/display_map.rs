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
        self.regions
            .iter()
            .any(|r| line > r.start_line && line <= r.end_line)
    }

    /// Returns `true` if `line` is the visible header of a folded region.
    #[inline]
    pub fn is_fold_header(&self, line: SourceLine) -> bool {
        self.regions.iter().any(|r| r.start_line == line)
    }

    /// Returns the next visible line after `from_line`, skipping folded lines.
    pub fn next_visible_line(
        &self,
        from_line: SourceLine,
        total_lines: usize,
    ) -> Option<SourceLine> {
        let mut line = from_line.index() + 1;
        while line < total_lines {
            if let Some(reg) = self
                .regions
                .iter()
                .find(|r| line > r.start_line.index() && line <= r.end_line.index())
            {
                line = reg.end_line.index() + 1;
            } else {
                return Some(SourceLine::new(line));
            }
        }
        None
    }

    /// Returns the previous visible line before `from_line`, skipping folded lines.
    pub fn prev_visible_line(&self, from_line: SourceLine) -> Option<SourceLine> {
        let line = from_line.index().checked_sub(1)?;
        if let Some(reg) = self
            .regions
            .iter()
            .find(|r| line > r.start_line.index() && line <= r.end_line.index())
        {
            Some(reg.start_line)
        } else {
            Some(SourceLine::new(line))
        }
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
