use super::{LineSource, SourceLine};
use crate::markdown::structure::{LineStructure, classify};
use gpui_sum_tree::{Bias, ContextLessSummary, Item, SeekTarget, SumTree};
use std::{cmp::Ordering, ops::Range};

#[derive(Clone, Debug)]
struct StructureLine {
    raw: LineStructure,
    projected: LineStructure,
}

impl StructureLine {
    fn new(line: &str) -> Self {
        let raw = classify(line);
        Self {
            projected: classify(&line[raw.quote_prefix..]),
            raw,
        }
    }
}

#[derive(Clone, Copy, Debug)]
struct Effect {
    state: usize,
    math_delims: usize,
}

/// A segment's effect for each incoming (raw fence, projected fence) parity.
#[derive(Clone, Debug)]
struct MathSummary([Effect; 4]);

impl Default for MathSummary {
    fn default() -> Self {
        Self(std::array::from_fn(|state| Effect {
            state,
            math_delims: 0,
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
            }
        }))
    }

    fn append(&mut self, other: &Self) {
        for effect in &mut self.0 {
            let next = other.0[effect.state];
            effect.state = next.state;
            effect.math_delims += next.math_delims;
        }
    }
}

#[derive(Clone, Debug, Default)]
struct StructureSummary {
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
            lines: 1,
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
pub(super) struct StructureIndex {
    tree: SumTree<StructureLine>,
    #[cfg(test)]
    pub(super) classified_lines: usize,
}

impl StructureIndex {
    pub(super) fn new(source: &(impl LineSource + ?Sized)) -> Self {
        Self {
            tree: SumTree::from_iter(
                source.source_lines().map(|line| StructureLine::new(&line)),
                (),
            ),
            #[cfg(test)]
            classified_lines: source.line_count(),
        }
    }

    pub(super) fn len(&self) -> usize {
        self.tree.summary().lines
    }

    pub(super) fn splice(
        &mut self,
        old: Range<usize>,
        source: &(impl LineSource + ?Sized),
        new: Range<usize>,
    ) {
        let mut cursor = self.tree.cursor::<StructureSummary>(());
        let mut tree = cursor.slice(&Lines(old.start), Bias::Right);
        cursor.seek_forward(&Lines(old.end), Bias::Right);
        tree.extend(
            new.clone()
                .map(|line| StructureLine::new(&source.source_line(SourceLine::new(line)))),
            (),
        );
        tree.append(cursor.suffix(), ());
        drop(cursor);
        self.tree = tree;
        #[cfg(test)]
        {
            self.classified_lines += new.len();
        }
    }

    fn math_count(&self, prefix: &StructureSummary) -> usize {
        let fences = self.tree.summary().fences;
        if fences % 2 == 1 && prefix.fences == fences {
            let before = prefix.through_last_fence.0[0];
            before.math_delims + prefix.after_last_fence.0[before.state].math_delims
        } else {
            prefix.normal.0[0].math_delims
        }
    }

    fn fence_line(&self, ordinal: usize) -> usize {
        let mut cursor = self.tree.cursor::<StructureSummary>(());
        cursor.seek(&Fences(ordinal), Bias::Right);
        cursor.start().lines
    }

    fn math_line(&self, ordinal: usize) -> usize {
        let mut cursor = self.tree.cursor::<StructureSummary>(());
        cursor.seek(
            &MathDelims {
                count: ordinal,
                index: self,
            },
            Bias::Right,
        );
        cursor.start().lines
    }

    pub(super) fn fence_state(&self, line: SourceLine) -> (bool, bool) {
        if line.index() >= self.len() {
            return (false, false);
        }
        let mut cursor = self.tree.cursor::<StructureSummary>(());
        cursor.seek(&Lines(line.index()), Bias::Right);
        let before = cursor.start().fences;
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

    pub(super) fn math_pair(&self, line: SourceLine) -> Option<(SourceLine, SourceLine)> {
        if line.index() >= self.len() {
            return None;
        }
        let mut cursor = self.tree.cursor::<StructureSummary>(());
        cursor.seek(&Lines(line.index()), Bias::Right);
        let before = self.math_count(cursor.start());
        let is_delim = self.math_count(&cursor.end()) > before;
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
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::buffer::{LineSource, SourceLine, TextBuffer};

    /// The former LineIndex build/query implementation, retained as an oracle.
    struct VecIndex {
        fence_pairs: Vec<(SourceLine, SourceLine)>,
        math_pairs: Vec<(SourceLine, SourceLine)>,
    }

    impl VecIndex {
        fn build(lines: &[String]) -> Self {
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
            Self {
                fence_pairs,
                math_pairs: crate::markdown::math_block_pairs(lines),
            }
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
            self.math_pairs
                .iter()
                .copied()
                .find(|&(start, end)| (start..=end).contains(&target))
        }

        fn is_math_delimiter(&self, target: SourceLine) -> bool {
            self.math_pairs
                .iter()
                .any(|&(start, end)| target == start || target == end)
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
