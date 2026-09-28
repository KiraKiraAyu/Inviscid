use super::*;

#[test]
fn test_column_goal_movement() {
    let mut buf = TextBuffer::from_str("Hello World Long Line\nShort\nAnother Long Line", None);
    buf.selection = Selection::cursor(Position::new(0, 15));

    buf.move_down_mode(false, false);
    assert_eq!(buf.selection.head, Position::new(1, 5));
    assert_eq!(buf.column_goal, Some(15));

    buf.move_down_mode(false, false);
    assert_eq!(buf.selection.head, Position::new(2, 15));
}

#[test]
fn test_selection_collapse() {
    let mut buf = TextBuffer::from_str("Hello World", None);
    buf.selection = Selection {
        anchor: Position::new(0, 2),
        head: Position::new(0, 7),
    };

    buf.move_left_mode(false, false);
    assert_eq!(buf.selection.head, Position::new(0, 2));
    assert!(buf.selection.is_empty());

    buf.selection = Selection {
        anchor: Position::new(0, 2),
        head: Position::new(0, 7),
    };
    buf.move_right_mode(false, false);
    assert_eq!(buf.selection.head, Position::new(0, 7));
    assert!(buf.selection.is_empty());
}

#[test]
fn test_word_navigation() {
    let mut buf = TextBuffer::from_str("fn hello_world() {", None);
    buf.selection = Selection::cursor(Position::new(0, 18));

    buf.move_to_previous_word(false);
    assert_eq!(buf.selection.head, Position::new(0, 17));

    buf.move_to_previous_word(false);
    assert_eq!(buf.selection.head, Position::new(0, 14));

    buf.move_to_previous_word(false);
    assert_eq!(buf.selection.head, Position::new(0, 3));

    buf.move_to_previous_word(false);
    assert_eq!(buf.selection.head, Position::new(0, 0));
}

#[test]
fn test_surround_selection() {
    let mut buf = TextBuffer::from_str("Hello World", None);
    buf.selection = Selection {
        anchor: Position::new(0, 6),
        head: Position::new(0, 11),
    };

    buf.surround_selection("**", "**");
    assert_eq!(buf.lines[0], "Hello **World**");
    assert_eq!(buf.selection.start(), Position::new(0, 6));
    assert_eq!(buf.selection.end(), Position::new(0, 15));
}

#[test]
fn test_move_and_duplicate_lines() {
    let mut buf = TextBuffer::from_str("Line 1\nLine 2\nLine 3", None);
    buf.selection = Selection::cursor(Position::new(1, 0));

    buf.move_lines_up();
    assert_eq!(buf.lines, vec!["Line 2", "Line 1", "Line 3"]);

    buf.move_lines_down();
    assert_eq!(buf.lines, vec!["Line 1", "Line 2", "Line 3"]);

    buf.duplicate_lines_down();
    assert_eq!(buf.lines, vec!["Line 1", "Line 2", "Line 2", "Line 3"]);
}

#[test]
fn test_indent_outdent_lines() {
    let mut buf = TextBuffer::from_str("Line 1\nLine 2", None);
    buf.selection = Selection {
        anchor: Position::new(0, 0),
        head: Position::new(1, 6),
    };

    buf.indent_lines();
    assert_eq!(buf.lines, vec!["    Line 1", "    Line 2"]);

    buf.outdent_lines();
    assert_eq!(buf.lines, vec!["Line 1", "Line 2"]);
}

#[test]
fn test_empty_code_block_backspace() {
    let mut buf = TextBuffer::from_str("Paragraph 1\n```rust\n\n```\nParagraph 2", None);
    buf.selection = Selection::cursor(Position::new(2, 0));
    buf.backspace_mode(false);
    assert_eq!(buf.lines, vec!["Paragraph 1", "", "", "Paragraph 2"]);
    assert_eq!(buf.selection.head, Position::new(1, 0));
}

#[test]
fn test_move_into_code_language_header() {
    let mut buf = TextBuffer::from_str("Paragraph 1\n```rust\ncode\n```\nParagraph 2", None);
    buf.selection = Selection::cursor(Position::new(0, 0));
    buf.move_down_mode(false, false);
    assert_eq!(buf.selection.head, Position::new(1, 7));

    buf.selection = Selection::cursor(Position::new(2, 0));
    buf.move_up_mode(false, false);
    assert_eq!(buf.selection.head, Position::new(1, 7));
}

#[test]
fn test_code_language_header_backspace() {
    let mut buf = TextBuffer::from_str("Paragraph 1\n```rust\ncode\n```\nParagraph 2", None);

    buf.selection = Selection::cursor(Position::new(1, 3));
    buf.backspace_mode(false);
    assert_eq!(buf.lines[1], "```rust");
    assert_eq!(buf.selection.head, Position::new(1, 3));

    buf.selection = Selection::cursor(Position::new(1, 7));
    buf.backspace_mode(false);
    assert_eq!(buf.lines[1], "```rus");
    assert_eq!(buf.selection.head, Position::new(1, 6));
}

#[test]
fn test_code_language_header_arrow_navigation() {
    let mut buf = TextBuffer::from_str("```rust\nfn main() {}\n```", None);
    buf.selection = Selection::cursor(Position::new(0, 7));

    buf.move_left_mode(false, false);
    assert_eq!(buf.selection.head, Position::new(0, 6));

    buf.move_left_mode(false, false);
    assert_eq!(buf.selection.head, Position::new(0, 5));

    buf.move_left_mode(false, false);
    assert_eq!(buf.selection.head, Position::new(0, 4));

    buf.move_left_mode(false, false);
    assert_eq!(buf.selection.head, Position::new(0, 3));

    buf.move_right_mode(false, false);
    assert_eq!(buf.selection.head, Position::new(0, 4));
}

#[test]
fn test_heading_backspace_at_marker() {
    let mut buf = TextBuffer::from_str("# Welcome to Inviscid", None);
    buf.selection = Selection::cursor(Position::new(0, 2));
    buf.backspace_mode(false);
    assert_eq!(buf.lines[0], "Welcome to Inviscid");
    assert_eq!(buf.selection.head, Position::new(0, 0));

    let mut buf2 = TextBuffer::from_str("## Core Features", None);
    buf2.selection = Selection::cursor(Position::new(0, 3));
    buf2.backspace_mode(false);
    assert_eq!(buf2.lines[0], "Core Features");
    assert_eq!(buf2.selection.head, Position::new(0, 0));
}

#[test]
fn test_heading_enter() {
    let mut buf = TextBuffer::from_str("# Welcome to Inviscid", None);
    buf.selection = Selection::cursor(Position::new(0, 2));
    buf.insert_newline();
    assert_eq!(buf.lines, vec!["", "# Welcome to Inviscid"]);
    assert_eq!(buf.selection.head, Position::new(1, 2));

    let mut buf2 = TextBuffer::from_str("# Welcome to Inviscid", None);
    buf2.selection = Selection::cursor(Position::new(0, 21));
    buf2.insert_newline();
    assert_eq!(buf2.lines, vec!["# Welcome to Inviscid", "", ""]);
    assert_eq!(buf2.selection.head, Position::new(2, 0));
}

#[test]
fn test_list_item_arrow_navigation() {
    let mut buf = TextBuffer::from_str("## Core Features\n- Native GPU Text", None);
    buf.selection = Selection::cursor(Position::new(0, 16));
    buf.move_right_mode(false, false);
    assert_eq!(buf.selection.head, Position::new(1, 2));

    buf.move_right_mode(false, false);
    assert_eq!(buf.selection.head, Position::new(1, 3));

    buf.move_left_mode(false, false);
    assert_eq!(buf.selection.head, Position::new(1, 2));

    buf.move_left_mode(false, false);
    assert_eq!(buf.selection.head, Position::new(0, 16));
}

#[test]
fn test_thematic_break_backspace_and_delete() {
    let mut buf = TextBuffer::from_str("Paragraph 1\n---\nParagraph 2", None);
    buf.selection = Selection::cursor(Position::new(1, 0));
    buf.backspace_mode(false);
    assert_eq!(buf.lines, vec!["Paragraph 1", "", "Paragraph 2"]);
    assert_eq!(buf.selection.head, Position::new(1, 0));

    let mut buf2 = TextBuffer::from_str("Paragraph 1\n---\nParagraph 2", None);
    buf2.selection = Selection::cursor(Position::new(1, 0));
    buf2.delete_forward_mode(false);
    assert_eq!(buf2.lines, vec!["Paragraph 1", "", "Paragraph 2"]);
    assert_eq!(buf2.selection.head, Position::new(1, 0));
}

#[test]
fn test_paragraph_enter_live_vs_source() {
    let mut buf = TextBuffer::from_str("Hello World", None);
    buf.selection = Selection::cursor(Position::new(0, 11));
    buf.insert_newline();
    assert_eq!(buf.lines, vec!["Hello World", "", ""]);
    assert_eq!(buf.selection.head, Position::new(2, 0));

    let mut buf_src = TextBuffer::from_str("Hello World", None);
    buf_src.selection = Selection::cursor(Position::new(0, 11));
    buf_src.insert_newline_mode(true);
    assert_eq!(buf_src.lines, vec!["Hello World", ""]);
    assert_eq!(buf_src.selection.head, Position::new(1, 0));
}

#[test]
fn test_arrow_navigation_across_code_block_and_thematic_break() {
    let mut buf = TextBuffer::from_str("```rust\nfn main() {\n}\n```\n\n---\n\nParagraph", None);
    buf.selection = Selection::cursor(Position::new(5, 0));
    buf.move_up_mode(false, false);
    assert_eq!(buf.selection.head, Position::new(2, 0));

    buf.move_up_mode(false, false);
    assert_eq!(buf.selection.head, Position::new(1, 0));

    buf.selection = Selection::cursor(Position::new(2, 0));
    buf.move_down_mode(false, false);
    assert_eq!(buf.selection.head, Position::new(5, 0));

    buf.move_down_mode(false, false);
    assert_eq!(buf.selection.head, Position::new(7, 0));
}

#[test]
fn test_trailing_empty_line_navigation_live_vs_source() {
    let mut buf = TextBuffer::from_str("Paragraph 1\n", None);
    buf.selection = Selection::cursor(Position::new(0, 0));
    buf.move_down_mode(false, false);
    assert_eq!(buf.selection.head, Position::new(0, 11));
    buf.move_down_mode(false, true);
    assert_eq!(buf.selection.head, Position::new(1, 0));
}

#[test]
fn test_fence_enter_and_backspace() {
    let mut buf = TextBuffer::from_str("```rust", None);
    buf.selection = Selection::cursor(Position::new(0, 7));
    buf.insert_newline();
    assert_eq!(buf.lines, vec!["```rust", "", "```", ""]);
    assert_eq!(buf.selection.head, Position::new(1, 0));

    buf.backspace_mode(false);
    assert_eq!(buf.lines, vec!["", ""]);
    assert_eq!(buf.selection.head, Position::new(0, 0));
}

#[test]
fn test_navigate_and_backspace_into_empty_code_block() {
    let mut buf = TextBuffer::from_str("Paragraph 1\n```rust\n\n```\nParagraph 2", None);
    buf.selection = Selection::cursor(Position::new(0, 0));
    buf.move_down_mode(false, false);
    assert_eq!(buf.selection.head, Position::new(1, 7));

    buf.move_down_mode(false, false);
    assert_eq!(buf.selection.head, Position::new(2, 0));

    buf.move_down_mode(false, false);
    assert_eq!(buf.selection.head, Position::new(4, 0));

    buf.move_up_mode(false, false);
    assert_eq!(buf.selection.head, Position::new(2, 0));

    buf.selection = Selection::cursor(Position::new(4, 0));
    buf.backspace_mode(false);
    assert_eq!(buf.selection.head, Position::new(2, 0));
}

#[test]
fn test_move_at_document_end_code_block() {
    let mut buf = TextBuffer::from_str("```rust\nfn main() {}\n```", None);
    buf.selection = Selection::cursor(Position::new(1, 0));
    buf.move_down_mode(false, false);
    assert_eq!(buf.lines, vec!["```rust", "fn main() {}", "```", "", ""]);
    assert_eq!(buf.selection.head, Position::new(4, 0));

    let mut buf2 = TextBuffer::from_str("```rust\nfn main() {}\n```", None);
    buf2.selection = Selection::cursor(Position::new(1, 12));
    buf2.move_right_mode(false, false);
    assert_eq!(buf2.lines, vec!["```rust", "fn main() {}", "```", "", ""]);
    assert_eq!(buf2.selection.head, Position::new(4, 0));
}

#[test]
fn test_insert_line_below_block_from_code_block() {
    let mut buf = TextBuffer::from_str("```rust\nline 1\nline 2\n```", None);
    buf.selection = Selection::cursor(Position::new(1, 3));
    buf.insert_line_below_block();
    assert_eq!(
        buf.lines,
        vec!["```rust", "line 1", "line 2", "```", "", ""]
    );
    assert_eq!(buf.selection.head, Position::new(5, 0));
}

#[test]
fn test_backspace_on_thematic_break_with_empty_lines() {
    let mut buf = TextBuffer::from_str("```rust\nfn main() {}\n```\n\n---\n\nParagraph", None);
    buf.selection = Selection::cursor(Position::new(4, 0));
    buf.backspace_mode(false);
    assert_eq!(
        buf.lines,
        vec!["```rust", "fn main() {}", "```", "", "Paragraph"]
    );
    assert_eq!(buf.selection.head, Position::new(3, 0));
}

#[test]
fn test_source_mode_linear_movement_and_editing() {
    let mut buf = TextBuffer::from_str(
        "## Heading 2\n\n```rust\nfn main() {}\n```\n- List item",
        None,
    );

    buf.selection = Selection::cursor(Position::new(0, 3));
    buf.move_left_mode(false, true);
    assert_eq!(buf.selection.head, Position::new(0, 2));

    buf.move_left_mode(false, true);
    assert_eq!(buf.selection.head, Position::new(0, 1));

    buf.move_left_mode(false, true);
    assert_eq!(buf.selection.head, Position::new(0, 0));

    buf.selection = Selection::cursor(Position::new(0, 2));
    buf.backspace_mode(true);
    assert_eq!(buf.lines[0], "# Heading 2");
    assert_eq!(buf.selection.head, Position::new(0, 1));

    buf.selection = Selection::cursor(Position::new(0, 0));
    buf.move_down_mode(false, true);
    assert_eq!(buf.selection.head, Position::new(1, 0));

    buf.move_down_mode(false, true);
    assert_eq!(buf.selection.head, Position::new(2, 0));

    buf.move_down_mode(false, true);
    assert_eq!(buf.selection.head, Position::new(3, 0));

    buf.move_down_mode(false, true);
    assert_eq!(buf.selection.head, Position::new(4, 0));

    buf.move_down_mode(false, true);
    assert_eq!(buf.selection.head, Position::new(5, 0));
}

#[test]
fn test_selected_text() {
    let mut buf = TextBuffer::from_str("# Main Heading\nParagraph content\n- List Item", None);

    buf.selection = Selection {
        anchor: Position::new(0, 2),
        head: Position::new(1, 9),
    };
    let copied = buf.selected_text().unwrap();
    assert_eq!(copied, "Main Heading\nParagraph");

    buf.selection = Selection {
        anchor: Position::new(0, 2),
        head: Position::new(0, 14),
    };
    let copied_single = buf.selected_text().unwrap();
    assert_eq!(copied_single, "Main Heading");

    buf.selection = Selection {
        anchor: Position::new(0, 7),
        head: Position::new(0, 14),
    };
    let copied_sub = buf.selected_text().unwrap();
    assert_eq!(copied_sub, "Heading");
}

#[test]
fn test_toggle_comment() {
    let mut buf = TextBuffer::from_str("<!-->", None);
    buf.selection = Selection::cursor(Position::new(0, 5));
    buf.toggle_comment();
    assert_eq!(buf.lines[0], "<!-- <!--> -->");

    let mut buf2 = TextBuffer::from_str("<!--->", None);
    buf2.selection = Selection::cursor(Position::new(0, 6));
    buf2.toggle_comment();
    assert_eq!(buf2.lines[0], "<!-- <!---> -->");

    let mut buf3 = TextBuffer::from_str("<!-- -->", None);
    buf3.selection = Selection::cursor(Position::new(0, 4));
    buf3.toggle_comment();
    assert_eq!(buf3.lines[0], "");

    let mut buf4 = TextBuffer::from_str("Hello World", None);
    buf4.selection = Selection::cursor(Position::new(0, 5));
    buf4.toggle_comment();
    assert_eq!(buf4.lines[0], "<!-- Hello World -->");
    buf4.toggle_comment();
    assert_eq!(buf4.lines[0], "Hello World");

    let mut buf5 = TextBuffer::from_str("    let x = 42;", None);
    buf5.selection = Selection::cursor(Position::new(0, 8));
    buf5.toggle_comment();
    assert_eq!(buf5.lines[0], "    <!-- let x = 42; -->");
    buf5.toggle_comment();
    assert_eq!(buf5.lines[0], "    let x = 42;");
}

#[test]
fn test_saved_baseline_dirty_tracking_with_undo_redo() {
    let mut buf = TextBuffer::from_str("Original Content", None);
    assert!(!buf.is_dirty);

    buf.insert_text("X");
    assert!(buf.is_dirty);
    assert_eq!(buf.lines[0], "XOriginal Content");

    buf.undo();
    assert!(!buf.is_dirty);
    assert_eq!(buf.lines[0], "Original Content");

    buf.redo();
    assert!(buf.is_dirty);
    assert_eq!(buf.lines[0], "XOriginal Content");

    buf.mark_as_saved();
    assert!(!buf.is_dirty);

    buf.insert_text("Y");
    assert!(buf.is_dirty);
    assert_eq!(buf.lines[0], "XYOriginal Content");

    buf.undo();
    assert!(!buf.is_dirty);
    assert_eq!(buf.lines[0], "XOriginal Content");

    buf.undo();
    assert!(buf.is_dirty);
    assert_eq!(buf.lines[0], "Original Content");

    buf.redo();
    assert!(!buf.is_dirty);
    assert_eq!(buf.lines[0], "XOriginal Content");
}

#[test]
fn test_manual_delete_back_to_saved_state() {
    let mut buf = TextBuffer::from_str("Hello World", None);
    assert!(!buf.is_dirty);

    buf.selection = Selection::cursor(Position::new(0, 11));
    buf.insert_text("!");
    assert_eq!(buf.lines[0], "Hello World!");
    assert!(buf.is_dirty);

    buf.backspace_mode(false);
    assert_eq!(buf.lines[0], "Hello World");
    assert!(!buf.is_dirty);

    buf.selection = Selection::cursor(Position::new(0, 5));
    buf.insert_text(" Beautiful");
    assert_eq!(buf.lines[0], "Hello Beautiful World");
    assert!(buf.is_dirty);

    buf.selection = Selection {
        anchor: Position::new(0, 5),
        head: Position::new(0, 15),
    };
    buf.insert_text("");
    assert_eq!(buf.lines[0], "Hello World");
    assert!(!buf.is_dirty);
}

#[test]
fn test_insert_text_crlf_normalization() {
    let mut buf = TextBuffer::from_str("line 1", None);
    buf.selection = Selection::cursor(Position::new(0, 6));
    buf.insert_text("\r\nline 2\r\nline 3");
    assert_eq!(buf.lines, vec!["line 1", "line 2", "line 3"]);
    assert_eq!(buf.selection.head, Position::new(2, 6));

    let mut buf2 = TextBuffer::from_str("", None);
    buf2.insert_text("first\rsecond");
    assert_eq!(buf2.lines, vec!["first", "second", ""]);
}

#[test]
fn test_outdent_partial_spaces_cursor() {
    let mut buf = TextBuffer::from_str("  indented text", None);
    buf.selection = Selection::cursor(Position::new(0, 10));
    buf.outdent_lines();
    assert_eq!(buf.lines[0], "indented text");
    assert_eq!(buf.selection.head, Position::new(0, 8));
}

#[test]
fn test_table_keyboard_navigation() {
    let initial_table = "| Col 1 | Col 2 |\n| :--- | :---: |\n| A | B |";
    let mut buf = TextBuffer::from_str(initial_table, None);

    buf.selection = Selection::cursor(Position::new(0, 2));

    assert!(buf.table_tab_forward());
    assert_eq!(buf.selection.head.line, 0);
    assert_eq!(buf.selection.head.col, 15);

    assert!(buf.table_tab_forward());
    assert_eq!(buf.selection.head.line, 2);
    assert_eq!(buf.selection.head.col, 3);

    assert!(buf.table_tab_forward());
    assert_eq!(buf.selection.head.line, 2);
    assert_eq!(buf.selection.head.col, 7);

    assert!(buf.table_tab_forward());
    assert_eq!(buf.lines.len(), 4);
    assert_eq!(buf.lines[3], "|  |  |");
    assert_eq!(buf.selection.head.line, 3);

    assert!(buf.table_tab_backward());
    assert_eq!(buf.selection.head.line, 2);
    assert_eq!(buf.selection.head.col, 7);

    assert!(buf.table_enter_next_row());
    assert_eq!(buf.selection.head.line, 3);

    assert!(buf.table_enter_next_row());
    assert_eq!(buf.lines.len(), 5);
    assert_eq!(buf.selection.head.line, 4);
}

#[test]
fn test_table_backspace_at_cell_start() {
    let initial_table = "|  | World |\n| --- | --- |\n| A | B |";
    let mut buf = TextBuffer::from_str(initial_table, None);

    buf.selection = Selection::cursor(Position::new(0, 2));
    assert!(buf.table_backspace());
    assert_eq!(buf.lines[0], "|  | World |");
    assert_eq!(buf.selection.head, Position::new(0, 2));

    buf.selection = Selection::cursor(Position::new(0, 6));
    assert!(buf.table_backspace());
    assert_eq!(buf.lines[0], "|  | orld |");
    assert_eq!(buf.selection.head, Position::new(0, 5));
}

#[test]
fn test_table_arrow_key_cell_navigation() {
    let initial_table = "| Col 1 | Col 2 |\n| --- | --- |\n| A | B |";
    let mut buf = TextBuffer::from_str(initial_table, None);

    buf.selection = Selection::cursor(Position::new(0, 2));

    for _ in 0..5 {
        buf.move_right_mode(false, false);
    }
    assert_eq!(buf.selection.head, Position::new(0, 7));

    buf.move_right_mode(false, false);
    assert_eq!(buf.selection.head, Position::new(0, 10));

    buf.move_left_mode(false, false);
    assert_eq!(buf.selection.head, Position::new(0, 7));
}

#[test]
fn test_cjk_table_navigation_and_editing() {
    let initial_table = "| 特性 | 说明 |\n| --- | --- |\n| 高性能 | GPUI |";
    let mut buf = TextBuffer::from_str(initial_table, None);

    let cells = crate::markdown::table::parse_table_row_cells(&buf.lines[0]);
    assert_eq!(cells[0].col_range, (2, 4));
    assert_eq!(cells[1].col_range, (7, 9));

    buf.selection = Selection::cursor(Position::new(0, 2));

    buf.move_right_mode(false, false);
    assert_eq!(buf.selection.head, Position::new(0, 3));

    buf.move_right_mode(false, false);
    assert_eq!(buf.selection.head, Position::new(0, 4));

    buf.move_right_mode(false, false);
    assert_eq!(buf.selection.head, Position::new(0, 7));

    buf.move_left_mode(false, false);
    assert_eq!(buf.selection.head, Position::new(0, 4));

    buf.insert_text("列表");
    assert_eq!(buf.lines[0], "| 特性列表 | 说明 |");
    assert_eq!(buf.selection.head, Position::new(0, 6));
}

#[test]
fn test_block_elements_separated_by_empty_lines() {
    let mut buf = TextBuffer::new();

    buf.insert_text("# The title");
    buf.insert_newline();
    assert_eq!(buf.lines, vec!["# The title", "", "", ""]);
    assert_eq!(buf.selection.head, Position::new(2, 0));

    buf.insert_text("something");
    buf.insert_newline();
    assert_eq!(buf.lines, vec!["# The title", "", "something", "", "", ""]);
    assert_eq!(buf.selection.head, Position::new(4, 0));

    buf.insert_text("```rust");
    buf.insert_newline();
    assert_eq!(
        buf.lines,
        vec![
            "# The title",
            "",
            "something",
            "",
            "```rust",
            "",
            "```",
            "",
            ""
        ]
    );
    assert_eq!(buf.selection.head, Position::new(5, 0));

    buf.insert_text("fn main() {");
    buf.insert_newline();
    buf.insert_text("    0");
    buf.insert_newline();
    buf.insert_text("}");
    assert_eq!(
        buf.lines,
        vec![
            "# The title",
            "",
            "something",
            "",
            "```rust",
            "fn main() {",
            "    0",
            "}",
            "```",
            "",
            ""
        ]
    );
    assert_eq!(buf.selection.head, Position::new(7, 1));

    buf.insert_line_below_block();
    assert_eq!(
        buf.lines,
        vec![
            "# The title",
            "",
            "something",
            "",
            "```rust",
            "fn main() {",
            "    0",
            "}",
            "```",
            "",
            "",
            ""
        ]
    );
    assert_eq!(buf.selection.head, Position::new(10, 0));

    buf.insert_text("1. i1");
    buf.insert_newline();
    assert_eq!(buf.lines[11], "2. ");
    assert_eq!(buf.selection.head, Position::new(11, 3));
    buf.insert_text("i2");
    buf.insert_newline();
    assert_eq!(buf.lines[12], "3. ");
    assert_eq!(buf.selection.head, Position::new(12, 3));
    buf.insert_text("i3");
    buf.insert_newline();
    assert_eq!(buf.lines[13], "4. ");

    buf.insert_newline();
    assert_eq!(buf.lines[13], "");
    assert_eq!(buf.lines[14], "");
    assert_eq!(buf.selection.head, Position::new(14, 0));

    buf.insert_text("w");
    buf.insert_newline();
    buf.insert_text("a");
    buf.insert_newline();
    buf.insert_text("b");
    buf.insert_newline();
    buf.insert_text("c");
    buf.insert_newline();
    buf.insert_text("d");

    let expected_markdown = "# The title\n\nsomething\n\n```rust\nfn main() {\n    0\n}\n```\n\n1. i1\n2. i2\n3. i3\n\nw\n\na\n\nb\n\nc\n\nd\n";
    let content = buf.to_string_content();
    assert_eq!(content, expected_markdown);
}

#[test]
fn test_list_item_continuation_and_exit() {
    let mut buf = TextBuffer::new();
    buf.insert_text("- item1");
    assert_eq!(buf.lines, vec!["- item1", ""]);
    assert_eq!(buf.selection.head, Position::new(0, 7));

    buf.insert_newline();
    assert_eq!(buf.lines, vec!["- item1", "- ", ""]);
    assert_eq!(buf.selection.head, Position::new(1, 2));

    buf.insert_text("item2");
    buf.insert_newline();
    assert_eq!(buf.lines, vec!["- item1", "- item2", "- ", ""]);
    assert_eq!(buf.selection.head, Position::new(2, 2));

    let mut buf_backspace = buf.clone();
    buf_backspace.backspace_mode(false);
    assert_eq!(buf_backspace.lines, vec!["- item1", "- item2", "", "", ""]);
    assert_eq!(buf_backspace.selection.head, Position::new(3, 0));
    buf_backspace.insert_text("paragraph via backspace");
    assert_eq!(
        buf_backspace.to_string_content(),
        "- item1\n- item2\n\nparagraph via backspace\n"
    );

    let mut buf_enter = buf.clone();
    buf_enter.insert_newline();
    assert_eq!(buf_enter.lines, vec!["- item1", "- item2", "", "", ""]);
    assert_eq!(buf_enter.selection.head, Position::new(3, 0));
    buf_enter.insert_text("paragraph via enter");
    assert_eq!(
        buf_enter.to_string_content(),
        "- item1\n- item2\n\nparagraph via enter\n"
    );
}

#[test]
fn test_enter_on_empty_separator_line_and_document_end() {
    let mut buf = TextBuffer::new();
    buf.insert_text("a");
    buf.insert_newline();
    buf.insert_text("1. a");
    assert_eq!(buf.lines, vec!["a", "", "1. a", ""]);

    buf.selection = Selection::cursor(Position::new(1, 0));
    buf.insert_newline();
    assert_eq!(buf.lines, vec!["a", "", "", "1. a", ""]);
    assert_eq!(buf.selection.head, Position::new(2, 0));

    buf.selection = Selection::cursor(Position::new(3, 4));
    buf.insert_newline();
    assert_eq!(buf.lines, vec!["a", "", "", "1. a", "2. ", ""]);
    assert_eq!(buf.selection.head, Position::new(4, 3));

    buf.insert_newline();
    assert_eq!(buf.lines, vec!["a", "", "", "1. a", "", "", ""]);
    assert_eq!(buf.selection.head, Position::new(5, 0));

    buf.insert_newline();
    assert_eq!(buf.lines, vec!["a", "", "", "1. a", "", "", "", "", ""]);
    assert_eq!(buf.selection.head, Position::new(7, 0));
}

#[test]
fn test_empty_document_enter() {
    let mut buf = TextBuffer::new();
    assert_eq!(buf.lines, vec!["", ""]);
    assert_eq!(buf.selection.head, Position::new(0, 0));

    buf.insert_newline();
    assert_eq!(buf.lines, vec!["", "", "", ""]);
    assert_eq!(buf.selection.head, Position::new(2, 0));

    buf.insert_newline();
    assert_eq!(buf.lines, vec!["", "", "", "", "", ""]);
    assert_eq!(buf.selection.head, Position::new(4, 0));

    let mut buf_nav = TextBuffer::new();
    assert_eq!(buf_nav.lines, vec!["", ""]);
    buf_nav.move_down_mode(false, false);
    assert_eq!(buf_nav.selection.head, Position::new(0, 0));

    let mut buf_src = TextBuffer::new();
    assert_eq!(buf_src.lines, vec!["", ""]);
    buf_src.insert_newline_mode(true);
    assert_eq!(buf_src.lines, vec!["", "", ""]);
    assert_eq!(buf_src.selection.head, Position::new(1, 0));
}

#[test]
fn test_select_all_delete_preserves_sentinel_lines() {
    let mut buf = TextBuffer::new();
    buf.insert_text("Some text in paragraph");
    buf.insert_newline();
    buf.insert_text("Second paragraph");
    assert!(buf.lines.len() >= 2);

    buf.select_all();
    assert!(!buf.selection.is_empty());

    buf.backspace_mode(false);
    assert_eq!(buf.lines, vec!["", ""]);
    assert_eq!(buf.selection.head, Position::new(0, 0));

    buf.insert_text("# Heading 1");
    buf.select_all();
    buf.delete_forward_mode(false);
    assert_eq!(buf.lines, vec!["", ""]);
    assert_eq!(buf.selection.head, Position::new(0, 0));

    buf.insert_text("1. Item 1\n2. Item 2");
    buf.select_all();
    buf.delete_lines();
    assert_eq!(buf.lines, vec!["", ""]);
    assert_eq!(buf.selection.head, Position::new(0, 0));
}

#[test]
fn test_list_middle_enter_splits_list() {
    let mut buf = TextBuffer::from_str("1. a\n2. b\n3. c", None);
    buf.selection = Selection::cursor(Position::new(0, 4));

    buf.insert_newline_mode(false);
    assert_eq!(buf.lines, vec!["1. a", "2. ", "2. b", "3. c"]);
    assert_eq!(buf.selection.head, Position::new(1, 3));

    buf.insert_newline_mode(false);
    assert_eq!(buf.lines, vec!["1. a", "", "", "", "2. b", "3. c"]);
    assert_eq!(buf.selection.head, Position::new(2, 0));

    buf.insert_text("x");
    assert_eq!(buf.lines, vec!["1. a", "", "x", "", "2. b", "3. c"]);
}

#[test]
fn test_typing_on_list_item_after_prefix() {
    let mut buf = TextBuffer::from_str("1. a\n2. a\n3. a\n4. b\n5. ", None);
    buf.selection = Selection::cursor(Position::new(4, 3));
    buf.insert_text("c");
    assert_eq!(buf.lines[4], "5. c");
}

#[test]
fn test_undo_stack_max_bound_and_delta_efficiency() {
    let initial = (0..100)
        .map(|i| format!("Line {}", i))
        .collect::<Vec<_>>()
        .join("\n");
    let mut buf = TextBuffer::from_str(&initial, None);

    buf.selection = Selection::cursor(Position::new(50, 6));
    buf.insert_text(" MODIFIED");
    assert_eq!(buf.lines[50], "Line 5 MODIFIED0");

    let last_step = buf.undo_stack.back().expect("expected undo step");
    assert_eq!(last_step.start_line, 50);
    assert_eq!(last_step.old_lines, vec!["Line 50"]);
    assert_eq!(last_step.new_lines, vec!["Line 5 MODIFIED0"]);

    buf.undo();
    assert_eq!(buf.lines[50], "Line 50");
    assert!(!buf.is_dirty);

    buf.redo();
    assert_eq!(buf.lines[50], "Line 5 MODIFIED0");
    assert!(buf.is_dirty);

    for i in 0..1050 {
        buf.selection = Selection::cursor(Position::new(0, 0));
        buf.insert_text(&format!("{}", i % 10));
    }
    assert!(buf.undo_stack.len() <= MAX_UNDO_STEPS);
}

#[test]
fn test_utf16_offset_mapping_roundtrip() {
    let content = "First Line\n第二行 中文\nThird 🚀 Rocket\nEnd";
    let buf = TextBuffer::from_str(content, None);

    for (line_idx, line) in buf.lines.iter().enumerate() {
        for col in 0..=line.chars().count() {
            let pos = Position::new(line_idx, col);
            let offset = buf.pos_to_utf16_offset(pos);
            let recovered_pos = buf.utf16_offset_to_pos(offset);
            assert_eq!(pos, recovered_pos);
        }
    }
}

#[test]
fn test_word_count_and_char_count_metrics() {
    let content = "The quick brown fox\njumps over\n\nthe lazy dog";
    let buf = TextBuffer::from_str(content, None);
    assert_eq!(buf.word_count(), 9);
    assert_eq!(buf.char_count(), content.chars().count());
}

#[test]
fn test_incremental_char_count() {
    let mut buf = TextBuffer::new();
    assert_eq!(buf.char_count(), 0);

    buf.insert_text("Hello");
    assert_eq!(buf.char_count(), buf.to_string_content().chars().count());

    buf.insert_newline();
    assert_eq!(buf.char_count(), buf.to_string_content().chars().count());

    buf.insert_text("World\nTest");
    assert_eq!(buf.char_count(), buf.to_string_content().chars().count());

    buf.undo();
    assert_eq!(buf.char_count(), buf.to_string_content().chars().count());

    buf.redo();
    assert_eq!(buf.char_count(), buf.to_string_content().chars().count());

    buf.backspace_mode(false);
    assert_eq!(buf.char_count(), buf.to_string_content().chars().count());
}

#[test]
fn test_set_cursor_clears_column_goal() {
    let mut buf = TextBuffer::from_str("Hello World Long Line\nShort\nAnother Long Line", None);
    buf.selection = Selection::cursor(Position::new(0, 15));

    buf.move_down_mode(false, false);
    assert_eq!(buf.selection.head, Position::new(1, 5));
    assert_eq!(buf.column_goal, Some(15));

    buf.set_cursor(Position::new(1, 2));
    assert_eq!(buf.column_goal, None);

    buf.move_down_mode(false, false);
    assert_eq!(buf.selection.head, Position::new(2, 2));
    assert_eq!(buf.column_goal, Some(2));
}

#[test]
fn test_empty_document_roundtrip() {
    let buf = TextBuffer::new();
    assert!(buf.is_empty());
    let exported = buf.to_string_content();
    assert_eq!(exported, "");

    let roundtrip_buf = TextBuffer::from_str(&exported, None);
    assert!(roundtrip_buf.is_empty());
    assert_eq!(roundtrip_buf.to_string_content(), "");
}

#[test]
fn test_undo_coalescing() {
    let mut buf = TextBuffer::new();
    buf.insert_text("a");
    buf.insert_text("b");
    buf.insert_text("c");

    assert_eq!(buf.undo_stack.len(), 1);
    assert_eq!(buf.lines[0], "abc");

    buf.undo();
    assert_eq!(buf.lines[0], "");
    assert_eq!(buf.undo_stack.len(), 0);

    buf.redo();
    assert_eq!(buf.lines[0], "abc");

    buf.insert_text(" ");
    buf.insert_text("d");
    assert_eq!(buf.undo_stack.len(), 2);
    assert_eq!(buf.lines[0], "abc d");

    buf.undo();
    assert_eq!(buf.lines[0], "abc");
    assert_eq!(buf.undo_stack.len(), 1);

    buf.undo();
    assert_eq!(buf.lines[0], "");
    assert_eq!(buf.undo_stack.len(), 0);
}

#[test]
fn test_backspace_with_cjk_characters() {
    let mut buf = TextBuffer::from_str("  哈哈", None);
    buf.selection = Selection::cursor(Position::new(0, 4));
    buf.backspace_mode(false);
    assert_eq!(buf.lines[0], "  哈");
    assert_eq!(buf.selection.head, Position::new(0, 3));

    let mut buf2 = TextBuffer::from_str("    哈", None);
    buf2.selection = Selection::cursor(Position::new(0, 4));
    buf2.backspace_mode(false);
    assert_eq!(buf2.lines[0], "哈");
    assert_eq!(buf2.selection.head, Position::new(0, 0));

    let mut buf3 = TextBuffer::from_str("    哈", None);
    buf3.selection = Selection::cursor(Position::new(0, 5));
    buf3.backspace_mode(false);
    assert_eq!(buf3.lines[0], "    ");
    assert_eq!(buf3.selection.head, Position::new(0, 4));

    let mut buf4 = TextBuffer::from_str("# 哈", None);
    buf4.selection = Selection::cursor(Position::new(0, 1));
    buf4.backspace_mode(false);
    assert_eq!(buf4.lines[0], "哈");
    assert_eq!(buf4.selection.head, Position::new(0, 0));

    let mut buf5 = TextBuffer::from_str("- [ ] 任务", None);
    buf5.selection = Selection::cursor(Position::new(0, 6));
    buf5.backspace_mode(false);
    assert_eq!(buf5.lines[0], "任务");
    assert_eq!(buf5.selection.head, Position::new(0, 0));

    let mut buf6 = TextBuffer::from_str("\u{3000}哈哈", None);
    buf6.selection = Selection::cursor(Position::new(0, 3));
    buf6.backspace_mode(false);
    assert_eq!(buf6.lines[0], "\u{3000}哈");
    assert_eq!(buf6.selection.head, Position::new(0, 2));
}

#[test]
fn test_memoized_word_count() {
    let mut buf = TextBuffer::from_str("one two three\nfour five", None);
    assert_eq!(buf.word_count(), 5);
    assert_eq!(buf.word_count(), 5);

    buf.selection = Selection::cursor(Position::new(1, 9));
    buf.insert_text(" six seven");
    assert_eq!(buf.word_count(), 7);

    buf.undo();
    assert_eq!(buf.word_count(), 5);

    buf.redo();
    assert_eq!(buf.word_count(), 7);
}

#[test]
fn test_edit_delta_tracking() {
    use crate::buffer::EditDelta;

    let mut buf = TextBuffer::from_str("line 0\nline 1\nline 2", None);
    assert_eq!(buf.last_edit_delta, None);

    buf.record_single_line_edit(1, |line, _| {
        line.push_str(" modified");
    });
    assert_eq!(
        buf.last_edit_delta,
        Some(EditDelta {
            start_line: 1,
            old_line_count: 1,
            new_line_count: 1,
        })
    );

    buf.selection = Selection::cursor(Position::new(1, 0));
    buf.insert_text("split\nacross\n");
    let delta = buf.last_edit_delta.unwrap();
    assert_eq!(delta.start_line, 1);

    buf.undo();
    assert!(buf.last_edit_delta.is_some());

    buf.redo();
    assert!(buf.last_edit_delta.is_some());
}

#[test]
fn test_buffer_accessors() {
    let mut buf = TextBuffer::from_str("hello world", Some(std::path::PathBuf::from("test.md")));

    assert_eq!(buf.file_path(), Some(std::path::Path::new("test.md")));
    assert_eq!(
        buf.file_path_buf(),
        Some(std::path::PathBuf::from("test.md"))
    );
    assert!(!buf.is_dirty());
    assert_eq!(buf.version(), 0);
    assert_eq!(buf.cursor_pos(), Position::new(0, 0));
    assert_eq!(buf.selection(), Selection::cursor(Position::new(0, 0)));
    assert_eq!(buf.last_edit_delta(), None);

    buf.insert_text("foo ");
    assert!(buf.is_dirty());
    assert!(buf.version() > 0);
    assert_eq!(buf.cursor_pos(), Position::new(0, 4));
    assert!(buf.last_edit_delta().is_some());

    buf.set_file_path(Some(std::path::PathBuf::from("renamed.md")));
    assert_eq!(buf.file_path(), Some(std::path::Path::new("renamed.md")));
    buf.set_file_path(None);
    assert_eq!(buf.file_path(), None);

    buf.column_goal = Some(99);
    buf.set_cursor(Position::new(100, 500));
    assert_eq!(buf.column_goal, None);
    assert_eq!(buf.cursor_pos(), Position::new(0, 15));

    buf.column_goal = Some(50);
    buf.set_selection(Selection {
        anchor: Position::new(100, 200),
        head: Position::new(200, 300),
    });
    assert_eq!(buf.column_goal, None);
    assert_eq!(buf.selection().anchor, Position::new(0, 15));
    assert_eq!(buf.selection().head, Position::new(0, 15));
}

#[test]
fn test_dirty_state_after_undo_and_new_edit() {
    let mut buf = TextBuffer::from_str("initial", None);
    assert!(!buf.is_dirty());

    buf.insert_text(" edit1");
    assert!(buf.is_dirty());
    let v1 = buf.version();

    buf.mark_as_saved();
    assert!(!buf.is_dirty());

    buf.undo();
    assert!(buf.is_dirty());

    buf.insert_text(" edit2");
    assert_eq!(buf.version(), v1);

    buf.update_dirty_state();
    assert!(buf.is_dirty());
}

#[test]
fn test_word_count_cache_after_undo_and_edit() {
    let mut buf = TextBuffer::from_str("initial", None);
    buf.set_cursor(Position::new(0, 7));
    let v0 = buf.version();
    assert_eq!(buf.word_count(), 1);

    buf.insert_text(" two three");
    assert_eq!(buf.word_count(), 3);
    let v1 = buf.version();

    buf.undo();
    assert_eq!(buf.version(), v0);
    assert_eq!(buf.word_count(), 1);

    buf.insert_text("X");
    assert_eq!(buf.version(), v1);
    assert_eq!(buf.word_count(), 1);
}

#[test]
fn test_single_newline_roundtrip() {
    let buf = TextBuffer::from_str("\n", None);
    assert!(!buf.is_empty());
    assert_eq!(buf.char_count(), 1);
    assert_eq!(buf.to_string_content(), "\n");

    let empty_buf = TextBuffer::from_str("", None);
    assert!(empty_buf.is_empty());
    assert_eq!(empty_buf.char_count(), 0);
    assert_eq!(empty_buf.to_string_content(), "");
}

#[test]
fn test_async_save_snapshot_with_concurrent_edits() {
    let mut buf = TextBuffer::from_str("Initial content", None);
    buf.insert_text(" - modified");
    assert!(buf.is_dirty());

    let snapshot = buf.saved_baseline_snapshot();

    buf.insert_text(" while saving");
    assert!(buf.is_dirty());

    buf.mark_saved_as(snapshot);
    assert!(buf.is_dirty());

    buf.undo();
    assert!(!buf.is_dirty());
}

#[test]
fn test_utf16_selection_and_ime_mark_helpers() {
    // "a😀b\ncd" -> 'a'(1), '😀'(2), 'b'(1), '\n'(1), 'c'(1), 'd'(1)
    let mut buf = TextBuffer::from_str("a😀b\ncd", None);
    buf.set_selection(Selection {
        anchor: Position::new(1, 1), // offset 6 ('c'|'d')
        head: Position::new(0, 1),   // offset 1 ('a'|'😀')
    });

    let (range, reversed) = buf.selected_utf16_range();
    assert_eq!(range, 1..6);
    assert!(reversed);

    // Mid-surrogate offset (2 is inside '😀' at 1..3) snaps to code-point start (1)
    let (text, adjusted) = buf.text_for_utf16_range(2..6);
    assert_eq!(adjusted, 1..6);
    assert_eq!(text, "😀b\nc");

    // IME preedit replacement & sub-selection clamping
    let marked = buf.replace_and_mark_utf16(Some(1..3), None, "ni", Some(0..10));
    assert_eq!(marked, Some(1..3));
    assert_eq!(buf.to_string_content(), "anib\ncd");
    assert_eq!(
        buf.selection(),
        Selection {
            anchor: Position::new(0, 1),
            head: Position::new(0, 3),
        }
    );

    // Subsequent IME commit falls back to marked_range when range_utf16 is None
    let resolved = buf.select_utf16_range(None, marked);
    assert_eq!(resolved, 1..3);
    buf.insert_text("你");
    assert_eq!(buf.to_string_content(), "a你b\ncd");
}
