use inviscid::buffer::{LineSource, Position, Selection, SourceLine, SourcePosition, TextBuffer};
use std::borrow::Cow;

#[test]
fn source_lines_match_existing_text_for_unicode_and_chunk_boundaries() {
    let long_line = "你🙂e\u{301}".repeat(4096);
    for source in ["", "\n", "a\nb\n", "你\r\n🙂", &long_line] {
        let buffer = TextBuffer::from_str(source, None);
        let expected = buffer.to_lines();
        for (idx, line) in expected.iter().enumerate() {
            let source_line = SourceLine::new(idx);
            assert_eq!(buffer.source_line(source_line), line.as_str());
            assert_eq!(expected.source_line(source_line), line.as_str());
            assert_eq!(buffer.line_len(source_line), line.chars().count());
        }
    }
    let short = TextBuffer::from_str("short\nline", None);
    assert!(matches!(
        short.source_line(SourceLine::new(0)),
        Cow::Borrowed(_)
    ));
    let long = TextBuffer::from_str(&long_line, None);
    assert!(matches!(
        long.source_line(SourceLine::new(0)),
        Cow::Owned(_)
    ));
}

#[test]
fn utf16_coordinates_match_a_scalar_oracle_and_clamp_surrogate_offsets() {
    for source in ["a🙂界\ne\u{301}\n", "🙂🙂", "a\n\n界"] {
        let buffer = TextBuffer::from_str(source, None);
        let (mut line, mut col, mut utf16) = (0, 0, 0);
        for ch in source.chars() {
            let pos = SourcePosition::new(SourceLine::new(line), col);
            assert_eq!(buffer.pos_to_utf16_offset(pos), utf16);
            assert_eq!(buffer.utf16_offset_to_pos(utf16), pos);
            if ch.len_utf16() == 2 {
                assert_eq!(buffer.utf16_offset_to_pos(utf16 + 1), pos);
            }
            utf16 += ch.len_utf16();
            if ch == '\n' {
                line += 1;
                col = 0;
            } else {
                col += 1;
            }
        }
        let end = SourcePosition::new(SourceLine::new(line), col);
        assert_eq!(buffer.pos_to_utf16_offset(end), utf16);
        assert_eq!(buffer.utf16_offset_to_pos(usize::MAX), end);
        assert_eq!(
            buffer
                .pos_to_utf16_offset(SourcePosition::new(SourceLine::new(usize::MAX), usize::MAX)),
            utf16
        );
    }
}

#[test]
fn empty_document_virtual_coordinates_survive_first_input_and_history() {
    let mut buffer = TextBuffer::new();
    let second = SourcePosition::new(SourceLine::new(1), 0);
    assert_eq!(buffer.pos_to_utf16_offset(second), 1);
    assert_eq!(buffer.utf16_offset_to_pos(usize::MAX), second);
    buffer.set_cursor(second.to_position());
    buffer.insert_text("🙂 two");
    assert_eq!(buffer.word_count(), 2);
    assert_eq!(
        buffer.pos_to_utf16_offset(SourcePosition::new(SourceLine::new(1), 1)),
        3
    );
    buffer.undo();
    assert_eq!(buffer.word_count(), 0);
    assert_eq!(buffer.utf16_offset_to_pos(1), second);
    buffer.redo();
    assert_eq!(buffer.word_count(), 2);
}

#[test]
fn word_count_tracks_selection_edits_clones_and_unicode_whitespace() {
    let mut buffer = TextBuffer::from_str("one two\n你\u{2003}🙂\nlast", None);
    assert_eq!(buffer.word_count(), 5);
    let original = buffer.clone();
    buffer.set_selection(Selection::new(
        Position::new(SourceLine::new(0), 3),
        Position::new(SourceLine::new(1), 1),
    ));
    buffer.insert_text("\tnew\n words\u{a0}");
    let expected = buffer.to_string_normalized().split_whitespace().count();
    assert_eq!(buffer.word_count(), expected);
    assert_eq!(original.word_count(), 5);
    buffer.undo();
    assert_eq!(buffer.word_count(), 5);
    buffer.redo();
    assert_eq!(buffer.word_count(), expected);
}
