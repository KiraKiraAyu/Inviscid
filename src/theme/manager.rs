use crate::config::AppConfig;
use crate::theme::{Theme, ThemeRegistry};

/// Encapsulates theme lifecycle management:
/// active runtime theme, registry lookup, and live hover preview / rollback.
#[derive(Clone, Debug)]
pub struct ThemeManager {
    current: Theme,
    registry: ThemeRegistry,
}

impl ThemeManager {
    pub fn new(saved_theme_name: &str) -> Self {
        let registry = ThemeRegistry::new();
        let current = registry.get_or_default(saved_theme_name);
        Self { current, registry }
    }

    pub fn from_config(config: &AppConfig) -> Self {
        Self::new(&config.theme)
    }

    pub fn theme(&self) -> &Theme {
        &self.current
    }

    pub fn list_themes(&self) -> &[String] {
        self.registry.list_themes()
    }

    /// Previews a theme by name. Returns true if the active theme changed.
    pub fn preview(&mut self, name: &str) -> bool {
        if self.current.name != name {
            self.current = self.registry.get_or_default(name);
            true
        } else {
            false
        }
    }

    /// Reverts preview to the saved theme preference. Returns true if restored.
    pub fn cancel_preview(&mut self, saved_theme_name: &str) -> bool {
        if self.current.name != saved_theme_name {
            self.current = self.registry.get_or_default(saved_theme_name);
            true
        } else {
            false
        }
    }

    /// Permanently switches to the target theme.
    pub fn switch_to(&mut self, name: &str) -> &Theme {
        self.current = self.registry.get_or_default(name);
        &self.current
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_theme_manager_preview_and_rollback() {
        let mut manager = ThemeManager::new("Catppuccin Mocha");
        assert_eq!(manager.theme().name, "Catppuccin Mocha");

        // Preview a new theme
        let changed = manager.preview("Nord");
        assert!(changed);
        assert_eq!(manager.theme().name, "Nord");

        // Previewing same theme returns false
        let changed_again = manager.preview("Nord");
        assert!(!changed_again);

        // Cancel preview back to Catppuccin Mocha
        let restored = manager.cancel_preview("Catppuccin Mocha");
        assert!(restored);
        assert_eq!(manager.theme().name, "Catppuccin Mocha");

        // Cancel preview when already matching saved theme returns false
        let restored_again = manager.cancel_preview("Catppuccin Mocha");
        assert!(!restored_again);

        // Permanent switch
        manager.switch_to("Dracula");
        assert_eq!(manager.theme().name, "Dracula");
    }
}
