//! Workspace-wide search: file discovery, the shared file-listing cache, the parallel scan, and
//! the result model. Nothing here depends on the UI framework.

mod cache;
mod discovery;
mod replace;
mod results;
mod scan;

pub use cache::WorkspaceFileCache;
pub use replace::{replace_all_in_file, replace_match_in_file};
pub use results::{FileSearchResult, WorkspaceMatch, WorkspaceSearchResults};
pub use scan::{ScanLimits, WorkspaceScan};
