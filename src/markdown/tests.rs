use super::projection::{ProjectedBlockKind, block_at_buffer, normalize_buffer_live_cursor};
use super::*;
use crate::buffer::{Position, SourceLine, TextBuffer};

#[test]
fn test_inline_focus_disclosure() {
    let lines = vec!["这是一段包含 **加粗文本** 的段落。".to_string()];
    let parsed = MarkdownScanner::scan_document(&lines);
    assert_eq!(parsed.len(), 1);

    let spans = &parsed[0].spans;
    for span in spans {
        if span.is_marker {
            assert!(!span.is_visible(false, 0, false));
        }
    }

    let marker_open = &spans[1];
    assert_eq!(marker_open.text, "**");
    assert!(marker_open.is_visible(true, 10, false));
}

#[test]
fn unclosed_fence_uses_code_markers_and_content() {
    let lines1 = vec!["```".to_string(), "hello".to_string()];
    let parsed1 = MarkdownScanner::scan_document(&lines1);
    assert_eq!(parsed1.len(), 2);
    let line0_visible: String = parsed1[0]
        .spans
        .iter()
        .filter(|s| s.is_visible(false, 0, false))
        .map(|s| s.text.as_str())
        .collect();
    assert_eq!(line0_visible, "");
    assert!(matches!(parsed1[1].kind, BlockKind::CodeBlockContent));

    let lines2 = vec!["```rust".to_string(), "hello".to_string()];
    let parsed2 = MarkdownScanner::scan_document(&lines2);
    assert_eq!(parsed2.len(), 2);
    let line1_visible: String = parsed2[0]
        .spans
        .iter()
        .filter(|s| s.is_visible(false, 0, false))
        .map(|s| s.text.as_str())
        .collect();
    assert_eq!(line1_visible, "rust");
    assert!(matches!(parsed2[1].kind, BlockKind::CodeBlockContent));
}

#[test]
fn test_blockquote_marker_only_starts_an_empty_quote_line() {
    let lines = vec![">".to_string(), "> hello".to_string()];
    let parsed = MarkdownScanner::scan_document(&lines);
    assert_eq!(parsed.len(), 2);
    assert!(matches!(parsed[0].kind, BlockKind::Paragraph));
    assert!(matches!(parsed[1].kind, BlockKind::Paragraph));
    assert!(parsed.iter().all(|line| line.quote.is_some()));

    let plain = MarkdownScanner::scan_document(&[">text".to_string()]);
    assert!(matches!(plain[0].kind, BlockKind::Paragraph));
}

#[test]
fn quoted_fenced_code_is_parsed_as_a_code_block() {
    let lines = vec![
        "> ```rust".to_string(),
        "> fn example() {}".to_string(),
        "> ```".to_string(),
        "> **after**".to_string(),
    ];
    let parsed = MarkdownScanner::scan_document(&lines);

    assert!(matches!(
        parsed[0].kind,
        BlockKind::CodeBlock {
            is_fence_start: true
        }
    ));
    assert_eq!(parsed[1].kind, BlockKind::CodeBlockContent);
    assert_eq!(
        parsed[1]
            .spans
            .iter()
            .filter(|span| !span.is_marker)
            .map(|span| span.text.as_str())
            .collect::<String>(),
        "fn example() {}"
    );
    assert!(matches!(
        parsed[2].kind,
        BlockKind::CodeBlock {
            is_fence_start: false
        }
    ));
    assert!(matches!(parsed[3].kind, BlockKind::Paragraph));
    assert!(parsed.iter().all(|line| line.quote.is_some()));
    assert!(
        parsed[3]
            .spans
            .iter()
            .any(|span| span.text == "after" && span.is_bold)
    );

    assert_eq!(
        MarkdownScanner::scan_range(&lines, 1..2, true),
        parsed[1..2]
    );
}

#[test]
fn quoted_markdown_uses_the_same_block_parsing_as_unquoted_markdown() {
    let lines = vec![
        "> # Heading".to_string(),
        "> - list item".to_string(),
        "> paragraph with **bold**".to_string(),
    ];
    let parsed = MarkdownScanner::scan_document(&lines);

    assert_eq!(parsed[0].kind, BlockKind::Heading { level: 1 });
    assert_eq!(parsed[1].kind, BlockKind::BulletList);
    assert_eq!(parsed[2].kind, BlockKind::Paragraph);
    assert!(parsed.iter().all(|line| line.quote.is_some()));
    assert!(
        parsed[0]
            .spans
            .iter()
            .any(|span| span.is_marker && span.text == "# ")
    );
    assert!(
        parsed[1]
            .spans
            .iter()
            .any(|span| span.is_marker && span.text == "- ")
    );
    assert!(
        parsed[2]
            .spans
            .iter()
            .any(|span| span.text == "bold" && span.is_bold)
    );
}

#[test]
fn nested_quote_uses_the_same_inner_block_parser() {
    let lines = vec![
        "> > # Nested heading".to_string(),
        "> > **bold**".to_string(),
    ];
    let parsed = MarkdownScanner::scan_document(&lines);

    assert_eq!(parsed[0].kind, BlockKind::Heading { level: 1 });
    assert_eq!(parsed[1].kind, BlockKind::Paragraph);
    assert!(parsed.iter().all(|line| line.quote.is_some()));
    assert!(
        parsed[0]
            .spans
            .iter()
            .any(|span| span.is_marker && span.text == "> > ")
    );
    assert!(
        parsed[1]
            .spans
            .iter()
            .any(|span| span.text == "bold" && span.is_bold)
    );
}

#[test]
fn quote_context_carries_depth_indentation_and_continuous_group_edges() {
    let lines = vec![
        "outside".to_string(),
        "> # Heading".to_string(),
        "> > **nested**".to_string(),
        ">   - indented item".to_string(),
        "> final".to_string(),
        "after".to_string(),
    ];
    let parsed = MarkdownScanner::scan_document(&lines);

    assert_eq!(parsed[0].quote, None);
    assert_eq!(
        parsed[1].quote,
        Some(QuoteContext {
            depth: 1,
            content_indent: 0,
            is_start: true,
            is_end: false,
        })
    );
    assert_eq!(
        parsed[2].quote,
        Some(QuoteContext {
            depth: 2,
            content_indent: 0,
            is_start: false,
            is_end: false,
        })
    );
    assert_eq!(
        parsed[3].quote,
        Some(QuoteContext {
            depth: 1,
            content_indent: 2,
            is_start: false,
            is_end: false,
        })
    );
    assert_eq!(
        parsed[4].quote,
        Some(QuoteContext {
            depth: 1,
            content_indent: 0,
            is_start: false,
            is_end: true,
        })
    );
    assert_eq!(parsed[5].quote, None);
}

#[test]
fn projected_block_keeps_inner_blocks_inside_a_continuous_blockquote() {
    let lines = vec![
        "outside".to_string(),
        "> # Heading".to_string(),
        "> - list item".to_string(),
        "> ```rust".to_string(),
        "> let value = 1;".to_string(),
        "> ```".to_string(),
        "> paragraph with **bold**".to_string(),
        "after".to_string(),
    ];
    let buf = TextBuffer::from_str(&lines.join("\n"), None);

    assert_eq!(
        block_at_buffer(&buf, SourceLine::new(0)).unwrap().kind,
        ProjectedBlockKind::Paragraph
    );
    assert_eq!(
        block_at_buffer(&buf, SourceLine::new(1)).unwrap().kind,
        ProjectedBlockKind::Heading { level: 1 }
    );
    assert_eq!(
        block_at_buffer(&buf, SourceLine::new(2)).unwrap().kind,
        ProjectedBlockKind::ListItem
    );
    let quoted_code = block_at_buffer(&buf, SourceLine::new(4)).unwrap();
    assert_eq!(
        quoted_code.kind,
        ProjectedBlockKind::CodeBlock { is_mermaid: false }
    );
    assert_eq!(
        quoted_code.source_range,
        SourceLine::new(3)..SourceLine::new(6)
    );
    assert_eq!(
        block_at_buffer(&buf, SourceLine::new(6)).unwrap().kind,
        ProjectedBlockKind::Quote
    );
    assert_eq!(
        block_at_buffer(&buf, SourceLine::new(7)).unwrap().kind,
        ProjectedBlockKind::Paragraph
    );
}

#[test]
fn test_hyperlink_parsing_and_focus_disclosure() {
    let lines = vec!["Check [GitHub](https://github.com) for more.".to_string()];
    let parsed = MarkdownScanner::scan_document(&lines);
    assert_eq!(parsed.len(), 1);

    let spans = &parsed[0].spans;
    assert_eq!(spans.len(), 5);

    assert_eq!(spans[0].text, "Check ");
    assert!(!spans[0].is_link);

    assert_eq!(spans[1].text, "[");
    assert!(spans[1].is_marker);
    assert!(spans[1].is_link);
    assert_eq!(spans[1].link_url, Some("https://github.com".to_string()));
    assert!(!spans[1].is_visible(false, 0, false));

    assert_eq!(spans[2].text, "GitHub");
    assert!(!spans[2].is_marker);
    assert!(spans[2].is_link);
    assert!(spans[2].is_visible(false, 0, false));

    assert_eq!(spans[3].text, "](https://github.com)");
    assert!(spans[3].is_marker);
    assert!(spans[3].is_link);
    assert!(!spans[3].is_visible(false, 0, false));

    assert!(spans[1].is_visible(true, 10, false));
    assert!(spans[3].is_visible(true, 10, false));
}

#[test]
fn test_autolink_parsing() {
    let lines = vec!["Visit <https://example.com> today.".to_string()];
    let parsed = MarkdownScanner::scan_document(&lines);
    assert_eq!(parsed.len(), 1);

    let spans = &parsed[0].spans;
    assert_eq!(spans[1].text, "<");
    assert_eq!(spans[2].text, "https://example.com");
    assert_eq!(spans[3].text, ">");
    assert!(spans[2].is_link);
    assert_eq!(spans[2].link_url, Some("https://example.com".to_string()));
}

#[test]
fn test_image_block_and_inline_parsing() {
    let lines = vec![
        "![Banner](https://example.com/banner.png)".to_string(),
        "Inline image ![icon](icon.png) in text".to_string(),
    ];
    let parsed = MarkdownScanner::scan_document(&lines);
    assert_eq!(parsed.len(), 2);

    assert_eq!(
        parsed[0].kind,
        BlockKind::Image {
            alt: "Banner".to_string(),
            url: "https://example.com/banner.png".to_string()
        }
    );

    assert_eq!(parsed[1].kind, BlockKind::Paragraph);
    let inline_spans = &parsed[1].spans;
    let icon_span = inline_spans.iter().find(|s| s.text == "icon").unwrap();
    assert!(icon_span.is_image);
}

#[test]
fn test_gfm_table_scanning_and_alignments() {
    let lines = vec![
        "| Header 1 | Header 2 | Header 3 |".to_string(),
        "| :--- | :---: | ---: |".to_string(),
        "| Row 1 **Bold** | `Code` | [Link](https://example.com) |".to_string(),
        "| Row 2 | Center | Right |".to_string(),
    ];

    let parsed = MarkdownScanner::scan_document(&lines);
    assert_eq!(parsed.len(), 4);

    match &parsed[0].kind {
        BlockKind::Table {
            is_header,
            is_delimiter,
            alignments,
            cells,
        } => {
            assert!(*is_header);
            assert!(!*is_delimiter);
            assert_eq!(alignments.len(), 3);
            assert_eq!(alignments[0], TableAlignment::Left);
            assert_eq!(alignments[1], TableAlignment::Center);
            assert_eq!(alignments[2], TableAlignment::Right);
            assert_eq!(cells.len(), 3);
            assert_eq!(cells[0].raw_content, "Header 1");
            assert_eq!(cells[1].raw_content, "Header 2");
            assert_eq!(cells[2].raw_content, "Header 3");
        }
        _ => panic!("Expected Table Header row"),
    }

    match &parsed[1].kind {
        BlockKind::Table {
            is_header,
            is_delimiter,
            alignments,
            ..
        } => {
            assert!(!*is_header);
            assert!(*is_delimiter);
            assert_eq!(alignments[0], TableAlignment::Left);
            assert_eq!(alignments[1], TableAlignment::Center);
            assert_eq!(alignments[2], TableAlignment::Right);
        }
        _ => panic!("Expected Table Delimiter row"),
    }

    match &parsed[2].kind {
        BlockKind::Table {
            is_header,
            is_delimiter,
            cells,
            ..
        } => {
            assert!(!*is_header);
            assert!(!*is_delimiter);
            assert_eq!(cells.len(), 3);
            assert_eq!(cells[0].raw_content, "Row 1 **Bold**");
            let bold_span = cells[0].spans.iter().find(|s| s.text == "Bold");
            assert!(bold_span.is_some());
            assert!(bold_span.unwrap().is_bold);

            let code_span = cells[1].spans.iter().find(|s| s.is_code && !s.is_marker);
            assert!(code_span.is_some());
            assert_eq!(code_span.unwrap().text, "Code");

            let link_span = cells[2].spans.iter().find(|s| s.is_link && !s.is_marker);
            assert!(link_span.is_some());
            assert_eq!(
                link_span.unwrap().link_url,
                Some("https://example.com".to_string())
            );
        }
        _ => panic!("Expected Table Data row"),
    }
}

#[test]
fn test_bold_italic_parsing_and_combinations() {
    let lines1 = vec!["Here is ***Bold Italic*** text.".to_string()];
    let parsed1 = MarkdownScanner::scan_document(&lines1);
    assert_eq!(parsed1.len(), 1);
    let spans1 = &parsed1[0].spans;
    assert_eq!(spans1.len(), 5);
    assert_eq!(spans1[1].text, "***");
    assert!(spans1[1].is_marker);
    assert!(spans1[1].is_bold);
    assert!(spans1[1].is_italic);

    assert_eq!(spans1[2].text, "Bold Italic");
    assert!(!spans1[2].is_marker);
    assert!(spans1[2].is_bold);
    assert!(spans1[2].is_italic);

    assert_eq!(spans1[3].text, "***");
    assert!(spans1[3].is_marker);

    assert!(!spans1[1].is_visible(false, 0, false));
    assert!(spans1[2].is_visible(false, 0, false));
    assert!(spans1[1].is_visible(true, 12, false));

    let lines2 = vec!["Here is ___Underscore Bold Italic___ text.".to_string()];
    let parsed2 = MarkdownScanner::scan_document(&lines2);
    let spans2 = &parsed2[0].spans;
    let bi2 = spans2
        .iter()
        .find(|s| s.text == "Underscore Bold Italic")
        .unwrap();
    assert!(bi2.is_bold && bi2.is_italic && !bi2.is_marker);

    let lines3 = vec!["Combined **_Asterisk Under_** and _**Under Asterisk**_.".to_string()];
    let parsed3 = MarkdownScanner::scan_document(&lines3);
    let spans3 = &parsed3[0].spans;
    let bi3_1 = spans3.iter().find(|s| s.text == "Asterisk Under").unwrap();
    assert!(bi3_1.is_bold && bi3_1.is_italic && !bi3_1.is_marker);
    let bi3_2 = spans3.iter().find(|s| s.text == "Under Asterisk").unwrap();
    assert!(bi3_2.is_bold && bi3_2.is_italic && !bi3_2.is_marker);

    let lines4 = vec!["**Bold** and *Italic* and ***Bold Italic*** together.".to_string()];
    let parsed4 = MarkdownScanner::scan_document(&lines4);
    let spans4 = &parsed4[0].spans;
    let bold_span = spans4.iter().find(|s| s.text == "Bold").unwrap();
    assert!(bold_span.is_bold && !bold_span.is_italic);
    let italic_span = spans4.iter().find(|s| s.text == "Italic").unwrap();
    assert!(!italic_span.is_bold && italic_span.is_italic);
    let bi_span = spans4.iter().find(|s| s.text == "Bold Italic").unwrap();
    assert!(bi_span.is_bold && bi_span.is_italic);
}

#[test]
fn test_projected_block_ranges_and_position_normalization() {
    let empty_buf = TextBuffer::from_str("\n", None);
    assert_eq!(
        normalize_buffer_live_cursor(&empty_buf, Position::new(SourceLine::new(1), 0)),
        Position::new(SourceLine::new(0), 0)
    );

    let doc_lines = [
        "# Heading 1",
        "",
        "Paragraph text",
        "",
        "```rust",
        "fn main() {",
        "    0",
        "}",
        "```",
        "",
        "| A | B |",
        "|---|---|",
        "| 1 | 2 |",
        "",
        "1. List item 1",
        "2. List item 2",
        "",
        "---",
    ];
    let doc = TextBuffer::from_str(&doc_lines.join("\n"), None);

    assert_eq!(
        block_at_buffer(&doc, SourceLine::new(0)).unwrap().kind,
        ProjectedBlockKind::Heading { level: 1 }
    );
    assert_eq!(
        block_at_buffer(&doc, SourceLine::new(2)).unwrap().kind,
        ProjectedBlockKind::Paragraph
    );
    let code = block_at_buffer(&doc, SourceLine::new(6)).unwrap();
    assert_eq!(
        code.kind,
        ProjectedBlockKind::CodeBlock { is_mermaid: false }
    );
    assert_eq!(code.source_range, SourceLine::new(4)..SourceLine::new(9));
    assert_eq!(code.editable_lines, SourceLine::new(4)..SourceLine::new(8));

    let table = block_at_buffer(&doc, SourceLine::new(10)).unwrap();
    assert_eq!(
        table.kind,
        ProjectedBlockKind::Table {
            delimiter_line: SourceLine::new(11),
        }
    );
    assert_eq!(table.source_range, SourceLine::new(10)..SourceLine::new(13));
    assert_eq!(
        block_at_buffer(&doc, SourceLine::new(14)).unwrap().kind,
        ProjectedBlockKind::ListItem
    );
    assert_eq!(
        block_at_buffer(&doc, SourceLine::new(17)).unwrap().kind,
        ProjectedBlockKind::ThematicBreak
    );

    assert_eq!(
        normalize_buffer_live_cursor(&doc, Position::new(SourceLine::new(6), 4)),
        Position::new(SourceLine::new(6), 4)
    );
    assert_eq!(
        normalize_buffer_live_cursor(&doc, Position::new(SourceLine::new(8), 0)),
        Position::new(SourceLine::new(7), 1)
    );
    assert_eq!(
        normalize_buffer_live_cursor(&doc, Position::new(SourceLine::new(9), 0)),
        Position::new(SourceLine::new(7), 0)
    );
    assert_eq!(
        normalize_buffer_live_cursor(&doc, Position::new(SourceLine::new(10), 2)),
        Position::new(SourceLine::new(10), 2)
    );
    assert_eq!(
        normalize_buffer_live_cursor(&doc, Position::new(SourceLine::new(12), 3)),
        Position::new(SourceLine::new(12), 3)
    );
}

#[test]
fn test_scan_range_equivalence() {
    let doc_lines: Vec<String> = vec![
        "# Heading 1".to_string(),
        "".to_string(),
        "Paragraph line 1".to_string(),
        "Paragraph line 2 with **bold**".to_string(),
        "".to_string(),
        "```rust".to_string(),
        "fn hello() {}".to_string(),
        "```".to_string(),
        "".to_string(),
        "| Col 1 | Col 2 |".to_string(),
        "| --- | --- |".to_string(),
        "| Val 1 | Val 2 |".to_string(),
        "".to_string(),
        "- [x] Task 1".to_string(),
    ];

    let full_scan = MarkdownScanner::scan_document(&doc_lines);
    assert_eq!(full_scan.len(), doc_lines.len());

    let full_range = MarkdownScanner::scan_range(&doc_lines, 0..doc_lines.len(), false);
    assert_eq!(full_scan, full_range);

    let paragraph_range = MarkdownScanner::scan_range(&doc_lines, 2..4, false);
    assert_eq!(paragraph_range, full_scan[2..4].to_vec());

    let code_content_range = MarkdownScanner::scan_range(&doc_lines, 6..7, true);
    assert_eq!(code_content_range, full_scan[6..7].to_vec());
}

#[test]
fn inline_math_spans_preserve_tex_and_source_ranges() {
    let spans = parse_inline_spans(r"Energy $E = mc^2$ and $$\frac{1}{2}$$.", 4);
    let math_spans: Vec<_> = spans.iter().filter_map(|span| span.math.as_ref()).collect();

    assert_eq!(math_spans.len(), 2);
    assert_eq!(math_spans[0].tex, "E = mc^2");
    assert_eq!(math_spans[0].source, "$E = mc^2$");
    assert!(!math_spans[0].is_display);
    assert_eq!(math_spans[1].tex, r"\frac{1}{2}");
    assert_eq!(math_spans[1].source, r"$$\frac{1}{2}$$");
    assert!(math_spans[1].is_display);
    assert!(math_spans[0].rendered.asset_path.starts_with("math/"));
    assert_eq!(
        spans
            .iter()
            .find(|span| span.math.is_some())
            .unwrap()
            .group_range,
        (11, 21)
    );
}

#[test]
fn display_math_block_spans_keep_the_full_tex_source() {
    let lines = vec![
        "$$".to_string(),
        r"\frac{a}{b} = c".to_string(),
        "$$".to_string(),
    ];
    let parsed = MarkdownScanner::scan_document(&lines);
    let math = parsed[0]
        .spans
        .iter()
        .find_map(|span| span.math.as_ref())
        .unwrap();

    assert_eq!(math.tex, r"\frac{a}{b} = c");
    assert!(math.is_display);
}

#[test]
fn math_block_range_ignores_delimiters_inside_code_fences() {
    let lines = vec![
        "```md".to_string(),
        "$$".to_string(),
        "```".to_string(),
        "$$".to_string(),
        "x".to_string(),
        "$$".to_string(),
    ];
    let projection = QuoteProjection::from_lines(&lines);

    assert_eq!(
        math_block_range_with_projection(&projection, crate::buffer::SourceLine::new(3)),
        Some((
            crate::buffer::SourceLine::new(3),
            crate::buffer::SourceLine::new(5)
        ))
    );
    assert_eq!(
        math_block_range_with_projection(&projection, crate::buffer::SourceLine::new(4)),
        Some((
            crate::buffer::SourceLine::new(3),
            crate::buffer::SourceLine::new(5)
        ))
    );
    assert_eq!(
        math_block_range_with_projection(&projection, crate::buffer::SourceLine::new(1)),
        None
    );
}

#[test]
fn unclosed_math_delimiter_does_not_change_incremental_scan_state() {
    let lines = vec![
        "Before".to_string(),
        "$$".to_string(),
        "unfinished formula".to_string(),
    ];
    let full_scan = MarkdownScanner::scan_document(&lines);
    let partial_scan = MarkdownScanner::scan_range(&lines, 2..3, false);

    assert_eq!(partial_scan, full_scan[2..3]);
}

#[test]
fn test_nested_inline_code_inside_bold() {
    let lines = vec!["2. **创建 `app.js` 文件**:".to_string()];
    let parsed = MarkdownScanner::scan_document(&lines);
    assert_eq!(parsed.len(), 1);

    let spans = &parsed[0].spans;
    let code_span = spans.iter().find(|s| s.is_code && !s.is_marker);
    assert!(code_span.is_some());
    let code = code_span.unwrap();
    assert_eq!(code.text, "app.js");
    assert!(code.is_bold);

    let code_markers: Vec<&InlineSpan> =
        spans.iter().filter(|s| s.is_code && s.is_marker).collect();
    assert_eq!(code_markers.len(), 2);
}

#[test]
fn test_nested_formatting_combinations() {
    let l1 = vec!["*创建 `app.js` 文件*".to_string()];
    let p1 = MarkdownScanner::scan_document(&l1);
    let code1 = p1[0]
        .spans
        .iter()
        .find(|s| s.is_code && !s.is_marker)
        .unwrap();
    assert_eq!(code1.text, "app.js");
    assert!(code1.is_italic);

    let l2 = vec!["~~删除 `temp.js` 文件~~".to_string()];
    let p2 = MarkdownScanner::scan_document(&l2);
    let code2 = p2[0]
        .spans
        .iter()
        .find(|s| s.is_code && !s.is_marker)
        .unwrap();
    assert_eq!(code2.text, "temp.js");
    assert!(code2.is_strikethrough);

    let l3 = vec!["[查看 `README.md`](https://example.com)".to_string()];
    let p3 = MarkdownScanner::scan_document(&l3);
    let code3 = p3[0]
        .spans
        .iter()
        .find(|s| s.is_code && !s.is_marker)
        .unwrap();
    assert_eq!(code3.text, "README.md");
    assert!(code3.is_link);
    assert_eq!(code3.link_url, Some("https://example.com".to_string()));

    let l4 = vec!["**配置 `foo * bar` 项**".to_string()];
    let p4 = MarkdownScanner::scan_document(&l4);
    let code4 = p4[0]
        .spans
        .iter()
        .find(|s| s.is_code && !s.is_marker)
        .unwrap();
    assert_eq!(code4.text, "foo * bar");
    assert!(code4.is_bold);
}

#[test]
fn test_nested_focus_disclosure() {
    let line = "2. **创建 `app.js` 文件**:";
    let parsed = MarkdownScanner::scan_document(&[line.to_string()]);
    let spans = &parsed[0].spans;

    let bold_open = spans
        .iter()
        .find(|s| s.text == "**" && s.span_range.0 == 3)
        .unwrap();
    let code_open = spans
        .iter()
        .find(|s| s.text == "`" && s.span_range.0 == 8)
        .unwrap();
    let code_close = spans
        .iter()
        .find(|s| s.text == "`" && s.span_range.0 == 15)
        .unwrap();
    let bold_close = spans
        .iter()
        .find(|s| s.text == "**" && s.span_range.0 == 19)
        .unwrap();

    assert!(!bold_open.is_visible(false, 0, false));
    assert!(!code_open.is_visible(false, 0, false));
    assert!(!code_close.is_visible(false, 0, false));
    assert!(!bold_close.is_visible(false, 0, false));

    assert!(bold_open.is_visible(true, 18, false));
    assert!(!code_open.is_visible(true, 18, false));
    assert!(!code_close.is_visible(true, 18, false));
    assert!(bold_close.is_visible(true, 18, false));

    assert!(bold_open.is_visible(true, 12, false));
    assert!(code_open.is_visible(true, 12, false));
    assert!(code_close.is_visible(true, 12, false));
    assert!(bold_close.is_visible(true, 12, false));
}

#[test]
fn test_selection_disclosure_hides_markers_at_group_end() {
    let parsed = MarkdownScanner::scan_document(&["This is **bold** text.".to_string()]);
    let spans = &parsed[0].spans;
    let bold_open = spans
        .iter()
        .find(|span| span.text == "**" && span.span_range.0 == 8)
        .unwrap();

    assert_eq!(bold_open.group_range, (8, 16));
    assert!(bold_open.is_visible(true, 16, false));
    assert!(!bold_open.is_visible_for_selection(true, 16, false, true));
}

#[test]
fn test_block_prefix_parsing() {
    assert_eq!(
        parse_block_prefix("  # Title"),
        Some(ParsedPrefix {
            kind: BlockPrefix::Heading { level: 1 },
            total_len: 4,
            leading_spaces: 2,
        })
    );
    assert_eq!(
        parse_block_prefix("- [X] Done task"),
        Some(ParsedPrefix {
            kind: BlockPrefix::Task {
                checked: true,
                marker: '-'
            },
            total_len: 6,
            leading_spaces: 0,
        })
    );
    assert_eq!(
        parse_block_prefix("  * [ ] Todo"),
        Some(ParsedPrefix {
            kind: BlockPrefix::Task {
                checked: false,
                marker: '*'
            },
            total_len: 8,
            leading_spaces: 2,
        })
    );
    assert_eq!(
        parse_block_prefix("12. Ordered Item"),
        Some(ParsedPrefix {
            kind: BlockPrefix::Ordered { num: 12 },
            total_len: 4,
            leading_spaces: 0,
        })
    );
}

#[test]
fn test_continuation_and_toggle() {
    assert_eq!(
        get_smart_continuation_prefix("  - [X] Task"),
        Some("  - [ ] ".to_string())
    );
    assert_eq!(
        toggle_task_checkbox("- [ ] Task 1"),
        Some("- [x] Task 1".to_string())
    );
    assert_eq!(
        toggle_task_checkbox("- [X] Task 2"),
        Some("- [ ] Task 2".to_string())
    );
    assert_eq!(
        toggle_task_checkbox("  * [x] Task 3"),
        Some("  * [ ] Task 3".to_string())
    );
}

#[test]
fn test_fenced_code_block_wasm_syntax_highlighting_and_incremental_scan() {
    use crate::syntax::{SyntaxToken, register_global_wasm_grammar};

    let wasm_bytes = include_bytes!("../../tests/fixtures/tree-sitter-json.wasm");
    register_global_wasm_grammar("json", wasm_bytes, None)
        .expect("Failed to register JSON WASM grammar");

    let lines = vec![
        "# Config".to_string(),
        "```json".to_string(),
        "{\"port\": 8080,".to_string(),
        " \"enabled\": true}".to_string(),
        "```".to_string(),
    ];

    let parsed = MarkdownScanner::scan_document(&lines);
    assert_eq!(parsed.len(), 5);
    assert_eq!(parsed[2].kind, BlockKind::CodeBlockContent);
    assert_eq!(parsed[3].kind, BlockKind::CodeBlockContent);

    // Line 2: {"port": 8080,
    assert!(
        parsed[2]
            .spans
            .iter()
            .any(|s| { s.text == "\"port\"" && s.syntax_token == Some(SyntaxToken::Variable) })
    );
    assert!(
        parsed[2]
            .spans
            .iter()
            .any(|s| { s.text == "8080" && s.syntax_token == Some(SyntaxToken::Number) })
    );

    // Line 3:  "enabled": true}
    assert!(
        parsed[3]
            .spans
            .iter()
            .any(|s| { s.text == "\"enabled\"" && s.syntax_token == Some(SyntaxToken::Variable) })
    );
    assert!(
        parsed[3]
            .spans
            .iter()
            .any(|s| { s.text == "true" && s.syntax_token == Some(SyntaxToken::Number) })
    );

    // Incremental scan starting mid-block (initial_in_code_fence = true) must resolve
    // the enclosing ```json fence and preserve multi-line AST context.
    let mid_scan = MarkdownScanner::scan_range(&lines, 3..4, true);
    assert_eq!(mid_scan.len(), 1);
    assert_eq!(mid_scan[0].kind, BlockKind::CodeBlockContent);
    assert_eq!(mid_scan[0].spans, parsed[3].spans);
}

#[test]
fn mermaid_fences_render_valid_diagrams_and_preserve_their_source() {
    let lines = [
        "Before".to_string(),
        "``` Mermaid ".to_string(),
        "flowchart LR".to_string(),
        "    A[Start] --> B[Done]".to_string(),
        "```".to_string(),
        "After".to_string(),
    ];
    let parsed = MarkdownScanner::scan_document(&lines);

    assert!(matches!(
        parsed[1].kind,
        BlockKind::MermaidBlock {
            is_fence_start: true
        }
    ));
    assert_eq!(parsed[2].kind, BlockKind::MermaidBlockContent);
    assert_eq!(parsed[3].kind, BlockKind::MermaidBlockContent);
    assert!(matches!(
        parsed[4].kind,
        BlockKind::MermaidBlock {
            is_fence_start: false
        }
    ));
    assert!(parsed[1].spans.iter().any(|span| span.mermaid.is_some()));
    assert_eq!(
        parsed.iter().map(|line| &line.raw_text).collect::<Vec<_>>(),
        lines.iter().collect::<Vec<_>>()
    );
    let mid_scan = MarkdownScanner::scan_range(&lines, 2..3, true);
    assert_eq!(mid_scan[0].kind, BlockKind::MermaidBlockContent);
    assert_eq!(mid_scan[0].spans, parsed[2].spans);

    let buf = TextBuffer::from_str(&lines.join("\n"), None);
    let mermaid = block_at_buffer(&buf, SourceLine::new(2)).unwrap();
    assert_eq!(
        mermaid.kind,
        ProjectedBlockKind::CodeBlock { is_mermaid: true }
    );
    assert_eq!(mermaid.source_range, SourceLine::new(1)..SourceLine::new(5));
}

#[test]
fn invalid_mermaid_and_other_fenced_languages_remain_code_blocks() {
    let lines = [
        "```mermaid".to_string(),
        "not a diagram".to_string(),
        "```".to_string(),
        "```rust".to_string(),
        "fn main() {}".to_string(),
        "```".to_string(),
    ];
    let parsed = MarkdownScanner::scan_document(&lines);

    assert!(matches!(
        parsed[0].kind,
        BlockKind::CodeBlock {
            is_fence_start: true
        }
    ));
    assert_eq!(parsed[1].kind, BlockKind::CodeBlockContent);
    assert!(matches!(
        parsed[3].kind,
        BlockKind::CodeBlock {
            is_fence_start: true
        }
    ));
    assert_eq!(parsed[4].kind, BlockKind::CodeBlockContent);
}

#[test]
fn mermaid_block_range_includes_both_fences_but_excludes_neighboring_lines() {
    let lines = [
        "before".to_string(),
        "```mermaid".to_string(),
        "flowchart TD".to_string(),
        "A --> B".to_string(),
        "```".to_string(),
        "after".to_string(),
    ];
    let projection = QuoteProjection::from_lines(&lines);

    assert_eq!(
        mermaid_block_range_with_projection(&projection, crate::buffer::SourceLine::new(3)),
        Some((
            crate::buffer::SourceLine::new(1),
            crate::buffer::SourceLine::new(4)
        ))
    );
    assert_eq!(
        mermaid_block_range_with_projection(&projection, crate::buffer::SourceLine::new(5)),
        None
    );
}
#[test]
fn quote_projection_reads_source_sequentially_without_random_line_lookups() {
    use crate::buffer::{LineSource, SourceLine};
    use crate::markdown::QuoteProjection;
    use std::borrow::Cow;
    use std::cell::Cell;

    struct SequentialSource {
        lines: Vec<String>,
        lookups: Cell<usize>,
    }

    impl LineSource for SequentialSource {
        fn line_count(&self) -> usize {
            self.lines.len()
        }

        fn source_line(&self, line: SourceLine) -> Cow<'_, str> {
            self.lookups.set(self.lookups.get() + 1);
            Cow::Borrowed(&self.lines[line.index()])
        }

        fn source_lines(&self) -> impl Iterator<Item = Cow<'_, str>> {
            self.lines.iter().map(|line| Cow::Borrowed(line.as_str()))
        }
    }

    let source = SequentialSource {
        lines: [
            "> 引用🙂",
            "```text",
            "> 原样保留",
            "```",
            "> 尾部",
            "```",
            "> 未闭合",
        ]
        .map(String::from)
        .to_vec(),
        lookups: Cell::new(0),
    };
    let projection = QuoteProjection::from_source(&source);
    assert_eq!(
        projection.normalized_lines(),
        [
            "引用🙂",
            "```text",
            "> 原样保留",
            "```",
            "尾部",
            "```",
            "> 未闭合"
        ]
    );
    assert_eq!(
        projection.quote_prefixes(),
        &[Some(2), None, None, None, Some(2), None, None]
    );
    assert_eq!(source.lookups.get(), 0);
}

#[test]
fn quote_projection_local_update_reads_only_changed_lines_and_matches_rebuild() {
    use crate::buffer::{EditDelta, LineSource, SourceLine};
    use std::{borrow::Cow, cell::RefCell};

    struct TrackedSource {
        lines: Vec<String>,
        reads: RefCell<Vec<usize>>,
    }
    impl LineSource for TrackedSource {
        fn line_count(&self) -> usize {
            self.lines.len()
        }
        fn source_line(&self, line: SourceLine) -> Cow<'_, str> {
            self.reads.borrow_mut().push(line.index());
            Cow::Borrowed(&self.lines[line.index()])
        }
    }

    let mut source = TrackedSource {
        lines: [
            "before",
            "> quote",
            "```text",
            "> literal",
            "```",
            "> tail",
            "```",
            "> unmatched",
        ]
        .map(String::from)
        .to_vec(),
        reads: RefCell::new(Vec::new()),
    };
    let mut projection = QuoteProjection::from_source(&source);
    for (index, replacement) in [
        (1, "> > 中文🙂"),
        (3, "> literal changed"),
        (5, "plain"),
        (7, "> unmatched changed"),
    ] {
        source.lines[index] = replacement.to_string();
        source.reads.borrow_mut().clear();
        assert!(projection.try_update_source(
            &source,
            EditDelta {
                start_line: SourceLine::new(index),
                old_line_count: 1,
                new_line_count: 1,
            }
        ));
        assert_eq!(*source.reads.borrow(), vec![index]);
        let full = QuoteProjection::from_lines(&source.lines);
        assert_eq!(projection.normalized_lines(), full.normalized_lines());
        assert_eq!(projection.quote_prefixes(), full.quote_prefixes());
    }
}

#[test]
fn quote_projection_rejects_structural_edits_without_partial_changes() {
    use crate::buffer::{EditDelta, SourceLine};
    let original = ["first", "> quote", "```text", "content", "```"].map(String::from);
    let mut projection = QuoteProjection::from_lines(&original);
    let normalized_before = projection.normalized_lines().to_vec();
    for (start, old_count, replacement) in [
        (0, 2, vec!["changed", "```"]),
        (2, 1, vec!["```rust"]),
        (2, 1, vec!["plain"]),
        (1, 1, vec!["one", "two"]),
        (1, 1, vec![]),
    ] {
        let mut edited = original.to_vec();
        edited.splice(
            start..start + old_count,
            replacement.iter().map(|s| s.to_string()),
        );
        assert!(!projection.try_update_source(
            &edited,
            EditDelta {
                start_line: SourceLine::new(start),
                old_line_count: old_count,
                new_line_count: replacement.len(),
            }
        ));
        assert_eq!(projection.normalized_lines(), normalized_before);
        assert_eq!(
            projection.quote_prefixes(),
            &[None, Some(2), None, None, None]
        );
    }
}

#[test]
fn quote_projection_local_updates_match_rebuild_across_fence_and_quote_transitions() {
    use crate::buffer::{EditDelta, SourceLine};
    let variants = [
        "plain",
        "> quote",
        "> > nested",
        "> ```text",
        "```",
        "```rust",
        "",
        "中文🙂",
    ];
    for middle in variants {
        let original = ["before", "```", "> literal", "```", middle, "> tail"].map(String::from);
        for index in 0..original.len() {
            for replacement in variants {
                let mut projection = QuoteProjection::from_lines(&original);
                let before_lines = projection.normalized_lines().to_vec();
                let before_prefixes = projection.quote_prefixes().to_vec();
                let mut edited = original.to_vec();
                edited[index] = replacement.to_string();
                if projection.try_update_source(
                    &edited,
                    EditDelta {
                        start_line: SourceLine::new(index),
                        old_line_count: 1,
                        new_line_count: 1,
                    },
                ) {
                    let full = QuoteProjection::from_lines(&edited);
                    assert_eq!(projection.normalized_lines(), full.normalized_lines());
                    assert_eq!(projection.quote_prefixes(), full.quote_prefixes());
                } else {
                    assert_eq!(projection.normalized_lines(), before_lines);
                    assert_eq!(projection.quote_prefixes(), before_prefixes);
                }
            }
        }
    }
}
#[test]
fn blank_run_terminal_separator_classification_matches_projection() {
    use crate::buffer::SourceLine;

    for prefix in [
        "",
        "body\n",
        "body\n\n",
        "body\n\n\n",
        "\n",
        "\n\n",
        "\n\n\n",
    ] {
        for suffix in ["# heading", "paragraph", "## next"] {
            let source = format!("{prefix}{suffix}");
            let lines: Vec<String> = source.split('\n').map(str::to_owned).collect();
            let block_line = SourceLine::new(lines.len() - 1);
            let expected = if block_line.index() > 0 && lines[block_line.index() - 1].is_empty() {
                is_syntactic_separator(&lines, block_line - 1)
            } else {
                false
            };
            assert_eq!(
                blank_run_before_ends_with_separator(&lines, block_line),
                expected,
                "{source:?}",
            );
        }
    }
}
#[test]
fn code_fence_footer_projects_to_last_editable_line() {
    for text in ["```rust\nx\n```", "```rust\n```", "```rust\nx"] {
        let buf = TextBuffer::from_str(text, None);
        let last = SourceLine::new(buf.line_count() - 1);
        let position = normalize_buffer_live_cursor(&buf, Position::new(last, 0));
        let expected = if text.ends_with("```") {
            last - 1
        } else {
            last
        };
        assert_eq!(position.line, expected, "{text}");
    }
}

#[test]
fn projected_block_unifies_containers_separators_and_normalization() {
    use crate::buffer::{Position, SourceLine, StructureIndex, TextBuffer};

    let doc = [
        "# Title",
        "",
        "Paragraph",
        "```mermaid",
        "graph TD",
        "```",
        "> $$",
        "> x + y",
        "> $$",
        "| H1 | H2 |",
        "| --- | --- |",
        "| A | B |",
        "---",
    ]
    .join("\n");
    let buffer = TextBuffer::from_str(&doc, None);
    let index = StructureIndex::new(&buffer);

    let heading = super::projection::block_at(&buffer, &index, SourceLine::new(0)).unwrap();
    assert_eq!(
        heading.kind,
        super::projection::ProjectedBlockKind::Heading { level: 1 }
    );
    assert!(!heading.is_enclosed_container);
    assert_eq!(heading.editable_min_col(&buffer, SourceLine::new(0)), 2);

    let sep = super::projection::block_at(&buffer, &index, SourceLine::new(1)).unwrap();
    assert_eq!(
        sep.kind,
        super::projection::ProjectedBlockKind::BlankSeparator
    );
    assert!(!sep.is_editable_line(SourceLine::new(1)));

    let mermaid = super::projection::block_at(&buffer, &index, SourceLine::new(4)).unwrap();
    assert_eq!(
        mermaid.kind,
        super::projection::ProjectedBlockKind::CodeBlock { is_mermaid: true }
    );
    assert!(mermaid.is_enclosed_container);
    assert_eq!(mermaid.source_range, SourceLine::new(3)..SourceLine::new(6));
    assert_eq!(
        mermaid.editable_lines,
        SourceLine::new(3)..SourceLine::new(5)
    );
    assert!(!mermaid.is_editable_line(SourceLine::new(5)));
    assert_eq!(
        mermaid.next_intra_block_editable_line(SourceLine::new(4)),
        None
    );
    assert_eq!(mermaid.editable_min_col(&buffer, SourceLine::new(3)), 3);
    assert_eq!(mermaid.editable_min_col(&buffer, SourceLine::new(4)), 0);

    let math = super::projection::block_at(&buffer, &index, SourceLine::new(7)).unwrap();
    assert_eq!(math.kind, super::projection::ProjectedBlockKind::MathBlock);
    assert!(math.is_enclosed_container);
    assert!(!math.is_editable_line(SourceLine::new(6)));
    assert!(math.is_editable_line(SourceLine::new(7)));
    assert!(!math.is_editable_line(SourceLine::new(8)));
    assert_eq!(
        math.next_intra_block_editable_line(SourceLine::new(7)),
        None
    );
    assert_eq!(math.editable_min_col(&buffer, SourceLine::new(7)), 2);

    let table = super::projection::block_at(&buffer, &index, SourceLine::new(11)).unwrap();
    assert_eq!(
        table.kind,
        super::projection::ProjectedBlockKind::Table {
            delimiter_line: SourceLine::new(10),
        }
    );
    assert!(table.is_enclosed_container);
    assert!(!table.is_editable_line(SourceLine::new(10)));
    assert!(table.is_editable_line(SourceLine::new(11)));
    assert_eq!(
        table.next_intra_block_editable_line(SourceLine::new(9)),
        None
    );

    // Cursor normalization across closing fence, math delimiters, and separators.
    assert_eq!(
        super::projection::normalize_live_cursor(
            &buffer,
            &index,
            Position::new(SourceLine::new(5), 0),
        ),
        Position::new(SourceLine::new(4), 8)
    );
    assert_eq!(
        super::projection::normalize_live_cursor(
            &buffer,
            &index,
            Position::new(SourceLine::new(6), 0),
        ),
        Position::new(SourceLine::new(7), 2)
    );
    assert_eq!(
        super::projection::normalize_live_cursor(
            &buffer,
            &index,
            Position::new(SourceLine::new(8), 0),
        ),
        Position::new(SourceLine::new(7), 7)
    );
    assert_eq!(
        super::projection::normalize_live_cursor(
            &buffer,
            &index,
            Position::new(SourceLine::new(1), 0),
        ),
        Position::new(SourceLine::new(0), 2)
    );
    assert_eq!(
        super::projection::adjacent_live_editable_line(&buffer, &index, SourceLine::new(7), true,),
        Some(SourceLine::new(9))
    );
    assert_eq!(
        super::projection::adjacent_live_editable_line(&buffer, &index, SourceLine::new(11), false,),
        Some(SourceLine::new(9))
    );
}

#[test]
fn live_buffer_delete_forward_respects_container_boundaries_and_merges_inside() {
    use crate::buffer::{Position, SourceLine, TextBuffer};

    // Code block: opener header and last body line do not pull adjacent lines,
    // while interior body lines still merge on forward Delete.
    let code_doc = "```rust\nline1\nline2\n```\nafter";
    for (row, col, expected) in [
        (0, 7, code_doc),
        (2, 5, code_doc),
        (1, 5, "```rust\nline1line2\n```\nafter"),
    ] {
        let mut live = TextBuffer::from_str(code_doc, None);
        live.set_cursor(Position::new(SourceLine::new(row), col));
        LiveBuffer::new(&mut live).delete_forward();
        assert_eq!(
            live.to_string_normalized(),
            expected,
            "code delete at ({row}, {col})"
        );
    }

    // Source mode performs raw forward-Delete even across closing fence lines.
    let mut source = TextBuffer::from_str(code_doc, None);
    source.set_cursor(Position::new(SourceLine::new(2), 5));
    source.delete_forward();
    assert_eq!(
        source.to_string_normalized(),
        "```rust\nline1\nline2```\nafter"
    );
    source.undo();
    assert_eq!(source.to_string_normalized(), code_doc);

    // Mermaid fence and Table boundaries are enclosed containers too.
    let mermaid_doc = "```mermaid\nA --> B\n```\nafter";
    let mut mermaid = TextBuffer::from_str(mermaid_doc, None);
    mermaid.set_cursor(Position::new(SourceLine::new(1), 7));
    LiveBuffer::new(&mut mermaid).delete_forward();
    assert_eq!(mermaid.to_string_normalized(), mermaid_doc);

    let table_doc = "| H1 | H2 |\n| --- | --- |\n| A | B |\nafter";
    for (row, col) in [(0, 11), (2, 9)] {
        let mut table = TextBuffer::from_str(table_doc, None);
        table.set_cursor(Position::new(SourceLine::new(row), col));
        LiveBuffer::new(&mut table).delete_forward();
        assert_eq!(
            table.to_string_normalized(),
            table_doc,
            "table delete at ({row}, {col})"
        );
    }
}

#[test]
fn live_buffer_handles_block_degeneration_math_and_single_undo() {
    use crate::buffer::{Position, SourceLine, TextBuffer};

    // Heading prefix backspace degenerates into paragraph and restores in one undo step.
    let mut heading = TextBuffer::from_str("## Title", None);
    heading.set_cursor(Position::new(SourceLine::new(0), 3));
    LiveBuffer::new(&mut heading).backspace();
    assert_eq!(heading.to_string_normalized(), "Title");
    assert_eq!(heading.cursor_pos(), Position::new(SourceLine::new(0), 0));
    heading.undo();
    assert_eq!(heading.to_string_normalized(), "## Title");
    assert_eq!(heading.cursor_pos(), Position::new(SourceLine::new(0), 3));

    // List item with adjacent item splits out as an isolated paragraph in one undo step.
    let mut list = TextBuffer::from_str("- one\n- two\n- three", None);
    list.set_cursor(Position::new(SourceLine::new(1), 2));
    LiveBuffer::new(&mut list).backspace();
    assert_eq!(list.to_string_normalized(), "- one\n\ntwo\n\n- three");
    list.undo();
    assert_eq!(list.to_string_normalized(), "- one\n- two\n- three");

    // Quoted display math protects delimiters on Backspace and last content line on Delete.
    let math_doc = "> > $$\n> > x\n> > $$\nafter";
    let mut math = TextBuffer::from_str(math_doc, None);
    math.set_cursor(Position::new(SourceLine::new(1), 5));
    LiveBuffer::new(&mut math).delete_forward();
    assert_eq!(math.to_string_normalized(), math_doc);
    math.set_cursor(Position::new(SourceLine::new(3), 0));
    LiveBuffer::new(&mut math).backspace();
    assert_eq!(math.to_string_normalized(), math_doc);
}

#[test]
fn live_buffer_large_document_editing_and_navigation_zero_materializations() {
    use crate::buffer::{Position, SourceLine, TextBuffer};

    let mut lines: Vec<String> = (0..5_000)
        .map(|i| match i % 6 {
            0 => format!("## Heading {i}"),
            1 => String::new(),
            2 => "```rust".to_string(),
            3 => format!("let item_{i} = {i};"),
            4 => "```".to_string(),
            _ => format!("Paragraph {i}"),
        })
        .collect();
    let table_row = lines.len();
    lines.extend([
        "| C1 | C2 |".to_string(),
        "| --- | --- |".to_string(),
        "| v1 | v2 |".to_string(),
    ]);
    let mut buffer = TextBuffer::from_str(&lines.join("\n"), None);
    let scans = crate::markdown::scanner::FULL_FENCE_SCANS.get();
    let mat = buffer.full_line_materializations();

    buffer.set_cursor(Position::new(SourceLine::new(0), 3));
    {
        let mut live = LiveBuffer::new(&mut buffer);
        live.move_down(false);
        live.move_right(false);
        live.move_left(false);
        live.move_to_line_start(false);
        live.insert_newline();
        live.backspace();
    }

    buffer.set_cursor(Position::new(SourceLine::new(3), 15));
    LiveBuffer::new(&mut buffer).delete_forward();
    LiveBuffer::new(&mut buffer).insert_line_below_block();

    buffer.set_cursor(Position::new(SourceLine::new(table_row + 2), 3));
    assert!(LiveBuffer::new(&mut buffer).table_tab_forward());
    assert!(LiveBuffer::new(&mut buffer).table_enter_next_row());

    assert_eq!(crate::markdown::scanner::FULL_FENCE_SCANS.get(), scans);
    assert_eq!(buffer.full_line_materializations(), mat);
}
