use std::collections::HashMap;

use crate::buffer::SourceLine;

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
    let parsed_lines = super::scanner::MarkdownScanner::scan_document(lines);
    let mut anchor_counts = HashMap::<String, usize>::new();

    parsed_lines
        .iter()
        .enumerate()
        .filter_map(|(index, parsed)| {
            let super::types::BlockKind::Heading { level } = parsed.kind else {
                return None;
            };

            let title = clean_heading_title(parsed.visible_text(false, 0, false));
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

fn clean_heading_title(title: String) -> String {
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
    fn slugify_collapses_punctuation_and_whitespace() {
        assert_eq!(
            slugify("A title: with punctuation!"),
            "a-title-with-punctuation"
        );
        assert_eq!(slugify(""), "heading");
    }
}
