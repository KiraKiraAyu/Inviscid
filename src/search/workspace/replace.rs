use std::path::Path;

use super::super::SearchQuery;
use super::results::WorkspaceMatch;
use crate::fs::save_document_chunks;

/// Replaces one search result in `path`, preserving the file's identity on disk.
pub fn replace_match_in_file(
    path: &Path,
    query: &SearchQuery,
    replacement: &str,
    matched: &WorkspaceMatch,
) -> std::io::Result<bool> {
    let source = std::fs::read_to_string(path)?;
    let (normalized, line_ending) = normalize_line_endings(&source);
    let mut lines: Vec<String> = normalized.split('\n').map(str::to_owned).collect();
    let line = match lines.get_mut(matched.line_number.saturating_sub(1)) {
        Some(line) => line,
        None => return Ok(false),
    };
    if line != matched.line_content.as_ref() {
        return Ok(false);
    }
    let (start, end) = matched.byte_range;
    let matched_text = match line.get(start..end) {
        Some(text)
            if !text.is_empty()
                && query
                    .regex()
                    .find_iter(line)
                    .any(|found| found.start() == start && found.end() == end) =>
        {
            text
        }
        _ => return Ok(false),
    };
    let replacement = query.expand_replacement(matched_text, replacement);
    line.replace_range(start..end, &replacement);
    write_content(path, &lines.join("\n"), line_ending).map(|()| true)
}

/// Replaces every match in the provided UTF-8 file and returns the number of replacements.
pub fn replace_all_in_file(
    path: &Path,
    query: &SearchQuery,
    replacement: &str,
) -> std::io::Result<usize> {
    let source = std::fs::read_to_string(path)?;
    let (normalized, line_ending) = normalize_line_endings(&source);
    let (replaced, count) = query.replace_all(
        &normalized,
        &replacement.replace("\r\n", "\n").replace('\r', "\n"),
    );
    if count == 0 {
        return Ok(0);
    }
    write_content(path, &replaced, line_ending)?;
    Ok(count)
}

#[derive(Clone, Copy)]
enum LineEnding {
    Lf,
    CrLf,
}

fn normalize_line_endings(source: &str) -> (String, LineEnding) {
    let line_ending = if source.contains("\r\n") {
        LineEnding::CrLf
    } else {
        LineEnding::Lf
    };
    (
        source.replace("\r\n", "\n").replace('\r', "\n"),
        line_ending,
    )
}

fn write_content(path: &Path, content: &str, line_ending: LineEnding) -> std::io::Result<()> {
    let output = match line_ending {
        LineEnding::Lf => content.to_owned(),
        LineEnding::CrLf => content.replace('\n', "\r\n"),
    };
    save_document_chunks(path, [&output[..]])?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::search::SearchOptions;
    use std::fs;
    use std::sync::Arc;

    fn query(raw: &str) -> SearchQuery {
        SearchQuery::new(raw, SearchOptions::default()).unwrap()
    }

    fn hit(line: &str, byte_range: (usize, usize)) -> WorkspaceMatch {
        WorkspaceMatch {
            line_number: 2,
            line_content: Arc::from(line),
            byte_range,
            char_range: byte_range,
        }
    }

    #[test]
    fn replaces_one_match_and_preserves_other_matches() {
        let temp_dir = tempfile::tempdir().unwrap();
        let path = temp_dir.path().join("notes.txt");
        fs::write(&path, "first\napple apple\nlast\n").unwrap();

        let count =
            replace_match_in_file(&path, &query("apple"), "pear", &hit("apple apple", (0, 5)))
                .unwrap();

        assert!(count);
        assert_eq!(
            fs::read_to_string(&path).unwrap(),
            "first\npear apple\nlast\n"
        );
    }

    #[test]
    fn replace_one_rejects_stale_result() {
        let temp_dir = tempfile::tempdir().unwrap();
        let path = temp_dir.path().join("notes.txt");
        fs::write(&path, "first\nchanged\nlast\n").unwrap();

        let count =
            replace_match_in_file(&path, &query("apple"), "pear", &hit("apple", (0, 5))).unwrap();

        assert!(!count);
        assert_eq!(fs::read_to_string(&path).unwrap(), "first\nchanged\nlast\n");
    }

    #[test]
    fn replaces_all_matches_with_regex_captures() {
        let temp_dir = tempfile::tempdir().unwrap();
        let path = temp_dir.path().join("notes.txt");
        fs::write(&path, "user_1 user_2\n").unwrap();
        let query = SearchQuery::new(
            r"user_(\d+)",
            SearchOptions {
                use_regex: true,
                ..SearchOptions::default()
            },
        )
        .unwrap();

        assert_eq!(replace_all_in_file(&path, &query, "id_$1").unwrap(), 2);
        assert_eq!(fs::read_to_string(&path).unwrap(), "id_1 id_2\n");
    }

    #[test]
    fn replacement_preserves_crlf_line_endings() {
        let temp_dir = tempfile::tempdir().unwrap();
        let path = temp_dir.path().join("notes.txt");
        fs::write(&path, "first\r\napple\r\nlast\r\n").unwrap();

        assert_eq!(
            replace_all_in_file(&path, &query("apple"), "pear").unwrap(),
            1
        );
        assert_eq!(
            fs::read_to_string(&path).unwrap(),
            "first\r\npear\r\nlast\r\n"
        );
    }
}
