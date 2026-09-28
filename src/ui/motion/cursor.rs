use std::f32::consts::PI;
use std::time::{Duration, Instant};

use gpui::{Context, Pixels, Task, px};

// Cursor Animation Constants

/// Half-period interval for cursor blinking (500ms on, 500ms off -> 1.0s full cycle).
pub const BLINK_INTERVAL: Duration = Duration::from_millis(500);

/// Duration the cursor remains completely solid immediately following typing or navigation.
pub const SOLID_REST_DURATION: Duration = Duration::from_millis(200);

/// Gamma curve exponent applied to cosine phase to match human visual perception.
pub const BREATHING_EXPONENT: f32 = 1.3;

/// Animation frame stepping interval (~60 FPS).
pub const FRAME_STEP_INTERVAL: Duration = Duration::from_millis(16);

// Caret Layout Constants

/// Standard caret width across all text input surfaces.
pub const CARET_WIDTH: Pixels = px(2.0);

/// Font size multiplier for caret height calculation.
pub const CARET_HEIGHT_SCALE: f32 = 1.25;

/// Margin deducted from line height to ensure caret does not touch adjacent lines.
pub const CARET_LINE_MARGIN: Pixels = px(1.0);

/// Minimum clamped caret height to maintain visibility with small fonts.
pub const CARET_MIN_HEIGHT: Pixels = px(14.0);

/// Spawns an epoch-guarded cursor animation task supporting both smooth cosine breathing
/// and discrete 500ms step blinking.
///
/// If `get_epoch(entity) != epoch`, the task exits so older timers do not overwrite newer
/// cursor interactions.
pub fn start_cursor_animation<T: 'static>(
    cx: &mut Context<T>,
    breathing: bool,
    epoch: usize,
    get_epoch: impl Fn(&T) -> usize + 'static + Send + Sync,
    set_opacity: impl Fn(&mut T, f32, &mut Context<T>) + 'static + Send + Sync,
) -> Task<()> {
    let executor = cx.background_executor().clone();
    cx.spawn(async move |this, cx| {
        if !breathing {
            executor.timer(BLINK_INTERVAL).await;
            let mut visible = true;
            loop {
                visible = !visible;
                let opacity = if visible { 1.0 } else { 0.0 };

                let continue_loop = this
                    .update(cx, |entity, cx| {
                        if (get_epoch)(entity) == epoch {
                            set_opacity(entity, opacity, cx);
                            true
                        } else {
                            false
                        }
                    })
                    .unwrap_or(false);

                if !continue_loop {
                    break;
                }

                executor.timer(BLINK_INTERVAL).await;
            }
        } else {
            executor.timer(SOLID_REST_DURATION).await;
            let start = Instant::now();
            let half_period_secs = BLINK_INTERVAL.as_secs_f32();
            loop {
                let elapsed = start.elapsed().as_secs_f32();
                let phase = (elapsed * PI / half_period_secs).cos();
                let opacity = (0.5 * (1.0 + phase)).powf(BREATHING_EXPONENT);

                let continue_loop = this
                    .update(cx, |entity, cx| {
                        if (get_epoch)(entity) == epoch {
                            set_opacity(entity, opacity, cx);
                            true
                        } else {
                            false
                        }
                    })
                    .unwrap_or(false);

                if !continue_loop {
                    break;
                }

                executor.timer(FRAME_STEP_INTERVAL).await;
            }
        }
    })
}

/// Calculates caret `(width, height)` from font size and line height, clamped to stay within
/// line bounds.
#[inline]
pub fn calculate_caret_size(font_size: Pixels, line_height: Pixels) -> (Pixels, Pixels) {
    let max_height = (line_height - CARET_LINE_MARGIN).max(px(0.0));
    let height = (font_size * CARET_HEIGHT_SCALE)
        .min(max_height)
        .max(CARET_MIN_HEIGHT.min(max_height));
    (CARET_WIDTH, height)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_calculate_caret_size_scaling_and_clamping() {
        // 12px font, 16px line height -> 12 * 1.25 = 15px
        let (w, h) = calculate_caret_size(px(12.0), px(16.0));
        assert_eq!(w, CARET_WIDTH);
        assert_eq!(h, px(15.0));

        // Clamped to minimum height (14px) for small fonts with ample line height
        let (w, h) = calculate_caret_size(px(8.0), px(20.0));
        assert_eq!(w, CARET_WIDTH);
        assert_eq!(h, px(14.0));

        // Clamped to line_height - 1px for large fonts
        let (w, h) = calculate_caret_size(px(20.0), px(22.0));
        assert_eq!(w, CARET_WIDTH);
        assert_eq!(h, px(21.0));

        // Compact line height clamps to line_height - 1px
        let (w, h) = calculate_caret_size(px(10.0), px(12.0));
        assert_eq!(w, CARET_WIDTH);
        assert_eq!(h, px(11.0));
        assert!(h <= px(12.0));

        // Very tight line height (6px)
        let (w, h) = calculate_caret_size(px(8.0), px(6.0));
        assert_eq!(w, CARET_WIDTH);
        assert_eq!(h, px(5.0));

        // Zero line height edge case
        let (w, h) = calculate_caret_size(px(14.0), px(0.0));
        assert_eq!(w, CARET_WIDTH);
        assert_eq!(h, px(0.0));
    }
}
