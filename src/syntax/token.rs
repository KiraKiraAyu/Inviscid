use gpui::Hsla;

pub use crate::markdown::SyntaxToken;
use crate::theme::Theme;

impl SyntaxToken {
    /// Maps a Tree-sitter capture name (e.g. `"keyword.control"`, `"function.method"`) to a [`SyntaxToken`].
    pub fn from_capture_name(capture_name: &str) -> Option<Self> {
        let name = capture_name.trim_start_matches('@');

        // Sub-captures whose category differs from their root prefix:
        // - `variable.builtin` (`this`, `self`, `super`) is highlighted as a language keyword
        // - `string.escape` / `string.regex` / `string.special` use the operator/escape accent
        // - `property.attribute` maps to attribute rather than variable
        match name {
            "variable.builtin" => Some(Self::Keyword),
            "string.escape" | "string.regex" | "string.special" => Some(Self::Operator),
            "property.attribute" => Some(Self::Attribute),
            _ => {
                let root = name.split('.').next().unwrap_or(name);
                match root {
                    "keyword" | "storageclass" | "include" | "conditional" | "repeat"
                    | "exception" => Some(Self::Keyword),
                    "function" | "method" | "constructor" => Some(Self::Function),
                    "type" | "module" | "namespace" | "interface" | "struct" | "enum" => {
                        Some(Self::Type)
                    }
                    "string" | "character" => Some(Self::String),
                    "number" | "float" | "boolean" | "constant" => Some(Self::Number),
                    "comment" => Some(Self::Comment),
                    "operator" => Some(Self::Operator),
                    "punctuation" | "delimiter" => Some(Self::Punctuation),
                    "variable" | "property" | "field" | "parameter" => Some(Self::Variable),
                    "attribute" | "tag" | "decorator" | "annotation" | "label" => {
                        Some(Self::Attribute)
                    }
                    _ => None,
                }
            }
        }
    }

    #[inline]
    pub fn color(self, theme: &Theme) -> Hsla {
        match self {
            Self::Keyword => theme.syntax_keyword,
            Self::Function => theme.syntax_function,
            Self::Type => theme.syntax_type,
            Self::String => theme.syntax_string,
            Self::Number => theme.syntax_number,
            Self::Comment => theme.syntax_comment,
            Self::Operator => theme.syntax_operator,
            Self::Punctuation => theme.syntax_punctuation,
            Self::Variable => theme.syntax_variable,
            Self::Attribute => theme.syntax_attribute,
        }
    }

    #[inline]
    pub fn is_italic(self) -> bool {
        matches!(self, Self::Comment)
    }
}
