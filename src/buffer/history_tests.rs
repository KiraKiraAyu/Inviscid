use super::*;
use std::collections::HashSet;

fn shared_bytes(fragment: &Rope, source: &Rope) -> usize {
    let chunks: HashSet<_> = source
        .chunks()
        .map(|chunk| (chunk.as_ptr(), chunk.len()))
        .collect();
    fragment
        .chunks()
        .filter(|chunk| chunks.contains(&(chunk.as_ptr(), chunk.len())))
        .map(str::len)
        .sum()
}

#[test]
fn history_fragments_share_document_chunks_and_survive_replay() {
    let original = "original line with words\n".repeat(45_000);
    let replacement = "replacement line with words\n".repeat(40_000);
    let mut buffer = TextBuffer::from_bytes(original.as_bytes(), None).unwrap();
    let old_snapshot = buffer.text_snapshot();
    buffer.select_all();
    buffer.insert_text(&replacement);
    let step = buffer.undo_stack.back().unwrap().clone();
    assert!(shared_bytes(&step.old_lines.text, &old_snapshot) > original.len() * 9 / 10);
    assert!(shared_bytes(&step.new_lines.text, &buffer.rope) > replacement.len() * 9 / 10);
    buffer.undo();
    assert_eq!(buffer.to_string_normalized(), original);
    assert!(shared_bytes(&buffer.rope, &step.old_lines.text) > original.len() * 9 / 10);
    buffer.redo();
    assert_eq!(buffer.to_string_normalized(), replacement);
    assert!(shared_bytes(&buffer.rope, &step.new_lines.text) > replacement.len() * 9 / 10);
    assert_eq!(old_snapshot.to_string(), original);
}

// The original Vec-based history application is retained as a replay oracle.
fn replay_vec(
    buffer: &mut TextBuffer,
    start_line: SourceLine,
    replaced_lines: &[String],
    inserted_lines: &[String],
    target_empty_doc: bool,
    target_document_line_count: usize,
) {
    if target_empty_doc {
        buffer.rope = Rope::new();
    } else if buffer.is_empty_doc {
        let mut lines = vec![""; target_document_line_count];
        let start = start_line.index();
        for (target, text) in lines[start..start + inserted_lines.len()]
            .iter_mut()
            .zip(inserted_lines)
        {
            *target = text.as_str();
        }
        buffer.rope = Rope::from_str(&lines.join("\n"));
    } else {
        buffer.replace_rope_lines(start_line, replaced_lines.len(), inserted_lines);
    }
    buffer.is_empty_doc = target_empty_doc;
    buffer.finish_content_metrics(start_line, replaced_lines, inserted_lines);
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct State {
    text: String,
    selection: Selection,
    words: usize,
    dirty: bool,
}

fn state(buffer: &TextBuffer) -> State {
    State {
        text: buffer.to_string_normalized(),
        selection: buffer.selection(),
        words: buffer.word_count(),
        dirty: buffer.is_dirty(),
    }
}

#[test]
fn random_history_matches_vec_replay_and_saved_states() {
    let mut buffer = TextBuffer::from_str("first\nsecond\nlast\n", None);
    let mut undo = VecDeque::new();
    let mut redo = Vec::new();
    let mut seed = 0x71ae_33f9_829c_5701_u64;
    let mut random = || {
        seed ^= seed << 13;
        seed ^= seed >> 7;
        seed ^= seed << 17;
        seed as usize
    };
    for iteration in 0..3000 {
        match random() % 6 {
            direction @ (0 | 1) => {
                let step = if direction == 0 {
                    buffer.undo_stack.back()
                } else {
                    buffer.redo_stack.back()
                }
                .cloned();
                let mut oracle = buffer.clone();
                if let Some(step) = step {
                    let (old, new, empty, count) = if direction == 0 {
                        (
                            &step.new_lines,
                            &step.old_lines,
                            step.old_empty_doc,
                            step.old_document_line_count,
                        )
                    } else {
                        (
                            &step.old_lines,
                            &step.new_lines,
                            step.new_empty_doc,
                            step.new_document_line_count,
                        )
                    };
                    if !old.is_empty() || !new.is_empty() {
                        replay_vec(
                            &mut oracle,
                            step.start_line,
                            &old.to_lines(),
                            &new.to_lines(),
                            empty,
                            count,
                        );
                    }
                }
                if direction == 0 {
                    buffer.undo();
                    if let Some((before, after)) = undo.pop_back() {
                        assert_eq!(state(&buffer), before, "undo at {iteration}");
                        redo.push((before, after));
                    }
                } else {
                    buffer.redo();
                    if let Some((before, after)) = redo.pop() {
                        assert_eq!(state(&buffer), after, "redo at {iteration}");
                        undo.push_back((before, after));
                    }
                }
                assert_eq!(buffer.rope, oracle.rope, "Vec replay at {iteration}");
                assert_eq!(buffer.word_count(), oracle.word_count());
            }
            action => {
                let line = SourceLine::new(random() % buffer.line_count());
                let col = random() % (buffer.line_len(line) + 1);
                buffer.set_cursor(Position::new(line, col));
                let before = state(&buffer);
                let version = buffer.version();
                buffer.last_edit_time = None;
                match action {
                    2 => buffer.insert_text(
                        ["ab", "\n", "x\r\ny", "\u{4f60}\u{1f642}", " \t"][random() % 5],
                    ),
                    3 => {
                        let removed = (random() % 3).min(buffer.line_count() - line.index());
                        let inserted = (0..random() % 3).map(|n| format!("row {n}")).collect();
                        buffer.replace_line_range(line, removed, inserted, buffer.selection());
                    }
                    4 => buffer.record_single_line_edit(line, |text, _| text.push('z')),
                    _ => crate::markdown::LiveBuffer::new(&mut buffer).backspace(),
                }
                let after = state(&buffer);
                if version != buffer.version() || before.selection != after.selection {
                    undo.push_back((before, after));
                    if undo.len() > MAX_UNDO_STEPS {
                        undo.pop_front();
                    }
                    redo.clear();
                }
            }
        }
    }
}

#[test]
fn rope_history_preserves_single_character_coalescing() {
    let mut buffer = TextBuffer::from_str("seed\ntail", None);
    buffer.set_cursor(Position::new(SourceLine::new(0), 4));
    for text in ["a", "b", "c"] {
        buffer.last_edit_time = Some(Instant::now());
        buffer.insert_text(text);
    }
    assert_eq!(buffer.undo_stack.len(), 1);
    assert_eq!(
        buffer.undo_stack.back().unwrap().old_lines.to_lines(),
        vec!["seed"]
    );
    assert_eq!(
        buffer.undo_stack.back().unwrap().new_lines.to_lines(),
        vec!["seedabc"]
    );
    buffer.undo();
    assert_eq!(buffer.to_string_normalized(), "seed\ntail");
    buffer.redo();
    assert_eq!(buffer.to_string_normalized(), "seedabc\ntail");
    buffer.insert_text(" ");
    assert_eq!(buffer.undo_stack.len(), 2);
    buffer.undo();
    assert_eq!(buffer.to_string_normalized(), "seedabc\ntail");
}
