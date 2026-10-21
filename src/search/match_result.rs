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

    pub fn replace_all(&self, text: &str, replacement: &str) -> (String, usize) {
        let mut count = 0;
        let replaced = self.regex.replace_all(text, |caps: &regex::Captures| {
            let m = caps.get(0).unwrap();
            if m.is_empty() {
                return m.as_str().to_string();
            }
            count += 1;
            if self.options.use_regex {
                let mut dst = String::new();
                caps.expand(replacement, &mut dst);
                dst
            } else {
                replacement.to_string()
            }
        });
        (replaced.into_owned(), count)
    }

    pub fn expand_replacement(&self, matched_text: &str, replacement: &str) -> String {
        if self.options.use_regex {
            if let Some(caps) = self.regex.captures(matched_text) {
                let mut dst = String::new();
                caps.expand(replacement, &mut dst);
                dst
            } else {
                replacement.to_string()
            }
        } else {
            replacement.to_string()
        }
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

    #[test]
    fn test_replace_all_literal_does_not_expand_dollar() {
        let query = SearchQuery::new("hello", SearchOptions::default()).unwrap();
        let (replaced, count) = query.replace_all("hello world hello", "hi $1");
        assert_eq!(count, 2);
        assert_eq!(replaced, "hi $1 world hi $1");
    }

    #[test]
    fn test_replace_all_case_insensitive() {
        let query = SearchQuery::new("hello", SearchOptions::default()).unwrap();
        let (replaced, count) = query.replace_all("Hello world HELLO", "hi");
        assert_eq!(count, 2);
        assert_eq!(replaced, "hi world hi");
    }

    #[test]
    fn test_replace_all_regex_with_capture_groups() {
        let opts = SearchOptions {
            use_regex: true,
            ..Default::default()
        };
        let query = SearchQuery::new(r"user_(\d+)", opts).unwrap();
        let (replaced, count) = query.replace_all("user_123 and user_456", "id_$1");
        assert_eq!(count, 2);
        assert_eq!(replaced, "id_123 and id_456");
    }

    #[test]
    fn test_expand_replacement_literal() {
        let query = SearchQuery::new("foo", SearchOptions::default()).unwrap();
        assert_eq!(query.expand_replacement("foo", "bar$1"), "bar$1");
    }

    #[test]
    fn test_expand_replacement_regex() {
        let opts = SearchOptions {
            use_regex: true,
            ..Default::default()
        };
        let query = SearchQuery::new(r"(\w+)-(\d+)", opts).unwrap();
        assert_eq!(query.expand_replacement("item-42", "$2/$1"), "42/item");
    }
}
