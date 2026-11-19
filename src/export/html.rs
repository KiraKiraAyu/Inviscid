//! Markdown-to-HTML body and document rendering.

use std::collections::HashMap;
use std::fmt::Write as _;
use std::path::Path;

use crate::buffer::LineSource;
use crate::markdown::{
    BlockKind, BlockPrefix, InlineSpan, MarkdownScanner, ParsedLine, SyntaxToken, TableAlignment,
    collect_indexed_headings, parse_block_prefix, strip_quote_prefix,
};
use crate::math::MathSvg;

use super::ExportOptions;
use super::assets::{ImageEmbedder, escape_html, is_safe_href, strip_svg_xml_prolog};
use super::css::write_document_css;

#[derive(Clone, Copy, PartialEq, Eq)]
enum ListKind {
    Unordered,
    Ordered,
}

#[derive(Clone, Copy)]
struct ListLevel {
    indent: usize,
    content_indent: usize,
    kind: ListKind,
    has_open_li: bool,
}

#[derive(Default)]
struct HtmlDocState {
    out: String,
    quote_depth: usize,
    in_paragraph: bool,
    prev_line_hard_break: bool,
    saw_blank_line_in_list: bool,
    in_table: bool,
    in_table_body: bool,
    list_stack: Vec<ListLevel>,
}

impl HtmlDocState {
    fn close_paragraph(&mut self) {
        if self.in_paragraph {
            self.out.push_str("</p>\n");
            self.in_paragraph = false;
        }
        self.prev_line_hard_break = false;
    }

    fn close_table(&mut self) {
        if self.in_table {
            if self.in_table_body {
                self.out.push_str("</tbody>\n");
                self.in_table_body = false;
            }
            self.out.push_str("</table>\n</div>\n");
            self.in_table = false;
        }
    }

    fn close_lists(&mut self) {
        while let Some(level) = self.list_stack.pop() {
            if level.has_open_li {
                self.out.push_str("</li>\n");
            }
            match level.kind {
                ListKind::Unordered => self.out.push_str("</ul>\n"),
                ListKind::Ordered => self.out.push_str("</ol>\n"),
            }
        }
        self.saw_blank_line_in_list = false;
        self.prev_line_hard_break = false;
    }

    fn close_leaf_containers(&mut self) {
        self.close_paragraph();
        self.close_table();
        self.close_lists();
    }

    fn sync_quote_depth(&mut self, target_depth: usize) {
        if self.quote_depth == target_depth {
            return;
        }
        self.close_leaf_containers();
        while self.quote_depth > target_depth {
            self.out.push_str("</blockquote>\n");
            self.quote_depth -= 1;
        }
        while self.quote_depth < target_depth {
            self.out.push_str("<blockquote>\n");
            self.quote_depth += 1;
        }
    }

    fn finish(mut self) -> String {
        self.sync_quote_depth(0);
        self.close_leaf_containers();
        self.out
    }
}

/// Renders a Markdown `LineSource` into a self-contained HTML5 document.
pub fn render_markdown_to_html(
    source: &(impl LineSource + ?Sized),
    options: &ExportOptions<'_>,
) -> String {
    let parsed_lines = MarkdownScanner::scan_source(source);
    let heading_anchors: HashMap<usize, String> =
        collect_indexed_headings(parsed_lines.iter().enumerate())
            .into_iter()
            .map(|item| (item.source_line.index(), item.anchor))
            .collect();

    let body = render_body_html(
        &parsed_lines,
        &heading_anchors,
        options.doc_path,
        options.theme.is_dark(),
    );
    let mut html = String::with_capacity(body.len() + 4096);
    let _ = writeln!(html, "<!DOCTYPE html>");
    let _ = writeln!(html, "<html lang=\"en\">");
    let _ = writeln!(html, "<head>");
    let _ = writeln!(html, "<meta charset=\"utf-8\">");
    let _ = writeln!(
        html,
        "<meta name=\"viewport\" content=\"width=device-width, initial-scale=1.0\">"
    );
    let _ = writeln!(html, "<title>{}</title>", escape_html(options.title));
    let _ = writeln!(html, "<style>");
    write_document_css(&mut html, options.theme);
    let _ = writeln!(html, "</style>");
    let _ = writeln!(html, "</head>");
    let _ = writeln!(html, "<body>");
    let _ = writeln!(html, "<main class=\"markdown-body\">");
    html.push_str(&body);
    let _ = writeln!(html, "</main>");
    let _ = writeln!(html, "</body>");
    let _ = writeln!(html, "</html>");
    html
}

fn render_body_html(
    parsed_lines: &[ParsedLine],
    heading_anchors: &HashMap<usize, String>,
    doc_path: Option<&Path>,
    is_dark_theme: bool,
) -> String {
    let mut state = HtmlDocState::default();
    let mut embedder = ImageEmbedder::default();
    let mut idx = 0;

    while idx < parsed_lines.len() {
        let parsed = &parsed_lines[idx];
        let target_quote_depth = parsed.quote.map_or(0, |q| q.depth);
        state.sync_quote_depth(target_quote_depth);

        match &parsed.kind {
            BlockKind::CodeBlock {
                is_fence_start: true,
            } => {
                state.close_leaf_containers();
                let lang = extract_fence_language(&parsed.raw_text);
                idx += 1;
                let mut code_lines = Vec::new();
                while idx < parsed_lines.len() {
                    match parsed_lines[idx].kind {
                        BlockKind::CodeBlockContent => {
                            code_lines.push(&parsed_lines[idx]);
                            idx += 1;
                        }
                        BlockKind::CodeBlock {
                            is_fence_start: false,
                        } => {
                            idx += 1;
                            break;
                        }
                        _ => break,
                    }
                }
                write_code_block_html(&mut state.out, lang, &code_lines);
                continue;
            }
            BlockKind::CodeBlock {
                is_fence_start: false,
            }
            | BlockKind::CodeBlockContent => {
                idx += 1;
            }
            BlockKind::MermaidBlock {
                is_fence_start: true,
            } => {
                state.close_leaf_containers();
                let scanned_svg = parsed.spans.iter().find_map(|s| s.mermaid.as_ref());
                idx += 1;
                let mut raw_mermaid_lines = Vec::new();
                while idx < parsed_lines.len() {
                    match parsed_lines[idx].kind {
                        BlockKind::MermaidBlockContent => {
                            raw_mermaid_lines.push(visible_plain_line_text(&parsed_lines[idx]));
                            idx += 1;
                        }
                        BlockKind::MermaidBlock {
                            is_fence_start: false,
                        } => {
                            idx += 1;
                            break;
                        }
                        _ => break,
                    }
                }
                let rendered_mermaid = if is_dark_theme {
                    crate::mermaid::prepare_mermaid_lines_for_theme(&raw_mermaid_lines, true)
                } else {
                    scanned_svg.map(|m| m.rendered.clone())
                };
                if let Some(mermaid) = rendered_mermaid {
                    let svg = strip_svg_xml_prolog(mermaid.svg_str());
                    if !svg.is_empty() {
                        let _ = writeln!(state.out, "<div class=\"mermaid-block\">{svg}</div>");
                    } else {
                        write_plain_code_block_html(&mut state.out, "mermaid", &raw_mermaid_lines);
                    }
                } else {
                    write_plain_code_block_html(&mut state.out, "mermaid", &raw_mermaid_lines);
                }
                continue;
            }
            BlockKind::MermaidBlock {
                is_fence_start: false,
            }
            | BlockKind::MermaidBlockContent => {
                idx += 1;
            }
            BlockKind::MathBlock { is_start: true } => {
                state.close_leaf_containers();
                let rendered_math = parsed.spans.iter().find_map(|s| s.math.as_ref());
                idx += 1;
                let mut tex_lines = Vec::new();
                while idx < parsed_lines.len() {
                    match parsed_lines[idx].kind {
                        BlockKind::MathBlockContent => {
                            tex_lines.push(visible_plain_line_text(&parsed_lines[idx]));
                            idx += 1;
                        }
                        BlockKind::MathBlock { is_start: false } => {
                            idx += 1;
                            break;
                        }
                        _ => break,
                    }
                }
                if let Some(math) = rendered_math {
                    write_display_math_block(&mut state.out, &math.rendered, &math.tex);
                } else {
                    write_plain_code_block_html(&mut state.out, "math", &tex_lines);
                }
                continue;
            }
            BlockKind::MathBlock { is_start: false } | BlockKind::MathBlockContent => {
                idx += 1;
            }
            BlockKind::Heading { level } => {
                state.close_leaf_containers();
                let lvl = (*level).clamp(1, 6);
                let inner = render_inline_spans_html(&parsed.spans, doc_path, &mut embedder);
                if let Some(anchor) = heading_anchors.get(&idx) {
                    let _ = writeln!(
                        state.out,
                        "<h{lvl} id=\"{}\">{inner}</h{lvl}>",
                        escape_html(anchor)
                    );
                } else {
                    let _ = writeln!(state.out, "<h{lvl}>{inner}</h{lvl}>");
                }
                idx += 1;
            }
            BlockKind::ThematicBreak => {
                state.close_leaf_containers();
                state.out.push_str("<hr>\n");
                idx += 1;
            }
            BlockKind::Image { alt, url } => {
                state.close_leaf_containers();
                state.out.push_str("<figure class=\"image-block\">");
                write_img_tag(&mut state.out, "", url, alt, doc_path, &mut embedder);
                state.out.push_str("</figure>\n");
                idx += 1;
            }
            BlockKind::Table {
                is_header,
                is_delimiter,
                alignments,
                cells,
            } => {
                state.close_paragraph();
                state.close_lists();
                if *is_delimiter {
                    idx += 1;
                    continue;
                }
                if *is_header {
                    state.close_table();
                    state
                        .out
                        .push_str("<div class=\"table-wrapper\">\n<table>\n<thead>\n<tr>");
                    for (col_idx, cell) in cells.iter().enumerate() {
                        let align_attr = alignment_attr(alignments.get(col_idx).copied());
                        let cell_html =
                            render_inline_spans_html(&cell.spans, doc_path, &mut embedder);
                        let _ = write!(state.out, "<th{align_attr}>{cell_html}</th>");
                    }
                    state.out.push_str("</tr>\n</thead>\n");
                    state.in_table = true;
                    state.in_table_body = false;
                } else {
                    if !state.in_table {
                        state
                            .out
                            .push_str("<div class=\"table-wrapper\">\n<table>\n");
                        state.in_table = true;
                    }
                    if !state.in_table_body {
                        state.out.push_str("<tbody>\n");
                        state.in_table_body = true;
                    }
                    state.out.push_str("<tr>");
                    for (col_idx, cell) in cells.iter().enumerate() {
                        let align_attr = alignment_attr(alignments.get(col_idx).copied());
                        let cell_html =
                            render_inline_spans_html(&cell.spans, doc_path, &mut embedder);
                        let _ = write!(state.out, "<td{align_attr}>{cell_html}</td>");
                    }
                    state.out.push_str("</tr>\n");
                }
                idx += 1;
            }
            BlockKind::BulletList => {
                state.close_paragraph();
                state.close_table();
                state.saw_blank_line_in_list = false;
                let (indent, content_indent) = list_metrics(&parsed.raw_text);
                let inline_html = render_inline_spans_html(&parsed.spans, doc_path, &mut embedder);
                push_list_item(
                    &mut state.out,
                    &mut state.list_stack,
                    indent,
                    content_indent,
                    ListKind::Unordered,
                    1,
                    None,
                    &inline_html,
                );
                state.prev_line_hard_break = line_has_hard_break(parsed);
                idx += 1;
            }
            BlockKind::TaskList { checked } => {
                state.close_paragraph();
                state.close_table();
                state.saw_blank_line_in_list = false;
                let (indent, content_indent) = list_metrics(&parsed.raw_text);
                let inline_html = render_inline_spans_html(&parsed.spans, doc_path, &mut embedder);
                push_list_item(
                    &mut state.out,
                    &mut state.list_stack,
                    indent,
                    content_indent,
                    ListKind::Unordered,
                    1,
                    Some(*checked),
                    &inline_html,
                );
                state.prev_line_hard_break = line_has_hard_break(parsed);
                idx += 1;
            }
            BlockKind::OrderedList { num } => {
                state.close_paragraph();
                state.close_table();
                state.saw_blank_line_in_list = false;
                let (indent, content_indent) = list_metrics(&parsed.raw_text);
                let inline_html = render_inline_spans_html(&parsed.spans, doc_path, &mut embedder);
                push_list_item(
                    &mut state.out,
                    &mut state.list_stack,
                    indent,
                    content_indent,
                    ListKind::Ordered,
                    *num,
                    None,
                    &inline_html,
                );
                state.prev_line_hard_break = line_has_hard_break(parsed);
                idx += 1;
            }
            BlockKind::Paragraph => {
                state.close_table();
                let after_quote = strip_quote_prefix(&parsed.raw_text);
                if after_quote.trim().is_empty() {
                    state.close_paragraph();
                    if !state.list_stack.is_empty() {
                        state.saw_blank_line_in_list = true;
                    }
                    idx += 1;
                    continue;
                }

                if !state.list_stack.is_empty() && state.saw_blank_line_in_list {
                    let para_indent = expanded_leading_indent(after_quote);
                    let top_content_indent =
                        state.list_stack.last().map_or(0, |lvl| lvl.content_indent);
                    if para_indent < top_content_indent {
                        state.close_lists();
                    }
                }

                if let Some((rendered, tex)) = standalone_display_math(parsed) {
                    state.close_paragraph();
                    state.saw_blank_line_in_list = false;
                    write_display_math_block(&mut state.out, rendered, tex);
                } else {
                    let inline_html =
                        render_inline_spans_html(&parsed.spans, doc_path, &mut embedder);
                    let content_html = if !state.list_stack.is_empty() {
                        inline_html.trim_start()
                    } else {
                        inline_html.as_str()
                    };
                    let is_direct_list_continuation = !state.list_stack.is_empty()
                        && !state.saw_blank_line_in_list
                        && !state.in_paragraph;

                    if is_direct_list_continuation {
                        append_line_break(&mut state.out, state.prev_line_hard_break);
                        state.out.push_str(content_html);
                    } else if !state.in_paragraph {
                        if !state.list_stack.is_empty() && state.saw_blank_line_in_list {
                            state.out.push('\n');
                            state.saw_blank_line_in_list = false;
                        }
                        state.out.push_str("<p>");
                        state.in_paragraph = true;
                        state.out.push_str(content_html);
                    } else {
                        append_line_break(&mut state.out, state.prev_line_hard_break);
                        state.out.push_str(content_html);
                    }
                    state.prev_line_hard_break = line_has_hard_break(parsed);
                }
                idx += 1;
            }
        }
    }

    state.finish()
}

fn expanded_leading_indent(line: &str) -> usize {
    let mut col = 0;
    for b in line.bytes() {
        match b {
            b' ' => col += 1,
            b'\t' => col += 4 - (col % 4),
            _ => break,
        }
    }
    col
}

fn list_metrics(raw_line: &str) -> (usize, usize) {
    let stripped = strip_quote_prefix(raw_line);
    let marker_indent = expanded_leading_indent(stripped);
    let marker_width = parse_block_prefix(stripped).map_or(2, |prefix| match prefix.kind {
        BlockPrefix::Bullet { .. } | BlockPrefix::Task { .. } => 2,
        BlockPrefix::Ordered { .. } => prefix.total_len.saturating_sub(prefix.leading_spaces),
        _ => 2,
    });
    (marker_indent, marker_indent + marker_width)
}

fn line_has_hard_break(parsed: &ParsedLine) -> bool {
    let after_quote = strip_quote_prefix(&parsed.raw_text);
    if after_quote.trim().is_empty() {
        return false;
    }
    let trailing_spaces = after_quote.bytes().rev().take_while(|&b| b == b' ').count();
    if trailing_spaces >= 2 {
        return true;
    }
    let trailing_backslashes = after_quote
        .bytes()
        .rev()
        .take_while(|&b| b == b'\\')
        .count();
    if trailing_backslashes % 2 == 1 {
        let last_non_marker = parsed
            .spans
            .iter()
            .rev()
            .find(|s| !s.is_marker && !s.is_always_hidden_in_live);
        return last_non_marker.is_some_and(|s| !s.is_code && s.math.is_none());
    }
    false
}

fn append_line_break(out: &mut String, hard_break: bool) {
    if hard_break {
        if out.ends_with('\\') {
            out.pop();
        } else {
            while out.ends_with(' ') {
                out.pop();
            }
        }
        out.push_str("<br>\n");
    } else {
        out.push('\n');
    }
}

#[allow(clippy::too_many_arguments)]
fn push_list_item(
    out: &mut String,
    stack: &mut Vec<ListLevel>,
    indent: usize,
    content_indent: usize,
    kind: ListKind,
    start_num: usize,
    task_checked: Option<bool>,
    content_html: &str,
) {
    while stack.last().is_some_and(|top| top.indent > indent) {
        if let Some(level) = stack.pop() {
            if level.has_open_li {
                out.push_str("</li>\n");
            }
            match level.kind {
                ListKind::Unordered => out.push_str("</ul>\n"),
                ListKind::Ordered => out.push_str("</ol>\n"),
            }
        }
    }

    if stack
        .last()
        .is_some_and(|top| top.indent == indent && top.kind != kind)
        && let Some(level) = stack.pop()
    {
        if level.has_open_li {
            out.push_str("</li>\n");
        }
        match level.kind {
            ListKind::Unordered => out.push_str("</ul>\n"),
            ListKind::Ordered => out.push_str("</ol>\n"),
        }
    }

    if stack.last().is_none_or(|top| top.indent < indent) {
        match kind {
            ListKind::Unordered => out.push_str("<ul>\n"),
            ListKind::Ordered => {
                if start_num != 1 {
                    let _ = writeln!(out, "<ol start=\"{start_num}\">");
                } else {
                    out.push_str("<ol>\n");
                }
            }
        }
        stack.push(ListLevel {
            indent,
            content_indent,
            kind,
            has_open_li: false,
        });
    } else if let Some(top) = stack.last_mut() {
        if top.has_open_li {
            out.push_str("</li>\n");
            top.has_open_li = false;
        }
        top.content_indent = content_indent;
    }

    if let Some(checked) = task_checked {
        let checked_attr = if checked { " checked" } else { "" };
        let checked_class = if checked { " task-checked" } else { "" };
        let _ = write!(
            out,
            "<li class=\"task-list-item{checked_class}\"><input type=\"checkbox\" disabled{checked_attr}><span>{content_html}</span>"
        );
    } else {
        let _ = write!(out, "<li>{content_html}");
    }

    if let Some(top) = stack.last_mut() {
        top.has_open_li = true;
    }
}

fn standalone_display_math(parsed: &ParsedLine) -> Option<(&MathSvg, &str)> {
    let mut found = None;
    for span in &parsed.spans {
        if span.is_marker || span.is_always_hidden_in_live {
            continue;
        }
        if let Some(math) = &span.math
            && math.is_display
        {
            if found.is_some() {
                return None;
            }
            found = Some((&math.rendered, math.tex.as_str()));
        } else if !span.text.trim().is_empty() {
            return None;
        }
    }
    found
}

fn write_display_math_block(out: &mut String, math: &MathSvg, tex: &str) {
    let (width_em, height_em) = math.size_em();
    let svg = strip_svg_xml_prolog(math.svg_str());
    let _ = writeln!(
        out,
        "<div class=\"math-block\"><span class=\"math-display\" style=\"width:{width_em:.4}em;height:{height_em:.4}em;\" title=\"{}\">{svg}</span></div>",
        escape_html(tex)
    );
}

fn extract_fence_language(raw_line: &str) -> &str {
    let stripped = strip_quote_prefix(raw_line);
    crate::markdown::fence_info(stripped)
        .and_then(|info| info.split_whitespace().next())
        .unwrap_or("")
}

fn visible_plain_line_text(parsed: &ParsedLine) -> String {
    let mut out = String::new();
    for span in &parsed.spans {
        if span.is_marker || span.is_always_hidden_in_live {
            continue;
        }
        out.push_str(&span.text);
    }
    out
}

fn write_code_block_shell(out: &mut String, lang: &str, write_body: impl FnOnce(&mut String)) {
    out.push_str("<div class=\"code-block\">\n");
    let header_label = if lang.is_empty() { "plaintext" } else { lang };
    let _ = writeln!(
        out,
        "<div class=\"code-header\"><span>{}</span></div>",
        escape_html(header_label)
    );
    if lang.is_empty() {
        out.push_str("<pre><code>");
    } else {
        let _ = write!(out, "<pre><code class=\"language-{}\">", escape_html(lang));
    }
    write_body(out);
    out.push_str("</code></pre>\n</div>\n");
}

fn write_code_block_html(out: &mut String, lang: &str, lines: &[&ParsedLine]) {
    write_code_block_shell(out, lang, |out| {
        for (line_idx, parsed) in lines.iter().enumerate() {
            if line_idx > 0 {
                out.push('\n');
            }
            for span in &parsed.spans {
                if span.is_marker || span.is_always_hidden_in_live {
                    continue;
                }
                if let Some(tok) = span.syntax_token {
                    let _ = write!(
                        out,
                        "<span class=\"{}\">{}</span>",
                        syntax_token_css_class(tok),
                        escape_html(&span.text)
                    );
                } else {
                    out.push_str(&escape_html(&span.text));
                }
            }
        }
    });
}

fn write_plain_code_block_html(out: &mut String, lang: &str, lines: &[String]) {
    write_code_block_shell(out, lang, |out| {
        for (line_idx, line) in lines.iter().enumerate() {
            if line_idx > 0 {
                out.push('\n');
            }
            out.push_str(&escape_html(line));
        }
    });
}

fn syntax_token_css_class(token: SyntaxToken) -> &'static str {
    match token {
        SyntaxToken::Keyword => "tok-keyword",
        SyntaxToken::Function => "tok-function",
        SyntaxToken::Type => "tok-type",
        SyntaxToken::String => "tok-string",
        SyntaxToken::Number => "tok-number",
        SyntaxToken::Comment => "tok-comment",
        SyntaxToken::Operator => "tok-operator",
        SyntaxToken::Punctuation => "tok-punctuation",
        SyntaxToken::Variable => "tok-variable",
        SyntaxToken::Attribute => "tok-attribute",
    }
}

fn alignment_attr(align: Option<TableAlignment>) -> &'static str {
    match align {
        Some(TableAlignment::Left) => " style=\"text-align:left;\"",
        Some(TableAlignment::Center) => " style=\"text-align:center;\"",
        Some(TableAlignment::Right) => " style=\"text-align:right;\"",
        Some(TableAlignment::None) | None => "",
    }
}

fn write_img_tag(
    out: &mut String,
    class_name: &str,
    raw_url: &str,
    alt: &str,
    doc_path: Option<&Path>,
    embedder: &mut ImageEmbedder,
) {
    out.push_str("<img");
    if !class_name.is_empty() {
        let _ = write!(out, " class=\"{class_name}\"");
    }
    if let Some(src) = embedder.resolve_image_src(raw_url, doc_path) {
        let _ = write!(out, " src=\"{}\"", escape_html(&src));
    }
    let _ = write!(out, " alt=\"{}\">", escape_html(alt));
}

fn render_inline_spans_html(
    spans: &[InlineSpan],
    doc_path: Option<&Path>,
    embedder: &mut ImageEmbedder,
) -> String {
    let mut out = String::new();
    let mut active_link: Option<String> = None;
    let mut idx = 0;

    while idx < spans.len() {
        let span = &spans[idx];

        if span.is_image
            && let Some(raw_url) = span.image_url.as_deref()
        {
            close_active_link(&mut out, &mut active_link);
            let group = span.group_range;
            let mut alt = String::new();
            let mut scan = idx;
            while scan < spans.len() && spans[scan].is_image && spans[scan].group_range == group {
                if !spans[scan].is_marker {
                    alt.push_str(&spans[scan].text);
                }
                scan += 1;
            }
            write_img_tag(&mut out, "inline-image", raw_url, &alt, doc_path, embedder);
            idx = scan;
            continue;
        }

        if span.is_marker || span.is_always_hidden_in_live {
            if span.is_marker && span.is_link {
                close_active_link(&mut out, &mut active_link);
            }
            idx += 1;
            continue;
        }

        let target_link = if span.is_link {
            span.link_url.as_deref().filter(|url| is_safe_href(url))
        } else {
            None
        };

        if active_link.as_deref() != target_link {
            close_active_link(&mut out, &mut active_link);
            if let Some(href) = target_link {
                let _ = write!(out, "<a href=\"{}\">", escape_html(href));
                active_link = Some(href.to_string());
            }
        }

        if let Some(math) = &span.math {
            let (width_em, height_em) = math.rendered.size_em();
            let svg = strip_svg_xml_prolog(math.rendered.svg_str());
            let class_name = if math.is_display {
                "math-display-inline"
            } else {
                "math-inline"
            };
            let _ = write!(
                out,
                "<span class=\"{class_name}\" style=\"width:{width_em:.4}em;height:{height_em:.4}em;\" title=\"{}\">{svg}</span>",
                escape_html(&math.tex)
            );
        } else {
            write_styled_inline_text(&mut out, span);
        }

        idx += 1;
    }

    close_active_link(&mut out, &mut active_link);
    out
}

fn close_active_link(out: &mut String, active_link: &mut Option<String>) {
    if active_link.take().is_some() {
        out.push_str("</a>");
    }
}

fn write_styled_inline_text(out: &mut String, span: &InlineSpan) {
    let escaped = escape_html(&span.text);
    if span.is_code {
        let _ = write!(out, "<code>{escaped}</code>");
        return;
    }
    if span.is_strikethrough {
        out.push_str("<del>");
    }
    if span.is_bold {
        out.push_str("<strong>");
    }
    if span.is_italic {
        out.push_str("<em>");
    }
    out.push_str(&escaped);
    if span.is_italic {
        out.push_str("</em>");
    }
    if span.is_bold {
        out.push_str("</strong>");
    }
    if span.is_strikethrough {
        out.push_str("</del>");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::buffer::TextBuffer;
    use crate::theme::DEFAULT_THEME;

    #[test]
    fn exports_full_markdown_elements_to_self_contained_html() {
        let temp = tempfile::tempdir().unwrap();
        let img_path = temp.path().join("dot.png");
        let png_bytes: &[u8] = &[
            0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a, 0x00, 0x00, 0x00, 0x0d, b'I', b'H',
            b'D', b'R', 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x08, 0x06, 0x00, 0x00,
            0x00, 0x1f, 0x15, 0xc4, 0x89, 0x00, 0x00, 0x00, 0x0a, b'I', b'D', b'A', b'T', 0x78,
            0x9c, 0x63, 0x00, 0x01, 0x00, 0x00, 0x05, 0x00, 0x01, 0x0d, 0x0a, 0x2d, 0xb4, 0x00,
            0x00, 0x00, 0x00, b'I', b'E', b'N', b'D', 0xae, 0x42, 0x60, 0x82,
        ];
        std::fs::write(&img_path, png_bytes).unwrap();
        let doc_path = temp.path().join("sample.md");

        let md = [
            "# Title <One>",
            "# Title <One>",
            "",
            "Paragraph with **bold**, *italic*, ~~strike~~, `inline`, [safe](https://example.com), [bad](javascript:alert(1)), and inline math $\\frac{1}{2}$.",
            "",
            "> Outer quote",
            "> > Nested quote",
            "",
            "- Bullet A",
            "  - Nested Bullet",
            "    1. Deep Ordered",
            "- [x] Done Task",
            "- [ ] Todo Task",
            "",
            "| Col A | Col B | Col C |",
            "| :--- | :---: | ---: |",
            "| Left | Center | Right |",
            "",
            "```rust",
            "fn hello() {}",
            "```",
            "",
            "$$",
            "\\sum_{i=1}^{n} i",
            "$$",
            "",
            "```mermaid",
            "flowchart LR",
            "    A[Start] --> B[End]",
            "```",
            "",
            "![Dot](dot.png)",
            "",
            "---",
        ]
        .join("\n");

        let buffer = TextBuffer::from_str(&md, Some(doc_path.clone()));
        let options = ExportOptions {
            title: "Sample <Doc>",
            doc_path: Some(&doc_path),
            theme: &DEFAULT_THEME,
        };
        let html = render_markdown_to_html(&buffer, &options);

        assert!(html.contains("<title>Sample &lt;Doc&gt;</title>"));
        assert!(html.contains("<h1 id=\"title-one\">Title &lt;One&gt;</h1>"));
        assert!(html.contains("<h1 id=\"title-one-2\">Title &lt;One&gt;</h1>"));
        assert!(html.contains("<strong>bold</strong>"));
        assert!(html.contains("<em>italic</em>"));
        assert!(html.contains("<del>strike</del>"));
        assert!(html.contains("<code>inline</code>"));
        assert!(html.contains("<a href=\"https://example.com\">safe</a>"));
        assert!(!html.contains("javascript:alert(1)"));
        assert!(html.contains("bad"));
        assert!(
            html.contains("<blockquote>\n<p>Outer quote</p>\n<blockquote>\n<p>Nested quote</p>")
        );
        assert!(html.contains(
            "<ul>\n<li>Bullet A<ul>\n<li>Nested Bullet<ol>\n<li>Deep Ordered</li>\n</ol>\n</li>\n</ul>"
        ));
        assert!(html.contains(
            "class=\"task-list-item task-checked\"><input type=\"checkbox\" disabled checked><span>Done Task</span>"
        ));
        assert!(html.contains("style=\"text-align:center;\""));
        assert!(html.contains("<pre><code class=\"language-rust\">"));
        assert!(html.contains("class=\"math-inline\""));
        assert!(html.contains("class=\"math-block\""));
        assert!(html.contains("class=\"mermaid-block\""));
        assert!(html.contains("<svg"));
        assert!(html.contains("src=\"data:image/png;base64,"));
        assert!(html.contains("<hr>"));
        assert!(html.contains("@media print"));
        assert!(html.contains("background: #ffffff;"));
    }

    #[test]
    fn paragraph_soft_and_hard_line_breaks_match_markdown_semantics() {
        let md = "Soft one\nSoft two\n\nHard space  \nNext\n\nHard slash\\\nNext";
        let buffer = TextBuffer::from_str(md, None);
        let options = ExportOptions {
            title: "Breaks",
            doc_path: None,
            theme: &DEFAULT_THEME,
        };
        let html = render_markdown_to_html(&buffer, &options);
        assert!(
            html.contains("<p>Soft one\nSoft two</p>"),
            "unexpected html:\n{html}"
        );
        assert!(
            html.contains("<p>Hard space<br>\nNext</p>"),
            "unexpected html:\n{html}"
        );
        assert!(
            html.contains("<p>Hard slash<br>\nNext</p>"),
            "unexpected html:\n{html}"
        );
    }

    #[test]
    fn loose_lists_and_tab_indented_continuations_preserve_list_structure() {
        let md = [
            "1. First item",
            "",
            "   Continuation paragraph.",
            "",
            "2. Second item",
            "",
            "\t- Tab nested child",
            "",
            "After list.",
        ]
        .join("\n");
        let buffer = TextBuffer::from_str(&md, None);
        let options = ExportOptions {
            title: "Lists",
            doc_path: None,
            theme: &DEFAULT_THEME,
        };
        let html = render_markdown_to_html(&buffer, &options);
        assert_eq!(html.matches("<ol>").count(), 1);
        assert!(html.contains("<li>First item\n<p>Continuation paragraph.</p>\n</li>"));
        assert!(html.contains("<li>Second item<ul>\n<li>Tab nested child</li>\n</ul>\n</li>"));
        assert!(html.contains("</ol>\n<p>After list.</p>"));
    }

    #[test]
    fn unclosed_math_block_is_not_re_evaluated_as_rendered_math() {
        let md = "$$\n\\frac{1}{2}\n";
        let buffer = TextBuffer::from_str(md, None);
        let options = ExportOptions {
            title: "Unclosed Math",
            doc_path: None,
            theme: &DEFAULT_THEME,
        };
        let html = render_markdown_to_html(&buffer, &options);
        assert!(!html.contains("class=\"math-block\""));
        assert!(html.contains("<code class=\"language-math\">"));
    }

    #[test]
    fn inline_unsafe_image_url_is_stripped() {
        let md = "Text ![alt](javascript:alert(1)) end";
        let buffer = TextBuffer::from_str(md, None);
        let options = ExportOptions {
            title: "Img",
            doc_path: None,
            theme: &DEFAULT_THEME,
        };
        let html = render_markdown_to_html(&buffer, &options);
        assert!(!html.contains("javascript:"));
        assert!(html.contains("alt=\"alt\""));
    }
}
