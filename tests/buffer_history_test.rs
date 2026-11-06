use inviscid::buffer::{Position, Selection, SourceLine, TextBuffer};

fn assert_document(buffer: &TextBuffer, text: &str, dirty: bool) {
    assert_eq!(buffer.to_string_normalized(), text);
    assert_eq!(buffer.is_empty(), text.is_empty());
    assert_eq!(buffer.char_count(), text.chars().count());
    assert_eq!(
        buffer.line_count(),
        if text.is_empty() {
            2
        } else {
            text.split('\n').count()
        }
    );
    assert_eq!(buffer.is_dirty(), dirty);
    let selection = buffer.selection();
    assert_eq!(buffer.clamp_position(selection.anchor), selection.anchor);
    assert_eq!(buffer.clamp_position(selection.head), selection.head);
}

#[test]
fn first_input_history_restores_text_metrics_and_selection() {
    for text in ["x", "你好🙂"] {
        let mut buffer = TextBuffer::new();
        let original_selection = buffer.selection();
        buffer.insert_text(text);
        let expected = format!("{text}\n");
        let edited_selection = buffer.selection();
        assert_document(&buffer, &expected, true);

        for _ in 0..3 {
            buffer.undo();
            assert_document(&buffer, "", false);
            assert_eq!(buffer.selection(), original_selection);
            buffer.redo();
            assert_document(&buffer, &expected, true);
            assert_eq!(buffer.selection(), edited_selection);
        }
    }
}

#[test]
fn first_input_history_restores_saved_baseline() {
    let mut buffer = TextBuffer::new();
    buffer.insert_text("saved");
    buffer.mark_as_saved();
    assert_document(&buffer, "saved\n", false);
    buffer.undo();
    assert_document(&buffer, "", true);
    buffer.redo();
    assert_document(&buffer, "saved\n", false);
}

#[test]
fn first_input_history_preserves_the_second_virtual_line() {
    let mut buffer = TextBuffer::new();
    let original_selection = Selection::cursor(Position::new(SourceLine::new(1), 0));
    buffer.set_selection(original_selection);
    buffer.insert_text("你🙂");
    let edited_selection = Selection::cursor(Position::new(SourceLine::new(1), 2));
    assert_document(&buffer, "\n你🙂", true);
    assert_eq!(buffer.selection(), edited_selection);
    buffer.undo();
    assert_document(&buffer, "", false);
    assert_eq!(buffer.selection(), original_selection);
    buffer.redo();
    assert_document(&buffer, "\n你🙂", true);
    assert_eq!(buffer.selection(), edited_selection);
}

#[test]
fn first_input_history_restores_coalesced_typing() {
    let mut buffer = TextBuffer::new();
    for text in ["x", "y", "z"] {
        buffer.insert_text(text);
    }
    assert_document(&buffer, "xyz\n", true);
    // Undo every step: this also covers systems where the coalescing time window expires.
    for _ in 0..3 {
        buffer.undo();
    }
    assert_document(&buffer, "", false);
    for _ in 0..3 {
        buffer.redo();
    }
    assert_document(&buffer, "xyz\n", true);
}

#[test]
fn deleting_a_pair_history_preserves_a_document_without_a_trailing_newline() {
    let mut buffer = TextBuffer::from_str("()", None);
    buffer.set_selection(Selection::cursor(Position::new(SourceLine::new(0), 1)));
    assert!(buffer.backspace_auto_pair());
    assert_document(&buffer, "", true);
    buffer.undo();
    assert_document(&buffer, "()", false);
    buffer.redo();
    assert_document(&buffer, "", true);
    buffer.insert_text("x");
    assert_document(&buffer, "x\n", true);
    buffer.undo();
    assert_document(&buffer, "", true);
    buffer.undo();
    assert_document(&buffer, "()", false);
}
