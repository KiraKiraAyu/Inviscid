//! Settings category definitions and navigation metadata.
//!
//! Defines the top-level categories available in the settings window sidebar
//! (`General`, `Appearance`, `Editor`, and `Keybindings`), along with their
//! display labels and icons.

use crate::i18n::TextKey;
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

    pub const fn id(self) -> &'static str {
        match self {
            Self::General => "General",
            Self::Appearance => "Appearance",
            Self::Editor => "Editor",
            Self::Keybindings => "Keybindings",
        }
    }

    pub const fn label_key(self) -> TextKey {
        match self {
            Self::General => TextKey::SettingsCategoryGeneral,
            Self::Appearance => TextKey::SettingsCategoryAppearance,
            Self::Editor => TextKey::SettingsCategoryEditor,
            Self::Keybindings => TextKey::SettingsCategoryKeybindings,
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
        let ids: HashSet<_> = all.iter().map(|c| c.id()).collect();
        let keys: HashSet<_> = all.iter().map(|c| c.label_key()).collect();
        assert_eq!(ids.len(), all.len());
        assert_eq!(keys.len(), all.len());
        assert!(all.iter().all(|c| !c.id().is_empty()));
    }
}
