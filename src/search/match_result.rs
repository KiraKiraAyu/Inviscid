use crate::buffer::Position;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SearchMatch {
    pub start: Position,
    pub end: Position,
}

impl SearchMatch {
    pub fn new(start: Position, end: Position) -> Self {
        Self { start, end }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SearchOptions {
    pub case_sensitive: bool,
    pub whole_word: bool,
    pub use_regex: bool,
    pub wrap_around: bool,
}

impl Default for SearchOptions {
    fn default() -> Self {
        Self {
            case_sensitive: false,
            whole_word: false,
            use_regex: false,
            wrap_around: true,
        }
    }
}

#[derive(Clone, Debug)]
pub struct SearchQuery {
    raw: String,
    options: SearchOptions,
    regex: regex::Regex,
}

impl SearchQuery {
    pub fn new(query: impl Into<String>, options: SearchOptions) -> Result<Self, String> {
        let raw = query.into();
        if raw.is_empty() {
            return Err("Search query cannot be empty".to_string());
        }

        let body = if options.use_regex {
            raw.clone()
        } else {
            regex::escape(&raw)
        };
        let pattern = if options.whole_word {
            format!(r"\b(?:{body})\b")
        } else {
            body
        };

        let regex = regex::RegexBuilder::new(&pattern)
            .case_insensitive(!options.case_sensitive)
            .multi_line(true)
            .build()
            .map_err(|e| format!("Invalid regex: {e}"))?;

        Ok(Self {
            raw,
            options,
            regex,
        })
    }

    pub fn raw(&self) -> &str {
        &self.raw
    }

    pub fn options(&self) -> &SearchOptions {
        &self.options
    }

    pub(crate) fn regex(&self) -> &regex::Regex {
        &self.regex
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_search_query_literal() {
        let query = SearchQuery::new("test", SearchOptions::default()).unwrap();
        assert_eq!(query.raw(), "test");
        assert!(!query.options().case_sensitive);
    }

    #[test]
    fn test_search_query_regex() {
        let opts = SearchOptions {
            use_regex: true,
            ..Default::default()
        };
        let query = SearchQuery::new(r"\w+", opts).unwrap();
        assert!(query.options().use_regex);
    }

    #[test]
    fn test_search_query_invalid_regex() {
        let opts = SearchOptions {
            use_regex: true,
            ..Default::default()
        };
        let result = SearchQuery::new(r"[invalid", opts);
        assert!(result.is_err());
    }

    #[test]
    fn test_search_query_empty() {
        let result = SearchQuery::new("", SearchOptions::default());
        assert!(result.is_err());
    }
}
