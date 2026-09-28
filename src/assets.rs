use gpui::{App, AssetSource, SharedString};
use rust_embed::RustEmbed;
use std::borrow::Cow;

#[derive(RustEmbed)]
#[folder = "assets"]
#[include = "fonts/**/*"]
#[include = "icons/**/*"]
#[include = "images/**/*"]
#[include = "themes/**/*"]
#[exclude = "*.DS_Store"]
pub struct Assets;

impl AssetSource for Assets {
    fn load(&self, path: &str) -> gpui::Result<Option<Cow<'static, [u8]>>> {
        let clean_path = path.trim_start_matches('/');
        Ok(Self::get(clean_path).map(|file| file.data))
    }

    fn list(&self, path: &str) -> gpui::Result<Vec<SharedString>> {
        let has_trailing_slash = path.ends_with('/');
        let prefix = path.trim_matches('/');

        Ok(Self::iter()
            .filter_map(|p| {
                if prefix.is_empty() {
                    Some(SharedString::from(p))
                } else if let Some(rest) = p.strip_prefix(prefix) {
                    if has_trailing_slash {
                        if rest.is_empty() || rest.starts_with('/') {
                            Some(SharedString::from(p))
                        } else {
                            None
                        }
                    } else if rest.is_empty() || rest.starts_with('/') || rest.starts_with('.') {
                        Some(SharedString::from(p))
                    } else {
                        None
                    }
                } else {
                    None
                }
            })
            .collect())
    }
}

impl Assets {
    /// Populate the [`TextSystem`] of the given context with all embedded `.ttf` and `.otf` fonts.
    pub fn load_fonts(&self, cx: &App) -> anyhow::Result<()> {
        let font_paths = self.list("fonts")?;
        let mut embedded_fonts = Vec::new();
        for font_path in font_paths {
            let lower = font_path.to_ascii_lowercase();
            if (lower.ends_with(".ttf") || lower.ends_with(".otf"))
                && let Some(font_bytes) = self.load(&font_path)?
            {
                embedded_fonts.push(font_bytes);
            }
        }
        if !embedded_fonts.is_empty() {
            cx.text_system().add_fonts(embedded_fonts)?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::IconName;

    #[test]
    fn test_all_declared_icons_exist() {
        for &icon in IconName::all() {
            assert!(
                Assets::get(icon.path()).is_some(),
                "Icon {:?} at path {} not found in Assets bundle",
                icon,
                icon.path()
            );
        }
    }

    #[test]
    fn test_no_dangling_embedded_icons() {
        for path in Assets::iter() {
            if path.starts_with("icons/") && path.ends_with(".svg") {
                assert!(
                    IconName::from_path(&path).is_some(),
                    "Dangling icon found in assets bundle with no IconName variant: {}",
                    path
                );
            }
        }
    }

    #[test]
    fn test_assets_load_all_icons() {
        let assets = Assets;
        let icon_list = assets.list("icons").unwrap();
        assert_eq!(icon_list.len(), IconName::all().len());

        for icon_path in icon_list {
            assert!(
                icon_path.starts_with("icons/"),
                "Asset path must be canonical relative path: {}",
                icon_path
            );
            let loaded = assets.load(icon_path.as_ref()).unwrap();
            assert!(loaded.is_some(), "Asset {} failed to load", icon_path);
            let loaded_bytes = loaded.unwrap();
            let content = std::str::from_utf8(&loaded_bytes).unwrap().trim();
            assert!(
                content.starts_with("<svg"),
                "Asset {} must be valid SVG",
                icon_path
            );
            assert!(
                content.ends_with("</svg>"),
                "Asset {} must be closed SVG",
                icon_path
            );
        }
    }

    #[test]
    fn test_assets_load_with_leading_slash() {
        let assets = Assets;
        let loaded = assets.load("icons/palette.svg").unwrap();
        assert!(loaded.is_some());
        assert_eq!(
            loaded.unwrap(),
            Assets::get(IconName::Palette.path()).unwrap().data
        );

        let loaded_slash = assets.load("/icons/palette.svg").unwrap();
        assert!(loaded_slash.is_some());
    }

    #[test]
    fn test_assets_list_prefix_filtering() {
        let assets = Assets;

        assert!(assets.list("").unwrap().len() >= IconName::all().len() + 5);

        assert_eq!(assets.list("icons").unwrap().len(), IconName::all().len());
        assert_eq!(assets.list("icons/").unwrap().len(), IconName::all().len());
        assert_eq!(assets.list("/icons").unwrap().len(), IconName::all().len());

        let palette = assets.list("icons/palette").unwrap();
        assert_eq!(palette.len(), 1);
        assert_eq!(palette[0], SharedString::from("icons/palette.svg"));

        let palette_exact = assets.list("icons/palette.svg").unwrap();
        assert_eq!(palette_exact.len(), 1);
        assert_eq!(palette_exact[0], SharedString::from("icons/palette.svg"));

        let check = assets.list("icons/check").unwrap();
        assert_eq!(check.len(), 1);
        assert_eq!(check[0], SharedString::from("icons/check.svg"));

        assert!(assets.list("icon").unwrap().is_empty());
        assert!(assets.list("theme").unwrap().is_empty());
        assert!(assets.list("icons/pal").unwrap().is_empty());
        assert!(assets.list("icons/palette/").unwrap().is_empty());

        let themes = assets.list("themes").unwrap();
        assert_eq!(themes.len(), 5);

        assert!(assets.list("fonts").unwrap().is_empty());
        assert!(assets.list("nonexistent").unwrap().is_empty());
    }

    #[gpui::test]
    fn test_assets_load_fonts(cx: &mut gpui::TestAppContext) {
        cx.update(|cx| {
            let res = Assets.load_fonts(cx);
            assert!(res.is_ok(), "load_fonts should succeed");
        });
    }
}
