use std::f32::consts::{FRAC_PI_2, PI, TAU};
use std::time::Duration;

use gpui::{
    Animation, AnimationExt, App, Bounds, ElementId, Hsla, IntoElement, ParentElement, Path,
    PathBuilder, Pixels, Point, RenderOnce, Styled, Window, canvas, div, point, px,
};

use crate::theme::ActiveTheme;
use crate::ui::IconSize;

#[derive(Clone)]
enum ProgressMode {
    Determinate(f32),
    Indeterminate(ElementId),
}

/// Compact, fixed-size circular ring progress indicator painted with GPUI paths.
#[derive(IntoElement)]
pub struct CircularProgress {
    mode: ProgressMode,
    size: Pixels,
    stroke_width: Pixels,
    track_color: Option<Hsla>,
    indicator_color: Option<Hsla>,
}

impl CircularProgress {
    pub fn determinate(fraction: f32) -> Self {
        Self {
            mode: ProgressMode::Determinate(sanitize_fraction(fraction)),
            size: IconSize::Small.pixels(),
            stroke_width: px(2.0),
            track_color: None,
            indicator_color: None,
        }
    }

    pub fn indeterminate(id: impl Into<ElementId>) -> Self {
        Self {
            mode: ProgressMode::Indeterminate(id.into()),
            size: IconSize::Small.pixels(),
            stroke_width: px(2.0),
            track_color: None,
            indicator_color: None,
        }
    }

    pub fn size(mut self, size: Pixels) -> Self {
        self.size = size;
        self
    }

    pub fn stroke_width(mut self, stroke_width: Pixels) -> Self {
        self.stroke_width = stroke_width;
        self
    }

    pub fn track_color(mut self, color: impl Into<Hsla>) -> Self {
        self.track_color = Some(color.into());
        self
    }

    pub fn indicator_color(mut self, color: impl Into<Hsla>) -> Self {
        self.indicator_color = Some(color.into());
        self
    }
}

pub(crate) fn sanitize_fraction(fraction: f32) -> f32 {
    if fraction.is_finite() {
        fraction.clamp(0.0, 1.0)
    } else {
        0.0
    }
}

fn circle_point(center: Point<Pixels>, radius: Pixels, angle_rad: f32) -> Point<Pixels> {
    let r = f32::from(radius);
    point(
        center.x + px(r * angle_rad.cos()),
        center.y + px(r * angle_rad.sin()),
    )
}

/// Builds a stroked circular arc from `start_angle_rad` clockwise by `sweep_rad`.
pub(crate) fn build_arc_path(
    bounds: Bounds<Pixels>,
    stroke_width: Pixels,
    start_angle_rad: f32,
    sweep_rad: f32,
) -> Option<Path<Pixels>> {
    if !sweep_rad.is_finite() || sweep_rad <= 0.001 {
        return None;
    }
    let clamped_sweep = sweep_rad.min(TAU);
    let min_dim = bounds.size.width.min(bounds.size.height);
    let radius = (min_dim - stroke_width) * 0.5;
    if radius <= px(0.5) {
        return None;
    }

    let center = point(
        bounds.origin.x + bounds.size.width * 0.5,
        bounds.origin.y + bounds.size.height * 0.5,
    );
    let radii = point(radius, radius);

    let mut builder = PathBuilder::stroke(stroke_width);
    let start_pt = circle_point(center, radius, start_angle_rad);
    builder.move_to(start_pt);

    if clamped_sweep > PI {
        let half = clamped_sweep * 0.5;
        let mid_pt = circle_point(center, radius, start_angle_rad + half);
        let end_pt = if clamped_sweep >= TAU - 0.001 {
            start_pt
        } else {
            circle_point(center, radius, start_angle_rad + clamped_sweep)
        };
        builder.arc_to(radii, px(0.0), false, true, mid_pt);
        builder.arc_to(radii, px(0.0), false, true, end_pt);
    } else {
        let end_pt = circle_point(center, radius, start_angle_rad + clamped_sweep);
        builder.arc_to(radii, px(0.0), false, true, end_pt);
    }

    builder.build().ok()
}

fn paint_ring(
    bounds: Bounds<Pixels>,
    stroke_width: Pixels,
    start_angle_rad: f32,
    sweep_rad: f32,
    track_color: Hsla,
    indicator_color: Hsla,
    window: &mut Window,
) {
    if let Some(track) = build_arc_path(bounds, stroke_width, -FRAC_PI_2, TAU) {
        window.paint_path(track, track_color);
    }
    if let Some(indicator) = build_arc_path(bounds, stroke_width, start_angle_rad, sweep_rad) {
        window.paint_path(indicator, indicator_color);
    }
}

impl RenderOnce for CircularProgress {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = cx.theme();
        let track_color = self.track_color.unwrap_or(theme.border);
        let indicator_color = self.indicator_color.unwrap_or(theme.text_accent);
        let size = self.size;
        let stroke_width = self.stroke_width;

        match self.mode {
            ProgressMode::Determinate(fraction) => div()
                .flex_none()
                .size(size)
                .child(
                    canvas(
                        |_bounds, _window, _cx| {},
                        move |bounds, (), window, _cx| {
                            paint_ring(
                                bounds,
                                stroke_width,
                                -FRAC_PI_2,
                                fraction * TAU,
                                track_color,
                                indicator_color,
                                window,
                            );
                        },
                    )
                    .size_full(),
                )
                .into_any_element(),
            ProgressMode::Indeterminate(id) => {
                let sweep = TAU * 0.28;
                div()
                    .flex_none()
                    .size(size)
                    .with_animation(
                        id,
                        Animation::new(Duration::from_millis(900)).repeat(),
                        move |_container, delta| {
                            let start_angle = -FRAC_PI_2 + delta * TAU;
                            div().size(size).child(
                                canvas(
                                    |_bounds, _window, _cx| {},
                                    move |bounds, (), window, _cx| {
                                        paint_ring(
                                            bounds,
                                            stroke_width,
                                            start_angle,
                                            sweep,
                                            track_color,
                                            indicator_color,
                                            window,
                                        );
                                    },
                                )
                                .size_full(),
                            )
                        },
                    )
                    .into_any_element()
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui::size;

    #[test]
    fn test_sanitize_fraction_clamps_and_handles_non_finite() {
        assert_eq!(sanitize_fraction(f32::NAN), 0.0);
        assert_eq!(sanitize_fraction(f32::INFINITY), 0.0);
        assert_eq!(sanitize_fraction(f32::NEG_INFINITY), 0.0);
        assert_eq!(sanitize_fraction(-0.5), 0.0);
        assert_eq!(sanitize_fraction(0.0), 0.0);
        assert_eq!(sanitize_fraction(0.42), 0.42);
        assert_eq!(sanitize_fraction(1.0), 1.0);
        assert_eq!(sanitize_fraction(2.5), 1.0);
    }

    #[test]
    fn test_build_arc_path_produces_valid_paths_across_progress_values() {
        let bounds = Bounds::new(point(px(0.0), px(0.0)), size(px(14.0), px(14.0)));
        let stroke = px(2.0);

        assert!(build_arc_path(bounds, stroke, -FRAC_PI_2, 0.0).is_none());
        for pct in [0.25_f32, 0.5, 0.75, 0.999, 1.0] {
            assert!(
                build_arc_path(bounds, stroke, -FRAC_PI_2, pct * TAU).is_some(),
                "expected valid path for fraction {pct}"
            );
        }

        // Tiny bounds smaller than stroke width should safely skip instead of panicking.
        let tiny = Bounds::new(point(px(0.0), px(0.0)), size(px(2.0), px(2.0)));
        assert!(build_arc_path(tiny, stroke, -FRAC_PI_2, TAU).is_none());
    }
}
