use gpui::{Pixels, Rems, px};

/// Unified spacing scale based on a 4px primary grid with a 2px micro-grid.
pub struct Spacing;

impl Spacing {
    /// 0px: Reset spacing, flush alignment.
    pub const NONE: Pixels = px(0.0);
    /// 2px: Micro spacing, title/subtitle gaps, switch padding, indicator offsets.
    pub const XXS: Pixels = px(2.0);
    /// 4px: Compact element gaps, dropdown menu padding, pill vertical padding.
    pub const XS: Pixels = px(4.0);
    /// 6px: Stepper element gaps, segmented option pill spacing.
    pub const SMD: Pixels = px(6.0);
    /// 8px: Icon-label gap, sidebar item padding, floating menu padding.
    pub const SM: Pixels = px(8.0);
    /// 10px: Menu item horizontal padding, sidebar option horizontal padding.
    pub const MDS: Pixels = px(10.0);
    /// 12px: Setting card padding, group container margins.
    pub const MD: Pixels = px(12.0);
    /// 16px: Setting item horizontal gap, section gap, card outer margin.
    pub const LG: Pixels = px(16.0);
    /// 20px: Settings content panel padding, modal margins.
    pub const XL: Pixels = px(20.0);
    /// 24px: Dialog window padding (e.g. About window), section separation.
    pub const XXL: Pixels = px(24.0);
    /// 32px: Top-level view boundary separation.
    pub const SECTION: Pixels = px(32.0);
}

/// Hierarchical corner radius scale.
///
/// Ensures nested elements follow the visual hierarchy rule:
/// inner radius < container radius < window radius.
pub struct Radius;

impl Radius {
    /// 0px: Square corners, flush dividers.
    pub const NONE: Pixels = px(0.0);
    /// 3px: Micro embedded controls (e.g. Stepper decrement/increment buttons).
    pub const XS: Pixels = px(3.0);
    /// 4px: Standard interactive items (MenuItem hover, OptionPill, small button).
    pub const SM: Pixels = px(4.0);
    /// 6px: Containers & cards (SettingRow card, dropdown menu border, sidebar active).
    pub const MD: Pixels = px(6.0);
    /// 8px: Flyouts, popups, secondary floating dialogs.
    pub const LG: Pixels = px(8.0);
    /// 12px: Standalone frameless windows (About window, modal panels).
    pub const XL: Pixels = px(12.0);
    /// 14px: Large decorative squircle containers (e.g. About window app icon).
    pub const XXL: Pixels = px(14.0);
    /// 9999px: Fully rounded / capsule (Switch track and thumb, circular badges).
    pub const FULL: Pixels = px(9999.0);
}

/// Standardized typography scale with responsive rem units based on window rem size (ui_font_size).
pub struct FontSize;

impl FontSize {
    /// 11px: Captions, keyboard shortcuts, status bar secondary indicators, copyright.
    pub const CAPTION: Rems = Rems(11.0 / 13.0);
    /// 12px: Secondary text, option descriptions, sidebar labels, compact buttons.
    pub const SM: Rems = Rems(12.0 / 13.0);
    /// 13px: Standard body text, setting row titles, button labels, menu labels.
    pub const BODY: Rems = Rems(1.0);
    /// 14px: Emphasized body text, subheadings.
    pub const SUBTITLE: Rems = Rems(14.0 / 13.0);
    /// 16px: Major card title, prominent section headings.
    pub const TITLE: Rems = Rems(16.0 / 13.0);
    /// 18px: Window title (e.g. About window title, view page heading).
    pub const H2: Rems = Rems(18.0 / 13.0);
    /// 22px: Large header text.
    pub const H1: Rems = Rems(22.0 / 13.0);
    /// 28px: Hero branding typography, logo characters.
    pub const HERO: Rems = Rems(28.0 / 13.0);
}

/// Baseline-aligned line height scale paired with `FontSize`.
pub struct LineHeight;

impl LineHeight {
    /// 14px: Paired with `FontSize::CAPTION` (11px).
    pub const CAPTION: Rems = Rems(14.0 / 13.0);
    /// 16px: Paired with `FontSize::SM` (12px).
    pub const SM: Rems = Rems(16.0 / 13.0);
    /// 18px: Paired with `FontSize::BODY` (13px).
    pub const BODY: Rems = Rems(18.0 / 13.0);
    /// 20px: Paired with `FontSize::SUBTITLE` (14px).
    pub const SUBTITLE: Rems = Rems(20.0 / 13.0);
    /// 22px: Paired with `FontSize::TITLE` (16px).
    pub const TITLE: Rems = Rems(22.0 / 13.0);
    /// 24px: Paired with `FontSize::H2` (18px).
    pub const H2: Rems = Rems(24.0 / 13.0);
    /// 32px: Paired with `FontSize::HERO` (28px).
    pub const HERO: Rems = Rems(32.0 / 13.0);
}

/// Standardized ergonomic control heights for desktop UI.
pub struct ControlHeight;

impl ControlHeight {
    /// 20px: Micro embedded controls (e.g. inline file-tree rename/creation input).
    pub const XS: Pixels = px(20.0);
    /// 24px: Compact controls (Stepper buttons, small pill, tab close button).
    pub const SM: Pixels = px(24.0);
    /// 28px: Standard controls (Button, OptionPill, MenuItem row).
    pub const MD: Pixels = px(28.0);
    /// 32px: Navigation / bar height (TitleBar, StatusBar, sidebar category item).
    pub const LG: Pixels = px(32.0);
    /// 34px: Window drag title bar height.
    pub const TITLEBAR: Pixels = px(34.0);
    /// 40px: Prominent action button, search input field.
    pub const XL: Pixels = px(40.0);
}

/// Dimension metrics for toggle switch components.
pub struct SwitchMetrics;

impl SwitchMetrics {
    /// Track width: 36px.
    pub const TRACK_WIDTH: Pixels = px(36.0);
    /// Track height: 20px.
    pub const TRACK_HEIGHT: Pixels = px(20.0);
    /// Track inner padding: 2px.
    pub const TRACK_PADDING: Pixels = px(2.0);
    /// Thumb diameter: 16px.
    pub const THUMB_SIZE: Pixels = px(16.0);
    /// Thumb travel translation: 16px.
    pub const THUMB_TRAVEL: Pixels = px(16.0);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_switch_metrics_exact_geometry() {
        let expected_width = SwitchMetrics::TRACK_PADDING * 2.0
            + SwitchMetrics::THUMB_SIZE
            + SwitchMetrics::THUMB_TRAVEL;
        assert_eq!(SwitchMetrics::TRACK_WIDTH, expected_width);
    }
}
