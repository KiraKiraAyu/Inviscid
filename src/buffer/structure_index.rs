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
        let projected = classify(&line[raw.quote_prefix..]);
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
                && lines.peek().is_some_and(|next| {
                    !next.is_boundary
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
}

impl Item for StructureLine {
    type Summary = StructureSummary;

    fn summary(&self, (): ()) -> StructureSummary {
        let parser = if self.is_boundary {
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
        };
        Ord::cmp(&self.target, &count)
    }
}

/// Prefix-only O(log N) index of fenced code blocks, math blocks, quote prefixes,
/// and blank separator runs. Opening delimiters take effect optimistically without lookahead.
#[derive(Clone, Debug)]
pub(crate) struct StructureIndex {
    tree: SumTree<StructureLine>,
    #[cfg(test)]
    pub(super) classified_lines: usize,
}

impl StructureIndex {
    pub(crate) fn new(source: &(impl LineSource + ?Sized)) -> Self {
        Self {
            tree: SumTree::from_iter(
                StructureLine::runs(source.source_lines().map(|line| StructureLine::new(&line))),
                (),
            ),
            #[cfg(test)]
            classified_lines: source.line_count(),
        }
    }

    pub(crate) fn len(&self) -> usize {
        self.tree.summary().lines
    }

    pub(crate) fn splice(
        &mut self,
        old: Range<usize>,
        source: &(impl LineSource + ?Sized),
        new: Range<usize>,
    ) {
        let mut cursor = self.tree.cursor::<StructureSummary>(());
        let mut tree = cursor.slice(&Lines(old.start), Bias::Right);
        if let Some(item) = cursor.item() {
            let count = old.start - cursor.start().lines;
            if count > 0 {
                tree.push(StructureLine { count, ..*item }, ());
            }
        }
        cursor.seek_forward(&Lines(old.end), Bias::Right);
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
        tree.extend(
            StructureLine::runs(
                new.clone()
                    .map(|line| StructureLine::new(&source.source_line(SourceLine::new(line)))),
            ),
            (),
        );
        tree.extend(tail, ());
        tree.append(cursor.suffix(), ());
        drop(cursor);
        self.tree = tree;
        #[cfg(test)]
        {
            self.classified_lines += new.len();
        }
    }

    fn seek_counter(&self, kind: PrefixCounter, ordinal: usize) -> SourceLine {
        let mut cursor = self.tree.cursor::<StructureSummary>(());
        cursor.seek(
            &PrefixSeek {
                kind,
                target: ordinal,
            },
            Bias::Right,
        );
        SourceLine::new(cursor.start().lines)
    }

    pub(crate) fn fence_range(&self, line: SourceLine) -> Option<(SourceLine, Option<SourceLine>)> {
        if line.index() >= self.len() {
            return None;
        }
        let mut cursor = self.tree.cursor::<StructureSummary>(());
        cursor.seek(&Lines(line.index()), Bias::Right);
        let before = cursor.start().parser.0[BlockState::Outside as usize];
        let after = cursor.end().parser.0[BlockState::Outside as usize];
        let total = self.tree.summary().parser.0[BlockState::Outside as usize];
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
        let mut cursor = self.tree.cursor::<StructureSummary>(());
        cursor.seek(&Lines(line.index()), Bias::Right);
        let before = cursor.start().parser.0[BlockState::Outside as usize];
        let after = cursor.end().parser.0[BlockState::Outside as usize];
        let total = self.tree.summary().parser.0[BlockState::Outside as usize];
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
        let mut cursor = self.tree.cursor::<StructureSummary>(());
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
        let mut cursor = self.tree.cursor::<StructureSummary>(());
        cursor.seek(&Lines(line.index()), Bias::Right);
        cursor.end().parser.0[BlockState::Outside as usize]
            .exit
            .is_code()
    }

    pub(crate) fn blank_range(&self, row: usize) -> Option<Range<usize>> {
        if row >= self.len() {
            return None;
        }
        let mut cursor = self.tree.cursor::<StructureSummary>(());
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

    /// Forward prefix-scan reference for optimistic blocks, independent of SumTree.
    struct VecIndex<'a> {
        lines: &'a [String],
        fences: Vec<Option<(SourceLine, Option<SourceLine>)>>,
        maths: Vec<Option<(SourceLine, Option<SourceLine>)>>,
        quote_prefixes: Vec<usize>,
    }

    impl<'a> VecIndex<'a> {
        fn build(lines: &'a [String]) -> Self {
            let mut state = BlockState::Outside;
            let mut fences = vec![None; lines.len()];
            let mut maths = vec![None; lines.len()];
            let mut quote_prefixes = vec![0; lines.len()];
            let mut active_open: Option<usize> = None;

            for (i, text) in lines.iter().enumerate() {
                let raw = classify(text);
                let projected = classify(&text[raw.quote_prefix..]);
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
                let open_line = SourceLine::new(open);
                if state.is_code() {
                    for slot in &mut fences[open..] {
                        *slot = Some((open_line, None));
                    }
                } else if state.is_math() {
                    for slot in &mut maths[open..] {
                        *slot = Some((open_line, None));
                    }
                }
            }

            Self {
                lines,
                fences,
                maths,
                quote_prefixes,
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
    }

    pub(in crate::buffer) fn assert_matches(index: &StructureIndex, lines: &[String]) {
        let oracle = VecIndex::build(lines);
        for row in 0..=lines.len() + 1 {
            let line = SourceLine::new(row);
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
            lines.splice(start..start + removed, replacement);
            index.splice(start..start + removed, &lines, start..start + added);
            assert_matches(&index, &lines);
        }
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
