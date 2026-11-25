use super::parser_summary::{BlockState, PrefixSummary};
use super::{LineSource, SourceLine};
use crate::markdown::structure::{LineStructure, classify};
use gpui_sum_tree::{Bias, ContextLessSummary, Item, SeekTarget, SumTree};
use std::{cmp::Ordering, ops::Range};

#[derive(Clone, Copy, Debug)]
struct StructureLine {
    count: usize,
    raw: LineStructure,
    projected: LineStructure,
    is_boundary: bool,
}

impl StructureLine {
    fn new(line: &str) -> Self {
        let raw = classify(line);
        let projected = if raw.quote_prefix == 0 {
            raw
        } else {
            classify(&line[raw.quote_prefix..])
        };
        let is_boundary = raw.is_fence || projected.is_fence || projected.is_math_delim;
        Self {
            count: 1,
            raw,
            projected,
            is_boundary,
        }
    }

    fn runs(lines: impl Iterator<Item = Self>) -> impl Iterator<Item = Self> {
        let mut lines = lines.peekable();
        std::iter::from_fn(move || {
            let mut run = lines.next()?;
            while !run.is_boundary
                && !run.projected.is_heading
                && lines.peek().is_some_and(|next| {
                    !next.is_boundary
                        && !next.projected.is_heading
                        && next.raw.is_blank == run.raw.is_blank
                        && next.projected.is_blank == run.projected.is_blank
                        && next.raw.quote_prefix == run.raw.quote_prefix
                })
            {
                run.count += lines.next().unwrap().count;
            }
            Some(run)
        })
    }
}

#[derive(Clone, Copy, Debug, Default)]
struct StructureSummary {
    lines: usize,
    parser: PrefixSummary,
    last_non_blank: [usize; BlockState::COUNT],
    outline_markers: usize,
}

impl Item for StructureLine {
    type Summary = StructureSummary;

    fn summary(&self, (): ()) -> StructureSummary {
        let is_outline_marker = self.is_boundary || self.projected.is_heading;
        let parser = if is_outline_marker {
            PrefixSummary::line(self.raw, self.projected)
        } else {
            PrefixSummary::neutral()
        };
        let raw_non_blank = usize::from(!self.raw.is_blank) * self.count;
        let projected_non_blank = usize::from(!self.projected.is_blank) * self.count;
        let last_non_blank = [
            projected_non_blank,
            raw_non_blank,
            projected_non_blank,
            projected_non_blank,
        ];
        StructureSummary {
            lines: self.count,
            parser,
            last_non_blank,
            outline_markers: usize::from(is_outline_marker),
        }
    }
}

impl ContextLessSummary for StructureSummary {
    fn zero() -> Self {
        Self::default()
    }

    fn add_summary(&mut self, other: &Self) {
        for (state, last) in self.last_non_blank.iter_mut().enumerate() {
            let mid = self.parser.0[state].exit as usize;
            let other_last = other.last_non_blank[mid];
            if other_last > 0 {
                *last = self.lines + other_last;
            }
        }
        self.lines += other.lines;
        self.parser.append(&other.parser);
        self.outline_markers += other.outline_markers;
    }
}

struct Lines(usize);

impl<'a> SeekTarget<'a, StructureSummary, StructureSummary> for Lines {
    fn cmp(&self, location: &StructureSummary, (): ()) -> Ordering {
        Ord::cmp(&self.0, &location.lines)
    }
}

struct NonBlank(usize);

impl<'a> SeekTarget<'a, StructureSummary, StructureSummary> for NonBlank {
    fn cmp(&self, location: &StructureSummary, (): ()) -> Ordering {
        Ord::cmp(&self.0, &location.last_non_blank[0])
    }
}

#[derive(Clone, Copy)]
enum PrefixCounter {
    CodeOpen,
    CodeClose,
    MathOpen,
    MathClose,
    Heading,
}

struct PrefixSeek {
    kind: PrefixCounter,
    target: usize,
}

impl<'a> SeekTarget<'a, StructureSummary, StructureSummary> for PrefixSeek {
    fn cmp(&self, location: &StructureSummary, (): ()) -> Ordering {
        let transition = location.parser.0[BlockState::Outside as usize];
        let count = match self.kind {
            PrefixCounter::CodeOpen => transition.code_opens,
            PrefixCounter::CodeClose => transition.code_closes,
            PrefixCounter::MathOpen => transition.math_opens,
            PrefixCounter::MathClose => transition.math_closes,
            PrefixCounter::Heading => transition.headings,
        };
        Ord::cmp(&self.target, &count)
    }
}

/// Paired O(log N) index of fenced code blocks, math blocks, quote prefixes,
/// and blank separator runs. Unclosed opening delimiters are neutralized so
/// only closed blocks take effect.
#[derive(Clone, Debug)]
pub(crate) struct StructureIndex {
    tree: SumTree<StructureLine>,
    effective_tree: Option<SumTree<StructureLine>>,
    #[cfg(test)]
    pub(super) classified_lines: usize,
}

fn seek_counter_in(
    tree: &SumTree<StructureLine>,
    kind: PrefixCounter,
    ordinal: usize,
) -> SourceLine {
    let mut cursor = tree.cursor::<StructureSummary>(());
    cursor.seek(
        &PrefixSeek {
            kind,
            target: ordinal,
        },
        Bias::Right,
    );
    SourceLine::new(cursor.start().lines)
}

fn neutralize_line(tree: &SumTree<StructureLine>, line_idx: usize) -> SumTree<StructureLine> {
    let mut cursor = tree.cursor::<StructureSummary>(());
    let mut next = cursor.slice(&Lines(line_idx), Bias::Right);
    if let Some(item) = cursor.item() {
        let prefix_count = line_idx - cursor.start().lines;
        if prefix_count > 0 {
            next.push(
                StructureLine {
                    count: prefix_count,
                    ..*item
                },
                (),
            );
        }
        next.push(
            StructureLine {
                count: 1,
                raw: LineStructure {
                    is_fence: false,
                    is_math_delim: false,
                    ..item.raw
                },
                projected: LineStructure {
                    is_fence: false,
                    is_math_delim: false,
                    ..item.projected
                },
                is_boundary: false,
            },
            (),
        );
    }
    cursor.seek_forward(&Lines(line_idx + 1), Bias::Right);
    let tail = cursor.item().and_then(|item| {
        let skipped = (line_idx + 1) - cursor.start().lines;
        (skipped > 0).then(|| StructureLine {
            count: item.count - skipped,
            ..*item
        })
    });
    if tail.is_some() {
        cursor.next();
    }
    next.extend(tail, ());
    next.append(cursor.suffix(), ());
    next
}

fn resolve_paired_tree(tree: &SumTree<StructureLine>) -> Option<SumTree<StructureLine>> {
    if tree.summary().parser.0[BlockState::Outside as usize].exit == BlockState::Outside {
        return None;
    }
    let mut current = tree.clone();
    loop {
        let total = current.summary().parser.0[BlockState::Outside as usize];
        let (kind, ordinal) = match total.exit {
            BlockState::Outside => break,
            BlockState::RawCode | BlockState::QuotedCode => {
                (PrefixCounter::CodeOpen, total.code_opens - 1)
            }
            BlockState::Math => (PrefixCounter::MathOpen, total.math_opens - 1),
        };
        let open_line = seek_counter_in(&current, kind, ordinal);
        current = neutralize_line(&current, open_line.index());
    }
    Some(current)
}

impl StructureIndex {
    pub(crate) fn new(source: &(impl LineSource + ?Sized)) -> Self {
        let tree = SumTree::from_iter(
            StructureLine::runs(source.source_lines().map(|line| StructureLine::new(&line))),
            (),
        );
        let effective_tree = resolve_paired_tree(&tree);
        Self {
            tree,
            effective_tree,
            #[cfg(test)]
            classified_lines: source.line_count(),
        }
    }

    pub(crate) fn len(&self) -> usize {
        self.tree.summary().lines
    }

    pub(crate) fn optimistic(&self) -> Self {
        Self {
            tree: self.tree.clone(),
            effective_tree: None,
            #[cfg(test)]
            classified_lines: self.classified_lines,
        }
    }

    pub(crate) fn has_unclosed_blocks(&self) -> bool {
        self.effective_tree.is_some()
    }

    fn active_tree(&self) -> &SumTree<StructureLine> {
        self.effective_tree.as_ref().unwrap_or(&self.tree)
    }

    pub(crate) fn heading_count(&self) -> usize {
        self.active_tree().summary().parser.0[BlockState::Outside as usize].headings
    }

    pub(crate) fn heading_at_or_before(&self, line: SourceLine) -> Option<SourceLine> {
        if self.len() == 0 {
            return None;
        }
        let target = line.index().min(self.len() - 1);
        let mut cursor = self.active_tree().cursor::<StructureSummary>(());
        cursor.seek(&Lines(target), Bias::Right);
        let count_through_line = cursor.end().parser.0[BlockState::Outside as usize].headings;
        if count_through_line == 0 {
            return None;
        }
        Some(self.seek_counter(PrefixCounter::Heading, count_through_line - 1))
    }

    pub(crate) fn heading_entries_in_range(&self, range: Range<usize>) -> Vec<(SourceLine, usize)> {
        let total = self.heading_count();
        let start = range.start.min(total);
        let end = range.end.min(total);
        if start >= end {
            return Vec::new();
        }
        let mut entries = Vec::with_capacity(end - start);
        let mut cursor = self.active_tree().cursor::<StructureSummary>(());
        for ordinal in start..end {
            cursor.seek_forward(
                &PrefixSeek {
                    kind: PrefixCounter::Heading,
                    target: ordinal,
                },
                Bias::Right,
            );
            let quote_prefix = cursor.item().map_or(0, |item| item.raw.quote_prefix);
            entries.push((SourceLine::new(cursor.start().lines), quote_prefix));
        }
        entries
    }

    #[cfg(test)]
    pub(crate) fn heading_lines(&self) -> Vec<SourceLine> {
        self.heading_entries_in_range(0..self.heading_count())
            .into_iter()
            .map(|(line, _)| line)
            .collect()
    }

    /// Splices `old` with `new` and returns whether headings or their line numbers may have changed.
    pub(crate) fn splice(
        &mut self,
        old: Range<usize>,
        source: &(impl LineSource + ?Sized),
        new: Range<usize>,
    ) -> bool {
        let mut cursor = self.tree.cursor::<StructureSummary>(());
        let mut tree = cursor.slice(&Lines(old.start), Bias::Right);
        let markers_before_old_start = cursor.start().outline_markers;
        if let Some(item) = cursor.item() {
            let count = old.start - cursor.start().lines;
            if count > 0 {
                tree.push(StructureLine { count, ..*item }, ());
            }
        }
        cursor.seek_forward(&Lines(old.end), Bias::Right);
        let removed_markers = cursor.start().outline_markers - markers_before_old_start;
        let tail = cursor.item().and_then(|item| {
            let skipped = old.end - cursor.start().lines;
            (skipped > 0).then(|| StructureLine {
                count: item.count - skipped,
                ..*item
            })
        });
        if tail.is_some() {
            cursor.next();
        }
        let mut new_has_marker = false;
        tree.extend(
            StructureLine::runs(new.clone().map(|line| {
                let structure_line =
                    StructureLine::new(&source.source_line(SourceLine::new(line)));
                new_has_marker |= structure_line.is_boundary || structure_line.projected.is_heading;
                structure_line
            })),
            (),
        );
        tree.extend(tail, ());
        tree.append(cursor.suffix(), ());
        drop(cursor);
        self.tree = tree;
        self.effective_tree = resolve_paired_tree(&self.tree);
        #[cfg(test)]
        {
            self.classified_lines += new.len();
        }
        removed_markers > 0
            || new_has_marker
            || (old.len() != new.len() && self.heading_count() > 0)
    }

    fn seek_counter(&self, kind: PrefixCounter, ordinal: usize) -> SourceLine {
        seek_counter_in(self.active_tree(), kind, ordinal)
    }

    pub(crate) fn fence_range(&self, line: SourceLine) -> Option<(SourceLine, Option<SourceLine>)> {
        if line.index() >= self.len() {
            return None;
        }
        let tree = self.active_tree();
        let mut cursor = tree.cursor::<StructureSummary>(());
        cursor.seek(&Lines(line.index()), Bias::Right);
        let before = cursor.start().parser.0[BlockState::Outside as usize];
        let after = cursor.end().parser.0[BlockState::Outside as usize];
        let total = tree.summary().parser.0[BlockState::Outside as usize];
        if after.code_opens > before.code_opens {
            let ordinal = before.code_opens;
            let close = (total.code_closes > ordinal)
                .then(|| self.seek_counter(PrefixCounter::CodeClose, ordinal));
            Some((line, close))
        } else if before.exit.is_code() {
            let ordinal = before.code_opens - 1;
            let open = self.seek_counter(PrefixCounter::CodeOpen, ordinal);
            let close = if after.code_closes > before.code_closes {
                Some(line)
            } else if total.code_closes > ordinal {
                Some(self.seek_counter(PrefixCounter::CodeClose, ordinal))
            } else {
                None
            };
            Some((open, close))
        } else {
            None
        }
    }

    pub(crate) fn math_range(&self, line: SourceLine) -> Option<(SourceLine, Option<SourceLine>)> {
        if line.index() >= self.len() {
            return None;
        }
        let tree = self.active_tree();
        let mut cursor = tree.cursor::<StructureSummary>(());
        cursor.seek(&Lines(line.index()), Bias::Right);
        let before = cursor.start().parser.0[BlockState::Outside as usize];
        let after = cursor.end().parser.0[BlockState::Outside as usize];
        let total = tree.summary().parser.0[BlockState::Outside as usize];
        if after.math_opens > before.math_opens {
            let ordinal = before.math_opens;
            let close = (total.math_closes > ordinal)
                .then(|| self.seek_counter(PrefixCounter::MathClose, ordinal));
            Some((line, close))
        } else if before.exit.is_math() {
            let ordinal = before.math_opens - 1;
            let open = self.seek_counter(PrefixCounter::MathOpen, ordinal);
            let close = if after.math_closes > before.math_closes {
                Some(line)
            } else if total.math_closes > ordinal {
                Some(self.seek_counter(PrefixCounter::MathClose, ordinal))
            } else {
                None
            };
            Some((open, close))
        } else {
            None
        }
    }

    /// Returns `(is_in_code_fence_or_opener, is_closing_code_fence)`.
    pub(crate) fn fence_state(&self, line: SourceLine) -> (bool, bool) {
        match self.fence_range(line) {
            Some((_, Some(close))) if close == line => (false, true),
            Some(_) => (true, false),
            None => (false, false),
        }
    }

    pub(crate) fn quote_prefix(&self, line: SourceLine) -> usize {
        if line.index() >= self.len() {
            return 0;
        }
        let mut cursor = self.active_tree().cursor::<StructureSummary>(());
        cursor.seek(&Lines(line.index()), Bias::Right);
        cursor.item().map_or(0, |item| {
            if cursor.start().parser.0[BlockState::Outside as usize]
                .exit
                .is_raw_code()
                || item.raw.is_fence
            {
                0
            } else {
                item.raw.quote_prefix
            }
        })
    }

    pub(crate) fn is_in_code_block_after(&self, line: SourceLine) -> bool {
        if line.index() >= self.len() {
            return false;
        }
        let mut cursor = self.active_tree().cursor::<StructureSummary>(());
        cursor.seek(&Lines(line.index()), Bias::Right);
        cursor.end().parser.0[BlockState::Outside as usize]
            .exit
            .is_code()
    }

    pub(crate) fn blank_range(&self, row: usize) -> Option<Range<usize>> {
        if row >= self.len() {
            return None;
        }
        let mut cursor = self.active_tree().cursor::<StructureSummary>(());
        cursor.seek(&Lines(row), Bias::Right);
        let start = cursor.end().last_non_blank[BlockState::Outside as usize];
        if start > row {
            return None;
        }
        cursor.seek_forward(&NonBlank(row + 1), Bias::Right);
        Some(start..cursor.start().lines)
    }
}

#[cfg(test)]
pub(in crate::buffer) mod tests {
    use super::*;
    use crate::buffer::{LineSource, SourceLine, TextBuffer};

    #[test]
    fn neutral_source_runs_share_one_structure_item_and_survive_splices() {
        let mut rows = vec!["paragraph".to_string(); 200_000];
        let mut index = StructureIndex::new(&rows);
        assert_eq!(index.tree.iter().count(), 1);
        assert_eq!(index.len(), rows.len());
        rows.splice(
            100_000..100_001,
            ["```".into(), "code".into(), "```".into()],
        );
        index.splice(100_000..100_001, &rows, 100_000..100_003);
        let expected = StructureIndex::new(&rows);
        for row in [
            0,
            99_999,
            100_000,
            100_001,
            100_002,
            100_003,
            rows.len() - 1,
        ] {
            let line = SourceLine::new(row);
            assert_eq!(index.fence_range(line), expected.fence_range(line));
            assert_eq!(index.math_range(line), expected.math_range(line));
            assert_eq!(index.blank_range(row), expected.blank_range(row));
        }
        rows.splice(100_000..100_003, vec![String::new(); 3]);
        index.splice(100_000..100_003, &rows, 100_000..100_003);
        assert_eq!(index.blank_range(100_001), Some(100_000..100_003));
    }

    #[test]
    fn navigation_summaries_keep_blank_metadata_compact() {
        assert!(std::mem::size_of::<StructureSummary>() <= 512);
    }

    /// Forward prefix-scan reference for paired blocks, independent of SumTree.
    struct VecIndex<'a> {
        lines: &'a [String],
        fences: Vec<Option<(SourceLine, Option<SourceLine>)>>,
        maths: Vec<Option<(SourceLine, Option<SourceLine>)>>,
        quote_prefixes: Vec<usize>,
        headings: Vec<SourceLine>,
    }

    impl<'a> VecIndex<'a> {
        fn build(lines: &'a [String]) -> Self {
            let mut suppressed = vec![false; lines.len()];
            let mut fences = vec![None; lines.len()];
            let mut maths = vec![None; lines.len()];
            let mut quote_prefixes = vec![0; lines.len()];
            let mut headings = Vec::new();

            loop {
                fences.fill(None);
                maths.fill(None);
                quote_prefixes.fill(0);
                headings.clear();
                let mut state = BlockState::Outside;
                let mut active_open: Option<usize> = None;

                for (i, text) in lines.iter().enumerate() {
                    let mut raw = classify(text);
                    let mut projected = classify(&text[raw.quote_prefix..]);
                    if suppressed[i] {
                        raw.is_fence = false;
                        raw.is_math_delim = false;
                        projected.is_fence = false;
                        projected.is_math_delim = false;
                    }
                    let qp = if state.is_raw_code() || raw.is_fence {
                        0
                    } else {
                        raw.quote_prefix
                    };
                    quote_prefixes[i] = qp;

                    match state {
                        BlockState::Outside => {
                            if raw.is_fence {
                                state = BlockState::RawCode;
                                active_open = Some(i);
                            } else if projected.is_fence {
                                state = BlockState::QuotedCode;
                                active_open = Some(i);
                            } else if projected.is_math_delim {
                                state = BlockState::Math;
                                active_open = Some(i);
                            } else if projected.is_heading {
                                headings.push(SourceLine::new(i));
                            }
                        }
                        BlockState::RawCode => {
                            let open = active_open.unwrap();
                            if raw.is_fence {
                                for slot in &mut fences[open..=i] {
                                    *slot = Some((SourceLine::new(open), Some(SourceLine::new(i))));
                                }
                                state = BlockState::Outside;
                                active_open = None;
                            }
                        }
                        BlockState::QuotedCode => {
                            let open = active_open.unwrap();
                            if projected.is_fence {
                                for slot in &mut fences[open..=i] {
                                    *slot = Some((SourceLine::new(open), Some(SourceLine::new(i))));
                                }
                                state = BlockState::Outside;
                                active_open = None;
                            }
                        }
                        BlockState::Math => {
                            let open = active_open.unwrap();
                            if projected.is_math_delim {
                                for slot in &mut maths[open..=i] {
                                    *slot = Some((SourceLine::new(open), Some(SourceLine::new(i))));
                                }
                                state = BlockState::Outside;
                                active_open = None;
                            }
                        }
                    }
                }

                if let Some(open) = active_open {
                    suppressed[open] = true;
                } else {
                    break;
                }
            }

            Self {
                lines,
                fences,
                maths,
                quote_prefixes,
                headings,
            }
        }

        fn fence_range(&self, target: SourceLine) -> Option<(SourceLine, Option<SourceLine>)> {
            self.fences.get(target.index()).copied().flatten()
        }

        fn fence_state(&self, target: SourceLine) -> (bool, bool) {
            match self.fence_range(target) {
                Some((_, Some(close))) if close == target => (false, true),
                Some(_) => (true, false),
                None => (false, false),
            }
        }

        fn math_range(&self, target: SourceLine) -> Option<(SourceLine, Option<SourceLine>)> {
            self.maths.get(target.index()).copied().flatten()
        }

        fn quote_prefix(&self, target: SourceLine) -> usize {
            self.quote_prefixes
                .get(target.index())
                .copied()
                .unwrap_or(0)
        }

        fn is_line_blank(&self, idx: usize) -> bool {
            let text = &self.lines[idx];
            let qp = self.quote_prefixes[idx];
            text[qp..].trim().is_empty()
        }

        fn blank_range(&self, row: usize) -> Option<Range<usize>> {
            if row >= self.lines.len() || !self.is_line_blank(row) {
                return None;
            }
            let mut start = row;
            while start > 0 && self.is_line_blank(start - 1) {
                start -= 1;
            }
            let mut end = row + 1;
            while end < self.lines.len() && self.is_line_blank(end) {
                end += 1;
            }
            Some(start..end)
        }

        fn heading_at_or_before(&self, line: SourceLine) -> Option<SourceLine> {
            let idx = self.headings.partition_point(|&h| h <= line);
            idx.checked_sub(1).map(|i| self.headings[i])
        }
    }

    pub(in crate::buffer) fn assert_matches(index: &StructureIndex, lines: &[String]) {
        let oracle = VecIndex::build(lines);
        assert_eq!(index.heading_count(), oracle.headings.len(), "{lines:?}");
        assert_eq!(index.heading_lines(), oracle.headings, "{lines:?}");
        assert_eq!(
            index.heading_entries_in_range(0..index.heading_count()),
            oracle
                .headings
                .iter()
                .map(|&h| (h, oracle.quote_prefix(h)))
                .collect::<Vec<_>>(),
            "{lines:?}"
        );
        for row in 0..=lines.len() + 1 {
            let line = SourceLine::new(row);
            assert_eq!(
                index.heading_at_or_before(line),
                oracle.heading_at_or_before(line),
                "heading_at_or_before at {row}: {lines:?}"
            );
            assert_eq!(
                index.fence_range(line),
                oracle.fence_range(line),
                "fence_range at {row}: {lines:?}"
            );
            assert_eq!(
                index.fence_state(line),
                oracle.fence_state(line),
                "fence_state at {row}: {lines:?}"
            );
            assert_eq!(
                index.math_range(line),
                oracle.math_range(line),
                "math_range at {row}: {lines:?}"
            );
            assert_eq!(
                index.quote_prefix(line),
                oracle.quote_prefix(line),
                "quote_prefix at {row}: {lines:?}"
            );
            assert_eq!(
                index.blank_range(row),
                oracle.blank_range(row),
                "blank_range at {row}: {lines:?}"
            );
        }
    }

    #[test]
    fn quoted_fences_math_and_unclosed_blocks_match_vec_oracle() {
        for text in [
            "",
            "```\n$$\n$$",
            "```\n$$\n$$\n```",
            "$$\n```\nx\n```\n$$",
            "> ```\n> $$\n> $$\n> ```\n$$\nx\n$$",
            "```\n> ```\n$$\n```\n$$\n> $$\n> $$",
            "```\n> ```\n$$\n> ```\n$$",
            "$$\n$$\n$$",
            "```\n```\n```",
            "$$\nunfinished",
            "> ```\n> unclosed",
        ] {
            let lines: Vec<String> = text.split('\n').map(str::to_owned).collect();
            assert_matches(&StructureIndex::new(&lines), &lines);
        }
    }

    #[test]
    fn ten_thousand_fixed_seed_splices_match_vec_oracle() {
        let fragments = [
            "",
            " ",
            "text",
            "# h",
            "  ## h2",
            "> ### q",
            "# ",
            "```",
            "  ```rust",
            "> ```",
            "$$",
            " $$ ",
            "> $$",
            "> ",
            "> > $$",
            "``",
        ];
        let mut seed = 0x5eed_d002_u64;
        let mut random = || {
            seed = seed
                .wrapping_mul(6364136223846793005)
                .wrapping_add(1442695040888963407);
            (seed >> 32) as usize
        };
        let mut lines: Vec<String> = (0..150)
            .map(|_| fragments[random() % fragments.len()].into())
            .collect();
        let mut index = StructureIndex::new(&lines);
        for _ in 0..10_000 {
            let start = random() % (lines.len() + 1);
            let removed = (random() % 5).min(lines.len() - start);
            let added = (random() % 5).min(300 - (lines.len() - removed));
            let replacement: Vec<String> = (0..added)
                .map(|_| fragments[random() % fragments.len()].into())
                .collect();
            let before_headings = index.heading_lines();
            lines.splice(start..start + removed, replacement);
            let outline_affected =
                index.splice(start..start + removed, &lines, start..start + added);
            assert_matches(&index, &lines);
            if index.heading_lines() != before_headings {
                assert!(
                    outline_affected,
                    "headings changed from {before_headings:?} to {:?} without outline_affected",
                    index.heading_lines()
                );
            }
        }
    }

    #[test]
    fn splice_reports_outline_affected_only_for_heading_boundary_or_line_shifts() {
        let mut lines: Vec<String> = vec!["# First".into(), "body".into(), "## Second".into()];
        let mut index = StructureIndex::new(&lines);

        // Single-line body edit: unaffected.
        lines[1] = "edited body".into();
        assert!(!index.splice(1..2, &lines, 1..2));

        // Body edit inserting a line while headings exist: affects line numbers.
        lines.splice(1..2, ["line a".into(), "line b".into()]);
        assert!(index.splice(1..2, &lines, 1..3));

        // Heading text edit: affected.
        lines[0] = "# First edited".into();
        assert!(index.splice(0..1, &lines, 0..1));

        // Fence insertion: affected.
        lines.splice(1..1, ["```".into()]);
        assert!(index.splice(1..1, &lines, 1..2));

        // Document without headings: inserting a body newline stays unaffected.
        let mut no_heading_lines: Vec<String> = vec!["alpha".into(), "beta".into()];
        let mut no_heading_index = StructureIndex::new(&no_heading_lines);
        no_heading_lines.splice(1..2, ["beta 1".into(), "beta 2".into()]);
        assert!(!no_heading_index.splice(1..2, &no_heading_lines, 1..3));
    }

    #[test]
    fn heading_entries_in_range_returns_requested_slice_and_clamps_bounds() {
        let lines: Vec<String> = vec![
            "# H0".into(),
            "body".into(),
            "> ## H1".into(),
            "### H2".into(),
            "#### H3".into(),
        ];
        let index = StructureIndex::new(&lines);
        assert_eq!(
            index.heading_entries_in_range(1..3),
            vec![(SourceLine::new(2), 2), (SourceLine::new(3), 0)]
        );
        assert_eq!(
            index.heading_entries_in_range(2..10),
            vec![(SourceLine::new(3), 0), (SourceLine::new(4), 0)]
        );
        assert!(index.heading_entries_in_range(4..10).is_empty());
        assert!(index.heading_entries_in_range(3..3).is_empty());
        #[allow(clippy::reversed_empty_ranges)]
        let inverted = index.heading_entries_in_range(3..2);
        assert!(inverted.is_empty());
    }

    #[test]
    fn cached_buffer_queries_splice_without_materializing_document() {
        let mut buffer = TextBuffer::from_str(
            &format!("```\n{}\n```\n$$\nx\n$$", "text\n".repeat(20_000)),
            None,
        );
        assert!(buffer.is_in_code_fence(SourceLine::new(10_000)));
        assert_eq!(buffer.full_line_materializations(), 0);
        buffer.record_single_line_edit(SourceLine::new(10_000), |line, _| *line = "$$".into());
        assert!(buffer.is_in_code_fence(SourceLine::new(10_000)));
        assert_eq!(buffer.full_line_materializations(), 0);
        assert_eq!(buffer.line_index().classified_lines(), 20_007);
        buffer.undo();
        assert!(buffer.is_in_code_fence(SourceLine::new(10_000)));
        buffer.redo();
        assert!(buffer.is_in_code_fence(SourceLine::new(10_000)));
        buffer.record_single_line_edit(SourceLine::new(0), |line, _| *line = "text".into());
        let lines: Vec<_> = buffer
            .source_lines()
            .map(|line| line.into_owned())
            .collect();
        let oracle = VecIndex::build(&lines);
        for row in [0, 10_000, buffer.line_count() - 1] {
            let row = SourceLine::new(row);
            assert_eq!(buffer.is_in_code_fence(row), oracle.fence_state(row).0);
        }
    }
}
