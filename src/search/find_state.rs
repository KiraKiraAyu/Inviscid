use crate::buffer::{Position, SourceLine};
use crate::search::{SearchMatch, SearchQuery};

#[derive(Clone, Debug, Default)]
pub struct FindState {
    query: Option<SearchQuery>,
    matches: Vec<SearchMatch>,
    current_idx: Option<usize>,
    revision: Option<u64>,
}

impl FindState {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn set_query(&mut self, query: SearchQuery) {
        self.query = Some(query);
        self.matches.clear();
        self.current_idx = None;
        self.revision = None;
    }

    pub fn set_matches(&mut self, matches: Vec<SearchMatch>, revision: u64, anchor: Position) {
        self.current_idx = if matches.is_empty() {
            None
        } else {
            Some(matches.partition_point(|m| m.start < anchor) % matches.len())
        };
        self.matches = matches;
        self.revision = Some(revision);
    }

    pub fn clear(&mut self) {
        *self = Self::default();
    }

    pub fn is_stale(&self, revision: u64) -> bool {
        self.query.is_some() && self.revision != Some(revision)
    }

    pub fn is_active(&self) -> bool {
        self.query.is_some()
    }

    pub fn query(&self) -> Option<&SearchQuery> {
        self.query.as_ref()
    }

    pub fn matches(&self) -> &[SearchMatch] {
        &self.matches
    }

    pub fn match_count(&self) -> usize {
        self.matches.len()
    }

    pub fn current_match_index(&self) -> Option<usize> {
        self.current_idx
    }

    pub fn current_match_number(&self) -> Option<usize> {
        self.current_idx.map(|idx| idx + 1)
    }

    pub fn current_match(&self) -> Option<&SearchMatch> {
        self.current_idx.and_then(|idx| self.matches.get(idx))
    }

    pub fn matches_on_line(&self, line: SourceLine) -> impl Iterator<Item = (usize, &SearchMatch)> {
        let line_idx = line.index();
        let first = self.matches.partition_point(|m| m.end.line < line);
        self.matches[first..]
            .iter()
            .enumerate()
            .take_while(move |(_, m)| m.start.line.index() <= line_idx)
            .map(move |(offset, m)| (first + offset, m))
    }

    pub fn next_match(&mut self, wrap_around: bool) -> Option<usize> {
        let len = self.matches.len();
        if len == 0 {
            return None;
        }
        let next = match self.current_idx {
            Some(idx) if idx + 1 < len => idx + 1,
            Some(idx) if !wrap_around => idx,
            Some(_) | None => 0,
        };
        self.current_idx = Some(next);
        Some(next)
    }

    pub fn prev_match(&mut self, wrap_around: bool) -> Option<usize> {
        let len = self.matches.len();
        if len == 0 {
            return None;
        }
        let prev = match self.current_idx {
            Some(idx) if idx > 0 => idx - 1,
            Some(_) if !wrap_around => 0,
            Some(_) | None => len - 1,
        };
        self.current_idx = Some(prev);
        Some(prev)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::search::SearchOptions;

    fn m(start: (usize, usize), end: (usize, usize)) -> SearchMatch {
        SearchMatch::new(
            Position::new(crate::buffer::SourceLine::new(start.0), start.1),
            Position::new(crate::buffer::SourceLine::new(end.0), end.1),
        )
    }

    fn state_with(matches: Vec<SearchMatch>) -> FindState {
        let mut state = FindState::new();
        state.set_query(SearchQuery::new("x", SearchOptions::default()).unwrap());
        state.set_matches(matches, 1, Position::default());
        state
    }

    #[test]
    fn test_find_state_new_is_inactive() {
        let state = FindState::new();
        assert!(!state.is_active());
        assert_eq!(state.match_count(), 0);
        assert!(state.current_match().is_none());
    }

    #[test]
    fn test_set_matches_selects_first_match_at_or_after_anchor() {
        let mut state = FindState::new();
        state.set_query(SearchQuery::new("x", SearchOptions::default()).unwrap());
        let matches = vec![m((0, 0), (0, 1)), m((2, 4), (2, 5)), m((5, 0), (5, 1))];

        state.set_matches(
            matches.clone(),
            1,
            Position::new(crate::buffer::SourceLine::new(2), 4),
        );
        assert_eq!(state.current_match_index(), Some(1));

        state.set_matches(
            matches.clone(),
            1,
            Position::new(crate::buffer::SourceLine::new(3), 0),
        );
        assert_eq!(state.current_match_index(), Some(2));
    }

    #[test]
    fn test_set_matches_wraps_to_first_when_anchor_is_past_last_match() {
        let mut state = FindState::new();
        state.set_query(SearchQuery::new("x", SearchOptions::default()).unwrap());
        state.set_matches(
            vec![m((0, 0), (0, 1))],
            1,
            Position::new(crate::buffer::SourceLine::new(9), 0),
        );
        assert_eq!(state.current_match_index(), Some(0));
    }

    #[test]
    fn test_set_matches_with_no_results_selects_nothing() {
        let mut state = FindState::new();
        state.set_query(SearchQuery::new("x", SearchOptions::default()).unwrap());
        state.set_matches(Vec::new(), 1, Position::default());
        assert!(state.is_active());
        assert_eq!(state.current_match_index(), None);
    }

    #[test]
    fn test_staleness_tracks_revision_and_query() {
        let mut state = FindState::new();
        assert!(!state.is_stale(1));

        state.set_query(SearchQuery::new("x", SearchOptions::default()).unwrap());
        assert!(state.is_stale(1));

        state.set_matches(Vec::new(), 1, Position::default());
        assert!(!state.is_stale(1));
        assert!(state.is_stale(2));
    }

    #[test]
    fn test_matches_on_line_returns_only_matches_touching_that_line() {
        let state = state_with(vec![
            m((0, 0), (0, 1)),
            m((1, 2), (1, 3)),
            m((1, 5), (3, 1)),
            m((4, 0), (4, 2)),
        ]);

        let ids = |line| {
            state
                .matches_on_line(line)
                .map(|(i, _)| i)
                .collect::<Vec<_>>()
        };
        assert_eq!(ids(SourceLine::new(0)), vec![0]);
        assert_eq!(ids(SourceLine::new(1)), vec![1, 2]);
        assert_eq!(ids(SourceLine::new(2)), vec![2]);
        assert_eq!(ids(SourceLine::new(3)), vec![2]);
        assert_eq!(ids(SourceLine::new(4)), vec![3]);
        assert!(ids(SourceLine::new(5)).is_empty());
    }

    #[test]
    fn test_next_match_with_wrap() {
        let mut state = state_with(vec![
            m((0, 0), (0, 1)),
            m((1, 0), (1, 1)),
            m((2, 0), (2, 1)),
        ]);
        assert_eq!(state.current_match_number(), Some(1));
        state.next_match(true);
        state.next_match(true);
        assert_eq!(state.current_match_number(), Some(3));
        state.next_match(true);
        assert_eq!(state.current_match_number(), Some(1));
    }

    #[test]
    fn test_next_match_without_wrap_stays_on_last() {
        let mut state = state_with(vec![m((0, 0), (0, 1)), m((1, 0), (1, 1))]);
        state.next_match(false);
        state.next_match(false);
        assert_eq!(state.current_match_number(), Some(2));
    }

    #[test]
    fn test_prev_match_with_wrap() {
        let mut state = state_with(vec![
            m((0, 0), (0, 1)),
            m((1, 0), (1, 1)),
            m((2, 0), (2, 1)),
        ]);
        state.prev_match(true);
        assert_eq!(state.current_match_number(), Some(3));
        state.prev_match(true);
        assert_eq!(state.current_match_number(), Some(2));
    }

    #[test]
    fn test_prev_match_without_wrap_stays_on_first() {
        let mut state = state_with(vec![m((0, 0), (0, 1)), m((1, 0), (1, 1))]);
        state.prev_match(false);
        assert_eq!(state.current_match_number(), Some(1));
    }

    #[test]
    fn test_clear_resets_everything() {
        let mut state = state_with(vec![m((0, 0), (0, 1))]);
        state.clear();
        assert!(!state.is_active());
        assert_eq!(state.match_count(), 0);
        assert!(!state.is_stale(1));
    }
}
