//! Numeric stepper bounds and arithmetic for settings.
//!
//! Provides [`NumericBounds`] to clamp float values within an inclusive range and step
//! in fixed granular increments without accumulating floating-point drift. Also defines
//! the canonical bounds for UI font size, editor font size, and line height multipliers.

pub const UI_FONT_SIZE_BOUNDS: NumericBounds = NumericBounds::new(11.0, 20.0, 1.0);
pub const EDITOR_FONT_SIZE_BOUNDS: NumericBounds = NumericBounds::new(12.0, 28.0, 1.0);
pub const LINE_HEIGHT_BOUNDS: NumericBounds = NumericBounds::new(1.2, 2.4, 0.1);

/// An inclusive range plus the granularity a stepper moves at.
///
/// Drives both the clamp and the row description, so the advertised range can't drift away from
/// the enforced one.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct NumericBounds {
    pub min: f32,
    pub max: f32,
    pub step: f32,
}

impl NumericBounds {
    pub const fn new(min: f32, max: f32, step: f32) -> Self {
        Self { min, max, step }
    }

    /// Moves one step up or down, snapping to step multiples so repeated float addition can't
    /// accumulate error, then clamps.
    pub fn step(&self, value: f32, up: bool) -> f32 {
        let units = (value / self.step).round() + if up { 1.0 } else { -1.0 };
        (units * self.step).clamp(self.min, self.max)
    }

    pub fn describe_px(&self) -> String {
        format!("{:.0}px to {:.0}px", self.min, self.max)
    }

    pub fn describe_ratio(&self) -> String {
        format!("{:.1} to {:.1}", self.min, self.max)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_numeric_bounds_steps_within_range() {
        assert_eq!(UI_FONT_SIZE_BOUNDS.step(13.0, true), 14.0);
        assert_eq!(UI_FONT_SIZE_BOUNDS.step(13.0, false), 12.0);
        assert_eq!(EDITOR_FONT_SIZE_BOUNDS.step(15.0, true), 16.0);
    }

    #[test]
    fn test_numeric_bounds_clamps_at_limits() {
        assert_eq!(UI_FONT_SIZE_BOUNDS.step(20.0, true), 20.0);
        assert_eq!(UI_FONT_SIZE_BOUNDS.step(11.0, false), 11.0);
        assert_eq!(EDITOR_FONT_SIZE_BOUNDS.step(28.0, true), 28.0);
        assert_eq!(EDITOR_FONT_SIZE_BOUNDS.step(12.0, false), 12.0);
        assert_eq!(LINE_HEIGHT_BOUNDS.step(2.4, true), 2.4);
        assert_eq!(LINE_HEIGHT_BOUNDS.step(1.2, false), 1.2);
    }

    #[test]
    fn test_line_height_stepping_does_not_drift() {
        // 0.1 has no exact f32 representation, so naive repeated addition accumulates error.
        let mut value = 1.6;
        for _ in 0..8 {
            value = LINE_HEIGHT_BOUNDS.step(value, true);
        }
        assert_eq!(value, 2.4);

        for _ in 0..12 {
            value = LINE_HEIGHT_BOUNDS.step(value, false);
        }
        assert_eq!(value, 1.2);
    }
}
