use gpui::prelude::FluentBuilder;
use gpui::*;

use crate::buffer::{LineSource, SourceLine};
use crate::editor::geometry::mermaid_preview_chrome;
use crate::editor::shaping::render_shaped_line;
use crate::editor::{Editor, LineRenderContext, geometry::LineTextSpec};
use crate::markdown::{BlockKind, ParsedLine};
use crate::ui::{Icon, IconName, IconSize};

use super::quote_text_color;

pub(super) fn render_heading_line(
    source_line: SourceLine,
    parsed: &ParsedLine,
    lrc: &LineRenderContext,
    spec: &LineTextSpec,
) -> AnyElement {
    let text_canvas = render_shaped_line(
        source_line,
        parsed,
        lrc,
        spec.font_size,
        spec.line_height,
        spec.override_color,
        spec.shaping_mode,
    );

    div()
        .w_full()
        .pl(lrc.indent_px)
        .pt(spec.padding_top)
        .pb(spec.padding_bottom)
        .child(text_canvas)
        .into_any_element()
}

pub(super) fn render_code_or_mermaid_fence(
    source_line: SourceLine,
    parsed: &ParsedLine,
    lrc: &LineRenderContext,
    spec: &LineTextSpec,
    is_fence_start: bool,
) -> AnyElement {
    let line_idx = source_line.index();
    let theme = lrc.theme;
    let is_active = lrc.is_active_line;
    let is_focused = lrc.is_focused;

    if matches!(parsed.kind, BlockKind::MermaidBlock { .. }) && !lrc.is_mermaid_block_editing {
        if is_fence_start {
            if let Some(preview) = lrc.mermaid_preview {
                let (width, height) =
                    crate::mermaid::display_size(preview, lrc.content_width, lrc.indent_px);
                let (mt, py) = mermaid_preview_chrome(lrc.font_size);
                return div()
                    .w_full()
                    .mt(mt)
                    .pl(lrc.indent_px)
                    .py(py)
                    .flex()
                    .justify_center()
                    .child(
                        img(preview.asset_path.clone())
                            .id(ElementId::NamedInteger(
                                "mermaid_img".into(),
                                line_idx as u64,
                            ))
                            .w(width)
                            .h(height),
                    )
                    .into_any_element();
            }
        } else {
            return div().w_full().h(px(0.0)).into_any_element();
        }
    }

    if is_fence_start {
        let lang_canvas = render_shaped_line(
            source_line,
            parsed,
            lrc,
            spec.font_size,
            spec.line_height,
            Some(theme.code_block_header_fg),
            spec.shaping_mode,
        );

        let fence_line = if parsed.quote.is_some() {
            let prefix_len = crate::markdown::quote_prefix_len(&parsed.raw_text);
            parsed.raw_text[prefix_len..].trim_start()
        } else {
            parsed.raw_text.trim_start()
        };
        let raw_lang = fence_line.strip_prefix("```").unwrap_or("").trim();

        div()
            .w_full()
            .mt(spec.margin_top)
            .bg(theme.code_block_header_bg)
            .rounded_t(px(10.0))
            .px(spec.padding_x)
            .pt(spec.padding_top)
            .pb(spec.padding_bottom)
            .flex()
            .justify_between()
            .items_center()
            .child(div().flex().items_center().gap(px(6.0)).child(
                if raw_lang.is_empty() && !(is_active && is_focused) {
                    div()
                        .text_size(spec.font_size)
                        .font_family(crate::platform::platform_monospace_font())
                        .text_color(theme.text_muted)
                        .child("plaintext")
                        .into_any_element()
                } else {
                    div().min_w(px(50.0)).child(lang_canvas).into_any_element()
                },
            ))
            .into_any_element()
    } else {
        div()
            .w_full()
            .h(spec.fixed_height.unwrap_or(px(6.0)))
            .mb(spec.margin_bottom)
            .bg(theme.code_block_bg)
            .rounded_b(px(10.0))
            .into_any_element()
    }
}

pub(super) fn render_code_or_mermaid_content(
    source_line: SourceLine,
    parsed: &ParsedLine,
    lrc: &LineRenderContext,
    spec: &LineTextSpec,
) -> AnyElement {
    let theme = lrc.theme;
    if matches!(parsed.kind, BlockKind::MermaidBlockContent) && !lrc.is_mermaid_block_editing {
        return div().w_full().h(px(0.0)).into_any_element();
    }
    let text_canvas = render_shaped_line(
        source_line,
        parsed,
        lrc,
        spec.font_size,
        spec.line_height,
        Some(theme.code_block_text),
        spec.shaping_mode,
    );

    div()
        .w_full()
        .bg(theme.code_block_bg)
        .px(spec.padding_x)
        .pt(spec.padding_top)
        .pb(spec.padding_bottom)
        .child(text_canvas)
        .into_any_element()
}

pub(super) fn render_thematic_break_line(
    lrc: &LineRenderContext,
    spec: &LineTextSpec,
) -> AnyElement {
    let theme = lrc.theme;
    div()
        .w_full()
        .h(spec.fixed_height.unwrap_or(px(32.0)))
        .flex()
        .flex_col()
        .justify_center()
        .child(div().w_full().h(px(1.5)).rounded(px(1.0)).bg(
            if lrc.is_active_line && lrc.is_focused {
                theme.text_accent
            } else {
                theme.thematic_break
            },
        ))
        .into_any_element()
}

pub(super) fn render_task_list_line(
    source_line: SourceLine,
    parsed: &ParsedLine,
    lrc: &LineRenderContext,
    spec: &LineTextSpec,
    checked: bool,
    editor: Option<&Entity<Editor>>,
) -> AnyElement {
    let line_idx = source_line.index();
    let theme = lrc.theme;
    let text_canvas = render_shaped_line(
        source_line,
        parsed,
        lrc,
        spec.font_size,
        spec.line_height,
        if checked {
            Some(theme.text_muted)
        } else {
            quote_text_color(parsed, theme)
        },
        spec.shaping_mode,
    );

    let marker = div()
        .id(ElementId::NamedInteger(
            "task_check".into(),
            line_idx as u64,
        ))
        .w(px(16.0))
        .h(px(16.0))
        .rounded(px(4.0))
        .border_1()
        .border_color(if checked {
            theme.task_box_checked_bg
        } else {
            theme.task_box_border
        })
        .bg(if checked {
            theme.task_box_checked_bg
        } else {
            transparent_black()
        })
        .flex()
        .items_center()
        .justify_center()
        .cursor_pointer()
        .when_some(editor, |this, ed| {
            let ed = ed.clone();
            this.on_mouse_down(MouseButton::Left, move |_, _window, cx| {
                ed.update(cx, |this, cx| {
                    this.toggle_task_checkbox(source_line, cx);
                });
            })
        })
        .child(if checked {
            Icon::new(IconName::Check)
                .size(IconSize::Indicator)
                .color(theme.task_box_checked_fg)
                .into_any_element()
        } else {
            div().into_any_element()
        });

    render_list_item_row(
        marker,
        spec.marker_width,
        text_canvas,
        spec.line_height,
        spec.padding_top,
        spec.padding_bottom,
        lrc.indent_px,
    )
}

pub(super) fn render_bullet_list_line(
    source_line: SourceLine,
    parsed: &ParsedLine,
    lrc: &LineRenderContext,
    spec: &LineTextSpec,
) -> AnyElement {
    let theme = lrc.theme;
    let text_canvas = render_shaped_line(
        source_line,
        parsed,
        lrc,
        spec.font_size,
        spec.line_height,
        quote_text_color(parsed, theme),
        spec.shaping_mode,
    );

    let marker = div()
        .w(px(5.5))
        .h(px(5.5))
        .rounded(px(3.0))
        .bg(theme.list_marker);

    render_list_item_row(
        marker,
        spec.marker_width,
        text_canvas,
        spec.line_height,
        spec.padding_top,
        spec.padding_bottom,
        lrc.indent_px,
    )
}

pub(super) fn render_ordered_list_line(
    source_line: SourceLine,
    parsed: &ParsedLine,
    lrc: &LineRenderContext,
    spec: &LineTextSpec,
    num: usize,
) -> AnyElement {
    let theme = lrc.theme;
    let num_label = format!("{}.", num);
    let text_canvas = render_shaped_line(
        source_line,
        parsed,
        lrc,
        spec.font_size,
        spec.line_height,
        quote_text_color(parsed, theme),
        spec.shaping_mode,
    );

    let marker = div()
        .text_size(px((lrc.font_size * (14.0 / 15.0)).round()))
        .text_color(theme.list_marker)
        .child(num_label);

    render_list_item_row(
        marker,
        spec.marker_width,
        text_canvas,
        spec.line_height,
        spec.padding_top,
        spec.padding_bottom,
        lrc.indent_px,
    )
}

pub(super) fn render_math_block_start_line(
    source_line: SourceLine,
    parsed: &ParsedLine,
    lines: &(impl LineSource + ?Sized),
    lrc: &LineRenderContext,
    spec: &LineTextSpec,
) -> AnyElement {
    let theme = lrc.theme;
    if lrc.is_math_block_editing {
        return div().w_full().h(px(0.0)).into_any_element();
    }
    if parsed.spans.iter().any(|span| span.math.is_some()) {
        let text_canvas = render_shaped_line(
            source_line,
            parsed,
            lrc,
            spec.font_size,
            spec.line_height,
            quote_text_color(parsed, theme),
            spec.shaping_mode,
        );

        return div()
            .w_full()
            .pl(lrc.indent_px)
            .pt(spec.padding_top)
            .pb(spec.padding_bottom)
            .child(text_canvas)
            .into_any_element();
    }

    let (raw_tex, render_error) =
        crate::math::resolve_math_block_fallback(lines, source_line, None);
    let collapsed_tex = raw_tex
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .collect::<Vec<_>>()
        .join(" ");
    let display_tex = if collapsed_tex.is_empty() {
        "$$".to_string()
    } else {
        collapsed_tex
    };
    let hint = render_error.unwrap_or_default();

    div()
        .w_full()
        .pl(lrc.indent_px)
        .pt(spec.padding_top)
        .pb(spec.padding_bottom)
        .child(
            div()
                .w_full()
                .h(spec.line_height)
                .px(crate::editor::geometry::MATH_EDITOR_PADDING_X)
                .rounded(px(4.0))
                .bg(theme.code_block_bg)
                .border_1()
                .border_color(theme.code_block_border)
                .flex()
                .items_center()
                .justify_between()
                .gap(px(12.0))
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .overflow_hidden()
                        .whitespace_nowrap()
                        .font_family(crate::platform::platform_monospace_font())
                        .text_size(px((lrc.font_size * (13.0 / 15.0)).round()))
                        .text_color(theme.code_block_text)
                        .child(display_tex),
                )
                .when(!hint.is_empty(), |row| {
                    row.child(
                        div()
                            .flex_none()
                            .max_w(px(260.0))
                            .overflow_hidden()
                            .whitespace_nowrap()
                            .text_size(px((lrc.font_size * (12.0 / 15.0)).round()))
                            .text_color(theme.text_muted)
                            .child(hint),
                    )
                }),
        )
        .into_any_element()
}

pub(super) fn render_math_block_content_line(
    source_line: SourceLine,
    parsed: &ParsedLine,
    lrc: &LineRenderContext,
    spec: &LineTextSpec,
) -> AnyElement {
    if !lrc.is_math_block_editing {
        return div().w_full().h(px(0.0)).into_any_element();
    }
    let mut source_lrc = *lrc;
    source_lrc.indent_px = px(0.0);
    let text_canvas = render_shaped_line(
        source_line,
        parsed,
        &source_lrc,
        spec.font_size,
        spec.line_height,
        spec.override_color,
        spec.shaping_mode,
    );

    let left_pane_w = lrc.content_width / 2.0;
    div()
        .w(left_pane_w)
        .min_w_0()
        .px(spec.padding_x)
        .pt(spec.padding_top)
        .pb(spec.padding_bottom)
        .child(text_canvas)
        .into_any_element()
}

pub(super) fn render_paragraph_line(
    source_line: SourceLine,
    parsed: &ParsedLine,
    lrc: &LineRenderContext,
    spec: &LineTextSpec,
) -> AnyElement {
    if lrc.is_syntactic_separator && !lrc.is_active_line {
        // Syntactic blank separator lines between blocks: collapsed to 0 height in Live Preview
        return div().h(px(0.0)).into_any_element();
    }

    let text_canvas = render_shaped_line(
        source_line,
        parsed,
        lrc,
        spec.font_size,
        spec.line_height,
        quote_text_color(parsed, lrc.theme),
        spec.shaping_mode,
    );

    div()
        .w_full()
        .min_h(spec.line_height)
        .pt(spec.padding_top)
        .pb(spec.padding_bottom)
        .child(text_canvas)
        .into_any_element()
}

fn render_list_item_row(
    marker: impl IntoElement,
    marker_width: Pixels,
    text_canvas: impl IntoElement,
    line_height: Pixels,
    padding_top: Pixels,
    padding_bottom: Pixels,
    indent_px: Pixels,
) -> AnyElement {
    div()
        .w_full()
        .pl(indent_px)
        .min_h(line_height)
        .pt(padding_top)
        .pb(padding_bottom)
        .flex()
        .flex_row()
        .items_start()
        .child(
            div()
                .w(marker_width)
                .h(line_height)
                .flex_none()
                .flex()
                .items_center()
                .justify_start()
                .child(marker),
        )
        .child(div().flex_grow().child(text_canvas))
        .into_any_element()
}
