pub mod color;
pub mod manager;
pub mod registry;

use gpui::Hsla;
use serde::Deserialize;

use color::HexColor;
pub use manager::ThemeManager;
pub use registry::ThemeRegistry;

impl gpui::Global for ThemeManager {}

/// Baseline palette: used until `ThemeManager` is registered, and the palette
/// every other theme inherits its unset colors from.
///
/// Parsed from the embedded `assets/themes/catppuccin-mocha.toml`, so the color
/// values live in exactly one place.
pub static DEFAULT_THEME: std::sync::LazyLock<Theme> =
    std::sync::LazyLock::new(registry::load_default_theme);

pub trait ActiveTheme {
    fn theme(&self) -> &Theme;
}

impl ActiveTheme for gpui::App {
    fn theme(&self) -> &Theme {
        self.try_global::<ThemeManager>()
            .map(|m| m.theme())
            .unwrap_or(&DEFAULT_THEME)
    }
}

/// Declares the theme token set and the two types derived from it:
/// [`Theme`] (all colors present) and [`ThemeDef`] (all colors optional).
macro_rules! define_theme {
    ( $( $field:ident ),* $(,)? ) => {
        /// A fully resolved theme: every color is guaranteed to be present.
        #[derive(Clone, Debug, PartialEq)]
        pub struct Theme {
            pub name: String,
            $( pub $field: Hsla, )*
        }

        /// A theme as written in a `*.toml` file: every color is optional,
        /// and unset colors are inherited from the base palette.
        #[derive(Clone, Debug, PartialEq, Deserialize)]
        #[serde(default)]
        pub(crate) struct ThemeDef {
            pub name: String,
            $( pub $field: Option<HexColor>, )*
        }

        impl Default for ThemeDef {
            fn default() -> Self {
                Self {
                    name: String::new(),
                    $( $field: None, )*
                }
            }
        }

        impl Default for Theme {
            fn default() -> Self {
                DEFAULT_THEME.clone()
            }
        }

        impl ThemeDef {
            /// Completes this definition without inheriting anything.
            ///
            /// Returns the tokens that were never specified, turning an
            /// incomplete base palette into an explicit failure.
            pub fn into_standalone(self) -> std::result::Result<Theme, Vec<&'static str>> {
                let missing: Vec<&'static str> = [
                    $( self.$field.is_none().then_some(stringify!($field)), )*
                ]
                .into_iter()
                .flatten()
                .collect();

                if !missing.is_empty() {
                    return Err(missing);
                }

                Ok(Theme {
                    name: self.name,
                    $( $field: self.$field.unwrap().into(), )*
                })
            }

            /// Completes this definition, inheriting every unset color from `base`.
            pub fn resolve(self, base: &Theme) -> Theme {
                Theme {
                    name: if self.name.is_empty() {
                        base.name.clone()
                    } else {
                        self.name
                    },
                    $( $field: self.$field.map(|c| c.into()).unwrap_or(base.$field), )*
                }
            }
        }

    };
}

define_theme! {
    // Base backgrounds and borders
    bg_app,
    bg_editor,
    bg_toolbar,
    bg_statusbar,
    border,
    border_subtle,

    // Text colors
    text_primary,
    text_secondary,
    text_muted,
    text_accent,

    // Markdown syntax and visual tokens
    heading_h1,
    heading_h2,
    heading_h3,
    heading_h4,
    heading_marker,

    inline_code_bg,
    inline_code_fg,

    code_block_bg,
    code_block_border,
    code_block_header_bg,
    code_block_header_fg,
    code_block_text,

    syntax_keyword,
    syntax_function,
    syntax_type,
    syntax_string,
    syntax_number,
    syntax_comment,
    syntax_operator,
    syntax_punctuation,
    syntax_variable,
    syntax_attribute,

    quote_border,
    quote_bg,
    quote_text,

    list_marker,
    task_box_border,
    task_box_checked_bg,
    task_box_checked_fg,

    thematic_break,
    link_fg,

    // Tables
    table_border,
    table_cell_border,
    table_header_bg,
    table_alt_bg,

    // Caret and selection
    cursor,
    selection,
    line_highlight,

    // Interactive controls and buttons
    btn_bg,
    btn_hover,
    btn_active,
    btn_text,

    // Primary buttons (e.g. default confirmation, submit, save)
    btn_primary_bg,
    btn_primary_hover,
    btn_primary_active,
    btn_primary_text,

    // Danger / Destructive buttons (e.g. Delete, Drop, Discard)
    btn_danger_bg,
    btn_danger_hover,
    btn_danger_active,
    btn_danger_text,

    // Status & Semantic Text
    text_danger,
    text_success,
    text_warning,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_partial_theme_inherits_from_base() {
        let partial_toml = r##"
name = "Partial"

[colors]
bg_editor = "#ff0000"
"##;
        let config: registry::ThemeConfig = toml::from_str(partial_toml).unwrap();
        let theme = config.into_theme(&DEFAULT_THEME);

        assert_eq!(theme.name, "Partial");
        assert_eq!(theme.bg_editor, color::parse_hex_color("#ff0000").unwrap());

        // Unset colors fall back to the embedded default palette
        assert_eq!(theme.bg_app, DEFAULT_THEME.bg_app);
        assert_eq!(theme.text_primary, DEFAULT_THEME.text_primary);
        assert_eq!(theme.line_highlight, DEFAULT_THEME.line_highlight);
    }

    #[test]
    fn test_standalone_definition_reports_missing_colors() {
        let def: ThemeDef = toml::from_str(r##"bg_editor = "#ff0000""##).unwrap();
        let missing = def.into_standalone().unwrap_err();

        assert!(missing.contains(&"bg_app"));
        assert!(missing.contains(&"text_primary"));
        assert!(!missing.contains(&"bg_editor"));
    }
}
