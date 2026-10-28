use inviscid::markdown::{BlockKind, MarkdownScanner, parse_inline_spans};

#[test]
fn latex_math_spans_preserve_source() {
    let input = r"Energy $E = mc^2$ and $$\frac{1}{2}$$.";
    let spans = parse_inline_spans(input, 4);

    assert_eq!(
        spans
            .iter()
            .map(|span| span.text.as_str())
            .collect::<String>(),
        input
    );
}

#[test]
fn unclosed_or_code_span_dollar_text_is_not_math() {
    let input = "$unfinished and `$x$`";
    let spans = parse_inline_spans(input, 0);
    assert_eq!(
        spans
            .iter()
            .map(|span| span.text.as_str())
            .collect::<String>(),
        input
    );
}

#[test]
fn invalid_latex_remains_literal_instead_of_becoming_a_blank_formula() {
    let input = r"Use $\unknowncommand{x}$ here";
    let spans = parse_inline_spans(input, 0);

    assert_eq!(
        spans
            .iter()
            .map(|span| span.text.as_str())
            .collect::<String>(),
        input
    );
}

#[test]
fn multiline_display_math_is_grouped_and_preserves_editable_source_lines() {
    let lines = vec![
        "Before".to_string(),
        "$$".to_string(),
        r"\frac{a}{b} = c".to_string(),
        "$$".to_string(),
        "After".to_string(),
    ];
    let parsed = MarkdownScanner::scan_document(&lines);

    assert!(matches!(
        parsed[1].kind,
        BlockKind::MathBlock { is_start: true }
    ));
    assert_eq!(parsed[2].kind, BlockKind::MathBlockContent);
    assert!(matches!(
        parsed[3].kind,
        BlockKind::MathBlock { is_start: false }
    ));
    assert_eq!(
        parsed
            .iter()
            .map(|line| line.raw_text.as_str())
            .collect::<Vec<_>>(),
        lines.iter().map(String::as_str).collect::<Vec<_>>()
    );
}

#[test]
fn invalid_and_empty_multiline_math_keep_their_block_boundaries() {
    for source in [
        vec!["$$", r"\unknowncommand{x}", "$$"],
        vec!["$$", "", "$$"],
    ] {
        let lines = source.into_iter().map(str::to_string).collect::<Vec<_>>();
        let parsed = MarkdownScanner::scan_document(&lines);

        assert!(matches!(
            parsed[0].kind,
            BlockKind::MathBlock { is_start: true }
        ));
        assert_eq!(parsed[1].kind, BlockKind::MathBlockContent);
        assert!(matches!(
            parsed[2].kind,
            BlockKind::MathBlock { is_start: false }
        ));
        assert_eq!(parsed[0].visible_text(false, 0, false), "");
    }
}
