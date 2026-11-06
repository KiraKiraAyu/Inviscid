use inviscid::buffer::{Position, Selection, SourceLine, TextBuffer, compute_lines_hash};

fn assert_fingerprint(buffer: &TextBuffer) {
    let snapshot = buffer.saved_baseline_snapshot();
    assert_eq!(
        snapshot.content_hash,
        compute_lines_hash(&buffer.to_lines())
    );
    assert_eq!(snapshot.line_count, buffer.line_count());
    assert_eq!(snapshot.char_count, buffer.char_count());
    assert_eq!(snapshot.is_empty_doc, buffer.is_empty());
}

fn assert_state(buffer: &TextBuffer, expected: &str, saved: &str) {
    assert_eq!(buffer.to_string_normalized(), expected);
    assert_eq!(buffer.is_dirty(), expected != saved);
    assert_fingerprint(buffer);
}

fn replace_lines(buffer: &mut TextBuffer, start: usize, count: usize, lines: &[&str]) {
    buffer.replace_line_range(
        SourceLine::new(start),
        count,
        lines.iter().map(|line| (*line).to_owned()).collect(),
        Selection::cursor(Position::default()),
    );
    assert_fingerprint(buffer);
}

#[test]
fn fingerprint_matches_logical_lines_after_loading_and_local_edits() {
    let long_line = "你🙂e\u{301}".repeat(2048);
    for source in ["", "\n", "\n\n", "a\nb", "a\nb\n", "a\r\nb\r\n", &long_line] {
        let mut buffer = TextBuffer::from_str(source, None);
        let original = buffer.to_string_normalized();
        assert_state(&buffer, &original, &original);
        buffer.record_single_line_edit(SourceLine::new(0), |line, _| line.push_str("界🙂"));
        let edited = buffer.to_string_normalized();
        assert_state(&buffer, &edited, &original);
        buffer.undo();
        assert_state(&buffer, &original, &original);
        buffer.redo();
        assert_state(&buffer, &edited, &original);
    }
}

#[test]
fn fingerprint_preserves_line_order_and_segmentation() {
    for (original, replacement) in [
        ("ab\ncd\nef", ["cd", "ab", "ef"]),
        ("ab\nc\nde", ["a", "bc", "de"]),
        ("你🙂\n好🙂\n末尾", ["好🙂", "你🙂", "末尾"]),
    ] {
        let mut buffer = TextBuffer::from_str(original, None);
        let original_hash = buffer.saved_baseline_snapshot().content_hash;
        replace_lines(&mut buffer, 0, 3, &replacement);
        assert_state(&buffer, &replacement.join("\n"), original);
        assert_ne!(buffer.saved_baseline_snapshot().content_hash, original_hash);
        buffer.undo();
        assert_state(&buffer, original, original);
        buffer.redo();
        assert_state(&buffer, &replacement.join("\n"), original);
    }
}

#[test]
fn dirty_tracking_recognizes_manual_restoration_at_multiple_locations() {
    let original = "alpha\nbeta\ngamma\ndelta\n";
    let mut buffer = TextBuffer::from_str(original, None);
    replace_lines(&mut buffer, 1, 1, &["BETA"]);
    replace_lines(&mut buffer, 3, 1, &["DELTA"]);
    assert_state(&buffer, "alpha\nBETA\ngamma\nDELTA\n", original);
    replace_lines(&mut buffer, 1, 1, &["beta"]);
    assert_state(&buffer, "alpha\nbeta\ngamma\nDELTA\n", original);
    replace_lines(&mut buffer, 3, 1, &["delta"]);
    assert_state(&buffer, original, original);
}

#[test]
fn saved_snapshot_survives_equal_length_edits_and_history_branches() {
    let mut buffer = TextBuffer::from_str("alpha\nbeta\ngamma", None);
    replace_lines(&mut buffer, 1, 1, &["BETA"]);
    let saved = buffer.saved_baseline_snapshot();
    replace_lines(&mut buffer, 1, 1, &["zeta"]);
    buffer.mark_saved_as(saved);
    assert_state(&buffer, "alpha\nzeta\ngamma", "alpha\nBETA\ngamma");
    buffer.undo();
    assert_state(&buffer, "alpha\nBETA\ngamma", "alpha\nBETA\ngamma");

    let mut branch = buffer.clone();
    replace_lines(&mut branch, 1, 1, &["ZETA"]);
    assert_state(&branch, "alpha\nZETA\ngamma", "alpha\nBETA\ngamma");
    assert_state(&buffer, "alpha\nBETA\ngamma", "alpha\nBETA\ngamma");
    branch.redo();
    assert_state(&branch, "alpha\nZETA\ngamma", "alpha\nBETA\ngamma");
    branch.undo();
    assert_state(&branch, "alpha\nBETA\ngamma", "alpha\nBETA\ngamma");
}

#[test]
fn fingerprint_tracks_empty_documents_and_partial_history_ranges() {
    for source in ["()", "()\n", "first\nsecond\nlast\n"] {
        let mut buffer = TextBuffer::from_str(source, None);
        let count = buffer.line_count();
        replace_lines(&mut buffer, 0, count, &["", ""]);
        assert_state(&buffer, "", source);
        buffer.undo();
        assert_state(&buffer, source, source);
        buffer.redo();
        assert_state(&buffer, "", source);
        buffer.set_cursor(Position::new(SourceLine::new(1), 0));
        buffer.insert_text("你🙂");
        assert_state(&buffer, "\n你🙂", source);
        buffer.undo();
        assert_state(&buffer, "", source);
        buffer.undo();
        assert_state(&buffer, source, source);
    }

    let mut buffer = TextBuffer::from_str("()", None);
    buffer.set_cursor(Position::new(SourceLine::new(0), 1));
    assert!(buffer.backspace_auto_pair());
    assert_state(&buffer, "", "()");
    buffer.undo();
    assert_state(&buffer, "()", "()");
    buffer.redo();
    assert_state(&buffer, "", "()");

    let empty = TextBuffer::new().saved_baseline_snapshot();
    let newline = TextBuffer::from_str("\n", None).saved_baseline_snapshot();
    // Both expose two empty logical lines; their document metadata distinguishes them.
    assert_eq!(empty.line_count, newline.line_count);
    assert_ne!(empty.is_empty_doc, newline.is_empty_doc);
    assert_ne!(empty.char_count, newline.char_count);
}

#[test]
fn fingerprint_tracks_range_ime_and_legacy_edits() {
    let original = "alpha\nbeta\ngamma\ndelta";
    let mut buffer = TextBuffer::from_str(original, None);
    buffer.replace_range(
        (
            Position::new(SourceLine::new(1), 1),
            Position::new(SourceLine::new(2), 2),
        ),
        "你🙂\n中\n末",
    );
    let replaced = buffer.to_string_normalized();
    assert_state(&buffer, &replaced, original);
    buffer.undo();
    assert_state(&buffer, original, original);
    buffer.redo();
    assert_state(&buffer, &replaced, original);

    buffer.replace_and_mark_utf16(Some(0..2), None, "🙂", None);
    assert_fingerprint(&buffer);
    buffer.record_edit(|lines, _| {
        lines.replace_source_lines_at(
            SourceLine::new(1),
            1,
            vec!["legacy".to_owned(), "界".to_owned()],
        );
    });
    let legacy = buffer.to_string_normalized();
    assert_fingerprint(&buffer);
    buffer.undo();
    assert_fingerprint(&buffer);
    buffer.redo();
    assert_state(&buffer, &legacy, original);
}

#[test]
fn fingerprint_tracks_markdown_commands_and_their_history() {
    let mut list = TextBuffer::from_str("- one\n- two\n- three\n", None);
    list.set_cursor(Position::new(SourceLine::new(1), 5));
    list.insert_newline_mode(false);
    assert_fingerprint(&list);
    list.insert_text("界🙂");
    assert_fingerprint(&list);
    list.move_lines_up();
    assert_fingerprint(&list);
    list.duplicate_lines_down();
    assert_fingerprint(&list);
    list.indent_lines_with_tab_size(4);
    assert_fingerprint(&list);
    list.outdent_lines_with_tab_size(4);
    assert_fingerprint(&list);
    for _ in 0..6 {
        list.undo();
        assert_fingerprint(&list);
    }
    for _ in 0..6 {
        list.redo();
        assert_fingerprint(&list);
    }

    let mut table = TextBuffer::from_str("| A | B |\n| --- | --- |\n| 你 | 🙂 |", None);
    table.set_cursor(Position::new(SourceLine::new(2), 6));
    assert!(table.table_enter_next_row());
    assert_fingerprint(&table);
    table.undo();
    assert_fingerprint(&table);
    table.redo();
    assert_fingerprint(&table);
}

#[test]
fn fingerprint_matches_a_line_oracle_through_deterministic_splices() {
    let original = (0..512)
        .map(|i| format!("line {i} 界🙂"))
        .collect::<Vec<_>>()
        .join("\n");
    let mut buffer = TextBuffer::from_str(&original, None);
    let mut expected = original.clone();
    let mut state = 0x9e37_79b9_u64;

    for step in 0..160 {
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        let mut lines = expected.split('\n').map(str::to_owned).collect::<Vec<_>>();
        let start = state as usize % (lines.len() + 1);
        let count = ((state >> 17) as usize % 9).min(lines.len() - start);
        let replacement_count = (state >> 33) as usize % 9;
        let replacement = (0..replacement_count)
            .map(|i| format!("edit {step}:{i} 🙂界"))
            .collect::<Vec<_>>();
        lines.splice(start..start + count, replacement.iter().cloned());
        let next = lines.join("\n");
        buffer.replace_line_range(
            SourceLine::new(start),
            count,
            replacement,
            Selection::cursor(Position::default()),
        );
        assert_state(&buffer, &next, &original);
        if next != expected {
            buffer.undo();
            assert_state(&buffer, &expected, &original);
            buffer.redo();
            assert_state(&buffer, &next, &original);
        }
        expected = next;
    }
}
