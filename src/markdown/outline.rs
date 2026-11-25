use std::collections::HashMap;

use crate::buffer::{LineSource, SourceLine};

/// A heading exposed by the document outline.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OutlineItem {
    pub level: u8,
    pub title: String,
    pub source_line: SourceLine,
    pub anchor: String,
}

/// Scans Markdown source lines into the headings shown by the outline panel.
pub fn collect_outline(lines: &[String]) -> Vec<OutlineItem> {
    collect_outline_from_source(lines)
}

/// Parses a single heading line without running the full block/window scanner.
pub(crate) fn parse_heading_line(raw_line: &str, quote_prefix: usize) -> Option<(u8, String)> {
    let projected = raw_line.get(quote_prefix..)?;
    let trimmed = projected.trim_start_matches([' ', '\t']);
    let (level, prefix_len) = super::prefix::parse_heading_prefix(trimmed)?;
    let content = &trimmed[prefix_len..];

    if !content
        .bytes()
        .any(|b| matches!(b, b'`' | b'$' | b'!' | b'[' | b'<' | b'*' | b'_' | b'~'))
    {
        return Some((level, clean_heading_title(content)));
    }

    let spans = super::inline::parse_inline_spans_with_render(content, 0, false);
    let mut visible = String::with_capacity(content.len());
    for span in &spans {
        if span.is_visible_for_selection(false, 0, false, false) {
            if let Some(math) = &span.math {
                visible.push_str(&math.source);
            } else {
                visible.push_str(&span.rendered_text(false, 0, false));
            }
        }
    }
    Some((level, clean_heading_title(&visible)))
}

/// Only headings need single-line parsing; body rows stay in the source Rope.
pub(crate) fn collect_outline_from_source(
    source: &(impl crate::buffer::LineSource + ?Sized),
) -> Vec<OutlineItem> {
    let source = source.source_snapshot();
    let index = source.structure_index();
    let total = index.heading_count();
    let mut anchor_counts = HashMap::<String, usize>::new();
    let mut items = Vec::with_capacity(total);

    for (source_line, quote_prefix) in index.heading_entries_in_range(0..total) {
        let raw_line = source.source_line(source_line);
        let Some((level, title)) = parse_heading_line(&raw_line, quote_prefix) else {
            continue;
        };
        let base_anchor = slugify(&title);
        let count = anchor_counts.entry(base_anchor.clone()).or_insert(0);
        *count += 1;
        let anchor = if *count == 1 {
            base_anchor
        } else {
            format!("{base_anchor}-{}", *count)
        };
        items.push(OutlineItem {
            level,
            title,
            source_line,
            anchor,
        });
    }

    items
}

pub(crate) fn collect_indexed_headings<'a>(
    rows: impl Iterator<Item = (usize, &'a super::types::ParsedLine)>,
) -> Vec<OutlineItem> {
    let mut anchor_counts = HashMap::<String, usize>::new();

    rows.filter_map(|(index, parsed)| {
        let super::types::BlockKind::Heading { level } = parsed.kind else {
            return None;
        };

        let title = clean_heading_title(&parsed.visible_text(false, 0, false));
        let base_anchor = slugify(&title);
        let count = anchor_counts.entry(base_anchor.clone()).or_insert(0);
        *count += 1;
        let anchor = if *count == 1 {
            base_anchor
        } else {
            format!("{base_anchor}-{}", *count)
        };

        Some(OutlineItem {
            level,
            title,
            source_line: SourceLine::new(index),
            anchor,
        })
    })
    .collect()
}

fn clean_heading_title(title: &str) -> String {
    let trimmed = title.trim();
    let mut end = trimmed.len();
    while let Some((index, character)) = trimmed[..end].char_indices().next_back() {
        if character == '#' {
            end = index;
        } else {
            break;
        }
    }

    if end < trimmed.len()
        && trimmed[..end]
            .chars()
            .next_back()
            .is_some_and(char::is_whitespace)
    {
        trimmed[..end].trim_end().to_string()
    } else {
        trimmed.to_string()
    }
}

fn slugify(title: &str) -> String {
    let mut slug = String::new();
    let mut pending_separator = false;

    for character in title.chars() {
        if character.is_alphanumeric() {
            if pending_separator && !slug.is_empty() {
                slug.push('-');
            }
            slug.extend(character.to_lowercase());
            pending_separator = false;
        } else if !slug.is_empty() {
            pending_separator = true;
        }
    }

    if slug.is_empty() {
        "heading".to_string()
    } else {
        slug
    }
}

#[cfg(test)]
mod tests {
    use super::slugify;

    #[test]
    fn source_outline_matches_full_parser_with_quoted_and_hidden_headings() {
        let lines: Vec<String> =
            "# one\n> ## quoted\n```\n# hidden\n```\n$$\n# hidden math\n$$\n# one\n"
                .split('\n')
                .map(str::to_owned)
                .collect();
        let buffer = crate::buffer::TextBuffer::from_str(&lines.join("\n"), None);
        assert_eq!(
            super::collect_outline_from_source(&buffer),
            super::collect_outline(&lines)
        );
    }

    #[test]
    fn slugify_collapses_punctuation_and_whitespace() {
        assert_eq!(
            slugify("A title: with punctuation!"),
            "a-title-with-punctuation"
        );
        assert_eq!(slugify(""), "heading");
    }

    #[test]
    fn parse_heading_line_handles_plain_and_inline_syntax() {
        assert_eq!(
            super::parse_heading_line("  ## Plain title ###", 0),
            Some((2, "Plain title".to_string()))
        );
        assert_eq!(
            super::parse_heading_line("> ### *Quoted* `code` [link](https://example.com) ~~old~~", 2),
            Some((3, "Quoted code link old".to_string()))
        );
        assert_eq!(
            super::parse_heading_line("# Math $x^2$ and ![alt](img.png)", 0),
            Some((1, "Math $x^2$ and alt".to_string()))
        );
        assert_eq!(
            super::parse_heading_line("# <https://example.com>", 0),
            Some((1, "https://example.com".to_string()))
        );
        assert_eq!(
            super::parse_heading_line("# ", 0),
            Some((1, String::new()))
        );
        assert_eq!(super::parse_heading_line("not a heading", 0), None);
    }

    #[test]
    fn source_outline_matches_full_scanner_across_inline_headings() {
        let lines: Vec<String> = vec![
            "# Plain Title".into(),
            "## *Bold* and `code` ##".into(),
            "> ### [Link](https://example.com) <user@example.com>".into(),
            "#### ~~Strike~~ and ![img](pic.png)".into(),
            "##### ".into(),
        ];
        let buffer = crate::buffer::TextBuffer::from_str(&lines.join("\n"), None);
        let parsed = crate::markdown::MarkdownScanner::scan_document(&lines);
        assert_eq!(
            super::collect_outline_from_source(&buffer),
            super::collect_indexed_headings(parsed.iter().enumerate())
        );
    }
}
