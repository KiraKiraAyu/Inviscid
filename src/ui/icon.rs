use gpui::{App, Hsla, IntoElement, Pixels, RenderOnce, Styled, Window, px, svg};

/// Identifiers for application built-in vector icons.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub enum IconName {
    Palette,
    ImageBroken,
    Check,
    Close,
    Menu,
    Folder,
    FolderOpen,
    ChevronDown,
    ChevronRight,
    FileText,
    PanelLeft,
    Settings,
    Keyboard,
    Lock,
}

impl IconName {
    /// Returns the canonical path to this icon within the asset bundle (e.g. `icons/palette.svg`).
    pub const fn path(self) -> &'static str {
        match self {
            Self::Palette => "icons/palette.svg",
            Self::ImageBroken => "icons/image_broken.svg",
            Self::Check => "icons/check.svg",
            Self::Close => "icons/close.svg",
            Self::Menu => "icons/menu.svg",
            Self::Folder => "icons/folder.svg",
            Self::FolderOpen => "icons/folder_open.svg",
            Self::ChevronDown => "icons/chevron_down.svg",
            Self::ChevronRight => "icons/chevron_right.svg",
            Self::FileText => "icons/file_text.svg",
            Self::PanelLeft => "icons/panel_left.svg",
            Self::Settings => "icons/settings.svg",
            Self::Keyboard => "icons/keyboard.svg",
            Self::Lock => "icons/lock.svg",
        }
    }

    /// Returns a slice containing all built-in icon identifiers.
    pub const fn all() -> &'static [IconName] {
        &[
            Self::Palette,
            Self::ImageBroken,
            Self::Check,
            Self::Close,
            Self::Menu,
            Self::Folder,
            Self::FolderOpen,
            Self::ChevronDown,
            Self::ChevronRight,
            Self::FileText,
            Self::PanelLeft,
            Self::Settings,
            Self::Keyboard,
            Self::Lock,
        ]
    }

    /// Resolves an asset path to its matching `IconName` variant, if any.
    pub fn from_path(path: &str) -> Option<IconName> {
        let normalized = path.trim_start_matches('/');
        Self::all()
            .iter()
            .copied()
            .find(|&icon| icon.path() == normalized)
    }
}

/// Standardized semantic icon sizes.
#[derive(Copy, Clone, Debug, PartialEq)]
pub enum IconSize {
    /// 10px (e.g. tab close button, inline indicator)
    Indicator,
    /// 12px (e.g. status bar item, checkbox)
    XSmall,
    /// 14px (e.g. toolbar, menu item)
    Small,
    /// 16px (e.g. independent action button, section header)
    Medium,
    /// 20px (e.g. large card hero, modal banner)
    Large,
}

impl IconSize {
    pub const fn pixels(self) -> Pixels {
        match self {
            Self::Indicator => px(10.0),
            Self::XSmall => px(12.0),
            Self::Small => px(14.0),
            Self::Medium => px(16.0),
            Self::Large => px(20.0),
        }
    }
}

#[derive(Clone, IntoElement)]
pub struct Icon {
    name: IconName,
    size: IconSize,
    color: Option<Hsla>,
}

impl Icon {
    pub fn new(name: IconName) -> Self {
        Self {
            name,
            size: IconSize::Small,
            color: None,
        }
    }

    pub fn size(mut self, size: IconSize) -> Self {
        self.size = size;
        self
    }

    pub fn color(mut self, color: impl Into<Hsla>) -> Self {
        self.color = Some(color.into());
        self
    }
}

impl From<IconName> for Icon {
    fn from(name: IconName) -> Self {
        Self::new(name)
    }
}

impl RenderOnce for Icon {
    fn render(self, window: &mut Window, _cx: &mut App) -> impl IntoElement {
        let color = self.color.unwrap_or_else(|| window.text_style().color);
        svg()
            .path(self.name.path())
            .size(self.size.pixels())
            .flex_none()
            .text_color(color)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_icon_name_path_resolution() {
        for &icon in IconName::all() {
            assert_eq!(IconName::from_path(icon.path()), Some(icon));
        }
    }
}
