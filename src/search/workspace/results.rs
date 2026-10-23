use std::path::PathBuf;
use std::sync::Arc;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WorkspaceMatch {
    /// 1-based line number, as displayed.
    pub line_number: usize,
    pub line_content: Arc<str>,
    pub byte_range: (usize, usize),
    pub char_range: (usize, usize),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FileSearchResult {
    pub path: PathBuf,
    pub relative_path: String,
    pub matches: Vec<WorkspaceMatch>,
}

/// Results accumulated while a scan streams in.
///
/// Files are kept ordered by relative path so the list does not reshuffle between runs of the
/// same query, whichever worker happens to finish a file first.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct WorkspaceSearchResults {
    files: Vec<FileSearchResult>,
    total_matches: usize,
    is_capped: bool,
}

impl WorkspaceSearchResults {
    pub fn files(&self) -> &[FileSearchResult] {
        &self.files
    }

    pub fn total_matches(&self) -> usize {
        self.total_matches
    }

    /// Whether some matches were left out because a total or per-file limit was hit.
    pub fn is_capped(&self) -> bool {
        self.is_capped
    }

    pub fn is_empty(&self) -> bool {
        self.files.is_empty()
    }

    /// Inserts a file at its sorted position. The scan caps the number of matches, which bounds
    /// the number of files and keeps the shifting cheap.
    pub fn insert(&mut self, file: FileSearchResult) {
        let idx = self
            .files
            .partition_point(|f| f.relative_path < file.relative_path);
        self.total_matches += file.matches.len();
        self.files.insert(idx, file);
    }

    pub fn mark_capped(&mut self) {
        self.is_capped = true;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn file(relative_path: &str, match_count: usize) -> FileSearchResult {
        let line: Arc<str> = Arc::from("x");
        FileSearchResult {
            path: PathBuf::from(relative_path),
            relative_path: relative_path.to_string(),
            matches: (0..match_count)
                .map(|i| WorkspaceMatch {
                    line_number: i + 1,
                    line_content: line.clone(),
                    byte_range: (0, 1),
                    char_range: (0, 1),
                })
                .collect(),
        }
    }

    #[test]
    fn insert_keeps_files_sorted_by_relative_path() {
        let mut results = WorkspaceSearchResults::default();
        results.insert(file("sub/b.md", 1));
        results.insert(file("a.txt", 2));
        results.insert(file("c.txt", 3));

        let order: Vec<_> = results
            .files()
            .iter()
            .map(|f| f.relative_path.as_str())
            .collect();
        assert_eq!(order, ["a.txt", "c.txt", "sub/b.md"]);
        assert_eq!(results.total_matches(), 6);
        assert!(!results.is_capped());
    }
}
