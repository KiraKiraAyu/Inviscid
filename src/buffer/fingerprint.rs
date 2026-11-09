use ropey::{Rope, RopeSlice};
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::sync::Arc;

// Leaves amortize indexing overhead over many lines. Branch summaries compose
// in sequence order, so balancing or splitting never changes a document hash.
const BLOCK_SIZE: usize = 64;
const HASH_BASE: u64 = 0x9e37_79b1_85eb_ca87;
type Link = Option<Arc<Node>>;

#[derive(Clone, Debug)]
pub(super) enum FingerprintState {
    Pending { baseline: Option<Rope> },
    Ready(LineFingerprints),
}

/// Immutable, GPUI-independent work that may be run on a background executor.
pub struct FingerprintJob {
    rope: Rope,
    baseline: Option<Rope>,
    revision: u64,
    baseline_revision: u64,
}

pub struct FingerprintResult {
    fingerprints: LineFingerprints,
    baseline_hash: Option<u64>,
    revision: u64,
    baseline_revision: u64,
}

impl FingerprintJob {
    pub fn run(self) -> FingerprintResult {
        let fingerprints = LineFingerprints::new(&self.rope);
        let baseline_hash = self.baseline.map(|baseline| {
            if self.revision == self.baseline_revision {
                fingerprints.content_hash()
            } else {
                LineFingerprints::new(&baseline).content_hash()
            }
        });
        FingerprintResult {
            fingerprints,
            baseline_hash,
            revision: self.revision,
            baseline_revision: self.baseline_revision,
        }
    }
}

impl super::TextBuffer {
    pub fn fingerprint_job(&self) -> Option<FingerprintJob> {
        match &self.line_fingerprints {
            FingerprintState::Pending { baseline } => Some(FingerprintJob {
                rope: self.rope.clone(),
                baseline: baseline.clone(),
                revision: self.revision,
                baseline_revision: self.saved_baseline.revision,
            }),
            FingerprintState::Ready(_) => None,
        }
    }

    pub fn install_fingerprints(&mut self, result: FingerprintResult, revision: u64) -> bool {
        if self.revision != revision
            || result.revision != revision
            || result.baseline_revision != self.saved_baseline.revision
            || !matches!(self.line_fingerprints, FingerprintState::Pending { .. })
        {
            return false;
        }
        if let Some(hash) = result.baseline_hash {
            self.saved_baseline.content_hash = hash;
        }
        self.line_fingerprints = FingerprintState::Ready(result.fingerprints);
        self.update_dirty_state();
        true
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Summary {
    hash: u64,
    power: u64,
    len: usize,
}

impl Default for Summary {
    fn default() -> Self {
        Self {
            hash: 0,
            power: 1,
            len: 0,
        }
    }
}

impl Summary {
    fn then(self, right: Self) -> Self {
        Self {
            hash: self.hash.wrapping_mul(right.power).wrapping_add(right.hash),
            power: self.power.wrapping_mul(right.power),
            len: self.len + right.len,
        }
    }

    fn from_hashes(hashes: impl IntoIterator<Item = u64>) -> Self {
        hashes.into_iter().fold(Self::default(), |summary, hash| {
            summary.then(Self {
                hash,
                power: HASH_BASE,
                len: 1,
            })
        })
    }

    fn content_hash(self) -> u64 {
        let mut hasher = DefaultHasher::new();
        self.len.hash(&mut hasher);
        self.hash.hash(&mut hasher);
        hasher.finish()
    }
}

#[derive(Clone, Debug)]
enum NodeKind {
    Leaf(Vec<u64>),
    Branch(Arc<Node>, Arc<Node>),
}

#[derive(Clone, Debug)]
struct Node {
    summary: Summary,
    height: u16,
    kind: NodeKind,
}

impl Node {
    fn leaf(hashes: Vec<u64>) -> Arc<Self> {
        debug_assert!(!hashes.is_empty() && hashes.len() <= BLOCK_SIZE);
        Arc::new(Self {
            summary: Summary::from_hashes(hashes.iter().copied()),
            height: 1,
            kind: NodeKind::Leaf(hashes),
        })
    }

    fn branch(left: Arc<Self>, right: Arc<Self>) -> Arc<Self> {
        Arc::new(Self {
            summary: left.summary.then(right.summary),
            height: left.height.max(right.height) + 1,
            kind: NodeKind::Branch(left, right),
        })
    }

    fn children(&self) -> (&Arc<Self>, &Arc<Self>) {
        let NodeKind::Branch(left, right) = &self.kind else {
            unreachable!("an internal AVL node must have two children");
        };
        (left, right)
    }

    fn edge_hashes(&self, first: bool) -> &[u64] {
        match &self.kind {
            NodeKind::Leaf(hashes) => hashes,
            NodeKind::Branch(left, right) => if first { left } else { right }.edge_hashes(first),
        }
    }
}

fn balance(left: Arc<Node>, right: Arc<Node>) -> Arc<Node> {
    if left.height > right.height + 1 {
        let (ll, lr) = left.children();
        if ll.height >= lr.height {
            Node::branch(ll.clone(), Node::branch(lr.clone(), right))
        } else {
            let (lrl, lrr) = lr.children();
            Node::branch(
                Node::branch(ll.clone(), lrl.clone()),
                Node::branch(lrr.clone(), right),
            )
        }
    } else if right.height > left.height + 1 {
        let (rl, rr) = right.children();
        if rr.height >= rl.height {
            Node::branch(Node::branch(left, rl.clone()), rr.clone())
        } else {
            let (rll, rlr) = rl.children();
            Node::branch(
                Node::branch(left, rll.clone()),
                Node::branch(rlr.clone(), rr.clone()),
            )
        }
    } else {
        Node::branch(left, right)
    }
}

fn join(left: Link, right: Link) -> Link {
    let (left, right) = match (left, right) {
        (None, tree) | (tree, None) => return tree,
        (Some(left), Some(right)) => (left, right),
    };
    Some(if left.height > right.height + 1 {
        let (ll, lr) = left.children();
        balance(ll.clone(), join(Some(lr.clone()), Some(right)).unwrap())
    } else if right.height > left.height + 1 {
        let (rl, rr) = right.children();
        balance(join(Some(left), Some(rl.clone())).unwrap(), rr.clone())
    } else {
        Node::branch(left, right)
    })
}

fn split(root: Link, at: usize) -> (Link, Link) {
    let Some(node) = root else {
        return (None, None);
    };
    if at == 0 {
        return (None, Some(node));
    }
    if at == node.summary.len {
        return (Some(node), None);
    }
    match &node.kind {
        NodeKind::Leaf(hashes) => (
            Some(Node::leaf(hashes[..at].to_vec())),
            Some(Node::leaf(hashes[at..].to_vec())),
        ),
        NodeKind::Branch(left, right) => {
            if at < left.summary.len {
                let (before, after) = split(Some(left.clone()), at);
                (before, join(after, Some(right.clone())))
            } else {
                let (before, after) = split(Some(right.clone()), at - left.summary.len);
                (join(Some(left.clone()), before), after)
            }
        }
    }
}

fn concat(left: Link, right: Link) -> Link {
    if let (Some(l), Some(r)) = (&left, &right) {
        let last = l.edge_hashes(false);
        let first = r.edge_hashes(true);
        if last.len() + first.len() <= BLOCK_SIZE {
            // Compact split boundary leaves so repeated small splices do not
            // gradually degrade into one heap allocation per document line.
            let hashes = last.iter().chain(first).copied().collect();
            let left_end = l.summary.len - last.len();
            let right_start = first.len();
            let (before, _) = split(left, left_end);
            let (_, after) = split(right, right_start);
            return join(join(before, Some(Node::leaf(hashes))), after);
        }
    }
    join(left, right)
}

fn build(hashes: impl IntoIterator<Item = u64>) -> Link {
    fn branches(leaves: &[Arc<Node>]) -> Arc<Node> {
        if leaves.len() == 1 {
            return leaves[0].clone();
        }
        let (left, right) = leaves.split_at(leaves.len() / 2);
        Node::branch(branches(left), branches(right))
    }
    let mut hashes = hashes.into_iter();
    let mut leaves = Vec::new();
    loop {
        let block: Vec<_> = hashes.by_ref().take(BLOCK_SIZE).collect();
        if block.is_empty() {
            break;
        }
        leaves.push(Node::leaf(block));
    }
    (!leaves.is_empty()).then(|| branches(&leaves))
}

fn replace(node: &mut Arc<Node>, start: usize, hashes: &[u64]) {
    if hashes.is_empty() {
        return;
    }
    let node = Arc::make_mut(node);
    match &mut node.kind {
        NodeKind::Leaf(block) => {
            block[start..start + hashes.len()].copy_from_slice(hashes);
            node.summary = Summary::from_hashes(block.iter().copied());
        }
        NodeKind::Branch(left, right) => {
            let left_count = left.summary.len.saturating_sub(start).min(hashes.len());
            if left_count > 0 {
                replace(left, start, &hashes[..left_count]);
            }
            if left_count < hashes.len() {
                replace(
                    right,
                    start.saturating_sub(left.summary.len),
                    &hashes[left_count..],
                );
            }
            node.summary = left.summary.then(right.summary);
        }
    }
}

fn splice(mut root: Link, start: usize, removed: usize, hashes: &[u64]) -> Link {
    if removed == hashes.len() {
        if let Some(node) = &mut root {
            replace(node, start, hashes);
        }
        return root;
    }
    let (before, rest) = split(root, start);
    let (_, after) = split(rest, removed);
    concat(concat(before, build(hashes.iter().copied())), after)
}

fn hash_line(line: RopeSlice<'_>) -> u64 {
    let end = line.len_chars()
        - usize::from(line.len_chars() > 0 && line.char(line.len_chars() - 1) == '\n');
    let mut hasher = DefaultHasher::new();
    for chunk in line.slice(..end).chunks() {
        hasher.write(chunk.as_bytes());
    }
    hasher.write_u8(0xff);
    hasher.finish()
}

pub(super) fn hash_lines(lines: &[String]) -> u64 {
    Summary::from_hashes(lines.iter().map(|line| {
        let mut hasher = DefaultHasher::new();
        line.hash(&mut hasher);
        hasher.finish()
    }))
    .content_hash()
}

/// A persistent AVL index of logical-line hashes. Text remains in the Rope;
/// each index leaf holds up to 64 hashes, and snapshots share unchanged nodes.
/// Editing rehashes affected lines and updates logarithmic tree paths; reading
/// the document fingerprint is constant-time. The index uses space proportional
/// to the number of lines, and editing a long line still hashes that whole line.
/// Like the previous dirty-state hash, this is not a cryptographic fingerprint.
#[derive(Clone, Debug)]
pub(super) struct LineFingerprints {
    root: Link,
    is_empty_doc: bool,
}

impl LineFingerprints {
    pub(super) fn new(rope: &Rope) -> Self {
        let is_empty_doc = rope.len_chars() == 0;
        let root = if is_empty_doc {
            let empty_hash = hash_line(RopeSlice::from(""));
            build([empty_hash; 2])
        } else {
            build(rope.lines().map(hash_line))
        };
        Self { root, is_empty_doc }
    }

    pub(super) fn content_hash(&self) -> u64 {
        self.root.as_ref().unwrap().summary.content_hash()
    }

    pub(super) fn update(&mut self, rope: &Rope, start: usize, removed: usize) {
        if self.is_empty_doc || rope.len_chars() == 0 {
            // A partial history range does not describe the virtual blank
            // lines materialized on the first edit or removed on clearing.
            *self = Self::new(rope);
            return;
        }
        let old_len = self.root.as_ref().unwrap().summary.len;
        let start = start.min(old_len);
        let removed = removed.min(old_len - start);
        let suffix = old_len - start - removed;
        let inserted = rope.len_lines() - start - suffix;
        let hashes: Vec<_> = rope.lines_at(start).take(inserted).map(hash_line).collect();
        self.root = splice(self.root.take(), start, removed, &hashes);
        debug_assert_eq!(self.root.as_ref().unwrap().summary.len, rope.len_lines());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn check(node: &Arc<Node>, expected: &[u64]) {
        assert_eq!(node.summary.len, expected.len());
        assert_eq!(node.summary, Summary::from_hashes(expected.iter().copied()));
        match &node.kind {
            NodeKind::Leaf(hashes) => {
                assert!(!hashes.is_empty() && hashes.len() <= BLOCK_SIZE);
                assert_eq!(hashes, expected);
                assert_eq!(node.height, 1);
            }
            NodeKind::Branch(left, right) => {
                assert!(left.height.abs_diff(right.height) <= 1);
                assert_eq!(node.height, left.height.max(right.height) + 1);
                let (before, after) = expected.split_at(left.summary.len);
                check(left, before);
                check(right, after);
            }
        }
    }

    #[test]
    fn sequence_summary_is_ordered_and_independent_of_tree_shape() {
        let expected: Vec<_> = (0..4097).collect();
        let mut root = build(expected.iter().copied());
        check(root.as_ref().unwrap(), &expected);
        for boundary in [0, 1, 63, 64, 65, 2048, 4096, 4097] {
            let (left, right) = split(root.take(), boundary);
            root = concat(left, right);
            check(root.as_ref().unwrap(), &expected);
        }
        assert_ne!(
            Summary::from_hashes([1, 2, 3]),
            Summary::from_hashes([3, 2, 1])
        );
        assert_ne!(Summary::from_hashes([0]), Summary::default());
    }

    #[test]
    fn splices_match_a_vector_and_remain_balanced() {
        let mut expected: Vec<u64> = (0..3000).collect();
        let mut root = build(expected.iter().copied());
        let mut state = 0x3415_9265_3589_7932_u64;
        for _ in 0..1200 {
            state = state.wrapping_mul(6364136223846793005).wrapping_add(1);
            let start = state as usize % (expected.len() + 1);
            let removed = ((state >> 17) as usize % 97).min(expected.len() - start);
            let inserted: Vec<_> = (0..((state >> 29) as usize % 101))
                .map(|i| state.wrapping_add(i as u64))
                .collect();
            root = splice(root, start, removed, &inserted);
            expected.splice(start..start + removed, inserted);
            if let Some(node) = &root {
                check(node, &expected);
            } else {
                assert!(expected.is_empty());
            }
        }
        root = splice(root, 0, expected.len(), &[]);
        assert!(root.is_none());
        root = splice(root, 0, 0, &[7, 8, 9]);
        check(root.as_ref().unwrap(), &[7, 8, 9]);
    }

    #[test]
    fn equal_length_replacements_preserve_cloned_snapshots() {
        let expected: Vec<_> = (0..4096).collect();
        let original = build(expected.iter().copied());
        let mut edited = original.clone();
        replace(edited.as_mut().unwrap(), 63, &[9000, 9001, 9002]);
        let mut changed = expected.clone();
        changed[63..66].copy_from_slice(&[9000, 9001, 9002]);
        check(edited.as_ref().unwrap(), &changed);
        check(original.as_ref().unwrap(), &expected);
        let NodeKind::Branch(_, original_right) = &original.as_ref().unwrap().kind else {
            panic!("expected a branch");
        };
        let NodeKind::Branch(_, edited_right) = &edited.as_ref().unwrap().kind else {
            panic!("expected a branch");
        };
        assert!(Arc::ptr_eq(original_right, edited_right));
    }

    #[test]
    fn rope_index_hashes_logical_lines_and_updates_only_the_edited_range() {
        let mut rope = Rope::from_str(&"original\n".repeat(10_000));
        let mut index = LineFingerprints::new(&rope);
        let before = index.content_hash();
        let start = rope.line_to_char(5000);
        rope.remove(start..start + 8);
        rope.insert(start, "modified");
        index.update(&rope, 5000, 1);
        assert_ne!(index.content_hash(), before);
        assert_eq!(
            index.content_hash(),
            LineFingerprints::new(&rope).content_hash()
        );
        let cloned = index.clone();
        rope.remove(start..start + 8);
        rope.insert(start, "original");
        index.update(&rope, 5000, 1);
        assert_eq!(index.content_hash(), before);
        assert_ne!(cloned.content_hash(), before);
    }

    #[test]
    fn rope_index_handles_empty_sentinels_and_partial_history_restoration() {
        let empty = Rope::new();
        let mut index = LineFingerprints::new(&empty);
        let empty_hash = index.content_hash();
        for text in ["x\n", "\n你🙂", "()", "\n", "a\nb\nc\n"] {
            let rope = Rope::from_str(text);
            index.update(&rope, 0, 1);
            assert_eq!(
                index.content_hash(),
                LineFingerprints::new(&rope).content_hash()
            );
            index.update(&empty, 0, rope.len_lines());
            assert_eq!(index.content_hash(), empty_hash);
        }
    }
}
