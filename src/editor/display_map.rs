/// Collapsed range of source lines `[start_line..=end_line]`.
/// `start_line` remains visible as the fold header; `start_line + 1 ..= end_line` are hidden.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FoldRegion {
    pub start_line: usize,
    pub end_line: usize,
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
    pub fn fold(&mut self, start_line: usize, end_line: usize) {
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
    pub fn unfold(&mut self, start_line: usize) -> bool {
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
    pub fn is_line_folded(&self, line: usize) -> bool {
        self.regions
            .iter()
            .any(|r| line > r.start_line && line <= r.end_line)
    }

    /// Returns `true` if `line` is the visible header of a folded region.
    #[inline]
    pub fn is_fold_header(&self, line: usize) -> bool {
        self.regions.iter().any(|r| r.start_line == line)
    }

    /// Returns the next visible line after `from_line`, skipping folded lines.
    pub fn next_visible_line(&self, from_line: usize, total_lines: usize) -> Option<usize> {
        let mut line = from_line + 1;
        while line < total_lines {
            if let Some(reg) = self
                .regions
                .iter()
                .find(|r| line > r.start_line && line <= r.end_line)
            {
                line = reg.end_line + 1;
            } else {
                return Some(line);
            }
        }
        None
    }

    /// Returns the previous visible line before `from_line`, skipping folded lines.
    pub fn prev_visible_line(&self, from_line: usize) -> Option<usize> {
        let line = from_line.checked_sub(1)?;
        if let Some(reg) = self
            .regions
            .iter()
            .find(|r| line > r.start_line && line <= r.end_line)
        {
            Some(reg.start_line)
        } else {
            Some(line)
        }
    }

    pub fn folded_regions(&self) -> &[FoldRegion] {
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
    pub fn version(&self) -> usize {
        self.fold_map.version()
    }

    pub fn fold_map(&self) -> &FoldMap {
        &self.fold_map
    }

    pub fn fold_map_mut(&mut self) -> &mut FoldMap {
        &mut self.fold_map
    }

    #[inline]
    pub fn next_visible_line(&self, from_line: usize, total_lines: usize) -> Option<usize> {
        self.fold_map.next_visible_line(from_line, total_lines)
    }

    #[inline]
    pub fn prev_visible_line(&self, from_line: usize) -> Option<usize> {
        self.fold_map.prev_visible_line(from_line)
    }
}
