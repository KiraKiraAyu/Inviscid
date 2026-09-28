use super::*;

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
fn test_triple_backticks_not_collapsed() {
    let lines1 = vec!["```".to_string(), "hello".to_string()];
    let parsed1 = MarkdownScanner::scan_document(&lines1);
    assert_eq!(parsed1.len(), 2);
    let line0_visible: String = parsed1[0]
        .spans
        .iter()
        .filter(|s| s.is_visible(false, 0, false))
        .map(|s| s.text.as_str())
        .collect();
    assert_eq!(line0_visible, "```");

    let lines2 = vec!["```rust".to_string(), "hello".to_string()];
    let parsed2 = MarkdownScanner::scan_document(&lines2);
    assert_eq!(parsed2.len(), 2);
    let line1_visible: String = parsed2[0]
        .spans
        .iter()
        .filter(|s| s.is_visible(false, 0, false))
        .map(|s| s.text.as_str())
        .collect();
    assert_eq!(line1_visible, "```rust");
}

#[test]
fn test_blockquote_requires_space() {
    let lines = vec![">".to_string(), "> hello".to_string()];
    let parsed = MarkdownScanner::scan_document(&lines);
    assert_eq!(parsed.len(), 2);
    assert!(matches!(parsed[0].kind, BlockKind::Paragraph));
    assert!(matches!(parsed[1].kind, BlockKind::BlockQuote));
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
fn test_visual_document_ast_and_position_projection() {
    use crate::buffer::Position;

    let empty_lines = vec!["".to_string(), "".to_string()];
    let doc0 = VisualDocument::from_lines(&empty_lines);
    assert_eq!(doc0.blocks.len(), 1);
    match &doc0.blocks[0] {
        VisualBlock::Paragraph { source_line } => {
            assert_eq!(*source_line, 0);
        }
        _ => panic!("Expected Paragraph block"),
    }

    let vpos0 = doc0.source_to_visual(Position::new(0, 0));
    assert_eq!(vpos0, VisualPosition::new(0, 0, 0));
    let vpos1 = doc0.source_to_visual(Position::new(1, 0));
    assert_eq!(vpos1, VisualPosition::new(0, 0, 0));

    let doc_lines = vec![
        "# Heading 1".to_string(),
        "".to_string(),
        "Paragraph text".to_string(),
        "".to_string(),
        "```rust".to_string(),
        "fn main() {".to_string(),
        "    0".to_string(),
        "}".to_string(),
        "```".to_string(),
        "".to_string(),
        "| A | B |".to_string(),
        "|---|---|".to_string(),
        "| 1 | 2 |".to_string(),
        "".to_string(),
        "1. List item 1".to_string(),
        "2. List item 2".to_string(),
        "".to_string(),
        "---".to_string(),
    ];

    let doc = VisualDocument::from_lines(&doc_lines);
    assert_eq!(doc.blocks.len(), 7);

    assert!(matches!(doc.blocks[0], VisualBlock::Heading { .. }));
    assert!(matches!(doc.blocks[1], VisualBlock::Paragraph { .. }));
    assert!(matches!(doc.blocks[2], VisualBlock::CodeBlock { .. }));
    assert!(matches!(doc.blocks[3], VisualBlock::Table { .. }));
    assert!(matches!(doc.blocks[4], VisualBlock::ListItem { .. }));
    assert!(matches!(doc.blocks[5], VisualBlock::ListItem { .. }));
    assert!(matches!(doc.blocks[6], VisualBlock::ThematicBreak { .. }));

    if let VisualBlock::CodeBlock {
        code_line_count,
        start_line,
        end_line,
    } = &doc.blocks[2]
    {
        assert_eq!(*code_line_count, 3);
        assert_eq!(*start_line, 4);
        assert_eq!(*end_line, 8);
    } else {
        panic!("Block 2 must be CodeBlock");
    }

    let vpos_code = doc.source_to_visual(Position::new(6, 4));
    assert_eq!(vpos_code, VisualPosition::new(2, 2, 4));
    let spos_code = doc.visual_to_source(vpos_code);
    assert_eq!(spos_code, Position::new(6, 4));

    let vpos_sep = doc.source_to_visual(Position::new(9, 0));
    assert_eq!(vpos_sep.block_idx, 2);

    let vpos_tbl_hdr = doc.source_to_visual(Position::new(10, 2));
    assert_eq!(vpos_tbl_hdr, VisualPosition::new(3, 0, 2));
    assert_eq!(doc.visual_to_source(vpos_tbl_hdr), Position::new(10, 2));

    let vpos_tbl_row = doc.source_to_visual(Position::new(12, 3));
    assert_eq!(vpos_tbl_row, VisualPosition::new(3, 1, 3));
    assert_eq!(doc.visual_to_source(vpos_tbl_row), Position::new(12, 3));
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
