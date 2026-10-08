use std::collections::HashMap;
use std::fs;
use std::path::Path;

use serde::Deserialize;

use super::{DEFAULT_THEME, Theme, ThemeDef};

/// Built-in themes shipped in the embedded asset bundle, in listing order.
/// The first entry supplies the baseline palette.
const BUILTIN_THEMES: [&str; 5] = [
    DEFAULT_THEME_FILE,
    "catppuccin-latte.toml",
    "dracula.toml",
    "github-light.toml",
    "nord.toml",
];

/// File name, inside `assets/themes/`, of the theme supplying the baseline palette.
const DEFAULT_THEME_FILE: &str = "catppuccin-mocha.toml";

/// A theme file as loaded from the bundle or from disk: a name plus a possibly
/// partial color block.
#[derive(Debug, Clone, Deserialize)]
pub(crate) struct ThemeConfig {
    pub name: String,
    #[serde(default)]
    pub colors: ThemeDef,
}

impl ThemeConfig {
    /// Applies the top-level name to the color block.
    fn into_colors(mut self) -> ThemeDef {
        self.colors.name = self.name;
        self.colors
    }

    /// Resolves into a complete theme, inheriting every unset color from `base`.
    pub fn into_theme(self, base: &Theme) -> Theme {
        self.into_colors().resolve(base)
    }

    /// Resolves into a complete theme with no inheritance: every color must be set.
    pub fn into_standalone(self) -> std::result::Result<Theme, Vec<&'static str>> {
        self.into_colors().into_standalone()
    }
}

/// Loads a built-in theme definition from the embedded asset bundle
/// (`assets/themes/<file_name>`).
///
/// Returns `None` after logging when the asset is missing or malformed.
fn load_builtin_theme(file_name: &str) -> Option<ThemeConfig> {
    let asset_path = format!("themes/{file_name}");
    let file = crate::assets::Assets::get(&asset_path)?;
    let content = std::str::from_utf8(&file.data)
        .map_err(|e| eprintln!("[theme] Invalid UTF-8 in builtin theme '{asset_path}': {e}"))
        .ok()?;
    toml::from_str::<ThemeConfig>(content)
        .map_err(|e| eprintln!("[theme] Failed to parse builtin theme '{asset_path}': {e}"))
        .ok()
}

/// Loads and fully resolves the baseline palette from the embedded bundle.
///
/// Panics on a missing, malformed or incomplete default theme: that asset ships
/// with the binary, so it can only fail because of a programming error.
pub(crate) fn load_default_theme() -> Theme {
    let config = load_builtin_theme(DEFAULT_THEME_FILE).unwrap_or_else(|| {
        panic!("[theme] Default theme asset 'themes/{DEFAULT_THEME_FILE}' is missing or malformed")
    });

    match config.into_standalone() {
        Ok(theme) => theme,
        Err(missing) => panic!(
            "[theme] Default theme '{DEFAULT_THEME_FILE}' leaves colors unset: {}",
            missing.join(", ")
        ),
    }
}

#[derive(Clone, Debug)]
pub struct ThemeRegistry {
    themes: HashMap<String, Theme>,
    theme_order: Vec<String>,
}

impl Default for ThemeRegistry {
    fn default() -> Self {
        Self::new()
    }
}

impl ThemeRegistry {
    pub fn new() -> Self {
        let mut registry = Self {
            themes: HashMap::new(),
            theme_order: Vec::new(),
        };

        registry.register_builtins();
        registry.load_external_themes();

        registry
    }

    /// Registers built-in preset themes from the embedded asset bundle
    fn register_builtins(&mut self) {
        for file_name in BUILTIN_THEMES {
            if let Some(config) = load_builtin_theme(file_name) {
                self.register_theme(config.into_theme(&DEFAULT_THEME));
            }
        }
    }

    /// Registers or overrides a theme in the registry
    fn register_theme(&mut self, theme: Theme) {
        let name = theme.name.clone();
        if !self.theme_order.contains(&name) {
            self.theme_order.push(name.clone());
        }
        self.themes.insert(name, theme);
    }

    /// Loads every `*.toml` theme from the user config directory
    /// (`~/.config/inviscid/themes` or `%APPDATA%/inviscid/themes`).
    ///
    /// Themes are only read once, at startup; adding or editing one requires a restart.
    fn load_external_themes(&mut self) {
        let Some(dir) = dirs::config_dir().map(|d| d.join("inviscid").join("themes")) else {
            return;
        };
        if dir.is_dir() {
            self.load_themes_from_dir(&dir);
        }
    }

    /// Loads all `*.toml` theme files from a specific directory
    fn load_themes_from_dir(&mut self, dir: &Path) {
        let entries = match fs::read_dir(dir) {
            Ok(entries) => entries,
            Err(e) => {
                eprintln!(
                    "[theme] Failed to read theme directory '{}': {e}",
                    dir.display()
                );
                return;
            }
        };

        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_file() && path.extension().and_then(|ext| ext.to_str()) == Some("toml") {
                match fs::read_to_string(&path) {
                    Ok(content) => match toml::from_str::<ThemeConfig>(&content) {
                        Ok(config) => {
                            self.register_theme(config.into_theme(&DEFAULT_THEME));
                        }
                        Err(e) => {
                            eprintln!(
                                "[theme] Failed to parse custom theme file '{}': {e}",
                                path.display()
                            );
                        }
                    },
                    Err(e) => {
                        eprintln!(
                            "[theme] Failed to read custom theme file '{}': {e}",
                            path.display()
                        );
                    }
                }
            }
        }
    }

    /// Retrieves a theme by name, falling back to the default palette
    pub fn get_or_default(&self, name: &str) -> Theme {
        match self.themes.get(name) {
            Some(theme) => theme.clone(),
            None => {
                eprintln!(
                    "[theme] Unknown theme '{name}', falling back to '{}'",
                    DEFAULT_THEME.name
                );
                Theme::default()
            }
        }
    }

    /// Returns list of all available theme names
    pub fn list_themes(&self) -> &[String] {
        &self.theme_order
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_theme_asset_is_complete() {
        let config = load_builtin_theme(DEFAULT_THEME_FILE).expect("asset must be embedded");
        assert_eq!(
            config.name,
            crate::config::AppConfig::default().theme,
            "config.rs default theme name must match the default theme asset"
        );
        assert!(
            config.into_standalone().is_ok(),
            "the default theme must specify every color"
        );
    }

    #[test]
    fn test_theme_registry_builtins() {
        let registry = ThemeRegistry::new();
        let list = registry.list_themes();

        assert!(list.contains(&"Catppuccin Mocha".to_string()));
        assert!(list.contains(&"Catppuccin Latte".to_string()));
        assert!(list.contains(&"Dracula".to_string()));
        assert!(list.contains(&"GitHub Light".to_string()));
        assert!(list.contains(&"Nord".to_string()));

        // Fallback for non-existent theme
        let fallback = registry.get_or_default("NonExistentTheme");
        assert_eq!(fallback.name, "Catppuccin Mocha");
    }

    #[test]
    fn test_load_themes_from_dir_registers_custom_theme() {
        let dir = std::env::temp_dir().join(format!("inviscid_theme_dir_{}", std::process::id()));
        let _ = fs::create_dir_all(&dir);
        let file = dir.join("temp_custom.toml");
        fs::write(
            &file,
            "name = \"Temp Custom\"\n\n[colors]\nbg_editor = \"#123456\"\n",
        )
        .unwrap();

        let mut registry = ThemeRegistry::new();
        registry.load_themes_from_dir(&dir);

        assert!(registry.list_themes().contains(&"Temp Custom".to_string()));

        let loaded = registry.get_or_default("Temp Custom");
        assert_eq!(loaded.name, "Temp Custom");
        assert_eq!(loaded.bg_editor, gpui::rgb(0x123456).into());
        // Unset colors still inherit from the baseline palette
        assert_eq!(loaded.text_primary, DEFAULT_THEME.text_primary);

        let _ = fs::remove_file(&file);
        let _ = fs::remove_dir_all(&dir);
    }
}
