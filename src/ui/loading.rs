use gpui::*;

/// Renders an animated metaball loading indicator.
pub fn render_loading_metaball_animation(
    id: impl Into<ElementId>,
    color: Hsla,
    size_px: Pixels,
) -> impl IntoElement {
    div()
        .size(size_px)
        .flex()
        .items_center()
        .justify_center()
        .with_animation(
            id,
            Animation::new(std::time::Duration::from_millis(750)).repeat(),
            move |_container, delta| {
                // Sine harmonic oscillation: 0.0 -> 1.0 -> 0.0
                let t = (1.0 - (delta * 2.0 * std::f32::consts::PI).cos()) / 2.0;
                let s = f32::from(size_px) / 24.0;

                let cx1 = (4.0 + 5.0 * t) * s;
                let r1 = (3.0 + 5.0 * t) * s;

                let cx2 = (15.0 + 5.0 * t) * s;
                let r2 = (8.0 - 5.0 * t) * s;

                let cy = 12.0 * s;

                let mut group = div().size(size_px).relative();

                // Circle 1
                group = group.child(
                    div()
                        .absolute()
                        .left(px(cx1 - r1))
                        .top(px(cy - r1))
                        .size(px(r1 * 2.0))
                        .rounded(px(r1))
                        .bg(color),
                );

                // Circle 2
                group = group.child(
                    div()
                        .absolute()
                        .left(px(cx2 - r2))
                        .top(px(cy - r2))
                        .size(px(r2 * 2.0))
                        .rounded(px(r2))
                        .bg(color),
                );

                // Gooey metaball connecting bridge
                let dist = cx2 - cx1;
                let max_connect_dist = (r1 + r2) * 1.35;
                if dist < max_connect_dist && dist > 0.1 {
                    let overlap = 1.0 - (dist / max_connect_dist);
                    let bridge_h = (r1.min(r2) * 1.6) * overlap;
                    group = group.child(
                        div()
                            .absolute()
                            .left(px(cx1))
                            .top(px(cy - bridge_h / 2.0))
                            .w(px(cx2 - cx1))
                            .h(px(bridge_h))
                            .rounded(px(bridge_h / 2.0))
                            .bg(color),
                    );
                }

                group
            },
        )
}
