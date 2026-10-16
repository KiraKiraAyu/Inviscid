//! Settings category definitions and navigation metadata.
//!
//! Defines the top-level categories available in the settings window sidebar
//! (`General`, `Appearance`, `Editor`, and `Keybindings`), along with their
//! display labels and icons.

use crate::ui::IconName;

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum SettingsCategory {
    General,
    Appearance,
    Editor,
    Keybindings,
}

impl SettingsCategory {
    pub const fn all() -> &'static [SettingsCategory] {
        &[
            Self::General,
            Self::Appearance,
            Self::Editor,
            Self::Keybindings,
        ]
    }

    pub const fn label(self) -> &'static str {
        match self {
            Self::General => "General",
            Self::Appearance => "Appearance",
            Self::Editor => "Editor",
            Self::Keybindings => "Keybindings",
        }
    }

    pub const fn icon(self) -> IconName {
        match self {
            Self::General => IconName::Settings,
            Self::Appearance => IconName::Palette,
            Self::Editor => IconName::FileText,
            Self::Keybindings => IconName::Keyboard,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn test_settings_categories_have_unique_labels() {
        let all = SettingsCategory::all();
        let labels: HashSet<_> = all.iter().map(|c| c.label()).collect();
        assert_eq!(labels.len(), all.len());
        assert!(all.iter().all(|c| !c.label().is_empty()));
    }
}
