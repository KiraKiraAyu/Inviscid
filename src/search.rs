//! Search functionality for Inviscid
//!
//! This module provides in-document find & replace, workspace-wide search,
//! and quick open file picker functionality.

mod engine;
mod find_state;
mod match_result;
mod workspace;

pub(crate) use engine::compute_rope_replace_all;
pub use engine::{find_matches, find_matches_in_rope};
pub(crate) use find_state::FindState;
pub(crate) use match_result::{SearchMatch, SearchToggle};
pub use match_result::{SearchOptions, SearchQuery};
pub(crate) use workspace::{
    FileSearchResult, ScanLimits, WorkspaceMatch, WorkspaceScan, WorkspaceSearchResults,
    replace_all_in_file, replace_match_in_file,
};
