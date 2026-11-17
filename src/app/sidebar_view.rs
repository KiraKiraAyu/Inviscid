use serde::{Deserialize, Serialize};

/// The panels the sidebar can show.
///
/// A leaf type shared by the workspace, which owns the sidebar state, and the status bar, which
/// renders the switcher, so neither has to depend on the other.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SidebarView {
    #[default]
    Files,
    Search,
}
