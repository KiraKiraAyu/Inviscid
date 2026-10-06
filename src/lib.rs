//! Inviscid: A WYSIWYG markdown editor built on GPUI.

#![forbid(dead_code)]
#![recursion_limit = "256"]
pub mod app;
pub mod assets;
pub mod buffer;
pub mod config;
pub mod editor;
pub mod fs;
pub mod http;
pub mod markdown;
pub mod platform;
pub mod syntax;
pub mod theme;
pub mod ui;
pub mod wasm;

pub use app::InviscidWindow;
pub use buffer::TextBuffer;
pub use editor::Editor;
pub use theme::{ActiveTheme, Theme, ThemeManager};
