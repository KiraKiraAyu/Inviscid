use super::*;

fn complete(buffer: &mut TextBuffer) {
    let revision = buffer.revision();
    let result = buffer.fingerprint_job().unwrap().run();
    assert!(buffer.install_fingerprints(result, revision));
    assert!(buffer.fingerprint_job().is_none());
}

#[test]
fn pending_fingerprints_reject_stale_results_and_restore_clean_after_undo() {
    let mut buffer = TextBuffer::from_bytes(b"first\nsecond", None).unwrap();
    assert!(!buffer.is_dirty());
    let initial_revision = buffer.revision();
    let stale = buffer.fingerprint_job().unwrap().run();
    buffer.record_single_line_edit(SourceLine::new(0), |line, _| *line = "other".into());
    assert!(buffer.is_dirty());
    assert!(!buffer.install_fingerprints(stale, initial_revision));
    buffer.undo();
    assert!(buffer.is_dirty());
    complete(&mut buffer);
    assert!(!buffer.is_dirty());
    buffer.redo();
    assert!(buffer.is_dirty());
    buffer.undo();
    assert!(!buffer.is_dirty());
}

#[test]
fn pending_save_rejects_old_baseline_and_preserves_ready_dirty_semantics() {
    let mut pending = TextBuffer::from_bytes(b"first\nsecond", None).unwrap();
    pending.record_single_line_edit(SourceLine::new(0), |line, _| *line = "saved".into());
    let revision = pending.revision();
    let before_save = pending.fingerprint_job().unwrap().run();
    pending.mark_as_saved();
    assert!(!pending.is_dirty());
    assert!(!pending.install_fingerprints(before_save, revision));
    complete(&mut pending);
    assert!(!pending.is_dirty());
    pending.undo();
    assert!(pending.is_dirty());
    pending.redo();
    assert!(!pending.is_dirty());
}

#[test]
fn ready_fingerprints_match_eager_after_pending_edits_and_snapshot_clones() {
    for text in ["", "first\nsecond", "\r\nword", "\u{754c}\n\u{1f642}"] {
        let mut pending = TextBuffer::from_bytes(text.as_bytes(), None).unwrap();
        let mut eager = TextBuffer::from_str(text, None);
        let mut retained = pending.clone();
        for buffer in [&mut pending, &mut eager] {
            buffer.record_single_line_edit(SourceLine::new(0), |line, _| *line = "changed".into());
        }
        complete(&mut pending);
        complete(&mut retained);
        assert!(!retained.is_dirty());
        assert_eq!(
            pending.saved_baseline_snapshot(),
            eager.saved_baseline_snapshot()
        );
        assert_eq!(pending.is_dirty(), eager.is_dirty());
        pending.undo();
        eager.undo();
        assert_eq!(pending.is_dirty(), eager.is_dirty());
    }
}
