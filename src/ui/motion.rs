pub mod cursor;
pub mod scroll;

pub use cursor::{calculate_caret_size, start_cursor_animation};
pub use scroll::{
    SMOOTH_SCROLL_DURATION, SmoothScrollPhysics, WHEEL_LINE_STEP_PX, cubic_ease_out,
    start_scroll_animation,
};
