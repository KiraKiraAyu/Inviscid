use gpui::{
    AnyElement, App, Bounds, Context, DispatchPhase, Element, ElementId, GlobalElementId, Hitbox,
    HitboxBehavior, InspectorElementId, IntoElement, LayoutId, Pixels, Point, ScrollDelta,
    ScrollHandle, ScrollWheelEvent, Task, Window, px,
};
use std::ops::{Deref, DerefMut};
use std::time::{Duration, Instant};

/// Standard pixels per line step (~33.33px/line).
///
/// On Windows, GPUI emits `ScrollDelta::Lines` where 1 standard mouse wheel tick (WHEEL_DELTA 120)
/// equals `wheel_scroll_lines` (default: 3 lines).
/// Therefore, 3 lines * (100.0 / 3.0) px/line = 100.0 px per wheel tick.
pub const WHEEL_LINE_STEP_PX: f32 = 100.0 / 3.0;

/// Legacy fixed smooth scroll duration (160ms), retained for [`cubic_ease_out`].
pub const SMOOTH_SCROLL_DURATION: Duration = Duration::from_millis(160);

/// Fallback timer interval (~120 FPS) when a scrollable entity has no attached `Window`.
pub const SCROLL_FRAME_INTERVAL: Duration = Duration::from_millis(8);

const DURATION_DIVISOR: f32 = 60.0;
const INVERSE_DELTA_RAMP_START_PX: f32 = 120.0;
const INVERSE_DELTA_RAMP_END_PX: f32 = 480.0;
const INVERSE_DELTA_MIN_DURATION: f32 = 6.0;
const INVERSE_DELTA_MAX_DURATION: f32 = 12.0;
const INVERSE_DELTA_SLOPE: f32 = (INVERSE_DELTA_MIN_DURATION - INVERSE_DELTA_MAX_DURATION)
    / (INVERSE_DELTA_RAMP_END_PX - INVERSE_DELTA_RAMP_START_PX);
const INVERSE_DELTA_OFFSET: f32 =
    INVERSE_DELTA_MAX_DURATION - INVERSE_DELTA_RAMP_START_PX * INVERSE_DELTA_SLOPE;
const MAX_PROGRAMMATIC_DURATION_SECS: f32 = 0.7;
const VELOCITY_EPSILON: f32 = 0.01;
const MIN_RETARGET_DURATION_SECS: f32 = 0.01;

/// Symmetric ease-in-out cubic bezier control points `CubicBezier(0.42, 0.0, 0.58, 1.0)`.
const EASE_IN_OUT_X1: f32 = 0.42;
const EASE_IN_OUT_Y1: f32 = 0.0;
const EASE_IN_OUT_X2: f32 = 0.58;
const EASE_IN_OUT_Y2: f32 = 1.0;

#[derive(Clone, Copy, Debug, PartialEq)]
struct CubicBezier {
    ax: f32,
    bx: f32,
    cx: f32,
    ay: f32,
    by: f32,
    cy: f32,
}

impl CubicBezier {
    const fn new(x1: f32, y1: f32, x2: f32, y2: f32) -> Self {
        let cx = 3.0 * x1;
        let bx = 3.0 * (x2 - x1) - cx;
        let ax = 1.0 - cx - bx;

        let cy = 3.0 * y1;
        let by = 3.0 * (y2 - y1) - cy;
        let ay = 1.0 - cy - by;

        Self {
            ax,
            bx,
            cx,
            ay,
            by,
            cy,
        }
    }

    const fn ease_in_out() -> Self {
        Self::new(
            EASE_IN_OUT_X1,
            EASE_IN_OUT_Y1,
            EASE_IN_OUT_X2,
            EASE_IN_OUT_Y2,
        )
    }

    /// Preserves initial velocity on retargeting by scaling the first control handle `y1 = x1 * slope`.
    fn with_initial_slope(slope: f32) -> Self {
        let slope = slope.clamp(-1000.0, 1000.0);
        Self::new(
            EASE_IN_OUT_X1,
            EASE_IN_OUT_X1 * slope,
            EASE_IN_OUT_X2,
            EASE_IN_OUT_Y2,
        )
    }

    #[inline]
    fn sample_x(&self, s: f32) -> f32 {
        ((self.ax * s + self.bx) * s + self.cx) * s
    }

    #[inline]
    fn sample_x_derivative(&self, s: f32) -> f32 {
        (3.0 * self.ax * s + 2.0 * self.bx) * s + self.cx
    }

    #[inline]
    fn sample_y(&self, s: f32) -> f32 {
        ((self.ay * s + self.by) * s + self.cy) * s
    }

    #[inline]
    fn sample_y_derivative(&self, s: f32) -> f32 {
        (3.0 * self.ay * s + 2.0 * self.by) * s + self.cy
    }

    fn solve_curve_x(&self, x: f32) -> f32 {
        let x = x.clamp(0.0, 1.0);
        let mut s = x;
        for _ in 0..6 {
            let err = self.sample_x(s) - x;
            if err.abs() < 1e-6 {
                return s;
            }
            let d = self.sample_x_derivative(s);
            if d.abs() < 1e-6 {
                break;
            }
            s = (s - err / d).clamp(0.0, 1.0);
        }

        let mut lo = 0.0_f32;
        let mut hi = 1.0_f32;
        s = x;
        for _ in 0..12 {
            let cur = self.sample_x(s);
            if (cur - x).abs() < 1e-6 {
                break;
            }
            if x > cur {
                lo = s;
            } else {
                hi = s;
            }
            s = 0.5 * (lo + hi);
        }
        s
    }

    fn evaluate(&self, t: f32) -> f32 {
        if t <= 0.0 {
            0.0
        } else if t >= 1.0 {
            1.0
        } else {
            self.sample_y(self.solve_curve_x(t))
        }
    }

    fn slope(&self, t: f32) -> f32 {
        let s = self.solve_curve_x(t);
        let dx = self.sample_x_derivative(s);
        if dx.abs() < 1e-6 {
            0.0
        } else {
            self.sample_y_derivative(s) / dx
        }
    }
}

/// Adaptive wheel animation duration using inverse-delta scaling:
/// 200ms for `<= 120px`, linearly ramping down to 100ms at `>= 480px`.
#[inline]
pub fn wheel_scroll_duration(distance_px: f32) -> Duration {
    let ticks = (INVERSE_DELTA_OFFSET + distance_px.abs() * INVERSE_DELTA_SLOPE)
        .clamp(INVERSE_DELTA_MIN_DURATION, INVERSE_DELTA_MAX_DURATION);
    Duration::from_secs_f64(f64::from(ticks) / f64::from(DURATION_DIVISOR))
}

/// Adaptive programmatic animation duration using square-root delta scaling:
/// `min(sqrt(|delta|), 0.7 * 60) / 60`.
#[inline]
pub fn programmatic_scroll_duration(distance_px: f32) -> Duration {
    let ticks = distance_px
        .abs()
        .sqrt()
        .min(MAX_PROGRAMMATIC_DURATION_SECS * DURATION_DIVISOR);
    Duration::from_secs_f64(f64::from(ticks) / f64::from(DURATION_DIVISOR))
}

/// Velocity-based upper bound on retargeted duration: `(new_delta / velocity) * 2.5`.
#[inline]
fn velocity_based_duration_bound(velocity_px_per_sec: f32, new_delta_px: f32) -> Duration {
    if new_delta_px.abs() < VELOCITY_EPSILON {
        return Duration::ZERO;
    }
    if velocity_px_per_sec.abs() < VELOCITY_EPSILON {
        return Duration::MAX;
    }
    let bound = (new_delta_px / velocity_px_per_sec) * 2.5;
    if bound < 0.0 {
        Duration::MAX
    } else {
        Duration::from_secs_f32(bound)
    }
}

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

/// Smooth scroll state driven by `CubicBezier(0.42, 0.0, 0.58, 1.0)` easing,
/// inverse-delta duration scaling for mouse wheel, delta-based duration scaling
/// for programmatic jumps, and slope-matched mid-flight retargeting with velocity-based
/// duration bounding.
#[derive(Clone, Debug)]
pub struct SmoothScrollPhysics {
    /// Lower bound in the caller's coordinate system, zero for absolute scrolling.
    pub min_scroll_top: Pixels,
    pub current_scroll_top: Pixels,
    pub target_scroll_top: Pixels,
    pub anim_start_scroll_top: Pixels,
    pub anim_start_time: Option<Instant>,
    pub anim_duration: Duration,
    max_scroll_top: Option<Pixels>,
    anim_curve: CubicBezier,
    last_step_elapsed: Duration,
}

impl Default for SmoothScrollPhysics {
    fn default() -> Self {
        Self::new()
    }
}

impl SmoothScrollPhysics {
    /// Move the coordinate origin without restarting an in-flight animation.
    pub fn compensate_layout(&mut self, delta: Pixels) {
        self.current_scroll_top += delta;
        self.target_scroll_top += delta;
        self.anim_start_scroll_top += delta;
        if let Some(max) = self.max_scroll_top.as_mut() {
            *max += delta;
        }
    }

    pub fn new() -> Self {
        Self {
            min_scroll_top: px(0.0),
            current_scroll_top: px(0.0),
            target_scroll_top: px(0.0),
            anim_start_scroll_top: px(0.0),
            anim_start_time: None,
            anim_duration: wheel_scroll_duration(100.0),
            max_scroll_top: None,
            anim_curve: CubicBezier::ease_in_out(),
            last_step_elapsed: Duration::ZERO,
        }
    }

    /// Direct 1:1 displacement (used for precision touchpads and thumb dragging).
    /// Bases movement on current visual position and cancels in-flight animation.
    pub fn scroll_direct(&mut self, delta: Pixels, max_scroll: Pixels) {
        let max_clamped = max_scroll.max(self.min_scroll_top);
        self.max_scroll_top = Some(max_clamped);
        self.current_scroll_top =
            (self.current_scroll_top - delta).clamp(self.min_scroll_top, max_clamped);
        self.target_scroll_top = self.current_scroll_top;
        self.anim_start_scroll_top = self.current_scroll_top;
        self.anim_start_time = None;
        self.anim_curve = CubicBezier::ease_in_out();
        self.last_step_elapsed = Duration::ZERO;
    }

    /// Sets the position directly with clamping (e.g. scrollbar thumb drag).
    pub fn set_direct(&mut self, position: Pixels, max_scroll: Pixels) {
        let max_clamped = max_scroll.max(self.min_scroll_top);
        self.max_scroll_top = Some(max_clamped);
        let clamped = position.clamp(self.min_scroll_top, max_clamped);
        self.current_scroll_top = clamped;
        self.target_scroll_top = clamped;
        self.anim_start_scroll_top = clamped;
        self.anim_start_time = None;
        self.anim_curve = CubicBezier::ease_in_out();
        self.last_step_elapsed = Duration::ZERO;
    }

    /// Instantaneous curve velocity at `elapsed` in pixels/second.
    fn current_velocity_px_per_sec(&self, elapsed: Duration) -> f32 {
        if self.anim_start_time.is_none() || self.anim_duration.is_zero() {
            return 0.0;
        }
        let duration_secs = self.anim_duration.as_secs_f32();
        let u = (elapsed.as_secs_f32() / duration_secs).clamp(0.0, 1.0);
        if u >= 1.0 {
            return 0.0;
        }
        let delta = f32::from(self.target_scroll_top - self.anim_start_scroll_top);
        self.anim_curve.slope(u) * (delta / duration_secs)
    }

    fn start_or_retarget(
        &mut self,
        new_target: Pixels,
        max_scroll: Pixels,
        base_duration: Duration,
        preserve_velocity: bool,
    ) {
        self.max_scroll_top = Some(max_scroll.max(self.min_scroll_top));
        let remaining = new_target - self.current_scroll_top;
        if remaining.abs() < px(0.5) {
            self.target_scroll_top = new_target;
            self.current_scroll_top = new_target;
            self.anim_start_scroll_top = new_target;
            self.anim_start_time = None;
            self.anim_curve = CubicBezier::ease_in_out();
            self.last_step_elapsed = Duration::ZERO;
            return;
        }

        let remaining_px = f32::from(remaining);
        let (new_duration, curve) = if preserve_velocity && self.anim_start_time.is_some() {
            let velocity = self.current_velocity_px_per_sec(self.last_step_elapsed);
            let bounded_duration =
                base_duration.min(velocity_based_duration_bound(velocity, remaining_px));
            if bounded_duration.as_secs_f32() < MIN_RETARGET_DURATION_SECS {
                self.target_scroll_top = new_target;
                self.current_scroll_top = new_target;
                self.anim_start_scroll_top = new_target;
                self.anim_start_time = None;
                self.anim_curve = CubicBezier::ease_in_out();
                self.last_step_elapsed = Duration::ZERO;
                return;
            }
            let new_slope = velocity * (bounded_duration.as_secs_f32() / remaining_px);
            (bounded_duration, CubicBezier::with_initial_slope(new_slope))
        } else {
            (base_duration, CubicBezier::ease_in_out())
        };

        self.target_scroll_top = new_target;
        self.anim_start_scroll_top = self.current_scroll_top;
        self.anim_duration = new_duration;
        self.anim_curve = curve;
        self.anim_start_time = Some(Instant::now());
        self.last_step_elapsed = Duration::ZERO;
    }

    /// Mouse wheel displacement using ease-in-out cubic bezier + inverse-delta duration
    /// scaling with initial-slope velocity preservation and instant direction reversal.
    pub fn scroll_by_wheel(&mut self, delta: Pixels, max_scroll: Pixels) {
        let is_animating = self.anim_start_time.is_some();
        let pending = self.target_scroll_top - self.current_scroll_top;

        let (base, is_reversal) = if is_animating {
            let moving_forward = pending > px(0.5);
            let moving_backward = pending < px(-0.5);
            let impulse_forward = delta < px(-0.01); // delta < 0 increases scroll offset (down/right)
            let impulse_backward = delta > px(0.01); // delta > 0 decreases scroll offset (up/left)

            if (moving_forward && impulse_backward) || (moving_backward && impulse_forward) {
                (self.current_scroll_top, true)
            } else {
                (self.target_scroll_top, false)
            }
        } else {
            (self.target_scroll_top, false)
        };

        let new_target =
            (base - delta).clamp(self.min_scroll_top, max_scroll.max(self.min_scroll_top));
        let distance_px = f32::from((new_target - self.current_scroll_top).abs());
        let base_duration = wheel_scroll_duration(distance_px);
        self.start_or_retarget(new_target, max_scroll, base_duration, !is_reversal);
    }

    /// Programmatic target movement using ease-in-out cubic bezier + delta-based duration scaling.
    pub fn set_target_scroll_top(&mut self, target: Pixels, max_scroll: Pixels) {
        let clamped = target.clamp(self.min_scroll_top, max_scroll.max(self.min_scroll_top));
        let distance_px = f32::from((clamped - self.current_scroll_top).abs());
        let base_duration = programmatic_scroll_duration(distance_px);
        let same_direction = self.anim_start_time.is_some()
            && f32::from(self.target_scroll_top - self.current_scroll_top)
                * f32::from(clamped - self.current_scroll_top)
                > 0.0;
        self.start_or_retarget(clamped, max_scroll, base_duration, same_direction);
    }

    /// Steps interpolation with custom elapsed duration (for testing & determinism).
    pub fn step_interpolation_with_elapsed(&mut self, elapsed: Duration) -> bool {
        if self.anim_start_time.is_none() {
            self.current_scroll_top = self.target_scroll_top;
            return false;
        }
        if self.anim_duration.is_zero() || elapsed >= self.anim_duration {
            self.current_scroll_top = self.target_scroll_top;
            self.anim_start_time = None;
            self.anim_curve = CubicBezier::ease_in_out();
            self.last_step_elapsed = Duration::ZERO;
            return false;
        }

        let u = (elapsed.as_secs_f32() / self.anim_duration.as_secs_f32()).clamp(0.0, 1.0);
        let delta = f32::from(self.target_scroll_top - self.anim_start_scroll_top);
        let next = self.anim_start_scroll_top + px(delta * self.anim_curve.evaluate(u));
        self.current_scroll_top = match self.max_scroll_top {
            Some(max) => next.clamp(self.min_scroll_top, max.max(self.min_scroll_top)),
            None => next.max(self.min_scroll_top),
        };
        self.last_step_elapsed = elapsed;
        true
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

/// Schedules a VSync-synchronized scroll animation step via `window.on_next_frame`.
///
/// Running `step_frame` at the start of `on_request_frame` guarantees that the scroll offset is
/// evaluated once per display VSync right before `window.draw` + `window.present`, eliminating
/// async timer-vs-VSync phase drift and duplicate frames.
pub fn schedule_vsync_scroll_frame<E: 'static>(
    entity: &mut E,
    window: &mut Window,
    cx: &mut Context<E>,
    get_state: impl Fn(&mut E) -> (&mut bool, &mut Option<Task<()>>) + Copy + 'static,
    step_frame: impl Fn(&mut E, &mut Context<E>) -> bool + Copy + 'static,
) {
    let (frame_scheduled, task_slot) = get_state(entity);
    *task_slot = None;
    if *frame_scheduled {
        return;
    }
    *frame_scheduled = true;
    let weak = cx.entity().downgrade();
    window.on_next_frame(move |window, cx| {
        let _ = weak.update(cx, |entity, cx| {
            let (frame_scheduled, _) = get_state(entity);
            *frame_scheduled = false;
            let still_animating = step_frame(entity, cx);
            cx.notify();
            if still_animating {
                schedule_vsync_scroll_frame(entity, window, cx, get_state, step_frame);
            }
        });
    });
}

/// Spawns a fallback timer smooth-scroll loop when no `Window` is available at the call site.
pub fn start_scroll_animation<E: 'static>(
    entity: &mut E,
    cx: &mut Context<E>,
    get_task_slot: impl Fn(&mut E) -> &mut Option<Task<()>> + Copy + 'static,
    step_frame: impl Fn(&mut E) -> bool + 'static,
) {
    start_scroll_animation_with_event(entity, cx, get_task_slot, move |entity, _cx| {
        step_frame(entity)
    });
}

pub fn start_scroll_animation_with_event<E: 'static>(
    entity: &mut E,
    cx: &mut Context<E>,
    get_task_slot: impl Fn(&mut E) -> &mut Option<Task<()>> + Copy + 'static,
    step_frame: impl Fn(&mut E, &mut Context<E>) -> bool + 'static,
) {
    if get_task_slot(entity).is_some() {
        return;
    }
    let executor = cx.background_executor().clone();
    let task = cx.spawn(async move |this, cx| {
        loop {
            executor.timer(SCROLL_FRAME_INTERVAL).await;
            let Ok(is_animating) = this.update(cx, |entity, cx| {
                let still_animating = step_frame(entity, cx);
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

/// Scroll direction axis for [`SmoothScrollController`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ScrollAxis {
    Vertical,
    Horizontal,
}

/// Reusable VSync-synchronized smooth-scroll controller for any GPUI [`ScrollHandle`] container
/// (`uniform_list`, `overflow_y_scroll`, `overflow_x_scroll`, etc.).
#[derive(Debug)]
pub struct SmoothScrollController {
    axis: ScrollAxis,
    pub physics: SmoothScrollPhysics,
    task: Option<Task<()>>,
    frame_scheduled: bool,
}

impl Deref for SmoothScrollController {
    type Target = SmoothScrollPhysics;

    #[inline]
    fn deref(&self) -> &Self::Target {
        &self.physics
    }
}

impl DerefMut for SmoothScrollController {
    #[inline]
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.physics
    }
}

impl SmoothScrollController {
    pub fn vertical() -> Self {
        Self::new(ScrollAxis::Vertical)
    }

    pub fn horizontal() -> Self {
        Self::new(ScrollAxis::Horizontal)
    }

    pub fn new(axis: ScrollAxis) -> Self {
        Self {
            axis,
            physics: SmoothScrollPhysics::new(),
            task: None,
            frame_scheduled: false,
        }
    }

    /// Cancels any active scroll animation and resets offset to `0`.
    pub fn reset(&mut self) {
        self.task = None;
        self.frame_scheduled = false;
        self.physics.set_direct(px(0.0), px(0.0));
    }

    #[inline]
    fn max_scroll_for(&self, handle: &ScrollHandle) -> Pixels {
        let max_offset = handle.max_offset();
        match self.axis {
            ScrollAxis::Vertical => max_offset.height,
            ScrollAxis::Horizontal => max_offset.width,
        }
    }

    #[inline]
    fn actual_offset_for(&self, handle: &ScrollHandle, max_scroll: Pixels) -> Pixels {
        let offset = handle.offset();
        let raw = match self.axis {
            ScrollAxis::Vertical => -offset.y,
            ScrollAxis::Horizontal => -offset.x,
        };
        raw.clamp(px(0.0), max_scroll.max(px(0.0)))
    }

    /// Synchronizes physics state with `handle` when not actively animating (e.g. after resize
    /// or `scroll_to_item`).
    pub fn sync_from_handle(&mut self, handle: &ScrollHandle) {
        let max_scroll = self.max_scroll_for(handle);
        let actual = self.actual_offset_for(handle, max_scroll);
        if !self.physics.is_animating()
            && (self.physics.current_scroll_top - actual).abs() > px(1.0)
        {
            self.physics.current_scroll_top = actual;
            self.physics.target_scroll_top = actual;
            self.physics.anim_start_scroll_top = actual;
        }
    }

    pub fn apply_to_handle(&self, handle: &ScrollHandle) {
        let current = handle.offset();
        let next = match self.axis {
            ScrollAxis::Vertical => Point::new(current.x, -self.physics.current_scroll_top),
            ScrollAxis::Horizontal => Point::new(-self.physics.current_scroll_top, current.y),
        };
        handle.set_offset(next);
    }

    fn step_and_apply(&mut self, handle: &ScrollHandle) -> bool {
        if !self.physics.is_animating() {
            return false;
        }
        let still_animating = self.physics.step_interpolation();
        self.apply_to_handle(handle);
        still_animating
    }

    /// Handles a wheel/touchpad event on a `ScrollHandle`-backed view.
    pub fn handle_wheel<E: 'static>(
        entity: &mut E,
        event: &ScrollWheelEvent,
        window: Option<&mut Window>,
        cx: &mut Context<E>,
        access: impl Fn(&mut E) -> (&mut Self, ScrollHandle) + Copy + 'static,
    ) {
        Self::handle_wheel_with(entity, event, window, cx, access, |_, _| {});
    }

    /// Like [`Self::handle_wheel`], invoking `after_step` after each scroll offset change.
    pub fn handle_wheel_with<E: 'static>(
        entity: &mut E,
        event: &ScrollWheelEvent,
        window: Option<&mut Window>,
        cx: &mut Context<E>,
        access: impl Fn(&mut E) -> (&mut Self, ScrollHandle) + Copy + 'static,
        after_step: impl Fn(&mut E, &mut Context<E>) + Copy + 'static,
    ) {
        let (max_scroll, axis) = {
            let (ctrl, handle) = access(entity);
            let max_scroll = ctrl.max_scroll_for(&handle);
            if max_scroll <= px(0.0) {
                return;
            }
            ctrl.sync_from_handle(&handle);
            (max_scroll, ctrl.axis)
        };

        match event.delta {
            ScrollDelta::Pixels(p) => {
                let delta = match axis {
                    ScrollAxis::Vertical => p.y,
                    ScrollAxis::Horizontal => {
                        if p.x.abs() > px(0.01) {
                            p.x
                        } else {
                            p.y
                        }
                    }
                };
                if delta.abs() > px(0.01) {
                    {
                        let (ctrl, handle) = access(entity);
                        ctrl.task = None;
                        ctrl.physics.scroll_direct(delta, max_scroll);
                        ctrl.apply_to_handle(&handle);
                    }
                    after_step(entity, cx);
                    cx.notify();
                }
            }
            ScrollDelta::Lines(l) => {
                let delta_lines = match axis {
                    ScrollAxis::Vertical => l.y,
                    ScrollAxis::Horizontal => {
                        if l.x.abs() > 0.01 {
                            l.x
                        } else {
                            l.y
                        }
                    }
                };
                let delta = px(delta_lines * WHEEL_LINE_STEP_PX);
                if delta.abs() > px(0.01) {
                    {
                        let (ctrl, _) = access(entity);
                        ctrl.physics.scroll_by_wheel(delta, max_scroll);
                    }
                    if let Some(window) = window {
                        Self::schedule_on_window(entity, window, cx, access, after_step);
                    } else {
                        Self::schedule_fallback(entity, cx, access, after_step);
                    }
                    cx.notify();
                }
            }
        }
    }

    /// Resumes or promotes an active scroll animation to VSync during `render`.
    pub fn continue_on_window<E: 'static>(
        entity: &mut E,
        window: &mut Window,
        cx: &mut Context<E>,
        access: impl Fn(&mut E) -> (&mut Self, ScrollHandle) + Copy + 'static,
    ) {
        Self::continue_on_window_with(entity, window, cx, access, |_, _| {});
    }

    /// Like [`Self::continue_on_window`], invoking `after_step` after each VSync step.
    pub fn continue_on_window_with<E: 'static>(
        entity: &mut E,
        window: &mut Window,
        cx: &mut Context<E>,
        access: impl Fn(&mut E) -> (&mut Self, ScrollHandle) + Copy + 'static,
        after_step: impl Fn(&mut E, &mut Context<E>) + Copy + 'static,
    ) {
        if access(entity).0.physics.is_animating() {
            Self::schedule_on_window(entity, window, cx, access, after_step);
        }
    }

    fn schedule_on_window<E: 'static>(
        entity: &mut E,
        window: &mut Window,
        cx: &mut Context<E>,
        access: impl Fn(&mut E) -> (&mut Self, ScrollHandle) + Copy + 'static,
        after_step: impl Fn(&mut E, &mut Context<E>) + Copy + 'static,
    ) {
        schedule_vsync_scroll_frame(
            entity,
            window,
            cx,
            move |entity| {
                let (ctrl, _) = access(entity);
                (&mut ctrl.frame_scheduled, &mut ctrl.task)
            },
            move |entity, cx| {
                let still_animating = {
                    let (ctrl, handle) = access(entity);
                    ctrl.step_and_apply(&handle)
                };
                after_step(entity, cx);
                still_animating
            },
        );
    }

    fn schedule_fallback<E: 'static>(
        entity: &mut E,
        cx: &mut Context<E>,
        access: impl Fn(&mut E) -> (&mut Self, ScrollHandle) + Copy + 'static,
        after_step: impl Fn(&mut E, &mut Context<E>) + Copy + 'static,
    ) {
        if access(entity).0.frame_scheduled {
            return;
        }
        start_scroll_animation_with_event(
            entity,
            cx,
            move |entity| &mut access(entity).0.task,
            move |entity, cx| {
                let still_animating = {
                    let (ctrl, handle) = access(entity);
                    ctrl.step_and_apply(&handle)
                };
                after_step(entity, cx);
                still_animating
            },
        );
    }

    /// Wraps a `ScrollHandle`-backed element (`uniform_list`, `overflow_*_scroll`, etc.) so
    /// scroll-wheel events are intercepted in `Bubble` phase *before* GPUI's built-in
    /// `Div::paint_scroll_listener` (which is registered at the end of `child.paint` and would
    /// otherwise run first in reverse `Bubble` order and teleport the scroll offset).
    pub fn wrap<E: 'static>(
        child: impl IntoElement,
        cx: &Context<E>,
        access: impl Fn(&mut E) -> (&mut Self, ScrollHandle) + Copy + 'static,
    ) -> AnyElement {
        Self::wrap_with(child, cx, access, |_, _| {})
    }

    /// Like [`Self::wrap`], invoking `after_step` after each scroll offset update.
    pub fn wrap_with<E: 'static>(
        child: impl IntoElement,
        cx: &Context<E>,
        access: impl Fn(&mut E) -> (&mut Self, ScrollHandle) + Copy + 'static,
        after_step: impl Fn(&mut E, &mut Context<E>) + Copy + 'static,
    ) -> AnyElement {
        let on_wheel = cx.listener(move |this, event: &ScrollWheelEvent, window, cx| {
            cx.stop_propagation();
            Self::handle_wheel_with(this, event, Some(window), cx, access, after_step);
        });
        SmoothScrollBox {
            child: child.into_any_element(),
            on_wheel: Some(Box::new(on_wheel)),
        }
        .into_any_element()
    }
}

struct SmoothScrollBox {
    child: AnyElement,
    on_wheel: Option<Box<dyn Fn(&ScrollWheelEvent, &mut Window, &mut App) + 'static>>,
}

impl IntoElement for SmoothScrollBox {
    type Element = Self;

    fn into_element(self) -> Self::Element {
        self
    }
}

impl Element for SmoothScrollBox {
    type RequestLayoutState = ();
    type PrepaintState = Hitbox;

    fn id(&self) -> Option<ElementId> {
        None
    }

    fn source_location(&self) -> Option<&'static core::panic::Location<'static>> {
        None
    }

    fn request_layout(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, Self::RequestLayoutState) {
        (self.child.request_layout(window, cx), ())
    }

    fn prepaint(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        _request_layout: &mut Self::RequestLayoutState,
        window: &mut Window,
        cx: &mut App,
    ) -> Self::PrepaintState {
        let hitbox = window.insert_hitbox(bounds, HitboxBehavior::Normal);
        self.child.prepaint(window, cx);
        hitbox
    }

    fn paint(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&InspectorElementId>,
        _bounds: Bounds<Pixels>,
        _request_layout: &mut Self::RequestLayoutState,
        hitbox: &mut Self::PrepaintState,
        window: &mut Window,
        cx: &mut App,
    ) {
        // Paint `child` first so our `Bubble` listener sits *after* `child`'s built-in
        // `paint_scroll_listener` in `mouse_listeners` and executes *before* it in `.rev()`.
        self.child.paint(window, cx);
        let hitbox = hitbox.clone();
        if let Some(on_wheel) = self.on_wheel.take() {
            window.on_mouse_event(move |event: &ScrollWheelEvent, phase, window, cx| {
                if phase == DispatchPhase::Bubble && hitbox.should_handle_scroll(window) {
                    on_wheel(event, window, cx);
                }
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn layout_compensation_preserves_animation_progress() {
        let mut scroll = SmoothScrollPhysics::new();
        scroll.set_direct(px(100.0), px(1000.0));
        scroll.scroll_by_wheel(px(-200.0), px(1000.0));
        scroll.step_interpolation_with_elapsed(Duration::from_millis(40));
        let time = scroll.anim_start_time;
        let current = scroll.current_scroll_top;
        scroll.compensate_layout(px(53.5));
        assert_eq!(scroll.current_scroll_top, current + px(53.5));
        assert_eq!(scroll.target_scroll_top, px(353.5));
        assert_eq!(scroll.anim_start_scroll_top, px(153.5));
        assert_eq!(scroll.anim_start_time, time);
        scroll.step_interpolation_with_elapsed(Duration::from_millis(40));
        assert_eq!(scroll.current_scroll_top, current + px(53.5));
    }

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
        assert_eq!(physics.anim_duration, wheel_scroll_duration(100.0));
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
        let duration = physics.anim_duration;
        assert_eq!(duration, Duration::from_millis(200));

        // Symmetric EaseInOut(0.42, 0.0, 0.58, 1.0): exact 50% progress at 50% duration.
        let still_animating = physics.step_interpolation_with_elapsed(Duration::from_millis(100));
        assert!(still_animating);
        assert!(physics.is_animating());
        assert!((physics.current_scroll_top - px(50.0)).abs() < px(0.01));

        let still_animating = physics.step_interpolation_with_elapsed(duration);
        assert!(!still_animating);
        assert!(!physics.is_animating());
        assert_eq!(physics.current_scroll_top, px(100.0));
        assert_eq!(physics.target_scroll_top, px(100.0));
        assert!(physics.anim_start_time.is_none());

        let still_animating = physics.step_interpolation_with_elapsed(Duration::from_millis(300));
        assert!(!still_animating);
        assert!(!physics.is_animating());
        assert_eq!(physics.current_scroll_top, px(100.0));
    }

    #[test]
    fn test_inverse_delta_wheel_duration_and_retargeting() {
        assert_eq!(wheel_scroll_duration(100.0), Duration::from_millis(200));
        assert_eq!(wheel_scroll_duration(120.0), Duration::from_millis(200));
        assert_eq!(wheel_scroll_duration(300.0), Duration::from_millis(150));
        assert_eq!(wheel_scroll_duration(480.0), Duration::from_millis(100));
        assert_eq!(wheel_scroll_duration(900.0), Duration::from_millis(100));

        let mut physics = SmoothScrollPhysics::new();
        let max_scroll = px(1000.0);

        physics.scroll_by_wheel(px(-100.0), max_scroll);
        assert_eq!(physics.target_scroll_top, px(100.0));

        assert!(physics.step_interpolation_with_elapsed(Duration::from_millis(80)));
        let mid_pos = physics.current_scroll_top;
        let v_before = physics.current_velocity_px_per_sec(Duration::from_millis(80));
        assert!(v_before > 0.0);

        // Second wheel notch in the same direction preserves instantaneous velocity via
        // initial-slope control point scaling and completes cleanly at 200px.
        physics.scroll_by_wheel(px(-100.0), max_scroll);
        assert_eq!(physics.target_scroll_top, px(200.0));
        assert_eq!(physics.anim_start_scroll_top, mid_pos);
        assert!(physics.is_animating());

        let v_after = physics.current_velocity_px_per_sec(Duration::ZERO);
        assert!(
            (v_after - v_before).abs() < 2.0,
            "velocity jumped across retarget: before={v_before}, after={v_after}"
        );

        let mut prev = mid_pos;
        for step_ms in (8..=300).step_by(8) {
            let active = physics.step_interpolation_with_elapsed(Duration::from_millis(step_ms));
            assert!(
                physics.current_scroll_top >= prev && physics.current_scroll_top <= px(200.0),
                "non-monotonic or overshoot at {step_ms}ms: {:?} -> {:?}",
                prev,
                physics.current_scroll_top
            );
            prev = physics.current_scroll_top;
            if !active {
                break;
            }
        }
        assert_eq!(physics.current_scroll_top, px(200.0));
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

    #[gpui::test]
    fn test_smooth_scroll_controller_vertical_and_horizontal(cx: &mut gpui::TestAppContext) {
        use gpui::prelude::*;
        use gpui::{Entity, div, point};

        struct ScrollHost {
            handle: ScrollHandle,
            ctrl: SmoothScrollController,
        }

        impl Render for ScrollHost {
            fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
                SmoothScrollController::continue_on_window(self, window, cx, |h| {
                    (&mut h.ctrl, h.handle.clone())
                });
                let is_vert = self.ctrl.axis == ScrollAxis::Vertical;
                SmoothScrollController::wrap(
                    div()
                        .id("scroll_host")
                        .w(px(100.0))
                        .h(px(100.0))
                        .track_scroll(&self.handle)
                        .when(is_vert, |d| d.overflow_y_scroll())
                        .when(!is_vert, |d| d.overflow_x_scroll())
                        .child(div().w(px(400.0)).h(px(400.0))),
                    cx,
                    |h| (&mut h.ctrl, h.handle.clone()),
                )
            }
        }

        let (vert_view, cx): (Entity<ScrollHost>, _) = cx.add_window_view(|_, _| ScrollHost {
            handle: ScrollHandle::new(),
            ctrl: SmoothScrollController::vertical(),
        });
        cx.run_until_parked();

        // Dispatch a real window ScrollWheelEvent through GPUI's mouse dispatch pipeline and
        // verify Div::paint_scroll_listener is intercepted *before* it teleports `handle.offset()`.
        cx.simulate_event(ScrollWheelEvent {
            position: point(px(50.0), px(50.0)),
            delta: ScrollDelta::Lines(point(0.0, -3.0)),
            ..Default::default()
        });
        vert_view.read_with(cx, |host, _| {
            assert_eq!(
                host.handle.offset().y,
                px(0.0),
                "GPUI built-in scroll listener must not teleport offset before smooth animation"
            );
            assert_eq!(host.ctrl.target_scroll_top, px(100.0));
            assert!(host.ctrl.is_animating());
        });
        vert_view.update(cx, |host, _| {
            host.ctrl.reset();
            host.handle.set_offset(Point::default());
        });
        cx.run_until_parked();

        vert_view.update(cx, |host, cx| {
            let max_h = host.handle.max_offset().height;
            assert!(max_h > px(0.0), "expected vertical overflow");

            // Touchpad pixel delta applies 1:1 immediately
            let pixel_ev = ScrollWheelEvent {
                delta: ScrollDelta::Pixels(point(px(0.0), px(-40.0))),
                ..Default::default()
            };
            SmoothScrollController::handle_wheel(host, &pixel_ev, None, cx, |h| {
                (&mut h.ctrl, h.handle.clone())
            });
            assert_eq!(host.ctrl.current_scroll_top, px(40.0));
            assert_eq!(host.handle.offset().y, px(-40.0));

            // Wheel line delta starts ease-in-out smooth scroll animation
            let line_ev = ScrollWheelEvent {
                delta: ScrollDelta::Lines(point(0.0, -3.0)),
                ..Default::default()
            };
            SmoothScrollController::handle_wheel(host, &line_ev, None, cx, |h| {
                (&mut h.ctrl, h.handle.clone())
            });
            assert_eq!(host.ctrl.target_scroll_top, px(140.0));
            assert!(host.ctrl.is_animating());

            host.ctrl.reset();
            assert!(!host.ctrl.is_animating());
            assert_eq!(host.ctrl.current_scroll_top, px(0.0));
        });

        // Verify horizontal controller uses X axis and falls back to Y wheel delta
        let mut cx2 = gpui::TestAppContext::single();
        let (horiz_view, cx2): (Entity<ScrollHost>, _) = cx2.add_window_view(|_, _| ScrollHost {
            handle: ScrollHandle::new(),
            ctrl: SmoothScrollController::horizontal(),
        });
        cx2.run_until_parked();

        horiz_view.update(cx2, |host, cx| {
            let max_w = host.handle.max_offset().width;
            assert!(max_w > px(0.0), "expected horizontal overflow");

            let wheel_ev = ScrollWheelEvent {
                delta: ScrollDelta::Lines(point(0.0, -3.0)),
                ..Default::default()
            };
            SmoothScrollController::handle_wheel(host, &wheel_ev, None, cx, |h| {
                (&mut h.ctrl, h.handle.clone())
            });
            assert_eq!(host.ctrl.target_scroll_top, px(100.0));
            assert!(host.ctrl.is_animating());
        });
    }
}
