use gpui::prelude::FluentBuilder;
use gpui::*;

use crate::buffer::{LineSource, SourceLine};
use crate::markdown::{BlockKind, ParsedLine};
use crate::theme::Theme;

use super::Editor;
use super::LineRenderContext;
use super::geometry::{TextShapingMode, resolve_line_text_spec};
use super::shaping::render_shaped_line;

mod blocks;
mod image;
mod table;

pub use image::{render_loading_metaball_animation, resolve_image_source};

/// Renders a line in Live Preview mode.
pub fn render_live_line_content(
    source_line: SourceLine,
    parsed: &ParsedLine,
    lines: &(impl LineSource + ?Sized),
    lrc: &LineRenderContext,
    editor: Option<&Entity<Editor>>,
) -> AnyElement {
    let spec = resolve_line_text_spec(
        parsed,
        lrc.mode,
        lrc.content_width,
        lrc.indent_px,
        lrc.quote_content_indent_px,
        lrc.font_size,
        lrc.line_height,
        lrc.theme,
    );

    match &parsed.kind {
        BlockKind::Heading { .. } => blocks::render_heading_line(source_line, parsed, lrc, &spec),
        BlockKind::CodeBlock { is_fence_start } | BlockKind::MermaidBlock { is_fence_start } => {
            blocks::render_code_or_mermaid_fence(source_line, parsed, lrc, &spec, *is_fence_start)
        }
        BlockKind::CodeBlockContent | BlockKind::MermaidBlockContent => {
            blocks::render_code_or_mermaid_content(source_line, parsed, lrc, &spec)
        }
        BlockKind::ThematicBreak => blocks::render_thematic_break_line(lrc, &spec),
        BlockKind::TaskList { checked } => {
            blocks::render_task_list_line(source_line, parsed, lrc, &spec, *checked, editor)
        }
        BlockKind::BulletList => blocks::render_bullet_list_line(source_line, parsed, lrc, &spec),
        BlockKind::OrderedList { num } => {
            blocks::render_ordered_list_line(source_line, parsed, lrc, &spec, *num)
        }
        BlockKind::Image { alt, url } => {
            image::render_image_line(source_line, parsed, lrc, &spec, alt, url)
        }
        BlockKind::Table {
            is_header,
            is_delimiter,
            alignments,
            cells,
        } => table::render_table_row(
            source_line,
            lines,
            lrc,
            *is_header,
            *is_delimiter,
            alignments,
            cells,
        ),
        BlockKind::MathBlock { is_start: true } => {
            blocks::render_math_block_start_line(source_line, parsed, lines, lrc, &spec)
        }
        BlockKind::MathBlock { is_start: false } => div().w_full().h(px(0.0)).into_any_element(),
        BlockKind::MathBlockContent => {
            blocks::render_math_block_content_line(source_line, parsed, lrc, &spec)
        }
        BlockKind::Paragraph => blocks::render_paragraph_line(source_line, parsed, lrc, &spec),
    }
}

#[inline]
pub(super) fn quote_text_color(parsed: &ParsedLine, theme: &Theme) -> Option<Hsla> {
    parsed.quote.map(|_| theme.quote_text)
}

/// Renders a line in Source mode with a line-number gutter.
pub fn render_source_line(
    source_line: SourceLine,
    parsed: &ParsedLine,
    lrc: &LineRenderContext,
    editor: Option<&Entity<Editor>>,
) -> AnyElement {
    let line_idx = source_line.index();
    let font_size = px(lrc.font_size);
    let line_height = px((lrc.font_size * lrc.line_height).round());
    let line_num_str = format!("{:>4} ", line_idx + 1);
    let content_element = render_shaped_line(
        source_line,
        parsed,
        lrc,
        font_size,
        line_height,
        None,
        TextShapingMode::RawSource,
    );

    div()
        .id(ElementId::NamedInteger("src_line".into(), line_idx as u64))
        .w_full()
        .min_h(line_height)
        .flex()
        .flex_row()
        .items_start()
        .bg(if lrc.is_active_line && lrc.is_focused {
            lrc.theme.line_highlight
        } else {
            transparent_black()
        })
        // Gutter line number ruler
        .child(
            div()
                .w(crate::editor::layout::SOURCE_GUTTER_WIDTH)
                .h(line_height)
                .flex_none()
                .pr(px(12.0))
                .text_color(if lrc.is_active_line && lrc.is_focused {
                    lrc.theme.text_accent
                } else {
                    lrc.theme.text_muted
                })
                .font_weight(if lrc.is_active_line && lrc.is_focused {
                    FontWeight::BOLD
                } else {
                    FontWeight::NORMAL
                })
                .text_size(px((lrc.font_size * (12.5 / 15.0)).round()))
                .font_family(crate::platform::platform_monospace_font())
                .cursor_pointer()
                .when_some(editor, |this, ed| {
                    let ed = ed.clone();
                    this.on_mouse_down(MouseButton::Left, move |_, window, cx| {
                        ed.update(cx, |this, cx| {
                            this.buffer.select_line_at(source_line);
                            window.focus(&this.focus_handle);
                            cx.notify();
                        });
                    })
                })
                .child(line_num_str),
        )
        // Line content area
        .child(div().flex_grow().w_full().child(content_element))
        .into_any_element()
}
