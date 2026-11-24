pub(crate) mod cursor;
pub(crate) mod scroll;

pub use cursor::{calculate_caret_size, start_cursor_animation};
pub use scroll::{
    SMOOTH_SCROLL_DURATION, SmoothScrollController, SmoothScrollPhysics, WHEEL_LINE_STEP_PX,
    cubic_ease_out, schedule_vsync_scroll_frame, start_scroll_animation_with_event,
};
