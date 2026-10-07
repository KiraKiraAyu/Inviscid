use crate::editor::{LineLayoutContext, get_line_layout_height};
use crate::markdown::{MarkdownScanner, is_syntactic_separator};

use gpui::px;

#[test]
fn test_line_layout_heights() {
    let lines = vec![
        "# Heading 1".to_string(),
        "".to_string(), // Syntactic separator (idx = 1)
        "## Heading 2".to_string(),
        "```rust".to_string(),
        "fn main() {}".to_string(),
        "```".to_string(),
        "---".to_string(),
        "- [ ] Task Item".to_string(),
        "Regular paragraph".to_string(),
        "".to_string(), // Trailing empty line created by Enter (idx = 9)
    ];

    let parsed_doc = MarkdownScanner::scan_document(&lines);
    assert_eq!(parsed_doc.len(), 10);

    // Verify syntactic separators
    assert!(is_syntactic_separator(&lines, 1));
    assert!(is_syntactic_separator(&lines, 9));

    // Live preview heights
    assert_eq!(
        get_line_layout_height(
            &lines,
            0,
            &parsed_doc[0],
            LineLayoutContext::live_preview(px(800.0)),
        ),
        px(54.0)
    );
    // Syntactic separator between blocks is collapsed when inactive
    assert_eq!(
        get_line_layout_height(
            &lines,
            1,
            &parsed_doc[1],
            LineLayoutContext::live_preview(px(800.0)),
        ),
        px(0.0)
    );
    // When active, syntactic separator expands with caret height
    assert_eq!(
        get_line_layout_height(
            &lines,
            1,
            &parsed_doc[1],
            LineLayoutContext {
                is_active: true,
                ..LineLayoutContext::live_preview(px(800.0))
            },
        ),
        px(32.0)
    );
    assert_eq!(
        get_line_layout_height(
            &lines,
            2,
            &parsed_doc[2],
            LineLayoutContext::live_preview(px(800.0)),
        ),
        px(50.0)
    );
    assert_eq!(
        get_line_layout_height(
            &lines,
            3,
            &parsed_doc[3],
            LineLayoutContext::live_preview(px(800.0)),
        ),
        px(34.0)
    );
    assert_eq!(
        get_line_layout_height(
            &lines,
            4,
            &parsed_doc[4],
            LineLayoutContext::live_preview(px(800.0)),
        ),
        px(24.0)
    );
    assert_eq!(
        get_line_layout_height(
            &lines,
            5,
            &parsed_doc[5],
            LineLayoutContext::live_preview(px(800.0)),
        ),
        px(12.0)
    );
    assert_eq!(
        get_line_layout_height(
            &lines,
            6,
            &parsed_doc[6],
            LineLayoutContext::live_preview(px(800.0)),
        ),
        px(32.0)
    );
    assert_eq!(
        get_line_layout_height(
            &lines,
            7,
            &parsed_doc[7],
            LineLayoutContext::live_preview(px(800.0)),
        ),
        px(32.0)
    );
    assert_eq!(
        get_line_layout_height(
            &lines,
            8,
            &parsed_doc[8],
            LineLayoutContext::live_preview(px(800.0)),
        ),
        px(32.0)
    );
    assert_eq!(
        get_line_layout_height(
            &lines,
            9,
            &parsed_doc[9],
            LineLayoutContext::live_preview(px(800.0)),
        ),
        px(0.0)
    );

    // Test mid-document empty line creation (e.g. Paragraph + trailing separator + newly created empty line + Blockquote)
    let mid_doc_lines = vec![
        "Paragraph 1".to_string(),
        "".to_string(), // trailing separator after Paragraph 1 (idx = 1)
        "".to_string(), // newly created empty paragraph line (idx = 2)
        "> Quote".to_string(),
    ];
    // idx = 1 is a separator (trailing separator after non-empty Paragraph 1)
    assert!(is_syntactic_separator(&mid_doc_lines, 1));
    // idx = 2 is not a separator (preceded by empty line 1, intentional empty paragraph)
    assert!(!is_syntactic_separator(&mid_doc_lines, 2));

    // Source mode heights (fixed 24.0 per line)
    for (idx, parsed) in parsed_doc.iter().enumerate() {
        assert_eq!(
            get_line_layout_height(&lines, idx, parsed, LineLayoutContext::source(px(800.0)),),
            px(24.0)
        );
    }
}

#[test]
fn test_empty_heading_converted_to_paragraph_no_spacing_jump() {
    let lines = vec![
        "Paragraph 1".to_string(), // idx = 0
        "".to_string(),            // idx = 1 (separator before heading)
        "".to_string(),            // idx = 2 (was "## ", now empty paragraph)
        "".to_string(),            // idx = 3 (separator before paragraph 2)
        "Paragraph 2".to_string(), // idx = 4
    ];
    let parsed_doc = MarkdownScanner::scan_document(&lines);
    assert_eq!(parsed_doc.len(), 5);

    // When cursor is on idx = 2 (active line = Some(2)), line 1 is recognized as separator (0px)
    assert_eq!(
        get_line_layout_height(
            &lines,
            1,
            &parsed_doc[1],
            LineLayoutContext::live_preview(px(800.0)),
        ),
        px(0.0)
    );
    // Line 2 is active line -> full height 32px
    assert_eq!(
        get_line_layout_height(
            &lines,
            2,
            &parsed_doc[2],
            LineLayoutContext {
                is_active: true,
                ..LineLayoutContext::live_preview(px(800.0))
            },
        ),
        px(32.0)
    );
    // Line 3 is before Paragraph 2 -> 0px
    assert_eq!(
        get_line_layout_height(
            &lines,
            3,
            &parsed_doc[3],
            LineLayoutContext::live_preview(px(800.0)),
        ),
        px(0.0)
    );

    let typed_lines = vec![
        "Paragraph 1".to_string(),
        "".to_string(),
        "a".to_string(),
        "".to_string(),
        "Paragraph 2".to_string(),
    ];
    let typed_parsed = MarkdownScanner::scan_document(&typed_lines);
    assert_eq!(
        get_line_layout_height(
            &typed_lines,
            1,
            &typed_parsed[1],
            LineLayoutContext::live_preview(px(800.0)),
        ),
        px(0.0)
    );
    assert_eq!(
        get_line_layout_height(
            &typed_lines,
            2,
            &typed_parsed[2],
            LineLayoutContext {
                is_active: true,
                ..LineLayoutContext::live_preview(px(800.0))
            },
        ),
        px(32.0)
    );
}

#[test]
fn test_link_only_paragraph_cursor_navigation_and_hit_testing() {
    use crate::editor::shaping::visible_byte_to_raw_col;

    let lines = vec!["[Inviscid](https://github.com)".to_string()];
    let parsed_doc = MarkdownScanner::scan_document(&lines);
    assert_eq!(parsed_doc.len(), 1);

    let spans = &parsed_doc[0].spans;
    let total_len = parsed_doc[0].raw_text.chars().count();
    assert_eq!(total_len, 30);

    // 1. Moving cursor to the right of the visible link in Live Preview (visible_byte == 8, total visible len of "Inviscid")
    // maps to the rightmost boundary of the link group: column 30 (`[Inviscid](https://github.com)|`)
    let end_col = visible_byte_to_raw_col(8, spans, false, 0, false, total_len);
    assert_eq!(end_col, 30);

    // 2. Beyond visible length (e.g. visible_byte == 20): still maps to 30
    let beyond_col = visible_byte_to_raw_col(20, spans, false, 0, false, total_len);
    assert_eq!(beyond_col, 30);

    // 3. At the start of the visible link (visible_byte == 0): maps to column 0 (`|[Inviscid](...)`)
    let start_col = visible_byte_to_raw_col(0, spans, false, 0, false, total_len);
    assert_eq!(start_col, 0);

    // 4. In the middle of "Inviscid" (visible_byte == 3 at "Inv|iscid"): maps to column 4 (`[Inv|iscid](...)`)
    let mid_col = visible_byte_to_raw_col(3, spans, false, 0, false, total_len);
    assert_eq!(mid_col, 4);
}

#[test]
fn test_resolve_image_source() {
    use crate::editor::render::resolve_image_source;
    use gpui::{ImageSource, Resource};
    use std::path::Path;

    // 1. Web URL
    let web_src = resolve_image_source(
        "https://www.testdomain.com/articles_assets/996188067/index.webp",
        None,
    );
    match web_src {
        ImageSource::Resource(Resource::Uri(uri)) => {
            assert_eq!(
                uri.as_ref(),
                "https://www.testdomain.com/articles_assets/996188067/index.webp"
            );
        }
        _ => panic!("Expected Resource::Uri"),
    }

    // 2. Local file path
    let local_src = resolve_image_source("C:/Users/photo.png", None);
    match local_src {
        ImageSource::Resource(Resource::Path(_)) => {}
        _ => panic!("Expected Resource::Path"),
    }

    // 3. file:// URI
    let file_uri_src = resolve_image_source("file:///C:/Users/photo.png", None);
    match file_uri_src {
        ImageSource::Resource(Resource::Path(_)) => {}
        _ => panic!("Expected Resource::Path"),
    }

    // 4. Relative image path with document base directory
    let doc_file = Path::new("D:/Notes/project/crypto.md");
    let rel_src = resolve_image_source("images/image_001.jpg", Some(doc_file));
    match rel_src {
        ImageSource::Resource(Resource::Path(p)) => {
            let p_str = p.to_string_lossy();
            assert!(p_str.contains("project") && p_str.contains("image_001.jpg"));
        }
        _ => panic!("Expected Resource::Path"),
    }
}

#[test]
fn test_drag_selection_head_marker_disclosure() {
    let lines = vec!["This is **bold** and *italic* text.".to_string()];
    let parsed_doc = MarkdownScanner::scan_document(&lines);
    assert_eq!(parsed_doc.len(), 1);

    let spans = &parsed_doc[0].spans;
    let is_active_line = true;
    let drag_head_col = 10;

    let bold_marker_1 = spans
        .iter()
        .find(|s| s.text == "**" && s.span_range.0 == 8)
        .unwrap();
    let bold_marker_2 = spans
        .iter()
        .find(|s| s.text == "**" && s.span_range.0 == 14)
        .unwrap();
    let italic_marker = spans
        .iter()
        .find(|s| s.text == "*" && s.span_range.0 == 21)
        .unwrap();

    // Bold markers are disclosed because drag head is inside group [8..16]
    assert!(bold_marker_1.is_visible(is_active_line, drag_head_col, false));
    assert!(bold_marker_2.is_visible(is_active_line, drag_head_col, false));

    // Italic marker is not disclosed because drag head is outside group [21..29]
    assert!(!italic_marker.is_visible(is_active_line, drag_head_col, false));
}

#[test]
fn test_image_active_cursor_placement() {
    use crate::editor::shaping::visible_byte_to_raw_col;

    let lines = vec!["![a](https://www.testdomain.com/index.webp)".to_string()];
    let parsed_doc = MarkdownScanner::scan_document(&lines);
    assert_eq!(parsed_doc.len(), 1);

    let spans = &parsed_doc[0].spans;
    let total_len = parsed_doc[0].raw_text.chars().count();

    assert_eq!(
        visible_byte_to_raw_col(0, spans, true, 0, false, total_len),
        0
    );
    assert_eq!(
        visible_byte_to_raw_col(2, spans, true, 2, false, total_len),
        2
    );
    assert_eq!(
        visible_byte_to_raw_col(3, spans, true, 3, false, total_len),
        3
    );
    assert_eq!(
        visible_byte_to_raw_col(15, spans, true, 15, false, total_len),
        15
    );

    let img_line =
        "![](https://www.testdomain.com/articles_assets/996188069/index.webp)".to_string();
    let img_doc = MarkdownScanner::scan_document(&[img_line]);
    let img_spans = &img_doc[0].spans;
    let img_total_len = img_doc[0].raw_text.chars().count();
    let i_col = img_doc[0].raw_text.find("/index.webp").unwrap() + 1;

    assert_eq!(
        visible_byte_to_raw_col(i_col, img_spans, true, i_col, false, img_total_len),
        i_col
    );
}

#[test]
fn test_image_shaping_style_consistency() {
    use crate::editor::shaping::get_block_style_for_line;
    use crate::theme::Theme;

    let theme = Theme::default();
    let lines = vec!["![](https://www.testdomain.com/index.webp)".to_string()];
    let parsed_doc = MarkdownScanner::scan_document(&lines);

    let (font_size, line_height, override_color) =
        get_block_style_for_line(&parsed_doc[0], &theme, false);
    assert_eq!(font_size, px(15.0));
    assert_eq!(line_height, px(24.0));
    assert_eq!(override_color, Some(theme.text_secondary));
}

#[test]
fn test_image_dimension_extraction_and_layout_height() {
    use crate::http::{extract_image_dimensions, record_image_size};

    // 1. Test PNG header parser: 1920x1080
    let mut png_bytes = vec![0x89, b'P', b'N', b'G', b'\r', b'\n', 0x1A, b'\n'];
    png_bytes.extend_from_slice(&[0, 0, 0, 13, b'I', b'H', b'D', b'R']);
    png_bytes.extend_from_slice(&1920u32.to_be_bytes());
    png_bytes.extend_from_slice(&1080u32.to_be_bytes());
    png_bytes.extend_from_slice(&[8, 6, 0, 0, 0]);

    let dims = extract_image_dimensions(&png_bytes);
    assert_eq!(dims, Some((1920, 1080)));

    // 2. Test get_line_layout_height with recorded dimensions
    let test_url = "https://www.testdomain.com/test_1080p.png";
    record_image_size(test_url, 1920, 1080);
    crate::http::mark_url_loaded(test_url);

    let lines = vec![format!("![]({})", test_url)];
    let parsed_doc = MarkdownScanner::scan_document(&lines);

    // With content_width = 800.0: expected image height = 1080 * 800 / 1920 = 450.0 px
    // Total height = base_h (38.0) + 450.0 + 12.0 = 500.0 px
    let h = get_line_layout_height(
        &lines,
        0,
        &parsed_doc[0],
        LineLayoutContext {
            is_active: true,
            ..LineLayoutContext::live_preview(px(800.0))
        },
    );
    assert_eq!(h, px(500.0));

    // 3. Test broken/failed image fallback card height (40px error card + 12px margins)
    let broken_url = "images/broken_non_existent.png";
    let broken_lines = vec![format!("![]({})", broken_url)];
    let broken_parsed = MarkdownScanner::scan_document(&broken_lines);

    // Inactive: base_h (8.0) + 40.0 (fallback card) + 12.0 (margins) = 60.0 px
    let h_broken_inactive = get_line_layout_height(
        &broken_lines,
        0,
        &broken_parsed[0],
        LineLayoutContext::live_preview(px(800.0)),
    );
    assert_eq!(h_broken_inactive, px(60.0));

    // Active: base_h (38.0) + 40.0 (fallback card) + 12.0 (margins) = 90.0 px
    let h_broken_active = get_line_layout_height(
        &broken_lines,
        0,
        &broken_parsed[0],
        LineLayoutContext {
            is_active: true,
            ..LineLayoutContext::live_preview(px(800.0))
        },
    );
    assert_eq!(h_broken_active, px(90.0));

    // 4. Test remote HTTP 404 failure transitions to 40px broken card
    let failed_remote_url = "https://example.com/non_existent_image_404.png";
    crate::http::mark_url_failed(failed_remote_url);
    let remote_fail_lines = vec![format!("![]({})", failed_remote_url)];
    let remote_fail_parsed = MarkdownScanner::scan_document(&remote_fail_lines);
    let h_remote_fail = get_line_layout_height(
        &remote_fail_lines,
        0,
        &remote_fail_parsed[0],
        LineLayoutContext::live_preview(px(800.0)),
    );
    assert_eq!(h_remote_fail, px(60.0));
}

#[test]
fn test_image_drag_selection_reveals_raw_markdown() {
    use crate::buffer::{Position, Selection};

    let lines = vec![
        "Paragraph before".to_string(),
        "![Banner](https://www.testdomain.com/banner.webp)".to_string(),
        "Paragraph after".to_string(),
    ];
    let parsed_doc = MarkdownScanner::scan_document(&lines);

    let selection = Selection {
        anchor: Position::new(0, 5),
        head: Position::new(2, 3),
    };

    let (start, end) = selection.range();
    let img_line_idx = 1;

    let is_selected_line = img_line_idx >= start.line && img_line_idx <= end.line;
    assert!(is_selected_line);

    let spans = &parsed_doc[1].spans;
    let is_active_caret = is_selected_line;

    for span in spans {
        assert!(span.is_visible(is_active_caret, 0, false));
    }
}

#[test]
fn test_multiline_selection_does_not_disclose_inactive_line_inline_markers() {
    let lines = vec![
        "Line 0 starting text".to_string(),
        "Line 1 with **bold** and *italic* words".to_string(),
        "Line 2 ending text".to_string(),
    ];
    let parsed_doc = MarkdownScanner::scan_document(&lines);

    let drag_head_col = 15;
    let is_active_line_1 = false;

    let spans_line_1 = &parsed_doc[1].spans;
    for span in spans_line_1 {
        if span.is_marker {
            assert!(!span.is_visible(is_active_line_1, drag_head_col, false));
        } else if span.is_bold || span.is_italic {
            assert!(span.is_visible(is_active_line_1, drag_head_col, false));
        }
    }
}

#[test]
fn test_ime_composition_after_heading_prefix() {
    use crate::buffer::{Position, Selection, TextBuffer};

    let mut buffer = TextBuffer::new();
    let mut marked_range: Option<std::ops::Range<usize>> = None;

    // 1. User types "# "
    buffer.insert_text("# ");
    assert_eq!(buffer.lines(), vec!["# ", ""]);
    assert_eq!(buffer.selection(), Selection::cursor(Position::new(0, 2)));

    // 2. User types 'w' in Chinese IME (composition starts with "w")
    let range_1 = marked_range.clone().unwrap_or_else(|| {
        let head = buffer.pos_to_utf16_offset(buffer.cursor_pos());
        let anchor = buffer.pos_to_utf16_offset(buffer.selection().anchor);
        head.min(anchor)..head.max(anchor)
    });
    let start_pos_1 = buffer.utf16_offset_to_pos(range_1.start);
    let end_pos_1 = buffer.utf16_offset_to_pos(range_1.end);
    buffer.set_selection(Selection {
        anchor: start_pos_1,
        head: end_pos_1,
    });
    buffer.insert_text("w");
    marked_range = Some(range_1.start..(range_1.start + 1));
    buffer.set_cursor(buffer.utf16_offset_to_pos(range_1.start + 1));

    assert_eq!(buffer.lines(), vec!["# w", ""]);
    assert_eq!(buffer.selection(), Selection::cursor(Position::new(0, 3)));
    assert_eq!(marked_range, Some(2..3));

    // 3. User types 'o' in Chinese IME (composition updates to "wo")
    let range_2 = marked_range.clone().unwrap_or_else(|| {
        let head = buffer.pos_to_utf16_offset(buffer.cursor_pos());
        let anchor = buffer.pos_to_utf16_offset(buffer.selection().anchor);
        head.min(anchor)..head.max(anchor)
    });
    assert_eq!(range_2, 2..3); // Replaces the marked "w" at 2..3
    let start_pos_2 = buffer.utf16_offset_to_pos(range_2.start);
    let end_pos_2 = buffer.utf16_offset_to_pos(range_2.end);
    buffer.set_selection(Selection {
        anchor: start_pos_2,
        head: end_pos_2,
    });
    buffer.insert_text("wo");
    marked_range = Some(range_2.start..(range_2.start + 2));
    buffer.set_cursor(buffer.utf16_offset_to_pos(range_2.start + 2));

    assert_eq!(buffer.lines(), vec!["# wo", ""]);
    assert_eq!(buffer.selection(), Selection::cursor(Position::new(0, 4)));
    assert_eq!(marked_range, Some(2..4));

    // 4. User confirms Chinese character "我"
    let range_3 = marked_range.take().unwrap();
    let start_pos_3 = buffer.utf16_offset_to_pos(range_3.start);
    let end_pos_3 = buffer.utf16_offset_to_pos(range_3.end);
    buffer.set_selection(Selection {
        anchor: start_pos_3,
        head: end_pos_3,
    });
    buffer.insert_text("我");

    assert_eq!(buffer.lines(), vec!["# 我", ""]);
    assert_eq!(buffer.selection(), Selection::cursor(Position::new(0, 3)));
    assert_eq!(marked_range, None);

    // Verify it is properly scanned as Heading 1
    let parsed_doc = MarkdownScanner::scan_document(buffer.lines());
    assert_eq!(parsed_doc.len(), 2);
    match &parsed_doc[0].kind {
        crate::markdown::BlockKind::Heading { level } => {
            assert_eq!(*level, 1);
        }
        _ => panic!("Expected Heading 1"),
    }
}

#[test]
fn test_text_soft_wrapping_and_multiline_height() {
    use crate::editor::shaping::calculate_wrapped_visual_lines;

    // 1. Short text fits in 1 line
    let short_text = "This is a short line.";
    let lines_short = calculate_wrapped_visual_lines(short_text, px(15.0), px(800.0));
    assert_eq!(lines_short, 1);

    // 2. Long text overflowing 800px width wraps into multiple lines
    let long_chinese_text = "SMC (Smart Money Concepts，聪明钱概念）是一套起源于外汇机构交易圈的分析框架。它的核心假设只有一句话：市场的走向由机构主导，机构的行为留下了可识别的痕迹。与传统技术指标（MACD、RSI、布林带）不同，SMC 不看滞后的统计数据，它看的是价格结构本身——那些机构建仓时必然留下的指纹。";
    let lines_long = calculate_wrapped_visual_lines(long_chinese_text, px(15.0), px(800.0));
    assert!(lines_long >= 3);

    // 3. Layout height scales proportionally with wrapped lines
    let lines_vec = vec![long_chinese_text.to_string()];
    let parsed_doc = MarkdownScanner::scan_document(&lines_vec);

    let h_live = get_line_layout_height(
        &lines_vec,
        0,
        &parsed_doc[0],
        LineLayoutContext::live_preview(px(800.0)),
    );
    // Height should be 8px padding + (lines_long * 24px)
    let expected_h = px(8.0) + px(24.0) * (lines_long as f32);
    assert_eq!(h_live, expected_h);
}

#[test]
fn test_table_row_layout_heights() {
    let lines = vec![
        "| Col 1 | Col 2 |".to_string(), // idx = 0 (Header)
        "| --- | --- |".to_string(),     // idx = 1 (Delimiter)
        "| A | B |".to_string(),         // idx = 2 (Data Row 1)
        "| C | D |".to_string(),         // idx = 3 (Last Data Row)
    ];

    let parsed_doc = MarkdownScanner::scan_document(&lines);
    assert_eq!(parsed_doc.len(), 4);

    // Live Preview:
    // Header row = 42px (34px cell + 8px top margin)
    assert_eq!(
        get_line_layout_height(
            &lines,
            0,
            &parsed_doc[0],
            LineLayoutContext::live_preview(px(800.0)),
        ),
        px(42.0)
    );
    // Delimiter row = 0px (always visually collapsed)
    assert_eq!(
        get_line_layout_height(
            &lines,
            1,
            &parsed_doc[1],
            LineLayoutContext::live_preview(px(800.0)),
        ),
        px(0.0)
    );
    // Intermediate Data row = 30px
    assert_eq!(
        get_line_layout_height(
            &lines,
            2,
            &parsed_doc[2],
            LineLayoutContext::live_preview(px(800.0)),
        ),
        px(30.0)
    );
    // Last Data row = 38px (30px cell + 8px bottom margin)
    assert_eq!(
        get_line_layout_height(
            &lines,
            3,
            &parsed_doc[3],
            LineLayoutContext::live_preview(px(800.0)),
        ),
        px(38.0)
    );
}

#[test]
fn test_multiline_wrapped_table_row_layout_height() {
    let lines = vec![
        "| 特性 | 说明 |".to_string(),
        "| --- | --- |".to_string(),
        "| 链接 | [查看文档](https://github.com/long/url/that/wraps/to/multiple/lines) |"
            .to_string(),
    ];

    let parsed_doc = MarkdownScanner::scan_document(&lines);
    assert_eq!(parsed_doc.len(), 3);

    // When inactive: hidden markers -> 1 line height (38px with bottom margin)
    let h_inactive = get_line_layout_height(
        &lines,
        2,
        &parsed_doc[2],
        LineLayoutContext::live_preview(px(400.0)),
    );
    assert_eq!(h_inactive, px(38.0));

    // When active at col 10 (inside the link): link markdown syntax is disclosed, wrapping to multiple lines -> row height expands!
    let h_active = get_line_layout_height(
        &lines,
        2,
        &parsed_doc[2],
        LineLayoutContext {
            is_active: true,
            cursor_col: 10,
            ..LineLayoutContext::live_preview(px(400.0))
        },
    );
    assert!(
        h_active > px(38.0),
        "Table row height dynamically expands when cell wraps onto multiple lines"
    );
}

#[test]
fn test_empty_document_live_preview_cursor_cannot_land_on_trailing_separator() {
    use crate::buffer::{Position, TextBuffer};

    let mut buf = TextBuffer::new();
    assert_eq!(buf.lines(), vec!["", ""]);

    // In Live Preview: line 0 is editable, line 1 is syntactic separator (collapsed to 0px)
    assert!(!is_syntactic_separator(buf.lines(), 0));
    assert!(is_syntactic_separator(buf.lines(), 1));

    // Navigating down from line 0 in Live Preview stays on line 0
    buf.move_down_mode(false, false);
    assert_eq!(buf.cursor_pos(), Position::new(0, 0));

    // In Source mode, line 1 is accessible
    buf.move_down_mode(false, true);
    assert_eq!(buf.cursor_pos(), Position::new(1, 0));
}

#[test]
fn test_ime_localized_text_for_range_accuracy() {
    let text = "Hello world!\n你好，世界！这是一段中文输入法测试。\nThird line here.\n";
    let buf = crate::buffer::TextBuffer::from_str(text, None);

    // Test single-line range (e.g. "你好，世界！")
    let start_pos = buf.utf16_offset_to_pos(13); // Start of second line
    assert_eq!(start_pos.line, 1);
    assert_eq!(start_pos.col, 0);

    let end_pos = buf.utf16_offset_to_pos(19); // 6 chars: 你好，世界！
    assert_eq!(end_pos.line, 1);
    assert_eq!(end_pos.col, 6);

    let line = &buf.lines()[1];
    let slice: String = line
        .chars()
        .skip(start_pos.col)
        .take(end_pos.col - start_pos.col)
        .collect();
    assert_eq!(slice, "你好，世界！");
}

#[test]
fn test_mouse_hit_testing_line_index_resolution() {
    use gpui::{Bounds, point, size};

    let lines = [
        "# Heading 1".to_string(),
        "First regular paragraph.".to_string(),
        "Second regular paragraph.".to_string(),
    ];
    let buf = crate::buffer::TextBuffer::from_str(&lines.join("\n"), None);
    let parsed_doc =
        std::sync::Arc::new(crate::markdown::MarkdownScanner::scan_document(buf.lines()));

    let mut cache = crate::editor::layout::DocumentLayoutCache {
        parsed_lines: parsed_doc.clone(),
        ..Default::default()
    };

    let content_width = px(800.0);
    let mut y = px(24.0);
    for (idx, parsed) in cache.parsed_lines.iter().enumerate() {
        let lcx = crate::editor::layout::LineLayoutContext::live_preview(content_width);
        let h = crate::editor::layout::get_line_layout_height(buf.lines(), idx, parsed, lcx);
        cache.line_y_offsets.push(y);
        cache.line_heights.push(h);
        y += h;
    }
    cache.total_height = y + px(24.0);

    // Document offsets:
    // Line 0 (Heading): y in [24.0, 78.0) (height 54.0)
    // Line 1 (Paragraph): y in [78.0, 110.0) (height 32.0)
    // Line 2 (Paragraph): y in [110.0, 142.0) (height 32.0)
    assert_eq!(cache.line_y_offsets[0], px(24.0));
    assert_eq!(cache.line_y_offsets[1], px(78.0));
    assert_eq!(cache.line_y_offsets[2], px(110.0));

    // Viewport layout: top in window coordinates is at 70px (TitleBar 36px + TabBar 34px)
    let viewport = Bounds {
        origin: point(px(0.0), px(70.0)),
        size: size(px(1000.0), px(700.0)),
    };
    let scroll_top = px(0.0);

    // Hit-testing helper simulating doc_y calculation
    let resolve_line = |point_y: gpui::Pixels, scroll: gpui::Pixels| -> usize {
        let doc_y = (point_y - viewport.origin.y + scroll).max(px(0.0));
        let idx = cache.line_y_offsets.partition_point(|&y| y <= doc_y);
        idx.saturating_sub(1).min(cache.parsed_lines.len() - 1)
    };

    // 1. Click on Line 0 (window Y = 70.0 + 24.0 + 10.0 = 104.0) -> Line 0
    assert_eq!(resolve_line(px(104.0), scroll_top), 0);

    // 2. Click on Line 1 (window Y = 70.0 + 78.0 + 10.0 = 158.0) -> Line 1
    assert_eq!(resolve_line(px(158.0), scroll_top), 1);

    // 3. Click on Line 2 (window Y = 70.0 + 110.0 + 10.0 = 190.0) -> Line 2
    assert_eq!(resolve_line(px(190.0), scroll_top), 2);

    // 4. Click in top padding (window Y = 70.0 + 10.0 = 80.0) -> snaps to Line 0
    assert_eq!(resolve_line(px(80.0), scroll_top), 0);

    // 5. With scroll offset = 50px:
    // Line 1 is now at doc_y = 78.0 -> window Y = 70.0 + 78.0 - 50.0 + 10.0 = 108.0
    assert_eq!(resolve_line(px(108.0), px(50.0)), 1);

    // Contrast with the previous bug where viewport.origin.y was 36px:
    let buggy_viewport_top = px(36.0);
    let click_bottom_of_line_0 = px(70.0 + 65.0); // 135px
    let correct_res = resolve_line(click_bottom_of_line_0, scroll_top);
    assert_eq!(correct_res, 0); // Correctly Line 0!
    let old_bug_res = {
        let d = (click_bottom_of_line_0 - buggy_viewport_top + scroll_top).max(px(0.0));
        cache
            .line_y_offsets
            .partition_point(|&y| y <= d)
            .saturating_sub(1)
    };
    assert_eq!(old_bug_res, 1); // Reproduces the old bug where Line 0 was mapped to Line 1!
}

#[test]
fn test_source_mode_line_index_resolution() {
    use crate::editor::RenderMode;
    use gpui::{Bounds, point, size};

    let lines = [
        "fn main() {".to_string(),
        "    println!(\"Hello!\");".to_string(),
        "}".to_string(),
    ];
    let buf = crate::buffer::TextBuffer::from_str(&lines.join("\n"), None);
    let parsed_doc =
        std::sync::Arc::new(crate::markdown::MarkdownScanner::scan_document(buf.lines()));

    let mut cache = crate::editor::layout::DocumentLayoutCache {
        parsed_lines: parsed_doc.clone(),
        render_mode: RenderMode::Source,
        ..Default::default()
    };

    let content_width = px(900.0);
    let mut y = px(24.0);
    for (idx, parsed) in cache.parsed_lines.iter().enumerate() {
        let lcx = crate::editor::layout::LineLayoutContext::source(content_width);
        let h = crate::editor::layout::get_line_layout_height(buf.lines(), idx, parsed, lcx);
        cache.line_y_offsets.push(y);
        cache.line_heights.push(h);
        y += h;
    }

    // In Source mode, every line height is exactly 24px
    assert_eq!(cache.line_heights[0], px(24.0));
    assert_eq!(cache.line_heights[1], px(24.0));
    assert_eq!(cache.line_heights[2], px(24.0));

    let viewport = Bounds {
        origin: point(px(0.0), px(70.0)),
        size: size(px(1000.0), px(700.0)),
    };

    // Test line index mapping for all lines
    for line in 0..3 {
        let y_middle = px(70.0) + cache.line_y_offsets[line] + px(12.0);
        let doc_y = (y_middle - viewport.origin.y).max(px(0.0));
        let idx = cache.line_y_offsets.partition_point(|&y| y <= doc_y);
        let resolved_line = idx.saturating_sub(1).min(cache.parsed_lines.len() - 1);
        assert_eq!(resolved_line, line, "Line {} should resolve exactly", line);
    }
}

#[test]
fn test_content_width_clamped_by_viewport_width() {
    // A very wide viewport must not produce an unbounded content column.
    let content_w = crate::editor::shaping::compute_content_width(
        px(1920.0),
        crate::editor::RenderMode::LivePreview,
    );
    assert!(content_w > px(0.0) && content_w <= px(860.0));
}

#[test]
fn test_hit_testing_monotonicity_and_strict_boundary_invariants() {
    let lines = [
        "# Chapter 1: Introduction".to_string(),
        "".to_string(), // Collapsed separator
        "This is a paragraph with several words for hit testing.".to_string(),
        "- [ ] First todo task item".to_string(),
        "- [x] Second completed task item".to_string(),
        "```rust".to_string(),
        "fn main() { println!(\"Hi\"); }".to_string(),
        "```".to_string(),
        "Final paragraph line.".to_string(),
    ];
    let buf = crate::buffer::TextBuffer::from_str(&lines.join("\n"), None);
    let parsed_doc =
        std::sync::Arc::new(crate::markdown::MarkdownScanner::scan_document(buf.lines()));

    let mut cache = crate::editor::layout::DocumentLayoutCache {
        parsed_lines: parsed_doc.clone(),
        ..Default::default()
    };

    let content_width = px(800.0);
    let mut y = px(24.0);
    for (idx, parsed) in cache.parsed_lines.iter().enumerate() {
        let lcx = crate::editor::layout::LineLayoutContext::live_preview(content_width);
        let h = crate::editor::layout::get_line_layout_height(buf.lines(), idx, parsed, lcx);
        cache.line_y_offsets.push(y);
        cache.line_heights.push(h);
        y += h;
    }
    cache.total_height = y + px(24.0);

    let viewport_top = px(70.0);

    let resolve = |win_y: gpui::Pixels, scroll: gpui::Pixels| -> usize {
        let doc_y = (win_y - viewport_top + scroll).max(px(0.0));
        let idx = cache
            .line_y_offsets
            .partition_point(|&offset| offset <= doc_y);
        idx.saturating_sub(1).min(cache.parsed_lines.len() - 1)
    };

    // 1. Boundary & Exact interval mapping for each line
    for line_idx in 0..cache.parsed_lines.len() {
        let top = cache.line_y_offsets[line_idx];
        let h = cache.line_heights[line_idx];
        if h <= px(0.0) {
            continue; // Skip collapsed lines
        }

        // Exact top of the line
        let win_top = viewport_top + top;
        let res_top = resolve(win_top, px(0.0));
        assert_eq!(
            res_top, line_idx,
            "Exact top of line {} should map to {}",
            line_idx, line_idx
        );

        // Middle of the line
        let win_mid = viewport_top + top + h / 2.0;
        let res_mid = resolve(win_mid, px(0.0));
        assert_eq!(
            res_mid, line_idx,
            "Middle of line {} should map to {}",
            line_idx, line_idx
        );

        // Exact bottom (epsilon before next line)
        let win_bottom = viewport_top + top + h - px(0.1);
        let res_bottom = resolve(win_bottom, px(0.0));
        assert_eq!(
            res_bottom, line_idx,
            "Bottom of line {} should map to {}",
            line_idx, line_idx
        );
    }

    // 2. Monotonicity: as window Y increases, resolved line index never decreases
    let mut last_resolved = 0;
    for sample_step in 0..300 {
        let sample_y = viewport_top + px(sample_step as f32 * 2.0);
        let resolved = resolve(sample_y, px(0.0));
        assert!(
            resolved >= last_resolved,
            "Hit-testing monotonicity violation: sample_y={:?} resolved={} < last_resolved={}",
            sample_y,
            resolved,
            last_resolved
        );
        last_resolved = resolved;
    }

    // 3. Scroll invariance: resolve(y, scroll) == resolve(y + scroll, 0)
    for scroll in [px(10.0), px(50.0), px(120.0), px(300.0)] {
        for sample_y in [px(80.0), px(120.0), px(200.0), px(350.0)] {
            let res_with_scroll = resolve(sample_y, scroll);
            let res_without_scroll = resolve(sample_y + scroll, px(0.0));
            assert_eq!(
                res_with_scroll, res_without_scroll,
                "Scroll invariance failed: at sample_y={:?}, scroll={:?}",
                sample_y, scroll
            );
        }
    }
}

#[test]
fn test_column_mapping_bijectivity_and_monotonicity() {
    use crate::editor::shaping::{raw_col_to_visible_byte, visible_byte_to_raw_col};
    use crate::markdown::parse_inline_spans;

    let raw_text = "Hello **bold text** and *italic* and `code` here!";
    let spans = parse_inline_spans(raw_text, 0);
    let total_len = raw_text.chars().count();

    // 1. In active caret mode (disclosed syntax), raw_col -> visible_byte -> raw_col is a bijective roundtrip
    for col in 0..=total_len {
        let vis_byte = raw_col_to_visible_byte(col, &spans, true, col, false);
        let roundtrip_col = visible_byte_to_raw_col(vis_byte, &spans, true, col, false, total_len);
        assert_eq!(
            roundtrip_col, col,
            "Active caret column bijection failed at col {}",
            col
        );
    }

    // 2. In inactive mode (collapsed syntax), raw_col -> visible_byte is monotonic non-decreasing
    let mut prev_byte = 0;
    for col in 0..=total_len {
        let vis_byte = raw_col_to_visible_byte(col, &spans, false, 0, false);
        assert!(
            vis_byte >= prev_byte,
            "Visible byte offset must be monotonic non-decreasing at col {}",
            col
        );
        prev_byte = vis_byte;
    }

    // 3. Source mode is always bijective regardless of active state
    for col in 0..=total_len {
        let vis_byte = raw_col_to_visible_byte(col, &spans, false, 0, true);
        let roundtrip = visible_byte_to_raw_col(vis_byte, &spans, false, 0, true, total_len);
        assert_eq!(
            roundtrip, col,
            "Source mode column bijection failed at col {}",
            col
        );
    }
}

#[test]
fn test_editor_clipboard_prefix_expansion_live_preview() {
    use crate::buffer::{Position, Selection, TextBuffer};
    use crate::editor::{Editor, RenderMode};

    let mut buf = TextBuffer::from_str("# Main Heading\nParagraph content\n- List Item", None);

    // 1. Selecting from start of visible heading (col = 2, after "# ") across multiline in Live Preview
    buf.set_selection(Selection {
        anchor: Position::new(0, 2),
        head: Position::new(1, 9),
    });
    let clip_text =
        Editor::selected_text_for_clipboard_internal(&buf, RenderMode::LivePreview).unwrap();
    assert_eq!(clip_text, "# Main Heading\nParagraph");

    // In Source mode, exact selection is preserved without prefix expansion
    let source_clip =
        Editor::selected_text_for_clipboard_internal(&buf, RenderMode::Source).unwrap();
    assert_eq!(source_clip, "Main Heading\nParagraph");

    // 2. Selecting entire single-line heading from col = 2 to end of line in Live Preview
    buf.set_selection(Selection {
        anchor: Position::new(0, 2),
        head: Position::new(0, 14),
    });
    let clip_single =
        Editor::selected_text_for_clipboard_internal(&buf, RenderMode::LivePreview).unwrap();
    assert_eq!(clip_single, "# Main Heading");

    // 3. Substring selection within heading (col = 7 to 14: "Heading") does not expand prefix
    buf.set_selection(Selection {
        anchor: Position::new(0, 7),
        head: Position::new(0, 14),
    });
    let clip_sub =
        Editor::selected_text_for_clipboard_internal(&buf, RenderMode::LivePreview).unwrap();
    assert_eq!(clip_sub, "Heading");
}

#[test]
fn test_auto_surround_pairs_mapping() {
    use crate::buffer::{Position, Selection, TextBuffer};

    let mut buf = TextBuffer::from_str("Selected text", None);
    buf.set_selection(Selection {
        anchor: Position::new(0, 0),
        head: Position::new(0, 8), // "Selected"
    });

    // Test bracket surround
    buf.surround_selection("(", ")");
    assert_eq!(buf.lines()[0], "(Selected) text");

    // Test Chinese book quote surround
    buf.set_selection(Selection {
        anchor: Position::new(0, 1),
        head: Position::new(0, 9), // "Selected"
    });
    buf.surround_selection("《", "》");
    assert_eq!(buf.lines()[0], "(《Selected》) text");
}

#[test]
fn test_image_layout_invalidates_and_resizes_on_cache_update() {
    let url = "https://example.com/responsive_header.png";
    let markdown = format!("# Title\n\n![Header]({})\n\nFooter paragraph.", url);
    let buf = crate::buffer::TextBuffer::from_str(&markdown, None);

    let parsed_doc =
        std::sync::Arc::new(crate::markdown::MarkdownScanner::scan_document(buf.lines()));
    let content_width = px(800.0);
    let cursor_line = 0;

    // 1. Initial layout calculation when image is not loaded
    let mut initial_cache = crate::editor::layout::DocumentLayoutCache {
        buffer_version: buf.version(),
        lines_len: buf.line_count(),
        render_mode: crate::editor::RenderMode::LivePreview,
        content_width,
        active_line: cursor_line,
        active_col: 0,
        doc_path: None,
        image_cache_version: crate::http::image_cache_version(),
        syntax_cache_version: crate::syntax::syntax_cache_version(),
        fold_version: 0,
        font_size: 15.0,
        line_height: 1.6,
        soft_wrap: true,
        parsed_lines: parsed_doc.clone(),
        line_heights: Vec::new(),
        line_y_offsets: Vec::new(),
        total_height: px(0.0),
        separators: Vec::new(),
    };

    let mut y = px(24.0);
    for (idx, parsed) in parsed_doc.iter().enumerate() {
        let lcx = crate::editor::layout::LineLayoutContext::live_preview(content_width);
        let h = crate::editor::layout::get_line_layout_height(buf.lines(), idx, parsed, lcx);
        initial_cache.line_y_offsets.push(y);
        initial_cache.line_heights.push(h);
        y += h;
    }
    initial_cache.total_height = y + px(24.0);

    // Image line is line 2 (0: # Title, 1: empty, 2: ![Header]...)
    // Initially not loaded: base_h(padding_vertical=8px) + 140px + 12px = 160px
    let placeholder_height = initial_cache.line_heights[2];
    assert_eq!(placeholder_height, px(8.0) + px(140.0) + px(12.0));

    // 2. Simulate image finishing downloading in background
    let initial_img_v = initial_cache.image_cache_version;
    crate::http::record_image_size(url, 1600, 800); // 2:1 aspect ratio -> 800px width yields 400px height
    crate::http::mark_url_loaded(url);
    let new_img_v = crate::http::image_cache_version();
    assert!(new_img_v > initial_img_v);

    // 3. Recompute layout with updated image cache
    let mut updated_cache = initial_cache.clone();
    let mut y = px(24.0);
    updated_cache.line_heights.clear();
    updated_cache.line_y_offsets.clear();
    for (idx, parsed) in parsed_doc.iter().enumerate() {
        let lcx = crate::editor::layout::LineLayoutContext::live_preview(content_width);
        let h = crate::editor::layout::get_line_layout_height(buf.lines(), idx, parsed, lcx);
        updated_cache.line_y_offsets.push(y);
        updated_cache.line_heights.push(h);
        y += h;
    }
    updated_cache.image_cache_version = new_img_v;
    updated_cache.total_height = y + px(24.0);

    // Image line height is now base_h(8px) + img_h(400px) + 12px = 420px
    let loaded_height = updated_cache.line_heights[2];
    assert_eq!(loaded_height, px(8.0) + px(400.0) + px(12.0));
    assert_ne!(placeholder_height, loaded_height);

    // Total document height expanded accordingly
    let height_growth = loaded_height - placeholder_height;
    assert_eq!(
        updated_cache.total_height - initial_cache.total_height,
        height_growth
    );
}

#[test]
fn test_display_map_fold_regions_and_visibility() {
    use crate::editor::display_map::DisplayMap;

    let mut dmap = DisplayMap::new();
    let total_lines = 20;

    // Initially no lines are folded
    for i in 0..total_lines {
        assert!(!dmap.fold_map().is_line_folded(i));
        assert!(!dmap.fold_map().is_fold_header(i));
    }

    // Fold section lines 5..=10 (5 is header, lines 6..=10 hidden)
    dmap.fold_map_mut().fold(5, 10);
    assert!(dmap.fold_map().is_fold_header(5));
    assert!(!dmap.fold_map().is_line_folded(5));
    for line in 6..=10 {
        assert!(dmap.fold_map().is_line_folded(line));
    }
    assert_eq!(dmap.next_visible_line(5, total_lines), Some(11));
    assert_eq!(dmap.prev_visible_line(11), Some(5));

    // Fold another section: lines 12..=15 (lines 13..=15 hidden)
    dmap.fold_map_mut().fold(12, 15);
    assert_eq!(dmap.next_visible_line(12, total_lines), Some(16));
    assert_eq!(dmap.prev_visible_line(16), Some(12));

    // Unfold all
    dmap.fold_map_mut().unfold_all();
    for i in 0..total_lines {
        assert!(!dmap.fold_map().is_line_folded(i));
    }
}

#[test]
fn test_folding_layout_height_and_navigation() {
    use crate::buffer::TextBuffer;
    use crate::editor::display_map::DisplayMap;
    use crate::editor::layout::{DocumentLayoutCache, LineLayoutContext, get_line_layout_height};
    use crate::markdown::MarkdownScanner;

    let markdown = "Line 0\nLine 1\nLine 2\nLine 3\nLine 4\nLine 5";
    let buf = TextBuffer::from_str(markdown, None);
    let parsed_doc = std::sync::Arc::new(MarkdownScanner::scan_document(buf.lines()));
    let content_width = px(800.0);

    let mut dmap = DisplayMap::new();
    assert_eq!(dmap.version(), 0);

    // Initial unfolded layout
    let mut initial_cache = DocumentLayoutCache {
        buffer_version: buf.version(),
        lines_len: buf.line_count(),
        render_mode: crate::editor::RenderMode::LivePreview,
        content_width,
        active_line: 0,
        active_col: 0,
        doc_path: None,
        image_cache_version: 0,
        syntax_cache_version: 0,
        fold_version: dmap.version(),
        font_size: 15.0,
        line_height: 1.6,
        soft_wrap: true,
        parsed_lines: parsed_doc.clone(),
        line_heights: Vec::new(),
        line_y_offsets: Vec::new(),
        total_height: px(0.0),
        separators: Vec::new(),
    };

    let mut y = px(24.0);
    for (idx, parsed) in parsed_doc.iter().enumerate() {
        let is_folded = dmap.fold_map().is_line_folded(idx);
        let lcx = LineLayoutContext::live_preview(content_width).with_folded(is_folded);
        let h = get_line_layout_height(buf.lines(), idx, parsed, lcx);
        initial_cache.line_y_offsets.push(y);
        initial_cache.line_heights.push(h);
        y += h;
    }
    initial_cache.total_height = y + px(24.0);

    // All lines should have non-zero height initially
    for h in &initial_cache.line_heights {
        assert!(*h > px(0.0));
    }

    // Fold lines 1..=3 (line 1 is header; lines 2, 3 folded away)
    dmap.fold_map_mut().fold(1, 3);
    assert_eq!(dmap.version(), 1);
    assert!(dmap.fold_map().is_fold_header(1));
    assert!(!dmap.fold_map().is_line_folded(1));
    assert!(dmap.fold_map().is_line_folded(2));
    assert!(dmap.fold_map().is_line_folded(3));
    assert!(!dmap.fold_map().is_line_folded(4));

    // Next / Prev navigation skipping folded lines
    assert_eq!(dmap.next_visible_line(0, buf.line_count()), Some(1));
    assert_eq!(dmap.next_visible_line(1, buf.line_count()), Some(4));
    assert_eq!(dmap.prev_visible_line(4), Some(1));

    // Recompute folded layout
    let mut folded_cache = DocumentLayoutCache {
        buffer_version: buf.version(),
        lines_len: buf.line_count(),
        render_mode: crate::editor::RenderMode::LivePreview,
        content_width,
        active_line: 0,
        active_col: 0,
        doc_path: None,
        image_cache_version: 0,
        syntax_cache_version: 0,
        fold_version: dmap.version(),
        font_size: 15.0,
        line_height: 1.6,
        soft_wrap: true,
        parsed_lines: parsed_doc.clone(),
        line_heights: Vec::new(),
        line_y_offsets: Vec::new(),
        total_height: px(0.0),
        separators: Vec::new(),
    };

    let mut y_folded = px(24.0);
    for (idx, parsed) in parsed_doc.iter().enumerate() {
        let is_folded = dmap.fold_map().is_line_folded(idx);
        let lcx = LineLayoutContext::live_preview(content_width).with_folded(is_folded);
        let h = get_line_layout_height(buf.lines(), idx, parsed, lcx);
        folded_cache.line_y_offsets.push(y_folded);
        folded_cache.line_heights.push(h);
        y_folded += h;
    }
    folded_cache.total_height = y_folded + px(24.0);

    // Folded lines 2 and 3 must have height 0
    assert_eq!(folded_cache.line_heights[2], px(0.0));
    assert_eq!(folded_cache.line_heights[3], px(0.0));
    // Header line 1 and remaining lines retain normal height
    assert_eq!(folded_cache.line_heights[1], initial_cache.line_heights[1]);
    assert_eq!(folded_cache.line_heights[4], initial_cache.line_heights[4]);

    // Total height shrunk by exactly the heights of lines 2 and 3
    let folded_height_sum = initial_cache.line_heights[2] + initial_cache.line_heights[3];
    assert_eq!(
        initial_cache.total_height - folded_cache.total_height,
        folded_height_sum
    );

    // Unfold line 1
    assert!(dmap.fold_map_mut().unfold(1));
    assert_eq!(dmap.version(), 2);
    assert!(!dmap.fold_map().is_line_folded(2));
    assert!(!dmap.fold_map().is_line_folded(3));
    assert_eq!(dmap.next_visible_line(1, buf.line_count()), Some(2));
}

#[test]
fn test_soft_wrap_multi_line_visual_calculation() {
    use crate::editor::shaping::calculate_wrapped_visual_lines;

    let short_text = "Hello world";
    assert_eq!(
        calculate_wrapped_visual_lines(short_text, px(14.0), px(800.0)),
        1
    );

    // 200 ASCII chars ~ 200 * (14.0 * 0.55 = 7.7px) ~ 1540px total width
    let long_ascii = "a".repeat(200);
    let wrapped_800 = calculate_wrapped_visual_lines(&long_ascii, px(14.0), px(800.0));
    assert!(
        wrapped_800 >= 2,
        "200 characters should wrap to at least 2 visual lines under 800px"
    );

    let wrapped_200 = calculate_wrapped_visual_lines(&long_ascii, px(14.0), px(200.0));
    assert!(
        wrapped_200 >= 7,
        "200 characters should wrap to at least 7 visual lines under 200px"
    );

    // Wide characters (CJK)
    let cjk_text = "中文测试".repeat(20); // 80 CJK chars -> 80 * 14px = 1120px
    let cjk_wrapped = calculate_wrapped_visual_lines(&cjk_text, px(14.0), px(400.0));
    assert!(
        cjk_wrapped >= 3,
        "80 CJK characters should wrap to at least 3 visual lines under 400px"
    );
}

#[test]
fn test_fold_redirection_and_boundary_invariants() {
    use crate::editor::display_map::DisplayMap;

    let mut dmap = DisplayMap::new();

    // Document with 30 lines
    let total_lines = 30;

    // Fold section A: lines 4..=8 (4 is header, 5..=8 hidden)
    dmap.fold_map_mut().fold(4, 8);
    // Fold section B: lines 12..=20 (12 is header, 13..=20 hidden)
    dmap.fold_map_mut().fold(12, 20);

    // Test line folding status
    assert!(dmap.fold_map().is_fold_header(4));
    assert!(!dmap.fold_map().is_line_folded(4));
    for line in 5..=8 {
        assert!(dmap.fold_map().is_line_folded(line));
    }
    assert!(!dmap.fold_map().is_line_folded(9));

    assert!(dmap.fold_map().is_fold_header(12));
    assert!(!dmap.fold_map().is_line_folded(12));
    for line in 13..=20 {
        assert!(dmap.fold_map().is_line_folded(line));
    }
    assert!(!dmap.fold_map().is_line_folded(21));

    // Next visible line navigation
    assert_eq!(dmap.next_visible_line(3, total_lines), Some(4));
    assert_eq!(dmap.next_visible_line(4, total_lines), Some(9)); // Skips 5..=8
    assert_eq!(dmap.next_visible_line(9, total_lines), Some(10));
    assert_eq!(dmap.next_visible_line(11, total_lines), Some(12));
    assert_eq!(dmap.next_visible_line(12, total_lines), Some(21)); // Skips 13..=20

    // Prev visible line navigation
    assert_eq!(dmap.prev_visible_line(21), Some(12)); // Skips 20 down to header 12
    assert_eq!(dmap.prev_visible_line(12), Some(11));
    assert_eq!(dmap.prev_visible_line(9), Some(4)); // Skips 8 down to header 4
    assert_eq!(dmap.prev_visible_line(4), Some(3));

    // Mouse click redirection simulation: if clicked line is folded, resolve to header
    let simulate_click = |clicked_line: usize| -> usize {
        if dmap.fold_map().is_line_folded(clicked_line) {
            dmap.fold_map()
                .folded_regions()
                .iter()
                .find(|r| clicked_line > r.start_line && clicked_line <= r.end_line)
                .map(|r| r.start_line)
                .unwrap_or(clicked_line)
        } else {
            clicked_line
        }
    };

    assert_eq!(simulate_click(4), 4);
    assert_eq!(simulate_click(5), 4);
    assert_eq!(simulate_click(7), 4);
    assert_eq!(simulate_click(8), 4);
    assert_eq!(simulate_click(9), 9);
    assert_eq!(simulate_click(15), 12);
    assert_eq!(simulate_click(20), 12);
    assert_eq!(simulate_click(21), 21);
}

#[test]
fn test_mouse_hit_testing_live_preview_offsets() {
    use crate::buffer::{Position, TextBuffer};
    use crate::config::AppConfig;
    use crate::editor::Editor;
    use crate::theme::ThemeManager;
    use gpui::{Bounds, TestAppContext, point, px, size};

    let mut cx = TestAppContext::single();
    let config = AppConfig::default().with_theme("Dracula");
    let theme_manager = ThemeManager::from_config(&config);
    cx.update(|cx| {
        cx.set_global(theme_manager);
        cx.set_global(config);
    });

    let lines = [
        "Normal paragraph text here.",
        "- [ ] Task item text",
        "- Bullet item text",
        "1. Ordered item text",
        "> Quote block text",
        "```rust",
        "fn main() {}",
        "```",
        "# Heading 1 title",
        "你好中文段落测试",
    ];

    let (editor, cx) = cx.add_window_view(|_window, cx| {
        let buf = TextBuffer::from_str(&lines.join("\n"), None);
        Editor::new_with_buffer(buf, cx)
    });
    cx.run_until_parked();

    cx.update(|window, cx| {
        editor.update(cx, |ed, _cx| {
            ed.viewport_bounds = Bounds {
                origin: point(px(0.0), px(70.0)),
                size: size(px(1000.0), px(800.0)),
            };
            ed.ensure_layout_cache();

            // Line 0: Normal paragraph ("Normal paragraph text here.")
            let line_0_bounds = ed.line_screen_bounds(0, ed.viewport_bounds);
            let click_0 = point(
                line_0_bounds.origin.x + px(1.0),
                line_0_bounds.origin.y + px(10.0),
            );
            assert_eq!(
                ed.find_pos_for_mouse_point(click_0, window),
                Position::new(0, 0)
            );

            // Line 1: TaskList ("- [ ] Task item text", prefix len 6)
            let line_1_bounds = ed.line_screen_bounds(1, ed.viewport_bounds);
            let click_1 = point(
                line_1_bounds.origin.x + px(1.0),
                line_1_bounds.origin.y + px(10.0),
            );
            assert_eq!(
                ed.find_pos_for_mouse_point(click_1, window),
                Position::new(1, 6)
            );
            assert_eq!(
                ed.get_pixel_x_for_col(Position::new(1, 6), window),
                px(24.0)
            );

            // Line 2: BulletList ("- Bullet item text", prefix len 2)
            let line_2_bounds = ed.line_screen_bounds(2, ed.viewport_bounds);
            let click_2 = point(
                line_2_bounds.origin.x + px(1.0),
                line_2_bounds.origin.y + px(10.0),
            );
            assert_eq!(
                ed.find_pos_for_mouse_point(click_2, window),
                Position::new(2, 2)
            );
            assert_eq!(
                ed.get_pixel_x_for_col(Position::new(2, 2), window),
                px(22.0)
            );

            // Line 3: OrderedList ("1. Ordered item text", prefix len 3)
            let line_3_bounds = ed.line_screen_bounds(3, ed.viewport_bounds);
            let click_3 = point(
                line_3_bounds.origin.x + px(1.0),
                line_3_bounds.origin.y + px(10.0),
            );
            assert_eq!(
                ed.find_pos_for_mouse_point(click_3, window),
                Position::new(3, 3)
            );
            assert_eq!(
                ed.get_pixel_x_for_col(Position::new(3, 3), window),
                px(24.0)
            );

            // Line 4: BlockQuote ("> Quote block text", prefix len 2)
            let line_4_bounds = ed.line_screen_bounds(4, ed.viewport_bounds);
            let click_4 = point(
                line_4_bounds.origin.x + px(1.0),
                line_4_bounds.origin.y + px(10.0),
            );
            assert_eq!(
                ed.find_pos_for_mouse_point(click_4, window),
                Position::new(4, 2)
            );
            assert_eq!(
                ed.get_pixel_x_for_col(Position::new(4, 2), window),
                px(18.0)
            );

            // Line 6: CodeBlockContent ("fn main() {}", starts at col 0)
            let line_6_bounds = ed.line_screen_bounds(6, ed.viewport_bounds);
            let click_6 = point(
                line_6_bounds.origin.x + px(1.0),
                line_6_bounds.origin.y + px(10.0),
            );
            assert_eq!(
                ed.find_pos_for_mouse_point(click_6, window),
                Position::new(6, 0)
            );
            assert_eq!(
                ed.get_pixel_x_for_col(Position::new(6, 0), window),
                px(15.0)
            );

            // Line 8: Heading ("# Heading 1 title", prefix len 2)
            let line_8_bounds = ed.line_screen_bounds(8, ed.viewport_bounds);
            let click_8 = point(
                line_8_bounds.origin.x + px(1.0),
                line_8_bounds.origin.y + px(20.0),
            );
            assert_eq!(
                ed.find_pos_for_mouse_point(click_8, window),
                Position::new(8, 2)
            );

            // Line 9: Chinese ("你好中文段落测试")
            let line_9_bounds = ed.line_screen_bounds(9, ed.viewport_bounds);
            let click_9 = point(
                line_9_bounds.origin.x + px(1.0),
                line_9_bounds.origin.y + px(10.0),
            );
            assert_eq!(
                ed.find_pos_for_mouse_point(click_9, window),
                Position::new(9, 0)
            );
        });
    });
}

#[gpui::test]
fn test_nested_inline_code_shaping_and_runs(cx: &mut gpui::TestAppContext) {
    cx.update(|_cx| {
        let text = "2. **创建 `app.js` 文件**:";
        let parsed = crate::markdown::MarkdownScanner::scan_document(&[text.to_string()]);
        assert_eq!(parsed.len(), 1);

        let scx = super::shaping::LineShapingContext::new(
            &crate::theme::DEFAULT_THEME,
            None,
            false,
            false,
            0,
        );
        let base_style = gpui::TextStyle::default();
        let (line_str, runs) =
            super::shaping::build_line_runs(&parsed[0].spans, scx, None, &base_style);

        // When inactive (not active caret), list prefix container is on the side, but span has prefix text, markers are hidden:
        // Visible text string contains "2. 创建 app.js 文件:"
        assert!(line_str.contains("创建 app.js 文件:"));

        // Find the "app.js" run:
        let app_js_run = runs
            .iter()
            .find(|r| r.background_color == Some(crate::theme::DEFAULT_THEME.inline_code_bg));
        assert!(
            app_js_run.is_some(),
            "Expected a TextRun with inline_code_bg"
        );
        let run = app_js_run.unwrap();
        assert_eq!(
            run.font.family,
            crate::platform::platform_monospace_font(),
            "Inline code should use platform monospace font"
        );
    });
}

#[gpui::test]
fn test_ensure_layout_cache_reentrancy_safe(cx: &mut gpui::TestAppContext) {
    let (editor, cx) = cx.add_window_view(|_window, cx| super::Editor::new(cx));
    cx.run_until_parked();

    cx.update(|_window, cx| {
        editor.update(cx, |ed, _cx| {
            use gpui::{Bounds, point, px, size};
            ed.viewport_bounds = Bounds {
                origin: point(px(0.0), px(0.0)),
                size: size(px(800.0), px(600.0)),
            };

            // 1. Acquire an immutable layout snapshot
            let old_snapshot = ed.layout_snapshot();

            // 2. Simulate an async image loading in the background while old_snapshot is still held
            crate::http::record_image_size("https://example.com/reentrancy_test.png", 200, 200);
            let current_v = crate::http::image_cache_version();
            assert!(current_v > old_snapshot.image_cache_version);

            // 3. Obtain a new snapshot while old_snapshot is still held by caller
            let new_snapshot = ed.layout_snapshot();
            assert_eq!(new_snapshot.image_cache_version, current_v);

            // 4. Invariants hold: both snapshots are valid and independent, zero RefCell collisions
            let total_h = ed.get_total_content_height();
            assert!(total_h > px(0.0));
            assert_eq!(total_h, new_snapshot.total_height);
        });
    });
}

/// Autoscroll: `scroll_to_cursor` must keep the cursor line inside the comfort margin band.
#[gpui::test]
fn test_scroll_to_cursor_keeps_cursor_within_comfort_margin(cx: &mut gpui::TestAppContext) {
    use crate::buffer::Position;
    use gpui::{Bounds, point, px, size};

    let content = (0..200)
        .map(|i| format!("Paragraph line number {}", i))
        .collect::<Vec<_>>()
        .join("\n");

    let (editor, cx) = cx.add_window_view(|_window, cx| {
        super::Editor::new_with_buffer(crate::buffer::TextBuffer::from_str(&content, None), cx)
    });
    cx.run_until_parked();

    let viewport_height = px(800.0);
    let comfort_margin = px(56.0);

    cx.update(|_window, cx| {
        editor.update(cx, |ed, _cx| {
            ed.viewport_bounds = Bounds {
                origin: point(px(0.0), px(0.0)),
                size: size(px(1000.0), viewport_height),
            };

            // 1. Cursor already well inside the viewport -> the scroll offset must not move.
            ed.buffer_mut().set_cursor(Position::new(10, 0));
            ed.scroll_state_mut().set_direct(px(0.0), px(100_000.0));
            ed.scroll_to_cursor();
            assert_eq!(ed.scroll_state().target_scroll_top, px(0.0));

            // 2. Cursor far below the viewport -> it must land inside the comfort band.
            let target_line = 150;
            ed.buffer_mut().set_cursor(Position::new(target_line, 0));
            ed.scroll_to_cursor();

            let (line_top, line_bottom) = ed.get_line_y_range(target_line);
            let scroll = ed.scroll_state().target_scroll_top;
            assert!(
                scroll + comfort_margin <= line_top,
                "cursor line top must sit at or below the top comfort margin"
            );
            assert!(
                line_bottom <= scroll + viewport_height - comfort_margin,
                "cursor line bottom must sit at or above the bottom comfort margin"
            );

            // 3. Cursor back at the top -> scrolling up must be honoured.
            ed.buffer_mut().set_cursor(Position::new(0, 0));
            ed.scroll_to_cursor();
            let scroll_up = ed.scroll_state().target_scroll_top;
            assert!(
                scroll_up < scroll,
                "moving the cursor above the viewport must reduce the scroll offset"
            );
            assert!(scroll_up <= ed.get_line_y_range(0).0);
        });
    });
}

/// The incremental layout path (`Editor::ensure_layout_cache` after an edit) must produce exactly
/// the same layout as a full rebuild of the same edited content.
#[gpui::test]
fn test_layout_cache_incremental_update_matches_full_rebuild(cx: &mut gpui::TestAppContext) {
    use gpui::{AppContext, Bounds, point, px, size};

    // Paragraph blocks separated by blank lines; line 45 sits inside the block [41, 50).
    let build_content = |line_45_suffix: &str| {
        (0..100)
            .map(|i| {
                if i % 10 == 0 {
                    String::new()
                } else if i == 45 {
                    format!("Paragraph block content line {}{}", i, line_45_suffix)
                } else {
                    format!("Paragraph block content line {}", i)
                }
            })
            .collect::<Vec<_>>()
            .join("\n")
    };

    let edit_suffix = " with a very long modification that alters wrapping";
    let original = build_content("");
    let edited = build_content(edit_suffix);

    let (editor, cx) = cx.add_window_view(|_window, cx| {
        super::Editor::new_with_buffer(crate::buffer::TextBuffer::from_str(&original, None), cx)
    });
    cx.run_until_parked();

    // Ground truth: a fresh editor whose first layout pass is a full rebuild of the edited text.
    let ground_truth = cx.update(|_window, cx| {
        cx.new(|cx| {
            super::Editor::new_with_buffer(crate::buffer::TextBuffer::from_str(&edited, None), cx)
        })
    });

    let viewport = Bounds {
        origin: point(px(0.0), px(0.0)),
        size: size(px(800.0), px(600.0)),
    };

    let (incremental, full) = cx.update(|_window, cx| {
        let incremental = editor.update(cx, |ed, _cx| {
            ed.viewport_bounds = viewport;
            ed.ensure_layout_cache();
            ed.buffer_mut()
                .record_single_line_edit(45, |line, _| line.push_str(edit_suffix));
            ed.ensure_layout_cache();
            ed.layout_snapshot()
        });
        let full = ground_truth.update(cx, |ed, _cx| {
            ed.viewport_bounds = viewport;
            ed.ensure_layout_cache();
            ed.layout_snapshot()
        });
        (incremental, full)
    });

    assert_eq!(
        incremental.parsed_lines.as_ref(),
        full.parsed_lines.as_ref(),
        "incrementally parsed lines must match a full rebuild"
    );
    assert_eq!(
        incremental.line_heights, full.line_heights,
        "incremental line heights must match a full rebuild"
    );
    assert_eq!(
        incremental.line_y_offsets, full.line_y_offsets,
        "incremental line Y offsets must match a full rebuild"
    );
    assert_eq!(
        incremental.total_height, full.total_height,
        "incremental total height must match a full rebuild"
    );
}

#[gpui::test]
fn test_debounced_auto_save_lifecycle(cx: &mut gpui::TestAppContext) {
    use std::time::Duration;

    struct TempTestFile {
        path: std::path::PathBuf,
    }
    impl Drop for TempTestFile {
        fn drop(&mut self) {
            let _ = std::fs::remove_file(&self.path);
        }
    }

    let file_name = format!(
        "inviscid_test_autosave_{}_{}.md",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    );
    let temp_path = std::env::temp_dir().join(file_name);
    std::fs::write(&temp_path, "Initial content\n").expect("Failed to write initial content");
    let _guard = TempTestFile {
        path: temp_path.clone(),
    };

    // Enable auto_save in AppConfig
    cx.update(|cx| {
        let mut config = crate::config::AppConfig::default();
        config.auto_save = true;
        cx.set_global(config);
    });

    let (editor, cx) = cx.add_window_view(|_window, cx| {
        let mut ed = super::Editor::new_with_path(temp_path.clone(), cx);
        ed.set_auto_save_delay(Duration::from_millis(50));
        ed
    });
    cx.run_until_parked();

    // Initial state: clean buffer, no auto save task
    cx.update(|_window, cx| {
        editor.update(cx, |ed, _cx| {
            assert!(!ed.is_dirty());
            assert!(!ed.has_auto_save_task());
        });
    });

    // 1. Perform edit via perform_edit (typing) -> buffer dirty, auto_save_task active
    cx.update(|_window, cx| {
        editor.update(cx, |ed, cx| {
            ed.perform_edit(cx, |ed| {
                ed.buffer_mut().insert_text("Added text ");
            });
            assert!(ed.is_dirty());
            assert!(ed.has_auto_save_task());
        });
    });

    // Advance virtual clock by 30ms (less than delay of 50ms) -> should NOT be saved yet
    cx.background_executor
        .advance_clock(Duration::from_millis(30));
    cx.run_until_parked();

    let content_after_30ms = std::fs::read_to_string(&temp_path).unwrap();
    assert_eq!(
        content_after_30ms, "Initial content\n",
        "File should not be saved before delay expires"
    );

    // 2. Perform another edit before 50ms expires -> debounce resets timer
    cx.update(|_window, cx| {
        editor.update(cx, |ed, cx| {
            ed.perform_edit(cx, |ed| {
                ed.buffer_mut().insert_text("more text ");
            });
            assert!(ed.is_dirty());
            assert!(ed.has_auto_save_task());
        });
    });

    // Advance by another 30ms (total 60ms, but only 30ms since last edit) -> still NOT saved
    cx.background_executor
        .advance_clock(Duration::from_millis(30));
    cx.run_until_parked();

    let content_after_debounce = std::fs::read_to_string(&temp_path).unwrap();
    assert_eq!(
        content_after_debounce, "Initial content\n",
        "Debounced edit should have reset timer"
    );

    // 3. Now let delay expire (advance 60ms) -> timer fires, auto-saves to disk
    cx.background_executor
        .advance_clock(Duration::from_millis(60));
    cx.run_until_parked();

    let content_after_save = std::fs::read_to_string(&temp_path).unwrap();
    assert!(
        content_after_save.contains("Added text more text"),
        "File must be saved to disk after debounce delay"
    );

    cx.update(|_window, cx| {
        editor.update(cx, |ed, _cx| {
            assert!(!ed.is_dirty(), "Editor should be clean after auto save");
            assert!(!ed.has_auto_save_task(), "Auto save task should be cleared");
        });
    });

    // 4. Test Undo cancellation: edit, then undo back to clean state
    cx.update(|_window, cx| {
        editor.update(cx, |ed, cx| {
            ed.perform_edit(cx, |ed| {
                ed.buffer_mut().insert_text("temporary edit");
            });
            assert!(ed.is_dirty());
            assert!(ed.has_auto_save_task());

            // Undo back to saved content
            ed.undo(cx);
            assert!(!ed.is_dirty(), "Editor should be clean after undo");
            assert!(
                !ed.has_auto_save_task(),
                "Auto save task must be cancelled on undo to clean"
            );
        });
    });

    // 5. Test Untitled document: should never schedule auto save
    let (untitled_editor, cx) = cx.add_window_view(|_window, cx| {
        let mut ed = super::Editor::new(cx);
        ed.set_auto_save_delay(Duration::from_millis(50));
        ed
    });
    cx.run_until_parked();

    cx.update(|_window, cx| {
        untitled_editor.update(cx, |ed, cx| {
            ed.perform_edit(cx, |ed| {
                ed.buffer_mut().insert_text("untitled text");
            });
            assert!(ed.is_dirty());
            assert!(
                !ed.has_auto_save_task(),
                "Untitled editor must never schedule auto save"
            );
        });
    });
}

#[gpui::test]
fn test_debounced_auto_save_edge_cases(cx: &mut gpui::TestAppContext) {
    use gpui::EntityInputHandler;
    use std::time::Duration;

    struct TempTestFile {
        path: std::path::PathBuf,
    }
    impl Drop for TempTestFile {
        fn drop(&mut self) {
            let _ = std::fs::remove_file(&self.path);
        }
    }

    let file_name = format!(
        "inviscid_test_autosave_edge_{}_{}.md",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    );
    let temp_path = std::env::temp_dir().join(file_name);
    std::fs::write(&temp_path, "Edge case base\n").expect("Failed to write initial content");
    let _guard = TempTestFile {
        path: temp_path.clone(),
    };

    // 1. When auto_save is disabled in AppConfig, no task is scheduled on edit
    cx.update(|cx| {
        let mut config = crate::config::AppConfig::default();
        config.auto_save = false;
        cx.set_global(config);
    });

    let (editor, cx) = cx.add_window_view(|_window, cx| {
        let mut ed = super::Editor::new_with_path(temp_path.clone(), cx);
        ed.set_auto_save_delay(Duration::from_millis(50));
        ed
    });
    cx.run_until_parked();

    cx.update(|_window, cx| {
        editor.update(cx, |ed, cx| {
            ed.perform_edit(cx, |ed| {
                ed.buffer_mut().insert_text("edit with auto_save disabled");
            });
            assert!(ed.is_dirty());
            assert!(
                !ed.has_auto_save_task(),
                "Disabled auto_save config must not schedule task"
            );
        });
    });

    // 2. Enable auto_save, verify manual save cancels pending task
    cx.update(|_window, cx| {
        let mut config = crate::config::AppConfig::default();
        config.auto_save = true;
        cx.set_global(config);
    });

    cx.update(|_window, cx| {
        editor.update(cx, |ed, cx| {
            ed.perform_edit(cx, |ed| {
                ed.buffer_mut().insert_text("new change");
            });
            assert!(ed.has_auto_save_task());

            // Manual save should immediately clear auto_save_task
            let task = ed.save_file_async(cx).expect("Save should succeed");
            assert!(
                !ed.has_auto_save_task(),
                "Manual save must cancel auto save task"
            );
            drop(task);
        });
    });

    // 3. Setting read-only mode cancels pending task
    cx.update(|_window, cx| {
        editor.update(cx, |ed, cx| {
            ed.perform_edit(cx, |ed| {
                ed.buffer_mut().insert_text("another change");
            });
            assert!(ed.has_auto_save_task());

            ed.set_read_only(true);
            assert!(
                !ed.has_auto_save_task(),
                "Setting read-only must cancel auto save task"
            );
            ed.set_read_only(false);
        });
    });

    // 4. InputHandler typing triggers debounced auto save
    cx.update(|window, cx| {
        editor.update(cx, |ed, cx| {
            ed.replace_text_in_range(None, "typed via input handler", window, cx);
            assert!(ed.is_dirty());
            assert!(
                ed.has_auto_save_task(),
                "InputHandler typing must trigger debounced auto save"
            );
        });
    });
}

#[test]
fn test_dynamic_tab_size_indent_and_outdent() {
    use crate::buffer::{Position, Selection, TextBuffer};
    let mut buf = TextBuffer::from_str("line1\nline2\nline3", None);
    buf.set_selection(Selection::new(Position::new(0, 0), Position::new(2, 3)));

    // Indent with tab_size 2
    buf.indent_lines_with_tab_size(2);
    assert_eq!(buf.to_string_content(), "  line1\n  line2\n  line3");

    // Outdent with tab_size 2
    buf.outdent_lines_with_tab_size(2);
    assert_eq!(buf.to_string_content(), "line1\nline2\nline3");

    // Indent with tab_size 4
    buf.indent_lines_with_tab_size(4);
    assert_eq!(buf.to_string_content(), "    line1\n    line2\n    line3");

    // Outdent with tab_size 4
    buf.outdent_lines_with_tab_size(4);
    assert_eq!(buf.to_string_content(), "line1\nline2\nline3");
}

#[test]
fn test_soft_wrap_toggle_visual_lines() {
    use crate::editor::shaping::calculate_wrapped_visual_lines_with_wrap;
    let long_text = "This is a very long line of text that exceeds standard container width and will wrap into multiple visual lines when soft wrap is active.";
    let width = px(200.0);
    let font_size = px(15.0);

    let wrapped_count = calculate_wrapped_visual_lines_with_wrap(long_text, font_size, width, true);
    assert!(
        wrapped_count > 1,
        "Should wrap when soft_wrap is true, got {}",
        wrapped_count
    );

    let unwrapped_count =
        calculate_wrapped_visual_lines_with_wrap(long_text, font_size, width, false);
    assert_eq!(
        unwrapped_count, 1,
        "Should never wrap when soft_wrap is false"
    );
}

#[gpui::test]
fn test_typography_and_settings_dynamic_sync(cx: &mut gpui::TestAppContext) {
    use super::{Editor, RenderMode};
    use gpui::Render;

    let mut config = crate::config::AppConfig::default();
    config.editor_font_size = 18.0;
    config.line_height = 2.0;
    config.soft_wrap = false;
    config.cursor_blink = false;
    config.default_render_mode = RenderMode::Source;
    cx.set_global(config);

    cx.add_window(|window, cx| {
        let mut editor = Editor::new(cx);
        assert_eq!(editor.render_mode, RenderMode::Source);
        assert_eq!(editor.font_size, 18.0);
        assert_eq!(editor.line_height, 2.0);
        assert_eq!(editor.soft_wrap, false);
        assert_eq!(editor.cursor_blink, false);
        assert!(!editor.has_blink_task());

        // Update global settings to simulate user modifying Settings panel
        let mut updated = cx.global::<crate::config::AppConfig>().clone();
        updated.editor_font_size = 24.0;
        updated.line_height = 1.4;
        updated.soft_wrap = true;
        updated.cursor_blink = true;
        updated.default_render_mode = RenderMode::LivePreview;
        cx.set_global(updated);

        // Trigger render pass
        let _ = editor.render(window, cx);

        // Editor dynamically adopts updated settings
        assert_eq!(editor.render_mode, RenderMode::LivePreview);
        assert_eq!(editor.font_size, 24.0);
        assert_eq!(editor.line_height, 1.4);
        assert_eq!(editor.soft_wrap, true);
        assert_eq!(editor.cursor_blink, true);
        assert_eq!(editor.cursor_breathing, true);

        // Toggle cursor_breathing to false (classic blink mode)
        let mut config_breathing = cx.global::<crate::config::AppConfig>().clone();
        config_breathing.cursor_breathing = false;
        cx.set_global(config_breathing);
        let _ = editor.render(window, cx);
        assert_eq!(editor.cursor_breathing, false);

        // When cursor blink is reset, blink task is actively created in classic blink mode
        editor.reset_cursor_blink(cx);
        assert!(editor.has_blink_task());

        // Explicit user override on render mode should be preserved
        editor.set_render_mode(RenderMode::Source, cx);
        assert!(editor.has_explicit_render_mode);

        let mut config3 = cx.global::<crate::config::AppConfig>().clone();
        config3.default_render_mode = RenderMode::LivePreview;
        cx.set_global(config3);

        let _ = editor.render(window, cx);
        assert_eq!(
            editor.render_mode,
            RenderMode::Source,
            "Explicit mode must not be overridden by default"
        );

        editor
    });
}

#[test]
fn test_list_item_with_markdown_links_layout_height_matches_visible_text() {
    use crate::editor::layout::{LineLayoutContext, get_block_typography, get_line_layout_height};
    use crate::markdown::MarkdownScanner;
    use gpui::px;

    let lines = vec![
        "- AI Logo generation [https://github.com/Nutlope/logocreator](https://github.com/Nutlope/logocreator)".to_string(),
        "- ACG 资源 [https://github.com/wotakumoe/wotaku](https://github.com/wotakumoe/wotaku)".to_string(),
    ];
    let parsed = MarkdownScanner::scan_document(&lines);
    assert_eq!(parsed.len(), 2);

    // In Live Preview under 800px width:
    // Visible text for line 0 is: "AI Logo generation https://github.com/Nutlope/logocreator" (57 chars)
    // It fits on a single visual line (57 * ~7px = ~400px < 800px).
    // Previously, raw_text (88 chars) wrapped or caused layout height to overestimate to 2 lines.
    let lcx = LineLayoutContext::live_preview(px(800.0));
    let typo = get_block_typography(&parsed[0].kind);
    let h0 = get_line_layout_height(&lines, 0, &parsed[0], lcx.clone());

    // Height must be exactly 1 visual line: padding_vertical + line_height
    assert_eq!(h0, typo.padding_vertical + typo.line_height);

    let h1 = get_line_layout_height(&lines, 1, &parsed[1], lcx);
    assert_eq!(h1, typo.padding_vertical + typo.line_height);
}

#[test]
fn test_source_mode_link_boundary_wrap_accuracy() {
    use crate::editor::LineLayoutContext;
    use crate::editor::layout::get_line_layout_height;
    use crate::markdown::MarkdownScanner;
    use gpui::px;

    // Line 9 from the user's issue:
    let line = "- 已收集了 763+ 款免费可商用的字体网站：ZeoSeven Fonts [https://fonts.zeoseven.com](https://fonts.zeoseven.com)";
    let lines = vec![line.to_string()];
    let parsed = MarkdownScanner::scan_document(&lines);

    let font_size: f32 = 15.0;
    let line_height: f32 = 1.6;
    let single_line_h = px((font_size * line_height).round());

    // 1. In Live Preview when inactive, markers are collapsed and visible text fits in 1 line under 800px width.
    let lcx_live =
        LineLayoutContext::live_preview(px(800.0)).with_metrics(font_size, line_height, true);
    let h_live = get_line_layout_height(&lines, 0, &parsed[0], lcx_live);
    let typo = crate::editor::layout::get_block_typography_with_metrics(
        &parsed[0].kind,
        font_size,
        line_height,
    );
    assert_eq!(h_live, typo.padding_vertical + typo.line_height);

    // 2. In Source mode under 818px (available width 766px after 52px gutter), full raw markdown wraps to 2 visual lines.
    let lcx_src = LineLayoutContext::source(px(818.0)).with_metrics(font_size, line_height, true);
    let h_src = get_line_layout_height(&lines, 0, &parsed[0], lcx_src);
    assert_eq!(h_src, single_line_h * 2.0);
}

#[test]
fn test_source_mode_long_url_wrap_height_prevents_overlap() {
    use crate::editor::LineLayoutContext;
    use crate::editor::layout::get_line_layout_height;
    use crate::markdown::MarkdownScanner;
    use gpui::px;

    let lines = vec![
        "- AI Logo generation [https://github.com/Nutlope](https://github.com/Nutlope)".to_string(),
        "- 开源第三方 Youtube。没有广告，同时会阻止 Google 使用Cookie 和 JavaScript 跟踪 [https://github.com/FreeTubeApp/FreeTube](https://github.com/FreeTubeApp/FreeTube)".to_string(),
    ];
    let parsed = MarkdownScanner::scan_document(&lines);

    let font_size: f32 = 15.0;
    let line_height: f32 = 1.6;
    let single_line_h = px((font_size * line_height).round());

    // 1. Wide container (912px width, available width 860px after 52px gutter)
    let lcx_wide = LineLayoutContext::source(px(912.0)).with_metrics(font_size, line_height, true);
    let h0_wide = get_line_layout_height(&lines, 0, &parsed[0], lcx_wide.clone());
    assert_eq!(
        h0_wide, single_line_h,
        "Line 0 fits on 1 visual line in wide container"
    );

    let h1_wide = get_line_layout_height(&lines, 1, &parsed[1], lcx_wide);
    assert_eq!(
        h1_wide,
        single_line_h * 2.0,
        "Line 1 exceeds 860px and wraps to 2 visual lines"
    );

    // 2. Narrow container (720px width, available width 668px after 52px gutter)
    let lcx_narrow =
        LineLayoutContext::source(px(720.0)).with_metrics(font_size, line_height, true);
    let h0_narrow = get_line_layout_height(&lines, 0, &parsed[0], lcx_narrow);
    assert_eq!(
        h0_narrow,
        single_line_h * 2.0,
        "Line 0 wraps to 2 visual lines in narrow container"
    );
}

#[gpui::test]
fn test_live_preview_link_caret_disclosure_differential_layout_height(
    cx: &mut gpui::TestAppContext,
) {
    use crate::buffer::{Position, TextBuffer};

    let content = "- 已收集了 763+ 款免费可商用的字体网站：ZeoSeven Fonts [https://fonts.zeoseven.com](https://fonts.zeoseven.com)\n- AI 漏洞挖掘自动化 [https://github.com/protectai/vulnhuntr](https://github.com/protectai/vulnhuntr)";

    let (editor, cx) = cx.add_window_view(|_window, cx| {
        super::Editor::new_with_buffer(TextBuffer::from_str(content, None), cx)
    });
    cx.run_until_parked();

    cx.update(|_window, cx| {
        editor.update(cx, |ed, _cx| {
            // 1. Cursor at line 0, column 0 (outside link): syntax markers are collapsed
            ed.buffer_mut().set_cursor(Position::new(0, 0));
            let layout1 = ed.layout_snapshot();
            let h0_collapsed = layout1.line_heights[0];
            let y1_collapsed = layout1.line_y_offsets[1];

            // 2. Cursor moves into the link (col 45): syntax markers expand
            ed.buffer_mut().set_cursor(Position::new(0, 45));
            let layout2 = ed.layout_snapshot();
            let h0_expanded = layout2.line_heights[0];
            let y1_expanded = layout2.line_y_offsets[1];

            assert!(
                h0_expanded > h0_collapsed,
                "Expanded line height ({:?}) must be greater than collapsed line height ({:?})",
                h0_expanded,
                h0_collapsed
            );
            assert!(
                y1_expanded > y1_collapsed,
                "Line 1 Y offset ({:?}) must shift down when Line 0 discloses link markers ({:?})",
                y1_expanded,
                y1_collapsed
            );

            // 3. Cursor moves back out of the link (col 0): line 0 collapses back to 1 line
            ed.buffer_mut().set_cursor(Position::new(0, 0));
            let layout3 = ed.layout_snapshot();
            assert_eq!(layout3.line_heights[0], h0_collapsed);
            assert_eq!(layout3.line_y_offsets[1], y1_collapsed);
        });
    });
}

#[gpui::test]
fn test_text_system_native_line_wrapper_layout(cx: &mut gpui::TestAppContext) {
    use crate::buffer::TextBuffer;
    use crate::editor::RenderMode;
    use gpui::px;

    let content = "# Heading Line\nThis is a long paragraph that wraps across multiple visual lines when constrained to a narrow viewport width.\n- Short bullet item";
    let (editor, cx) = cx.add_window_view(|_window, cx| {
        super::Editor::new_with_buffer(TextBuffer::from_str(content, None), cx)
    });
    cx.run_until_parked();

    cx.update(|_window, cx| {
        editor.update(cx, |ed, _cx| {
            assert!(
                ed.text_system.is_some(),
                "Editor must hold native TextSystem handle"
            );

            let layout = ed.layout_snapshot();
            assert_eq!(layout.line_heights.len(), 3);
            assert!(layout.total_height > px(0.0));

            ed.set_render_mode(RenderMode::Source, _cx);
            let src_layout = ed.layout_snapshot();
            assert_eq!(src_layout.render_mode, RenderMode::Source);
            assert_eq!(src_layout.line_heights.len(), 3);
        });
    });
}

#[gpui::test]
fn test_line_wrapper_matches_shape_text_across_widths(cx: &mut gpui::TestAppContext) {
    use crate::buffer::Position;
    use crate::buffer::TextBuffer;
    use gpui::px;

    let content = "## 2025.4\n- AI Logo generation [https://github.com/Nutlope/logocreator](https://github.com/Nutlope/logocreator)\n- 开源第三方 Youtube。没有广告，同时会阻止 Google 使用Cookie 和 JavaScript 跟踪 https://github.com/FreeTubeApp/FreeTube";

    let (editor, cx) = cx.add_window_view(|_window, cx| {
        super::Editor::new_with_buffer(TextBuffer::from_str(content, None), cx)
    });
    cx.run_until_parked();

    cx.update(|window, cx| {
        editor.update(cx, |ed, _cx| {
            let ts = ed.text_system.clone().unwrap();
            ed.viewport_bounds =
                gpui::Bounds::new(gpui::Point::default(), gpui::size(px(1200.0), px(800.0)));

            // 1. First layout snapshot when cursor is at (0, 0)
            ed.buffer_mut().set_cursor(Position::new(0, 0));
            let layout_initial = ed.layout_snapshot();
            assert_eq!(layout_initial.line_heights[1], px(32.0));
            assert_eq!(layout_initial.line_y_offsets[2], px(106.0));

            // 2. Move cursor to "Nut|lope" in line 1
            ed.buffer_mut().set_cursor(Position::new(1, 26));
            let layout = ed.layout_snapshot();
            assert_eq!(layout.line_heights[1], px(56.0));
            assert_eq!(layout.line_y_offsets[2], px(130.0));
            assert!(layout.line_y_offsets[2] >= layout.line_y_offsets[1] + layout.line_heights[1]);

            let parsed_lines = &layout.parsed_lines;
            let line1 = &parsed_lines[1];
            let vis_text = line1.visible_text(true, 26, false);
            let typo = crate::editor::layout::get_block_typography_with_metrics(
                &line1.kind,
                ed.font_size,
                ed.line_height,
            );

            let font = gpui::font(crate::platform::platform_ui_font());
            let theme = crate::theme::Theme::default();
            let scx =
                crate::editor::shaping::LineShapingContext::new(&theme, None, false, true, 26);
            let (line_str, runs) = crate::editor::shaping::build_line_runs(
                &line1.spans,
                scx,
                None,
                &window.text_style(),
            );

            // 3. Verify that line_wrapper wrap count and shape_text wrap count match across widths
            for w_val in (600..=950).step_by(5) {
                let test_w = px(w_val as f32);
                let mut w_seg = ts.line_wrapper(font.clone(), typo.font_size);
                let wrapper_count = w_seg
                    .wrap_line(&[gpui::LineFragment::text(&vis_text)], test_w)
                    .count()
                    + 1;
                let shaped = window
                    .text_system()
                    .shape_text(
                        line_str.clone().into(),
                        typo.font_size,
                        &runs,
                        Some(test_w),
                        None,
                    )
                    .unwrap();
                let shape_count = shaped[0].wrap_boundaries.len() + 1;
                assert_eq!(
                    wrapper_count, shape_count,
                    "Mismatch at width {:?}: wrapper={} vs shape={}",
                    test_w, wrapper_count, shape_count
                );
            }
        });
    });
}

/// Verifies that IME composition keeps marker disclosure pinned to the composition anchor.
#[gpui::test]
fn test_ime_composition_pins_disclosure_to_composition_anchor(cx: &mut gpui::TestAppContext) {
    use crate::buffer::{Position, TextBuffer};
    use gpui::{Bounds, EntityInputHandler, point, px, size};

    let content = "已收集了 763+ 款免费可商用的字体网站：ZeoSeven Fonts [https://fonts.zeoseven.com](https://fonts.zeoseven.com)\n尾随行";

    let (editor, cx) = cx.add_window_view(|_window, cx| {
        super::Editor::new_with_buffer(TextBuffer::from_str(content, None), cx)
    });
    cx.run_until_parked();

    cx.update(|window, cx| {
        editor.update(cx, |ed, cx| {
            ed.viewport_bounds = Bounds {
                origin: point(px(0.0), px(70.0)),
                size: size(px(1000.0), px(800.0)),
            };

            let line_len = ed.buffer_mut().line(0).unwrap().chars().count();
            ed.buffer_mut().set_cursor(Position::new(0, line_len));
            let before = ed.layout_snapshot();
            let before_height = before.line_heights[0];
            let before_total = before.total_height;
            let before_visible = before.parsed_lines[0]
                .visible_text(true, line_len, false)
                .chars()
                .count();
            assert!(before_total > before_height + before.line_heights[1]);

            ed.replace_and_mark_text_in_range(None, "ni", Some(2..2), window, cx);

            assert_eq!(
                ed.disclosure_col(),
                line_len,
                "disclosure must stay pinned to the composition anchor, not follow the preedit tail"
            );
            let during = ed.layout_snapshot();
            assert_eq!(
                during.line_heights[0], before_height,
                "an in-flight preedit must not re-flow its own line"
            );
            assert_eq!(
                during.total_height, before_total,
                "an in-flight preedit must not shift subsequent lines"
            );
            assert_eq!(
                during.parsed_lines[0]
                    .visible_text(true, ed.disclosure_col(), false)
                    .chars()
                    .count(),
                before_visible,
                "an in-flight preedit must not change the projected text"
            );
            assert_eq!(
                crate::editor::shaping::ime_preedit_overlay(
                    &during.parsed_lines[0].spans,
                    true,
                    ed.disclosure_col(),
                    false,
                ),
                Some((
                    "ni".to_string(),
                    before.parsed_lines[0]
                        .visible_text(true, line_len, false)
                        .len()
                )),
                "the preedit must still be paintable as an overlay, anchored where the visible text ends"
            );

            ed.replace_text_in_range(None, "你", window, cx);
            let after = ed.layout_snapshot();
            assert_eq!(ed.disclosure_col(), line_len + 1);
            assert!(
                after.line_heights[0] < before_height,
                "markers hide once the committed caret leaves the construct"
            );
        });
    });
}

/// Verifies that composing on an empty line projects the preedit as an overlay without growing line height.
#[gpui::test]
fn test_ime_composition_on_empty_line_is_overlay_only(cx: &mut gpui::TestAppContext) {
    use crate::buffer::{Position, TextBuffer};
    use gpui::{Bounds, EntityInputHandler, point, px, size};

    let (editor, cx) = cx.add_window_view(|_window, cx| {
        super::Editor::new_with_buffer(TextBuffer::from_str("上一行\n", None), cx)
    });
    cx.run_until_parked();

    cx.update(|window, cx| {
        editor.update(cx, |ed, cx| {
            ed.viewport_bounds = Bounds {
                origin: point(px(0.0), px(70.0)),
                size: size(px(1000.0), px(800.0)),
            };
            ed.buffer_mut().set_cursor(Position::new(1, 0));
            let before = ed.layout_snapshot();
            let before_height = before.line_heights[1];
            assert!(
                before.parsed_lines[1]
                    .visible_text(true, 0, false)
                    .is_empty(),
                "the fixture line must start empty"
            );

            ed.replace_and_mark_text_in_range(None, "ni", Some(2..2), window, cx);

            let during = ed.layout_snapshot();
            assert!(
                during.parsed_lines[1]
                    .visible_text(true, 0, false)
                    .is_empty(),
                "an in-flight preedit must not become line content"
            );
            assert_eq!(
                during.line_heights[1], before_height,
                "an empty line must not grow while composing"
            );
            assert_eq!(during.total_height, before.total_height);
            assert_eq!(
                crate::editor::shaping::ime_preedit_overlay(
                    &during.parsed_lines[1].spans,
                    true,
                    ed.disclosure_col(),
                    false,
                ),
                Some(("ni".to_string(), 0)),
                "the empty-line render path must still be able to paint the preedit"
            );
        });
    });
}

/// Verifies that an active IME composition does not shift document geometry while the candidate is open.
#[gpui::test]
fn test_ime_composition_does_not_move_document_geometry(cx: &mut gpui::TestAppContext) {
    use crate::buffer::{Position, TextBuffer};
    use gpui::{Bounds, EntityInputHandler, point, px, size};

    let long_cjk = "这是一段用于测试软换行行为的中文段落，组合输入时行内断行点是否会发生变化，直接决定整行会不会在输入字母时抖动。".repeat(3);
    let content = [long_cjk.as_str(), "尾随行"].join("\n");

    let (editor, cx) = cx.add_window_view(|_window, cx| {
        super::Editor::new_with_buffer(TextBuffer::from_str(&content, None), cx)
    });
    cx.run_until_parked();

    cx.update(|window, cx| {
        editor.update(cx, |ed, cx| {
            ed.viewport_bounds = Bounds {
                origin: point(px(0.0), px(70.0)),
                size: size(px(1000.0), px(800.0)),
            };
            let theme = crate::theme::Theme::default();
            let font_size = px(ed.font_size);
            let line_height = px((ed.font_size * ed.line_height).round());

            let probe = |ed: &super::Editor,
                         window: &mut gpui::Window,
                         caret_col: usize,
                         tracked_col: usize| {
                let layout = ed.layout_snapshot();
                let parsed = &layout.parsed_lines[0];
                let disclosure = ed.disclosure_col();
                let available = crate::editor::shaping::compute_available_line_width_with_mode(
                    crate::editor::shaping::compute_content_width(px(1000.0), ed.render_mode),
                    &parsed.kind,
                    ed.render_mode,
                );
                let scx = crate::editor::shaping::LineShapingContext::new(
                    &theme, None, false, true, disclosure,
                );
                let (line_str, runs) = crate::editor::shaping::build_line_runs(
                    &parsed.spans,
                    scx,
                    None,
                    &window.text_style(),
                );
                let shaped = window
                    .text_system()
                    .shape_text(line_str.into(), font_size, &runs, Some(available), None)
                    .unwrap();
                let follow_x = shaped[0]
                    .position_for_index(
                        crate::editor::shaping::raw_col_to_visible_byte(
                            tracked_col,
                            &parsed.spans,
                            true,
                            disclosure,
                            false,
                        ),
                        line_height,
                    )
                    .map(|p| p.x)
                    .unwrap_or(px(0.0));
                let caret_row = shaped[0]
                    .position_for_index(
                        crate::editor::shaping::raw_col_to_visible_byte(
                            caret_col,
                            &parsed.spans,
                            true,
                            disclosure,
                            false,
                        ),
                        line_height,
                    )
                    .map(|p| (f32::from(p.y) / f32::from(line_height)).round() as usize)
                    .unwrap_or(0);
                (
                    shaped[0].wrap_boundaries.len() + 1,
                    caret_row,
                    follow_x,
                    layout.line_heights[0],
                    layout.total_height,
                )
            };

            let line_len = ed.buffer_mut().line(0).unwrap().chars().count();
            let preedit = "zhongguo";
            let u16_len = preedit.encode_utf16().count();
            for col in [20usize, 90] {
                assert!(col < line_len);
                ed.buffer_mut().set_cursor(Position::new(0, col));
                let before = probe(ed, window, col, col);

                ed.replace_and_mark_text_in_range(
                    None,
                    preedit,
                    Some(u16_len..u16_len),
                    window,
                    cx,
                );
                let during = probe(ed, window, col + u16_len, col + u16_len);

                assert_eq!(
                    during, before,
                    "col {col}: the document geometry must be untouched while composing"
                );

                ed.replace_text_in_range(None, "你", window, cx);
                let settled = ed.layout_snapshot();
                assert_eq!(
                    crate::editor::shaping::ime_preedit_overlay(
                        &settled.parsed_lines[0].spans,
                        true,
                        ed.disclosure_col(),
                        false,
                    ),
                    None,
                    "col {col}: the preedit overlay must be gone once the candidate is committed"
                );
                assert!(
                    ed.buffer_mut().line(0).unwrap().contains('你'),
                    "col {col}: the committed character must land in the document"
                );
            }
        });
    });
}

/// Verifies that the Backspace key event that empties an IME composition is not replayed on the document.
#[gpui::test]
fn test_ime_consumed_backspace_is_not_replayed_on_document(cx: &mut gpui::TestAppContext) {
    use crate::buffer::{Position, TextBuffer};
    use gpui::EntityInputHandler;

    let (editor, cx) = cx.add_window_view(|_window, cx| {
        super::Editor::new_with_buffer(TextBuffer::from_str("中文", None), cx)
    });
    cx.run_until_parked();

    cx.update(|window, cx| {
        editor.update(cx, |ed, _cx| {
            ed.buffer_mut().set_cursor(Position::new(0, 2));

            // 1. Start composition with "n" as marked text.
            ed.replace_and_mark_text_in_range(None, "n", Some(1..1), window, _cx);
            assert_eq!(ed.buffer_mut().line(0).unwrap(), "中文n");
            assert!(ed.marked_range.is_some());
            assert!(
                ed.edit_key_belongs_to_ime(),
                "a live composition owns the key"
            );

            // 2. Backspace clears the preedit and publishes the empty composition before the key action runs.
            ed.replace_and_mark_text_in_range(None, "", None, window, _cx);
            assert_eq!(
                ed.buffer_mut().line(0).unwrap(),
                "中文",
                "the pending letter is removed, the confirmed characters are not"
            );
            assert!(ed.marked_range.is_none());

            // 3. The follow-up key event is recognized once and consumed.
            assert!(
                ed.edit_key_belongs_to_ime(),
                "the key press that emptied the preedit must not reach the document"
            );
            assert!(
                !ed.edit_key_belongs_to_ime(),
                "the consumed-key token must not linger beyond its own key press"
            );

            ed.ime_consumed_edit_key_at =
                Some(std::time::Instant::now() - std::time::Duration::from_millis(500));
            assert!(!ed.edit_key_belongs_to_ime());
            assert!(
                ed.ime_consumed_edit_key_at.is_none(),
                "a stale token is consumed rather than left behind"
            );
        });
    });
}

/// Verifies that the IME preedit overlay sits on the row baseline and remains visible across caret blinks.
#[test]
fn test_ime_preedit_overlay_sits_on_the_row_baseline_and_outlives_the_caret() {
    use crate::editor::shaping::{calculate_caret_size, ime_preedit_placement};
    use gpui::{Bounds, point, px, size};

    let font_size = px(16.0);
    let line_height = px(26.0);
    let (caret_w, caret_h) = calculate_caret_size(font_size, line_height);
    let line_origin = point(px(40.0), px(70.0));
    let anchor = point(px(120.0), px(26.0));
    let caret = Bounds::new(
        point(
            line_origin.x + anchor.x,
            (line_origin.y + anchor.y + (line_height - caret_h) / 2.0).round(),
        ),
        size(caret_w, caret_h.round()),
    );

    let (origin, caret_out) = ime_preedit_placement(line_origin, anchor, px(48.0), None);
    assert_eq!(
        origin,
        point(line_origin.x + anchor.x, line_origin.y + anchor.y),
        "the preedit must be placed on the anchor's baseline even while the caret is invisible"
    );
    assert!(caret_out.is_none());

    let (origin, caret_out) = ime_preedit_placement(line_origin, anchor, px(48.0), Some(caret));
    assert_eq!(origin.y, line_origin.y + anchor.y);
    assert_ne!(
        origin.y, caret.origin.y,
        "the caret is centered in the line box, so anchoring the preedit at it pushes the \
         composition string below the line"
    );
    assert_eq!(
        caret_out.unwrap().origin.x,
        origin.x + px(48.0),
        "the caret follows the composition string"
    );
}

#[test]
fn test_syntax_highlighting_runs_in_live_preview_and_source_modes() {
    use crate::editor::shaping::{LineShapingContext, build_line_runs};
    use crate::markdown::InlineSpan;
    use crate::syntax::SyntaxToken;
    use crate::theme::Theme;
    use gpui::{FontStyle, TextStyle};

    let theme = Theme::default();
    let base_style = TextStyle::default();

    let spans = vec![
        InlineSpan::syntax("fn", (0, 2), Some(SyntaxToken::Keyword)),
        InlineSpan::plain(" ", (2, 3)),
        InlineSpan::syntax("main", (3, 7), Some(SyntaxToken::Function)),
        InlineSpan::syntax("()", (7, 9), Some(SyntaxToken::Punctuation)),
        InlineSpan::plain(" ", (9, 10)),
        InlineSpan::syntax("// entry", (10, 18), Some(SyntaxToken::Comment)),
    ];

    let live_scx = LineShapingContext::new(&theme, Some(theme.code_block_text), false, false, 0);
    let (live_text, live_runs) = build_line_runs(&spans, live_scx, None, &base_style);
    assert_eq!(live_text, "fn main() // entry");
    assert_eq!(live_runs.len(), 6);
    assert_eq!(live_runs[0].color, theme.syntax_keyword);
    assert_eq!(live_runs[1].color, theme.code_block_text);
    assert_eq!(live_runs[2].color, theme.syntax_function);
    assert_eq!(live_runs[3].color, theme.syntax_punctuation);
    assert_eq!(live_runs[4].color, theme.code_block_text);
    assert_eq!(live_runs[5].color, theme.syntax_comment);
    assert_eq!(live_runs[5].font.style, FontStyle::Italic);

    let src_scx = LineShapingContext::new(&theme, None, true, false, 0);
    let (src_text, src_runs) = build_line_runs(&spans, src_scx, None, &base_style);
    assert_eq!(src_text, "fn main() // entry");
    assert_eq!(src_runs.len(), 6);
    assert_eq!(src_runs[0].color, theme.syntax_keyword);
    assert_eq!(src_runs[1].color, theme.text_primary);
    assert_eq!(src_runs[2].color, theme.syntax_function);
    assert_eq!(src_runs[3].color, theme.syntax_punctuation);
    assert_eq!(src_runs[4].color, theme.text_primary);
    assert_eq!(src_runs[5].color, theme.syntax_comment);
    assert_eq!(src_runs[5].font.style, FontStyle::Italic);
}

#[gpui::test]
fn test_code_block_incremental_edit_and_syntax_cache_invalidation(cx: &mut gpui::TestAppContext) {
    use crate::buffer::{Position, TextBuffer};
    use crate::syntax::{SyntaxToken, register_global_wasm_grammar};

    let wasm_bytes = include_bytes!("../../tests/fixtures/tree-sitter-json.wasm");
    register_global_wasm_grammar("json", wasm_bytes, None)
        .expect("Failed to register JSON WASM grammar");

    let doc = "```json\n{\n\n  \"count\": 1\n}\n```";
    let (editor, cx) = cx.add_window_view(|_window, cx| {
        super::Editor::new_with_buffer(TextBuffer::from_str(doc, None), cx)
    });
    cx.run_until_parked();

    cx.update(|_window, cx| {
        editor.update(cx, |ed, _cx| {
            let snap1 = ed.layout_snapshot();
            assert!(snap1.parsed_lines[3].spans.iter().any(|s| {
                s.text == "\"count\"" && s.syntax_token == Some(SyntaxToken::Variable)
            }));
            assert!(
                snap1.parsed_lines[3]
                    .spans
                    .iter()
                    .any(|s| { s.text == "1" && s.syntax_token == Some(SyntaxToken::Number) })
            );

            // Edit inside the code block across a blank line: change `1` (Number) to `"ok"` (String)
            ed.buffer_mut().set_cursor(Position::new(3, 11));
            ed.buffer_mut().drag_selection_to(Position::new(3, 12));
            ed.buffer_mut().insert_text("\"ok\"");

            let snap2 = ed.layout_snapshot();
            assert!(
                snap2.parsed_lines[3]
                    .spans
                    .iter()
                    .any(|s| { s.text == "\"ok\"" && s.syntax_token == Some(SyntaxToken::String) })
            );
        });
    });
}

#[gpui::test]
fn test_editor_save_preserves_line_ending_and_tracks_mtime(cx: &mut gpui::TestAppContext) {
    let temp_dir = tempfile::tempdir().expect("create temp dir");
    let test_file = temp_dir.path().join("crlf_doc.md");
    let crlf_content = "# Title\r\n\r\nFirst paragraph\r\n";
    std::fs::write(&test_file, crlf_content).unwrap();

    let (editor, cx) =
        cx.add_window_view(|_window, cx| super::Editor::new_with_path(test_file.clone(), cx));
    cx.run_until_parked();

    cx.update(|_window, cx| {
        editor.update(cx, |ed, _cx| {
            assert_eq!(ed.buffer().line_ending(), crate::buffer::LineEnding::CrLf);
            assert!(!ed.is_externally_modified());
        });
    });

    cx.update(|_window, cx| {
        editor.update(cx, |ed, cx| {
            ed.perform_edit(cx, |ed| {
                ed.buffer_mut().insert_text("Updated line\r\n");
            });
            let task = ed.save_file_async(cx).expect("save should succeed");
            cx.spawn(async move |_this, _cx| {
                task.await.unwrap();
            })
            .detach();
        });
    });
    cx.run_until_parked();

    let read_back = std::fs::read_to_string(&test_file).unwrap();
    assert!(read_back.contains("\r\n"));

    cx.update(|_window, cx| {
        editor.update(cx, |ed, _cx| {
            assert!(!ed.is_dirty());
            assert!(!ed.is_externally_modified());
        });
    });
}
