//! Search functionality for Inviscid
//!
//! This module provides in-document find & replace, workspace-wide search,
//! and quick open file picker functionality.

mod engine;
mod find_state;
mod match_result;

pub use engine::find_matches;
pub use find_state::FindState;
pub use match_result::{SearchMatch, SearchOptions, SearchQuery};
