pub(crate) mod editing;
pub(crate) mod indent;
pub(crate) mod inline;
pub(crate) mod outline;
pub(crate) mod prefix;
pub(crate) mod projection;
pub(crate) mod scanner;
pub(crate) mod structure;
pub(crate) mod table;
pub(crate) mod types;

#[cfg(test)]
mod tests;

pub use editing::LiveBuffer;

pub(crate) use indent::*;
pub use inline::parse_inline_spans;
pub(crate) use inline::*;
pub(crate) use outline::*;
pub use outline::{OutlineItem, collect_outline};
pub(crate) use prefix::*;
pub(crate) use projection::*;
pub use scanner::MarkdownScanner;
pub(crate) use scanner::{ScanOptions, is_mermaid_fence, *};
pub use types::BlockKind;
pub(crate) use types::*;
