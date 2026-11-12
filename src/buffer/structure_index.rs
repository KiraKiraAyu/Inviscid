use super::{LineSource, SourceLine};
use crate::markdown::structure::{LineStructure, classify};
use gpui_sum_tree::{Bias, ContextLessSummary, Dimension, Item, SeekTarget, SumTree};
use std::sync::Arc;
use std::{cmp::Ordering, ops::Range};

#[derive(Clone, Debug)]
struct StructureLine {
    count: usize,
    raw: LineStructure,
    projected: LineStructure,
    parser: Option<Arc<super::parser_summary::ParserEffects>>,
}

impl StructureLine {
    fn new(line: &str) -> Self {
        let raw = classify(line);
        let projected = classify(&line[raw.quote_prefix..]);
        Self {
            count: 1,
            projected,
            raw,
            parser: (raw.is_fence
                || raw.is_math_delim
                || projected.is_fence
                || projected.is_math_delim)
                .then(|| Arc::new(super::parser_summary::ParserEffects::line(raw, projected))),
        }
    }

    fn runs(lines: impl Iterator<Item = Self>) -> impl Iterator<Item = Self> {
        let mut lines = lines.peekable();
        std::iter::from_fn(move || {
            let mut run = lines.next()?;
            while run.parser.is_none()
                && lines.peek().is_some_and(|next| {
                    next.parser.is_none() && next.raw == run.raw && next.projected == run.projected
                })
            {
                run.count += lines.next().unwrap().count;
            }
            Some(run)
        })
    }
}

#[derive(Clone, Copy, Debug)]
struct Effect {
    state: usize,
    math_delims: usize,
    code_fences: usize,
}

/// A segment's effect for each incoming (raw fence, projected fence) parity.
#[derive(Clone, Debug)]
struct MathSummary([Effect; 4]);

impl Default for MathSummary {
    fn default() -> Self {
        Self(std::array::from_fn(|state| Effect {
            state,
            math_delims: 0,
            code_fences: 0,
        }))
    }
}

impl MathSummary {
    fn line(line: &StructureLine, project_inside_raw_fence: bool) -> Self {
        Self(std::array::from_fn(|state| {
            let raw_fence = state & 2 != 0;
            let in_code = state & 1 != 0;
            let syntax = if !raw_fence || project_inside_raw_fence {
                line.projected
            } else {
                line.raw
            };
            Effect {
                state: state ^ (usize::from(line.raw.is_fence) * 2) ^ usize::from(syntax.is_fence),
                math_delims: usize::from(!in_code && !syntax.is_fence && syntax.is_math_delim),
                code_fences: usize::from(syntax.is_fence),
            }
        }))
    }

    fn append(&mut self, other: &Self) {
        for effect in &mut self.0 {
            let next = other.0[effect.state];
            effect.state = next.state;
            effect.math_delims += next.math_delims;
            effect.code_fences += next.code_fences;
        }
    }
}

/// Last nonblank position plus one, with zero denoting an entirely blank segment.
#[derive(Clone, Debug, Default)]
struct BlankSummary {
    normal: [usize; 2],
    projected: usize,
    through_last_fence: [usize; 2],
    after_last_fence: usize,
}

impl BlankSummary {
    fn line(line: &StructureLine) -> Self {
        let normal = [
            usize::from(!line.projected.is_blank) * line.count,
            usize::from(!line.raw.is_blank) * line.count,
        ];
        let projected = normal[0];
        Self {
            normal,
            projected,
            through_last_fence: if line.raw.is_fence { normal } else { [0; 2] },
            after_last_fence: if line.raw.is_fence { 0 } else { projected },
        }
    }

    fn append(&mut self, other: &Self, lines: usize, fences: usize, other_fences: usize) {
        let extend = |before, after| if after == 0 { before } else { lines + after };
        if other_fences > 0 {
            self.through_last_fence = std::array::from_fn(|state| {
                extend(
                    self.normal[state],
                    other.through_last_fence[state ^ (fences % 2)],
                )
            });
            self.after_last_fence = if other.after_last_fence == 0 {
                0
            } else {
                lines + other.after_last_fence
            };
        } else {
            self.after_last_fence = extend(self.after_last_fence, other.projected);
        }
        self.normal = std::array::from_fn(|state| {
            extend(self.normal[state], other.normal[state ^ (fences % 2)])
        });
        self.projected = extend(self.projected, other.projected);
    }
}

#[derive(Clone, Debug, Default)]
struct StructureSummary {
    blank: BlankSummary,
    parser: Option<Arc<super::parser_summary::ParserEffects>>,
    lines: usize,
    fences: usize,
    normal: MathSummary,
    projected: MathSummary,
    // An unmatched final raw fence does not suppress quote projection. Keeping
    // the split at the last raw fence lets prefix queries apply this exception.
    through_last_fence: MathSummary,
    after_last_fence: MathSummary,
}

impl Item for StructureLine {
    type Summary = StructureSummary;

    fn summary(&self, (): ()) -> StructureSummary {
        let normal = MathSummary::line(self, false);
        let projected = MathSummary::line(self, true);
        StructureSummary {
            blank: BlankSummary::line(self),
            parser: self.parser.clone(),
            lines: self.count,
            fences: usize::from(self.raw.is_fence),
            through_last_fence: if self.raw.is_fence {
                normal.clone()
            } else {
                MathSummary::default()
            },
            after_last_fence: if self.raw.is_fence {
                MathSummary::default()
            } else {
                projected.clone()
            },
            normal,
            projected,
        }
    }
}

impl ContextLessSummary for StructureSummary {
    fn zero() -> Self {
        Self::default()
    }

    fn add_summary(&mut self, other: &Self) {
        if let Some(other) = &other.parser {
            if let Some(parser) = &mut self.parser {
                Arc::make_mut(parser).append(other);
            } else {
                self.parser = Some(other.clone());
            }
        }
        self.add_navigation(other);
    }
}

impl StructureSummary {
    fn add_navigation(&mut self, other: &Self) {
        self.blank
            .append(&other.blank, self.lines, self.fences, other.fences);
        if other.fences > 0 {
            self.through_last_fence = self.normal.clone();
            self.through_last_fence.append(&other.through_last_fence);
            self.after_last_fence = other.after_last_fence.clone();
        } else {
            self.after_last_fence.append(&other.projected);
        }
        self.lines += other.lines;
        self.fences += other.fences;
        self.normal.append(&other.normal);
        self.projected.append(&other.projected);
    }
}

struct Lines(usize);
#[derive(Clone, Default)]
struct Navigation(StructureSummary);

impl<'a> Dimension<'a, StructureSummary> for Navigation {
    fn zero((): ()) -> Self {
        Self::default()
    }
    fn add_summary(&mut self, summary: &'a StructureSummary, (): ()) {
        self.0.add_navigation(summary);
    }
}

impl<'a> SeekTarget<'a, StructureSummary, Navigation> for Lines {
    fn cmp(&self, location: &Navigation, (): ()) -> Ordering {
        Ord::cmp(&self.0, &location.0.lines)
    }
}

impl<'a> SeekTarget<'a, StructureSummary, Navigation> for Fences {
    fn cmp(&self, location: &Navigation, (): ()) -> Ordering {
        Ord::cmp(&self.0, &location.0.fences)
    }
}

impl<'a> SeekTarget<'a, StructureSummary, Navigation> for MathDelims<'_> {
    fn cmp(&self, location: &Navigation, (): ()) -> Ordering {
        Ord::cmp(&self.count, &self.index.math_count(&location.0))
    }
}
struct Fences(usize);
struct MathDelims<'a> {
    count: usize,
    index: &'a StructureIndex,
}

impl<'a> SeekTarget<'a, StructureSummary, StructureSummary> for Lines {
    fn cmp(&self, location: &StructureSummary, (): ()) -> Ordering {
        Ord::cmp(&self.0, &location.lines)
    }
}

impl<'a> SeekTarget<'a, StructureSummary, StructureSummary> for Fences {
    fn cmp(&self, location: &StructureSummary, (): ()) -> Ordering {
        Ord::cmp(&self.0, &location.fences)
    }
}

impl<'a> SeekTarget<'a, StructureSummary, StructureSummary> for MathDelims<'_> {
    fn cmp(&self, location: &StructureSummary, (): ()) -> Ordering {
        Ord::cmp(&self.count, &self.index.math_count(location))
    }
}

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
                tree.push(
                    StructureLine {
                        count,
                        ..item.clone()
                    },
                    (),
                );
            }
        }
        cursor.seek_forward(&Lines(old.end), Bias::Right);
        let tail = cursor.item().and_then(|item| {
            let skipped = old.end - cursor.start().lines;
            (skipped > 0).then(|| StructureLine {
                count: item.count - skipped,
                ..item.clone()
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

    fn math_count(&self, prefix: &StructureSummary) -> usize {
        self.effective(prefix).math_delims
    }

    fn effective(&self, prefix: &StructureSummary) -> Effect {
        prefix.normal.0[0]
    }

    fn fence_line(&self, ordinal: usize) -> usize {
        let mut cursor = self.tree.cursor::<Navigation>(());
        cursor.seek(&Fences(ordinal), Bias::Right);
        cursor.start().0.lines
    }

    fn math_line(&self, ordinal: usize) -> usize {
        let mut cursor = self.tree.cursor::<Navigation>(());
        cursor.seek(
            &MathDelims {
                count: ordinal,
                index: self,
            },
            Bias::Right,
        );
        cursor.start().0.lines
    }

    pub(super) fn has_fence_after(&self, line: SourceLine) -> bool {
        let count = self.tree.summary().fences;
        count > 0 && self.fence_line(count - 1) > line.index()
    }

    pub(crate) fn fence_state(&self, line: SourceLine) -> (bool, bool) {
        if line.index() >= self.len() {
            return (false, false);
        }
        let mut cursor = self.tree.cursor::<Navigation>(());
        cursor.seek(&Lines(line.index()), Bias::Right);
        let before = cursor.start().0.fences;
        let is_fence = cursor.item().unwrap().raw.is_fence;
        let pair = before / 2 * 2;
        if pair + 1 >= self.tree.summary().fences {
            return (false, false);
        }
        if is_fence && self.fence_line(pair + 1) == line.index() {
            (false, true)
        } else {
            (is_fence || before % 2 == 1, false)
        }
    }

    /// A code opener takes effect immediately, including before a closing fence arrives.
    pub(crate) fn streaming_fence(&self, line: SourceLine) -> Option<(usize, Option<usize>)> {
        let mut cursor = self.tree.cursor::<Navigation>(());
        cursor.seek(&Lines(line.index()), Bias::Right);
        let item = cursor.item()?;
        let before = cursor.start().0.fences;
        if !item.raw.is_fence && before % 2 == 0 {
            return self
                .projected_fence_pair(line)
                .map(|(start, end)| (start.index(), Some(end.index())));
        }
        let pair = before / 2 * 2;
        Some((
            self.fence_line(pair),
            (pair + 1 < self.tree.summary().fences).then(|| self.fence_line(pair + 1)),
        ))
    }

    pub(crate) fn math_pair(&self, line: SourceLine) -> Option<(SourceLine, SourceLine)> {
        if line.index() >= self.len() {
            return None;
        }
        let mut cursor = self.tree.cursor::<Navigation>(());
        cursor.seek(&Lines(line.index()), Bias::Right);
        let before = self.math_count(&cursor.start().0);
        let is_delim = self.math_count(&cursor.end().0) > before;
        if !is_delim && before % 2 == 0 {
            return None;
        }
        let pair = before / 2 * 2;
        if pair + 1 >= self.math_count(self.tree.summary()) {
            return None;
        }
        Some((
            SourceLine::new(self.math_line(pair)),
            SourceLine::new(self.math_line(pair + 1)),
        ))
    }

    pub(super) fn is_math_delimiter(&self, line: SourceLine) -> bool {
        self.math_pair(line)
            .is_some_and(|(start, end)| line == start || line == end)
    }

    pub(crate) fn quote_prefix(&self, line: SourceLine) -> usize {
        let mut cursor = self.tree.cursor::<Navigation>(());
        cursor.seek(&Lines(line.index()), Bias::Right);
        cursor.item().map_or(0, |item| {
            if cursor.start().0.fences % 2 == 1 || item.raw.is_fence {
                0
            } else {
                item.raw.quote_prefix
            }
        })
    }

    pub(crate) fn projected_fence_open_after(&self, line: SourceLine) -> bool {
        let mut cursor = self.tree.cursor::<Navigation>(());
        cursor.seek(&Lines(line.index()), Bias::Right);
        self.effective(&cursor.end().0).code_fences % 2 == 1
    }

    pub(crate) fn blank_range(&self, row: usize) -> Option<Range<usize>> {
        let mut cursor = self.tree.cursor::<Navigation>(());
        cursor.seek(&Lines(row), Bias::Right);
        cursor.item()?;
        let start = self.last_non_blank(&cursor.end().0);
        if start > row {
            return None;
        }
        cursor.seek(
            &NonBlank {
                count: row + 1,
                index: self,
            },
            Bias::Right,
        );
        Some(start..cursor.start().0.lines)
    }

    fn last_non_blank(&self, prefix: &StructureSummary) -> usize {
        prefix.blank.normal[0]
    }

    fn segment_parser(
        &self,
        summary: &StructureSummary,
        raw_before: usize,
    ) -> super::parser_summary::ParserSummary {
        let Some(parser) = &summary.parser else {
            return Default::default();
        };
        parser.normal[raw_before % 2].clone()
    }

    fn parser_effect(&self, prefix: &StructureSummary) -> super::parser_summary::ParseEffect {
        let before = self.segment_parser(prefix, 0);
        before.with_suffix(self.segment_lookahead(prefix.lines..self.len()))
    }

    fn segment_lookahead(&self, range: Range<usize>) -> super::parser_summary::Lookahead {
        let mut cursor = self.tree.cursor::<Navigation>(());
        cursor.seek(&Lines(range.start), Bias::Right);
        let raw_before = cursor.start().0.fences;
        let suffix: Navigation = cursor.summary(&Lines(range.end), Bias::Right);
        let summary = suffix.0;
        let effects: [Effect; 2] = std::array::from_fn(|code| {
            let state = raw_before % 2 * 2 + code;
            summary.normal.0[state]
        });
        super::parser_summary::Lookahead::new(
            effects[0].code_fences,
            effects.map(|effect| effect.math_delims > 0),
        )
    }

    pub(crate) fn parser_pair(&self, line: SourceLine) -> Option<(SourceLine, SourceLine)> {
        self.tree.summary().parser.as_ref()?;
        let mut cursor = self.tree.cursor::<StructureSummary>(());
        cursor.seek(&Lines(line.index()), Bias::Right);
        cursor.item()?;
        let before = self.parser_effect(cursor.start());
        let after = self.parser_effect(&cursor.end());
        let ordinal = if before.starts > before.ends {
            before.starts - 1
        } else if after.starts > before.starts {
            before.starts
        } else {
            return None;
        };
        let locate = |closing| {
            let mut cursor = self.tree.cursor::<StructureSummary>(());
            cursor.seek(
                &ParserBoundary {
                    ordinal,
                    closing,
                    index: self,
                },
                Bias::Right,
            );
            SourceLine::new(cursor.start().lines.min(self.len().saturating_sub(1)))
        };
        Some((locate(false), locate(true)))
    }

    pub(crate) fn projected_fence_pair(
        &self,
        line: SourceLine,
    ) -> Option<(SourceLine, SourceLine)> {
        let mut cursor = self.tree.cursor::<Navigation>(());
        cursor.seek(&Lines(line.index()), Bias::Right);
        cursor.item()?;
        let before = self.effective(&cursor.start().0).code_fences;
        let delimiter = self.effective(&cursor.end().0).code_fences > before;
        if !delimiter && before % 2 == 0 {
            return None;
        }
        let pair = before / 2 * 2;
        if pair + 1 >= self.effective(self.tree.summary()).code_fences {
            return None;
        }
        let locate = |ordinal| {
            let mut cursor = self.tree.cursor::<Navigation>(());
            cursor.seek(
                &ProjectedFences {
                    ordinal,
                    index: self,
                },
                Bias::Right,
            );
            SourceLine::new(cursor.start().0.lines)
        };
        Some((locate(pair), locate(pair + 1)))
    }

    pub(crate) fn math_lookahead_end(&self, from: usize) -> Option<usize> {
        if !self.segment_lookahead(from..self.len()).has_math() {
            return None;
        }
        let mut cursor = self.tree.cursor::<Navigation>(());
        cursor.seek(&MathLookahead { from, index: self }, Bias::Right);
        Some(cursor.start().0.lines)
    }
}

struct ProjectedFences<'a> {
    ordinal: usize,
    index: &'a StructureIndex,
}

struct NonBlank<'a> {
    count: usize,
    index: &'a StructureIndex,
}
impl<'a> SeekTarget<'a, StructureSummary, Navigation> for NonBlank<'_> {
    fn cmp(&self, location: &Navigation, (): ()) -> Ordering {
        Ord::cmp(&self.count, &self.index.last_non_blank(&location.0))
    }
}
impl<'a> SeekTarget<'a, StructureSummary, Navigation> for ProjectedFences<'_> {
    fn cmp(&self, location: &Navigation, (): ()) -> Ordering {
        Ord::cmp(
            &self.ordinal,
            &self.index.effective(&location.0).code_fences,
        )
    }
}

struct MathLookahead<'a> {
    from: usize,
    index: &'a StructureIndex,
}

impl<'a> SeekTarget<'a, StructureSummary, Navigation> for MathLookahead<'_> {
    fn cmp(&self, location: &Navigation, (): ()) -> Ordering {
        let found = location.0.lines > self.from
            && self
                .index
                .segment_lookahead(self.from..location.0.lines)
                .has_math();
        Ord::cmp(&0, &usize::from(found))
    }
}

struct ParserBoundary<'a> {
    ordinal: usize,
    closing: bool,
    index: &'a StructureIndex,
}

impl<'a> SeekTarget<'a, StructureSummary, StructureSummary> for ParserBoundary<'_> {
    fn cmp(&self, location: &StructureSummary, (): ()) -> Ordering {
        let effect = self.index.parser_effect(location);
        Ord::cmp(
            &self.ordinal,
            &if self.closing {
                effect.ends
            } else {
                effect.starts
            },
        )
    }
}

#[cfg(test)]
mod tests {
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
            assert_eq!(index.fence_state(line), expected.fence_state(line));
            assert_eq!(index.parser_pair(line), expected.parser_pair(line));
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

    #[test]
    fn parser_queries_reuse_prefix_summaries_and_cheap_lookahead() {
        let rows: Vec<String> = "```text\ncode\n```\n$$\nx\n$$\n"
            .repeat(1000)
            .lines()
            .map(str::to_owned)
            .collect();
        let index = StructureIndex::new(&rows);
        super::super::parser_summary::COMPOSITIONS.set(0);
        for row in 3000..3010 {
            index.parser_pair(SourceLine::new(row));
        }
        assert!(
            super::super::parser_summary::COMPOSITIONS.get() < 5000,
            "{} compositions",
            super::super::parser_summary::COMPOSITIONS.get()
        );
    }

    #[test]
    fn navigation_queries_do_not_compose_parser_transitions() {
        let lines: Vec<String> = "```\nx\n```\n$$\nx\n$$\n"
            .repeat(20)
            .split('\n')
            .map(str::to_owned)
            .collect();
        let index = StructureIndex::new(&lines);
        super::super::parser_summary::COMPOSITIONS.set(0);
        for row in 0..lines.len() {
            index.fence_state(SourceLine::new(row));
            index.math_pair(SourceLine::new(row));
        }
        assert_eq!(super::super::parser_summary::COMPOSITIONS.get(), 0);
    }

    /// Full-scan reference for navigation queries, independent of tree summaries.
    struct VecIndex<'a> {
        fence_pairs: Vec<(SourceLine, SourceLine)>,
        lines: &'a [String],
    }

    impl<'a> VecIndex<'a> {
        fn build(lines: &'a [String]) -> Self {
            let mut fence_pairs = Vec::new();
            let mut open = None;
            for (i, line) in lines.iter().enumerate() {
                if line.trim_start().starts_with("```") {
                    match open {
                        Some(open_idx) => {
                            fence_pairs.push((SourceLine::new(open_idx), SourceLine::new(i)));
                            open = None;
                        }
                        None => open = Some(i),
                    }
                }
            }
            Self { fence_pairs, lines }
        }

        fn fence_state(&self, target: SourceLine) -> (bool, bool) {
            for &(open, close) in &self.fence_pairs {
                if target == open {
                    return (true, false);
                } else if target == close {
                    return (false, true);
                } else if target > open && target < close {
                    return (true, false);
                }
            }
            (false, false)
        }

        fn math_pair(&self, target: SourceLine) -> Option<(SourceLine, SourceLine)> {
            crate::markdown::math_block_range(self.lines, target)
        }

        fn is_math_delimiter(&self, target: SourceLine) -> bool {
            self.math_pair(target)
                .is_some_and(|(start, end)| target == start || target == end)
        }
    }

    fn assert_matches(index: &StructureIndex, lines: &[String]) {
        let oracle = VecIndex::build(lines);
        for row in 0..=lines.len() + 1 {
            let line = SourceLine::new(row);
            assert_eq!(
                index.fence_state(line),
                oracle.fence_state(line),
                "fence at {row}: {lines:?}"
            );
            assert_eq!(
                index.math_pair(line),
                oracle.math_pair(line),
                "math at {row}: {lines:?}"
            );
            assert_eq!(
                index.is_math_delimiter(line),
                oracle.is_math_delimiter(line)
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
