use crate::buffer::Position;
use crate::search::match_result::{SearchMatch, SearchQuery};
use ropey::Rope;
use std::sync::atomic::{AtomicBool, Ordering};

/// Byte ranges shared by document search and workspace line search.
pub(crate) fn find_byte_ranges<'a>(
    text: &'a str,
    query: &'a SearchQuery,
) -> impl Iterator<Item = std::ops::Range<usize>> + 'a {
    query
        .regex()
        .find_iter(text)
        .filter(|found| !found.is_empty())
        .map(|found| found.range())
}

pub fn find_matches(text: &str, query: &SearchQuery, max_matches: usize) -> Vec<SearchMatch> {
    let mut cursor = PositionCursor::default();
    find_byte_ranges(text, query)
        .take(max_matches)
        .map(|m| {
            let start = cursor.advance_to(text, m.start);
            let end = cursor.advance_to(text, m.end);
            SearchMatch::new(start, end)
        })
        .collect()
}

/// Searches an immutable snapshot. Cancellation discards the whole result.
/// Literal queries retain only a bounded window; regex queries need contiguous text.
pub fn find_matches_in_rope(
    rope: &Rope,
    query: &SearchQuery,
    max_matches: usize,
    cancel: &AtomicBool,
) -> Option<Vec<SearchMatch>> {
    if cancel.load(Ordering::Relaxed) {
        return None;
    }
    if max_matches == 0 {
        return Some(Vec::new());
    }
    if query.options().use_regex {
        let mut text = String::with_capacity(rope.len_bytes());
        for chunk in rope.chunks() {
            if cancel.load(Ordering::Relaxed) {
                return None;
            }
            text.push_str(chunk);
        }
        let mut cursor = PositionCursor::default();
        let mut matches = Vec::new();
        for range in find_byte_ranges(&text, query).take(max_matches) {
            if cancel.load(Ordering::Relaxed) {
                return None;
            }
            matches.push(SearchMatch::new(
                cursor.advance_to(&text, range.start),
                cursor.advance_to(&text, range.end),
            ));
        }
        return (!cancel.load(Ordering::Relaxed)).then_some(matches);
    }

    // Unicode simple case folding preserves the number of scalars. Each literal
    // scalar can therefore consume at most four bytes, plus one lookahead scalar
    // for a whole-word boundary. Keep one preceding scalar for the left boundary.
    let lookahead = query
        .raw()
        .chars()
        .count()
        .saturating_mul(4)
        .saturating_add(4);
    let mut window = String::new();
    let mut base = 0;
    let mut next_start = 0;
    let mut matches = Vec::new();
    let mut positions = RopePositionCursor::new(rope);
    for chunk in rope.chunks() {
        if cancel.load(Ordering::Relaxed) {
            return None;
        }
        window.push_str(chunk);
        let is_last = base + window.len() == rope.len_bytes();
        let safe_end = if is_last {
            window.len()
        } else {
            window.floor_char_boundary(window.len().saturating_sub(lookahead))
        };
        while next_start - base < safe_end {
            if cancel.load(Ordering::Relaxed) {
                return None;
            }
            let Some(found) = query.regex().find_at(&window, next_start - base) else {
                break;
            };
            if found.start() >= safe_end {
                break;
            }
            let start = base + found.start();
            let end = base + found.end();
            matches.push(SearchMatch::new(
                positions.advance_to(start),
                positions.advance_to(end),
            ));
            if matches.len() == max_matches {
                return (!cancel.load(Ordering::Relaxed)).then_some(matches);
            }
            next_start = end;
        }
        next_start = next_start.max(base + safe_end);
        let consumed = next_start - base;
        let retain_from = window[..consumed]
            .char_indices()
            .next_back()
            .map_or(0, |(i, _)| i);
        window.drain(..retain_from);
        base += retain_from;
    }
    (!cancel.load(Ordering::Relaxed)).then_some(matches)
}

struct RopePositionCursor<'a> {
    chunks: ropey::iter::Chunks<'a>,
    remaining: &'a str,
    cursor: PositionCursor,
}

impl<'a> RopePositionCursor<'a> {
    fn new(rope: &'a Rope) -> Self {
        Self {
            chunks: rope.chunks(),
            remaining: "",
            cursor: PositionCursor::default(),
        }
    }

    fn advance_to(&mut self, byte: usize) -> Position {
        while self.cursor.byte < byte {
            if self.remaining.is_empty() {
                self.remaining = self.chunks.next().expect("match within snapshot");
            }
            let count = (byte - self.cursor.byte).min(self.remaining.len());
            self.cursor.advance(&self.remaining[..count]);
            self.remaining = &self.remaining[count..];
        }
        self.cursor.pos
    }
}

#[derive(Default)]
struct PositionCursor {
    byte: usize,
    pos: Position,
}

impl PositionCursor {
    fn advance_to(&mut self, text: &str, byte: usize) -> Position {
        self.advance(&text[self.byte..byte]);
        self.pos
    }

    fn advance(&mut self, gap: &str) {
        match memchr::memrchr(b'\n', gap.as_bytes()) {
            Some(last_newline) => {
                self.pos.line += memchr::memchr_iter(b'\n', gap.as_bytes()).count();
                self.pos.col = gap[last_newline + 1..].chars().count();
            }
            None => self.pos.col += gap.chars().count(),
        }
        self.byte += gap.len();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::search::match_result::SearchOptions;

    #[test]
    fn test_literal_search_case_sensitive() {
        let text = "Hello World\nHello Rust\nhello world";
        let query = SearchQuery::new(
            "Hello",
            SearchOptions {
                case_sensitive: true,
                ..Default::default()
            },
        )
        .unwrap();

        let matches = find_matches(text, &query, 100);
        assert_eq!(matches.len(), 2);
        assert_eq!(
            matches[0].start,
            Position::new(crate::buffer::SourceLine::new(0), 0)
        );
        assert_eq!(
            matches[0].end,
            Position::new(crate::buffer::SourceLine::new(0), 5)
        );
        assert_eq!(
            matches[1].start,
            Position::new(crate::buffer::SourceLine::new(1), 0)
        );
    }

    #[test]
    fn test_literal_search_case_insensitive() {
        let text = "Hello World\nHello Rust\nhello world";
        let query = SearchQuery::new("hello", SearchOptions::default()).unwrap();

        let matches = find_matches(text, &query, 100);
        assert_eq!(matches.len(), 3);
    }

    #[test]
    fn test_regex_search() {
        let text = "foo123bar\nfoo456bar\nfooxbar";
        let query = SearchQuery::new(
            r"foo\d+bar",
            SearchOptions {
                use_regex: true,
                ..Default::default()
            },
        )
        .unwrap();

        let matches = find_matches(text, &query, 100);
        assert_eq!(matches.len(), 2);
        assert_eq!(
            matches[0].start,
            Position::new(crate::buffer::SourceLine::new(0), 0)
        );
        assert_eq!(
            matches[1].start,
            Position::new(crate::buffer::SourceLine::new(1), 0)
        );
    }

    #[test]
    fn test_multiline_match() {
        let text = "start\nmiddle\nend";
        let query = SearchQuery::new(
            r"start\nmiddle",
            SearchOptions {
                use_regex: true,
                ..Default::default()
            },
        )
        .unwrap();

        let matches = find_matches(text, &query, 100);
        assert_eq!(matches.len(), 1);
        assert_eq!(
            matches[0].start,
            Position::new(crate::buffer::SourceLine::new(0), 0)
        );
        assert_eq!(
            matches[0].end,
            Position::new(crate::buffer::SourceLine::new(1), 6)
        );
    }

    #[test]
    fn test_max_matches_limit() {
        let text = "a\n".repeat(1000);
        let query = SearchQuery::new("a", SearchOptions::default()).unwrap();

        let matches = find_matches(&text, &query, 100);
        assert_eq!(matches.len(), 100);
    }

    #[test]
    fn test_empty_text() {
        let text = "";
        let query = SearchQuery::new("test", SearchOptions::default()).unwrap();

        let matches = find_matches(text, &query, 100);
        assert_eq!(matches.len(), 0);
    }

    #[test]
    fn test_no_matches() {
        let text = "hello world";
        let query = SearchQuery::new("xyz", SearchOptions::default()).unwrap();

        let matches = find_matches(text, &query, 100);
        assert_eq!(matches.len(), 0);
    }

    #[test]
    fn test_unicode_handling() {
        let text = "Hello 世界\n你好 World";
        let matches = find_matches(text, &query("世界", SearchOptions::default()), 100);
        assert_eq!(
            matches,
            vec![SearchMatch::new(
                Position::new(crate::buffer::SourceLine::new(0), 6),
                Position::new(crate::buffer::SourceLine::new(0), 8)
            )]
        );

        let matches = find_matches(text, &query("world", SearchOptions::default()), 100);
        assert_eq!(
            matches,
            vec![SearchMatch::new(
                Position::new(crate::buffer::SourceLine::new(1), 3),
                Position::new(crate::buffer::SourceLine::new(1), 8)
            )]
        );
    }

    fn query(text: &str, options: SearchOptions) -> SearchQuery {
        SearchQuery::new(text, options).unwrap()
    }

    #[test]
    fn test_case_folding_that_changes_byte_length_keeps_original_positions() {
        // `İ` (2 bytes) lowercases to `i̇` (3 bytes): offsets taken from a lowercased copy
        // would point one byte past the real match.
        let text = "İİ hello";
        let matches = find_matches(text, &query("HELLO", SearchOptions::default()), 100);
        assert_eq!(
            matches,
            vec![SearchMatch::new(
                Position::new(crate::buffer::SourceLine::new(0), 3),
                Position::new(crate::buffer::SourceLine::new(0), 8)
            )]
        );
    }

    #[test]
    fn test_whole_word_applies_to_literal_queries() {
        let text = "cat catalog cat";
        let options = SearchOptions {
            whole_word: true,
            ..Default::default()
        };
        let matches = find_matches(text, &query("cat", options), 100);
        assert_eq!(matches.len(), 2);
        assert_eq!(
            matches[1].start,
            Position::new(crate::buffer::SourceLine::new(0), 12)
        );
    }

    #[test]
    fn test_whole_word_keeps_regex_semantics() {
        let text = "cat cot cut";
        let options = SearchOptions {
            whole_word: true,
            use_regex: true,
            ..Default::default()
        };
        assert_eq!(find_matches(text, &query("c.t", options), 100).len(), 3);
    }

    #[test]
    fn test_literal_matches_do_not_overlap() {
        let matches = find_matches("aaaa", &query("aa", SearchOptions::default()), 100);
        assert_eq!(
            matches,
            vec![
                SearchMatch::new(
                    Position::new(crate::buffer::SourceLine::new(0), 0),
                    Position::new(crate::buffer::SourceLine::new(0), 2)
                ),
                SearchMatch::new(
                    Position::new(crate::buffer::SourceLine::new(0), 2),
                    Position::new(crate::buffer::SourceLine::new(0), 4)
                ),
            ]
        );
    }

    #[test]
    fn test_regex_skips_empty_matches() {
        let options = SearchOptions {
            use_regex: true,
            ..Default::default()
        };
        let matches = find_matches("axb", &query("x*", options), 100);
        assert_eq!(
            matches,
            vec![SearchMatch::new(
                Position::new(crate::buffer::SourceLine::new(0), 1),
                Position::new(crate::buffer::SourceLine::new(0), 2)
            )]
        );
    }

    #[test]
    fn test_regex_anchors_match_at_every_line() {
        let options = SearchOptions {
            use_regex: true,
            ..Default::default()
        };
        let matches = find_matches("foo\nbar\nfoo", &query("^foo", options), 100);
        assert_eq!(matches.len(), 2);
        assert_eq!(
            matches[1].start,
            Position::new(crate::buffer::SourceLine::new(2), 0)
        );
    }

    #[test]
    fn test_positions_stay_correct_deep_into_a_large_document() {
        let text = "hello world\n".repeat(100_000);
        let matches = find_matches(&text, &query("world", SearchOptions::default()), usize::MAX);
        assert_eq!(matches.len(), 100_000);
        assert_eq!(
            matches[99_999],
            SearchMatch::new(
                Position::new(crate::buffer::SourceLine::new(99_999), 6),
                Position::new(crate::buffer::SourceLine::new(99_999), 11),
            )
        );
    }
}
