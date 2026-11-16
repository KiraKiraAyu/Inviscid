use crate::buffer::{EditDelta, LineSource, SourceSnapshot, StructureIndex};
use crate::markdown::{MarkdownScanner, ParsedLine, ScanResourcePolicy};
use std::{collections::VecDeque, ops::Range, sync::Arc};

pub(crate) const WINDOW_ROWS: usize = 256;
pub(crate) const MAX_WINDOWS: usize = 8;

#[derive(Clone, Debug)]
pub(crate) struct WindowEntry {
    pub range: Range<usize>,
    pub rows: Arc<Vec<ParsedLine>>,
}

#[derive(Clone, Debug)]
pub(crate) struct EditWindowPatch {
    pub delta: EditDelta,
    pub structural: bool,
    pub local_new_range: Range<usize>,
    pub is_code_interior: bool,
    pub code_close: Option<usize>,
}

pub(crate) fn apply_preedit(
    rows: &mut [ParsedLine],
    range_start: usize,
    preedit: Option<&(usize, Range<usize>)>,
) {
    let Some((line, columns)) = preedit else {
        return;
    };
    if *line < range_start || *line >= range_start + rows.len() {
        return;
    }
    let row = &mut rows[*line - range_start];
    row.spans = crate::markdown::mark_ime_preedit(&row.spans, (columns.start, columns.end));
    if let crate::markdown::BlockKind::Table { cells, .. } = &mut row.kind {
        for cell in cells {
            cell.spans =
                crate::markdown::mark_ime_preedit(&cell.spans, (columns.start, columns.end));
        }
    }
}

pub(crate) fn shift_range(range: &Range<usize>, line_delta: isize) -> Range<usize> {
    let start = (range.start as isize + line_delta).max(0) as usize;
    let end = (range.end as isize + line_delta).max(start as isize) as usize;
    start..end
}

impl EditWindowPatch {
    pub fn line_delta(&self) -> isize {
        self.delta.new_line_count as isize - self.delta.old_line_count as isize
    }

    pub fn dirty_old_range(&self) -> Range<usize> {
        let end = (self.local_new_range.end as isize - self.line_delta())
            .max(self.local_new_range.start as isize) as usize;
        self.local_new_range.start..end
    }

    /// Shifts unaffected windows and drops invalidated ones without touching row contents.
    pub fn shift_or_invalidate(&self, windows: &mut VecDeque<WindowEntry>) {
        if self.structural {
            let cut = self.local_new_range.start;
            windows.retain(|window| window.range.end <= cut);
            return;
        }
        let line_delta = self.line_delta();
        let dirty_old = self.dirty_old_range();
        let mut updated = VecDeque::with_capacity(windows.len());
        while let Some(mut window) = windows.pop_front() {
            if window.range.end <= dirty_old.start {
                updated.push_back(window);
            } else if window.range.start >= dirty_old.end {
                if self.is_code_interior
                    && window.range.start < dirty_old.end + 32
                    && self
                        .code_close
                        .is_none_or(|close| window.range.start <= close)
                {
                    continue;
                }
                window.range = shift_range(&window.range, line_delta);
                updated.push_back(window);
            }
        }
        *windows = updated;
    }

    /// Applies an incremental edit to cached windows, surgically rescanning only the edited
    /// slice inside an overlapping window and shifting subsequent windows without rescanning.
    pub fn apply_edit(
        &self,
        windows: &mut VecDeque<WindowEntry>,
        source: &SourceSnapshot,
        index: &StructureIndex,
        preedit: Option<&(usize, Range<usize>)>,
    ) -> (usize, Vec<Range<usize>>) {
        if self.structural {
            self.shift_or_invalidate(windows);
            return (0, Vec::new());
        }

        let line_delta = self.line_delta();
        let dirty_old = self.dirty_old_range();
        let edit_start = self.delta.start_line.index();
        let edit_new_end = edit_start + self.delta.new_line_count;
        let mut total_scanned = 0usize;
        let mut rescanned_ranges = Vec::new();
        let mut updated = VecDeque::with_capacity(windows.len() + 1);

        while let Some(mut window) = windows.pop_front() {
            if window.range.end <= dirty_old.start {
                updated.push_back(window);
                continue;
            }
            if window.range.start >= dirty_old.end {
                if self.is_code_interior
                    && window.range.start < dirty_old.end + 32
                    && self
                        .code_close
                        .is_none_or(|close| window.range.start <= close)
                {
                    continue;
                }
                window.range = shift_range(&window.range, line_delta);
                updated.push_back(window);
                continue;
            }
            if window.range.start <= dirty_old.start && dirty_old.end <= window.range.end {
                let new_win_end = (window.range.end as isize + line_delta) as usize;
                let max_len = WINDOW_ROWS * 2;
                let scan_target = if self.is_code_interior {
                    let start = edit_start.max(window.range.start);
                    let end = self
                        .code_close
                        .map_or(new_win_end, |close| (close + 1).min(new_win_end));
                    start..end
                } else {
                    self.local_new_range.clone()
                };
                if !scan_target.is_empty()
                    && scan_target.start >= window.range.start
                    && scan_target.end <= new_win_end
                    && scan_target.end <= source.line_count()
                    && scan_target.len() <= max_len
                {
                    let (scanned_range, mut scanned_rows) =
                        MarkdownScanner::scan_snapshot_window_with_policy(
                            source,
                            index,
                            scan_target,
                            ScanResourcePolicy::UiDeferred,
                        );
                    if scanned_range.start >= window.range.start
                        && scanned_range.end <= new_win_end
                        && scanned_range.start <= edit_start
                        && scanned_range.end >= edit_new_end
                    {
                        apply_preedit(&mut scanned_rows, scanned_range.start, preedit);
                        total_scanned += scanned_rows.len();
                        rescanned_ranges.push(scanned_range.clone());
                        let old_local_start = scanned_range.start - window.range.start;
                        let old_local_end = ((scanned_range.end as isize - line_delta) as usize)
                            - window.range.start;
                        let rows = Arc::make_mut(&mut window.rows);
                        rows.splice(old_local_start..old_local_end, scanned_rows);
                        window.range.end = new_win_end;
                        debug_assert_eq!(window.rows.len(), window.range.len());

                        if window.rows.len() > max_len {
                            let split_at = WINDOW_ROWS.min(window.rows.len());
                            let tail_rows = Arc::make_mut(&mut window.rows).split_off(split_at);
                            let mid = window.range.start + split_at;
                            let tail_range = mid..window.range.end;
                            window.range.end = mid;
                            updated.push_back(window);
                            if updated.len() < MAX_WINDOWS {
                                updated.push_back(WindowEntry {
                                    range: tail_range,
                                    rows: Arc::new(tail_rows),
                                });
                            }
                        } else {
                            updated.push_back(window);
                        }
                        continue;
                    }
                }
            }
            // Window straddles dirty boundary or expanded beyond bounds: drop for clean reload.
        }

        while updated.len() > MAX_WINDOWS {
            updated.pop_front();
        }
        *windows = updated;
        (total_scanned, rescanned_ranges)
    }
}
