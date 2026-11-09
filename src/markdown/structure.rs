#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classifies_raw_structure_without_stripping_quotes() {
        for (line, blank, fence, math, quote) in [
            ("", true, false, false, 0),
            (" \t", true, false, false, 0),
            ("  ```rust", false, true, false, 0),
            ("\t````", false, true, false, 0),
            ("``", false, false, false, 0),
            ("> ```", false, false, false, 2),
            (" > > $$ ", false, false, false, 5),
            (" $$ \t", false, false, true, 0),
            ("$$x", false, false, false, 0),
            ("> ", false, false, false, 2),
            ("text", false, false, false, 0),
        ] {
            assert_eq!(
                classify(line),
                LineStructure {
                    is_blank: blank,
                    is_fence: fence,
                    is_math_delim: math,
                    quote_prefix: quote,
                },
                "{line:?}"
            );
        }
    }
}
/// Raw source syntax; callers explicitly classify the projected content when needed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct LineStructure {
    pub is_blank: bool,
    pub is_fence: bool,
    pub is_math_delim: bool,
    pub quote_prefix: usize,
}

pub(crate) fn classify(line: &str) -> LineStructure {
    LineStructure {
        is_blank: line.trim().is_empty(),
        is_fence: super::prefix::is_fence_line(line),
        is_math_delim: line.trim() == "$$",
        quote_prefix: super::indent::quote_prefix_len(line),
    }
}
