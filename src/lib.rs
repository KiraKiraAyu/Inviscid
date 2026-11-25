//! Inviscid: A WYSIWYG markdown editor built on GPUI.

#![forbid(dead_code)]
#![recursion_limit = "256"]
pub mod app;
pub mod assets;
pub mod buffer;
pub mod config;
pub mod editor;
pub(crate) mod export;
pub(crate) mod fs;
pub mod http;
pub mod markdown;
pub(crate) mod math;
pub mod media;
pub(crate) mod mermaid;
pub(crate) mod platform;
pub mod search;
pub(crate) mod sync;
pub mod syntax;
pub mod theme;
pub mod ui;
pub(crate) mod wasm;

pub use app::InviscidWindow;
pub use buffer::TextBuffer;
pub use editor::Editor;
pub use theme::{ActiveTheme, Theme, ThemeManager};
