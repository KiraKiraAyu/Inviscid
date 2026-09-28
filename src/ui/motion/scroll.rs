use gpui::{Context, Pixels, Task, px};
use std::time::Duration;

/// Standard pixels per line step (~33.33px/line).
///
/// On Windows, GPUI emits `ScrollDelta::Lines` where 1 standard mouse wheel tick (WHEEL_DELTA 120)
/// equals `wheel_scroll_lines` (default: 3 lines).
/// Therefore, 3 lines * (100.0 / 3.0) px/line = 100.0 px per wheel tick.
pub const WHEEL_LINE_STEP_PX: f32 = 100.0 / 3.0;

/// Standard smooth scroll animation duration (160ms).
pub const SMOOTH_SCROLL_DURATION: Duration = Duration::from_millis(160);

/// Frame interval for smooth scroll interpolation (~120 FPS).
pub const SCROLL_FRAME_INTERVAL: Duration = Duration::from_millis(8);

/// Cubic ease-out interpolation curve: `1 - (1 - t)^3`.
///
/// Returns `(current_offset, is_animating)`.
#[inline]
pub fn cubic_ease_out(
    start: Pixels,
    target: Pixels,
    elapsed: Duration,
    duration: Duration,
) -> (Pixels, bool) {
    if duration.is_zero() || elapsed >= duration {
        (target, false)
    } else {
        let t = (elapsed.as_secs_f32() / duration.as_secs_f32()).clamp(0.0, 1.0);
        let one_minus_t = 1.0 - t;
        let progress = 1.0 - one_minus_t * one_minus_t * one_minus_t;
        let current = start + (target - start) * progress;
        (current, true)
    }
}

/// Smooth scroll state supporting direct 1:1 touchpad displacement and cubic ease-out wheel steps.
#[derive(Clone, Debug)]
pub struct SmoothScrollPhysics {
    pub current_scroll_top: Pixels,
    pub target_scroll_top: Pixels,
    pub anim_start_scroll_top: Pixels,
    pub anim_start_time: Option<std::time::Instant>,
    pub anim_duration: Duration,
}

impl Default for SmoothScrollPhysics {
    fn default() -> Self {
        Self::new()
    }
}

impl SmoothScrollPhysics {
    pub fn new() -> Self {
        Self {
            current_scroll_top: px(0.0),
            target_scroll_top: px(0.0),
            anim_start_scroll_top: px(0.0),
            anim_start_time: None,
            anim_duration: SMOOTH_SCROLL_DURATION,
        }
    }

    /// Direct 1:1 displacement (used for precision touchpads and thumb dragging).
    /// Bases movement on current visual position and cancels in-flight animation.
    pub fn scroll_direct(&mut self, delta: Pixels, max_scroll: Pixels) {
        self.current_scroll_top = (self.current_scroll_top - delta).clamp(px(0.0), max_scroll);
        self.target_scroll_top = self.current_scroll_top;
        self.anim_start_scroll_top = self.current_scroll_top;
        self.anim_start_time = None;
    }

    /// Sets the position directly with clamping (e.g. scrollbar thumb drag).
    pub fn set_direct(&mut self, position: Pixels, max_scroll: Pixels) {
        let clamped = position.clamp(px(0.0), max_scroll);
        self.current_scroll_top = clamped;
        self.target_scroll_top = clamped;
        self.anim_start_scroll_top = clamped;
        self.anim_start_time = None;
    }

    /// Mouse wheel impulse displacement (cubic ease-out with impulse stacking and instant reversal).
    ///
    /// Consecutive wheel steps in the same direction stack onto `target_scroll_top`. Reversing
    /// direction while an animation is in flight anchors the new target to `current_scroll_top`
    /// so turnaround is immediate.
    pub fn scroll_by_wheel(&mut self, delta: Pixels, max_scroll: Pixels) {
        let is_animating = self.anim_start_time.is_some();
        let pending = self.target_scroll_top - self.current_scroll_top;

        let base = if is_animating {
            let moving_forward = pending > px(0.5);
            let moving_backward = pending < px(-0.5);
            let impulse_forward = delta < px(-0.01); // delta < 0 increases scroll offset (down/right)
            let impulse_backward = delta > px(0.01); // delta > 0 decreases scroll offset (up/left)

            if (moving_forward && impulse_backward) || (moving_backward && impulse_forward) {
                self.current_scroll_top
            } else {
                self.target_scroll_top
            }
        } else {
            self.target_scroll_top
        };

        let new_target = (base - delta).clamp(px(0.0), max_scroll);
        if (new_target - self.current_scroll_top).abs() < px(0.5) {
            self.target_scroll_top = new_target;
            self.current_scroll_top = new_target;
            self.anim_start_scroll_top = new_target;
            self.anim_start_time = None;
        } else {
            self.target_scroll_top = new_target;
            self.anim_start_scroll_top = self.current_scroll_top;
            self.anim_start_time = Some(std::time::Instant::now());
        }
    }

    /// Programmatic target movement (e.g. scroll cursor or item into view).
    pub fn set_target_scroll_top(&mut self, target: Pixels, max_scroll: Pixels) {
        let clamped = target.clamp(px(0.0), max_scroll);
        if (clamped - self.current_scroll_top).abs() > px(0.5) {
            self.target_scroll_top = clamped;
            self.anim_start_scroll_top = self.current_scroll_top;
            self.anim_start_time = Some(std::time::Instant::now());
        } else {
            self.target_scroll_top = clamped;
            self.current_scroll_top = clamped;
            self.anim_start_scroll_top = clamped;
            self.anim_start_time = None;
        }
    }

    /// Steps cubic ease-out interpolation with custom elapsed duration (for testing & determinism).
    pub fn step_interpolation_with_elapsed(&mut self, elapsed: Duration) -> bool {
        if self.anim_start_time.is_none() {
            self.current_scroll_top = self.target_scroll_top;
            return false;
        }
        let (new_current, is_animating) = cubic_ease_out(
            self.anim_start_scroll_top,
            self.target_scroll_top,
            elapsed,
            self.anim_duration,
        );
        self.current_scroll_top = new_current;
        if !is_animating {
            self.anim_start_time = None;
        }
        is_animating
    }

    /// Performs one step of smooth scroll interpolation based on real time.
    pub fn step_interpolation(&mut self) -> bool {
        let Some(start_time) = self.anim_start_time else {
            self.current_scroll_top = self.target_scroll_top;
            return false;
        };
        self.step_interpolation_with_elapsed(start_time.elapsed())
    }

    #[inline]
    pub fn is_animating(&self) -> bool {
        self.anim_start_time.is_some()
    }
}

/// Spawns a ~120 FPS (8ms) smooth-scroll interpolation loop if not already running.
pub fn start_scroll_animation<E: 'static>(
    entity: &mut E,
    cx: &mut Context<E>,
    get_task_slot: impl Fn(&mut E) -> &mut Option<Task<()>> + Copy + 'static,
    step_frame: impl Fn(&mut E) -> bool + 'static,
) {
    if get_task_slot(entity).is_some() {
        return;
    }
    let executor = cx.background_executor().clone();
    let task = cx.spawn(async move |this, cx| {
        loop {
            executor.timer(SCROLL_FRAME_INTERVAL).await;
            let Ok(is_animating) = this.update(cx, |entity, cx| {
                let still_animating = step_frame(entity);
                cx.notify();
                still_animating
            }) else {
                break;
            };
            if !is_animating {
                break;
            }
        }
        let _ = this.update(cx, |entity, _cx| {
            *get_task_slot(entity) = None;
        });
    });
    *get_task_slot(entity) = Some(task);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cubic_ease_out_math() {
        let start = px(0.0);
        let target = px(100.0);
        let duration = SMOOTH_SCROLL_DURATION;

        // t = 0: starts at origin
        let (val, active) = cubic_ease_out(start, target, Duration::ZERO, duration);
        assert_eq!(val, px(0.0));
        assert!(active);

        // t = 80ms (halfway): 1 - (1 - 0.5)^3 = 0.875
        let (val, active) = cubic_ease_out(start, target, Duration::from_millis(80), duration);
        assert!((val - px(87.5)).abs() < px(0.01));
        assert!(active);

        // t = 160ms (duration boundary): reaches target and stops
        let (val, active) = cubic_ease_out(start, target, Duration::from_millis(160), duration);
        assert_eq!(val, px(100.0));
        assert!(!active);

        // t > 160ms: clamped to target with active = false
        let (val, active) = cubic_ease_out(start, target, Duration::from_millis(200), duration);
        assert_eq!(val, px(100.0));
        assert!(!active);

        // Zero duration edge case
        let (val, active) = cubic_ease_out(start, target, Duration::ZERO, Duration::ZERO);
        assert_eq!(val, px(100.0));
        assert!(!active);
    }

    #[test]
    fn test_smooth_scroll_physics_defaults() {
        let physics = SmoothScrollPhysics::new();
        assert_eq!(physics.current_scroll_top, px(0.0));
        assert_eq!(physics.target_scroll_top, px(0.0));
        assert!(!physics.is_animating());
        assert_eq!(physics.anim_duration, SMOOTH_SCROLL_DURATION);
    }

    #[test]
    fn test_scroll_direct_and_clamp() {
        // In GPUI: scroll_offset = current_offset - delta.
        let mut physics = SmoothScrollPhysics::new();
        let max_scroll = px(500.0);

        physics.scroll_direct(px(-150.0), max_scroll);
        assert_eq!(physics.current_scroll_top, px(150.0));
        assert_eq!(physics.target_scroll_top, px(150.0));
        assert!(!physics.is_animating());

        physics.scroll_direct(px(-500.0), max_scroll);
        assert_eq!(physics.current_scroll_top, px(500.0));

        physics.scroll_direct(px(600.0), max_scroll);
        assert_eq!(physics.current_scroll_top, px(0.0));
    }

    #[test]
    fn test_smooth_scroll_full_lifecycle() {
        let mut physics = SmoothScrollPhysics::new();
        let max_scroll = px(1000.0);

        physics.scroll_by_wheel(px(-100.0), max_scroll);
        assert_eq!(physics.target_scroll_top, px(100.0));
        assert!(physics.is_animating());

        let still_animating = physics.step_interpolation_with_elapsed(Duration::from_millis(80));
        assert!(still_animating);
        assert!(physics.is_animating());
        assert!((physics.current_scroll_top - px(87.5)).abs() < px(0.01));

        let still_animating = physics.step_interpolation_with_elapsed(Duration::from_millis(160));
        assert!(!still_animating);
        assert!(!physics.is_animating());
        assert_eq!(physics.current_scroll_top, px(100.0));
        assert_eq!(physics.target_scroll_top, px(100.0));
        assert!(physics.anim_start_time.is_none());

        let still_animating = physics.step_interpolation_with_elapsed(Duration::from_millis(200));
        assert!(!still_animating);
        assert!(!physics.is_animating());
        assert_eq!(physics.current_scroll_top, px(100.0));
    }

    #[test]
    fn test_wheel_scroll_impulse_stacking() {
        let mut physics = SmoothScrollPhysics::new();
        let max_scroll = px(1000.0);

        physics.scroll_by_wheel(px(-100.0), max_scroll);
        assert_eq!(physics.target_scroll_top, px(100.0));

        assert!(physics.step_interpolation_with_elapsed(Duration::from_millis(80)));
        assert!((physics.current_scroll_top - px(87.5)).abs() < px(0.01));

        // Second wheel notch in the same direction stacks onto target_scroll_top
        physics.scroll_by_wheel(px(-100.0), max_scroll);
        assert_eq!(physics.target_scroll_top, px(200.0));
        assert_eq!(physics.anim_start_scroll_top, physics.current_scroll_top);
        assert!(physics.is_animating());
    }

    #[test]
    fn test_wheel_scroll_instant_direction_reversal_both_ways() {
        let mut physics = SmoothScrollPhysics::new();
        let max_scroll = px(1000.0);

        // Moving forward/down, then reversing backward/up
        physics.scroll_by_wheel(px(-500.0), max_scroll);
        assert_eq!(physics.target_scroll_top, px(500.0));

        assert!(physics.step_interpolation_with_elapsed(Duration::from_millis(40)));
        let mid_pos_down = physics.current_scroll_top;
        assert!(mid_pos_down > px(100.0) && mid_pos_down < px(400.0));

        // Reversing direction anchors the new target to mid_pos_down rather than 500px
        physics.scroll_by_wheel(px(100.0), max_scroll);
        assert_eq!(physics.target_scroll_top, mid_pos_down - px(100.0));
        assert_eq!(physics.anim_start_scroll_top, mid_pos_down);
        assert!(physics.is_animating());

        // Moving backward/up, then reversing forward/down
        physics.set_direct(px(600.0), max_scroll);
        physics.scroll_by_wheel(px(500.0), max_scroll);
        assert_eq!(physics.target_scroll_top, px(100.0));

        assert!(physics.step_interpolation_with_elapsed(Duration::from_millis(40)));
        let mid_pos_up = physics.current_scroll_top;
        assert!(mid_pos_up > px(150.0) && mid_pos_up < px(550.0));

        physics.scroll_by_wheel(px(-100.0), max_scroll);
        assert_eq!(physics.target_scroll_top, mid_pos_up + px(100.0));
        assert_eq!(physics.anim_start_scroll_top, mid_pos_up);
        assert!(physics.is_animating());
    }

    #[test]
    fn test_wheel_scroll_boundary_clamping_and_noop() {
        let mut physics = SmoothScrollPhysics::new();
        let max_scroll = px(500.0);

        physics.scroll_by_wheel(px(100.0), max_scroll);
        assert_eq!(physics.current_scroll_top, px(0.0));
        assert_eq!(physics.target_scroll_top, px(0.0));
        assert!(!physics.is_animating());

        physics.set_direct(px(450.0), max_scroll);
        physics.scroll_by_wheel(px(-100.0), max_scroll);
        assert_eq!(physics.target_scroll_top, px(500.0));
        assert!(physics.is_animating());

        physics.set_direct(px(500.0), max_scroll);
        physics.scroll_by_wheel(px(-100.0), max_scroll);
        assert_eq!(physics.current_scroll_top, px(500.0));
        assert_eq!(physics.target_scroll_top, px(500.0));
        assert!(!physics.is_animating());
    }

    #[test]
    fn test_subpixel_threshold_snapping() {
        let mut physics = SmoothScrollPhysics::new();
        let max_scroll = px(500.0);
        physics.set_direct(px(100.0), max_scroll);

        // Target difference <= 0.5px snaps immediately without launching animation
        physics.set_target_scroll_top(px(100.3), max_scroll);
        assert_eq!(physics.current_scroll_top, px(100.3));
        assert_eq!(physics.target_scroll_top, px(100.3));
        assert!(!physics.is_animating());

        // Target difference > 0.5px launches smooth animation
        physics.set_target_scroll_top(px(101.0), max_scroll);
        assert_eq!(physics.current_scroll_top, px(100.3));
        assert_eq!(physics.target_scroll_top, px(101.0));
        assert!(physics.is_animating());
    }
}
