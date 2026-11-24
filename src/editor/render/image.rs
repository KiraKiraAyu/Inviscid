use gpui::*;

use crate::buffer::SourceLine;
use crate::editor::LineRenderContext;
use crate::editor::geometry::{IMAGE_CARD_MARGIN_Y, IMAGE_SOURCE_GAP, LineTextSpec};
use crate::editor::shaping::render_shaped_line;
use crate::markdown::ParsedLine;
use crate::theme::Theme;
use crate::ui::{Icon, IconName, IconSize};

pub(super) fn render_image_line(
    source_line: SourceLine,
    parsed: &ParsedLine,
    lrc: &LineRenderContext,
    spec: &LineTextSpec,
    alt: &str,
    url: &str,
) -> AnyElement {
    let selection = lrc.selection;
    let is_selected_line = if !selection.is_empty() {
        let (start, end) = selection.range();
        source_line >= start.line && source_line <= end.line
    } else {
        false
    };

    let should_show_source = lrc.is_active_line || is_selected_line;

    let mut img_lrc = *lrc;
    img_lrc.is_active_line = should_show_source;
    img_lrc.is_disclosure_line = should_show_source;

    let text_canvas = render_shaped_line(
        source_line,
        parsed,
        &img_lrc,
        spec.font_size,
        spec.line_height,
        Some(lrc.theme.text_secondary),
        spec.shaping_mode,
    );

    let preview_card = render_image_preview_card(
        source_line,
        alt,
        url,
        lrc.doc_path,
        lrc.theme,
        lrc.content_width,
    );

    if should_show_source {
        div()
            .w_full()
            .pt(spec.padding_top)
            .pb(spec.padding_bottom)
            .flex()
            .flex_col()
            .gap(IMAGE_SOURCE_GAP)
            .child(div().w_full().h(spec.line_height).child(text_canvas))
            .child(preview_card)
            .into_any_element()
    } else {
        div()
            .w_full()
            .pt(spec.padding_top)
            .pb(spec.padding_bottom)
            .flex()
            .flex_col()
            .child(div().h(px(0.0)).overflow_hidden().child(text_canvas))
            .child(preview_card)
            .into_any_element()
    }
}

pub fn resolve_image_source(url: &str, doc_path: Option<&std::path::Path>) -> ImageSource {
    let clean = url.trim();
    if let Some(path) = crate::http::parse_file_url_path(clean, doc_path) {
        ImageSource::Resource(Resource::Path(path.into()))
    } else {
        ImageSource::Resource(Resource::Uri(clean.to_string().into()))
    }
}

fn render_image_preview_card(
    source_line: SourceLine,
    alt: &str,
    url: &str,
    doc_path: Option<&std::path::Path>,
    theme: &Theme,
    content_width: Pixels,
) -> impl IntoElement {
    let line_idx = source_line.index();
    let alt_string = if alt.is_empty() {
        "Image".to_string()
    } else {
        alt.to_string()
    };
    let image_source = resolve_image_source(url, doc_path);
    let accent_color = theme.text_accent;
    let muted_color = theme.text_muted;

    let img_element = img(image_source)
        .id(ElementId::NamedInteger(
            "editor_img".into(),
            line_idx as u64,
        ))
        .w(content_width)
        .max_w_full()
        .rounded(px(6.0))
        .with_loading(move || {
            div()
                .w_full()
                .h(px(140.0))
                .flex()
                .items_center()
                .justify_center()
                .child(render_loading_metaball_animation(
                    ElementId::NamedInteger("loading_metaball".into(), line_idx as u64),
                    accent_color,
                    px(48.0),
                ))
                .into_any_element()
        })
        .with_fallback(move || {
            div()
                .w_full()
                .h(px(40.0))
                .flex()
                .items_center()
                .justify_center()
                .gap(px(6.0))
                .text_size(px(12.0))
                .text_color(muted_color)
                .child(
                    Icon::new(IconName::ImageBroken)
                        .size(IconSize::Small)
                        .color(muted_color),
                )
                .child(format!("Image failed to load: {}", alt_string))
                .into_any_element()
        });

    div()
        .w_full()
        .my(IMAGE_CARD_MARGIN_Y)
        .flex()
        .flex_col()
        .items_center()
        .child(img_element)
}

/// Renders an animated loading indicator.
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
