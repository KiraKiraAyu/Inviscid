use inviscid::buffer::TextBuffer;
use inviscid::search::{SearchOptions, SearchQuery, find_matches, find_matches_in_rope};
use ropey::Rope;
use std::sync::atomic::AtomicBool;

#[test]
fn rope_search_matches_contiguous_search_across_chunks_and_unicode_boundaries() {
    let text = format!(
        "{}\nKelvin K k \u{212a} foo foobar foo\u{301}\nend",
        "ab\u{1f642}".repeat(2500)
    );
    let rope = Rope::from_str(&text);
    assert!(rope.chunks().count() > 1);
    let boundary = rope.chunks().next().unwrap().len();
    let start = text[..boundary].char_indices().rev().nth(3).unwrap().0;
    let end = boundary
        + text[boundary..]
            .chars()
            .take(8)
            .map(char::len_utf8)
            .sum::<usize>();
    for raw in [
        &text[start..end],
        "K",
        "foo",
        "\nKelvin",
        "missing",
        "ab\u{1f642}",
    ] {
        for case_sensitive in [false, true] {
            for whole_word in [false, true] {
                let query = SearchQuery::new(
                    raw,
                    SearchOptions {
                        case_sensitive,
                        whole_word,
                        ..Default::default()
                    },
                )
                .unwrap();
                for limit in [0, 1, 17, usize::MAX] {
                    assert_eq!(
                        find_matches_in_rope(&rope, &query, limit, &AtomicBool::new(false))
                            .unwrap(),
                        find_matches(&text, &query, limit),
                        "query {raw:?}, case {case_sensitive}, word {whole_word}, limit {limit}"
                    );
                }
            }
        }
    }
}

#[test]
fn regex_snapshot_search_preserves_multiline_and_empty_match_semantics() {
    let text = "one\ntwo 42\nthree";
    for raw in [r"one\ntwo", r"(?s)one.*three", r"^", r"\b", r"\d+"] {
        let query = SearchQuery::new(
            raw,
            SearchOptions {
                use_regex: true,
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(
            find_matches_in_rope(&Rope::from_str(text), &query, 100, &AtomicBool::new(false))
                .unwrap(),
            find_matches(text, &query, 100)
        );
    }
}

#[test]
fn search_snapshot_is_immutable_and_cancelled_search_has_no_results() {
    let mut buffer = TextBuffer::from_str("needle", None);
    let snapshot = buffer.text_snapshot();
    buffer.insert_text("changed ");
    let query = SearchQuery::new("needle", SearchOptions::default()).unwrap();
    assert_eq!(
        find_matches_in_rope(&snapshot, &query, 10, &AtomicBool::new(false)).unwrap(),
        find_matches("needle", &query, 10)
    );
    assert!(find_matches_in_rope(&snapshot, &query, 10, &AtomicBool::new(true)).is_none());
}

#[test]
fn literal_windows_preserve_nonoverlap_word_boundaries_and_long_needles() {
    let text = format!(
        "{} {}\n{}\n{}",
        "a".repeat(6000),
        "\u{212a} k \u{17f} S \u{301}foo foo! ".repeat(120),
        "x\u{2028}y\r\nz",
        "a".repeat(6000)
    );
    let rope = Rope::from_str(&text);
    let long = "a".repeat(3000);
    for raw in [
        "aaa",
        "K",
        "s",
        "foo",
        "foo!",
        "y\r\nz",
        "x\u{2028}y",
        &long,
    ] {
        for whole_word in [false, true] {
            let query = SearchQuery::new(
                raw,
                SearchOptions {
                    whole_word,
                    ..Default::default()
                },
            )
            .unwrap();
            assert_eq!(
                find_matches_in_rope(&rope, &query, usize::MAX, &AtomicBool::new(false)).unwrap(),
                find_matches(&text, &query, usize::MAX),
                "{raw:?}, word {whole_word}"
            );
        }
    }
}
