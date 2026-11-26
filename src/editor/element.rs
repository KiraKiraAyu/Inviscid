use gpui::prelude::FluentBuilder;
use gpui::*;

use crate::buffer::LineSource;
use crate::editor::render::{render_live_line_content, render_source_line};
use crate::editor::shaping::compute_content_width;
use crate::editor::{Editor, LineRenderContext, RenderMode};
use crate::theme::ActiveTheme;

pub struct EditorElement {
    editor: Entity<Editor>,
    interactivity: Interactivity,
}

impl EditorElement {
    pub fn new(editor: Entity<Editor>) -> Self {
        let mut interactivity = Interactivity::default();
        interactivity.base_style.size.width = Some(relative(1.0).into());
        interactivity.base_style.size.height = Some(relative(1.0).into());
        Self {
            editor,
            interactivity,
        }
    }
}

impl IntoElement for EditorElement {
    type Element = Self;

    fn into_element(self) -> Self::Element {
        self
    }
}

impl Styled for EditorElement {
    fn style(&mut self) -> &mut StyleRefinement {
        &mut self.interactivity.base_style
    }
}

impl InteractiveElement for EditorElement {
    fn interactivity(&mut self) -> &mut Interactivity {
        &mut self.interactivity
    }
}

const SCROLLBAR_THUMB_WIDTH_IDLE: Pixels = px(5.0);
const SCROLLBAR_THUMB_WIDTH_DRAGGING: Pixels = px(8.0);
const SCROLLBAR_THUMB_RIGHT_MARGIN: Pixels = px(2.0);

pub struct ScrollbarDrawInfo {
    pub thumb_bounds: Bounds<Pixels>,
    pub thumb_color: Hsla,
}

pub struct DropIndicatorDrawInfo {
    pub y: Pixels,
    pub left: Pixels,
    pub width: Pixels,
    pub color: Hsla,
}

pub struct EditorPrepaintState {
    pub hitbox: Option<Hitbox>,
    pub lines: Vec<AnyElement>,
    pub scrollbar: Option<ScrollbarDrawInfo>,
    pub drop_indicator: Option<DropIndicatorDrawInfo>,
}

impl Element for EditorElement {
    type RequestLayoutState = ();
    type PrepaintState = EditorPrepaintState;

    fn id(&self) -> Option<ElementId> {
        self.interactivity.element_id.clone()
    }

    fn source_location(&self) -> Option<&'static core::panic::Location<'static>> {
        None
    }

    fn request_layout(
        &mut self,
        global_id: Option<&GlobalElementId>,
        inspector_id: Option<&InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, Self::RequestLayoutState) {
        let layout_id = self.interactivity.request_layout(
            global_id,
            inspector_id,
            window,
            cx,
            |style, window, cx| window.request_layout(style, None, cx),
        );
        (layout_id, ())
    }

    fn prepaint(
        &mut self,
        global_id: Option<&GlobalElementId>,
        inspector_id: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        _request_layout: &mut Self::RequestLayoutState,
        window: &mut Window,
        cx: &mut App,
    ) -> Self::PrepaintState {
        let is_focused = self.editor.read(cx).focus_handle.is_focused(window);

        // Synchronously commit exact Taffy layout bounds to Editor and cancel ghost blink task if unfocused
        let has_active_drag = cx.has_active_drag();
        self.editor.update(cx, |editor, cx| {
            editor.viewport_bounds = bounds;
            editor.sync_theme(cx);
            editor.prepare_viewport_layout();
            if !has_active_drag && editor.external_drop_target.is_some() {
                editor.external_drop_target = None;
            }
            if !is_focused && editor.blink_task.is_some() {
                editor.cursor_opacity = 0.0;
                editor.blink_task = None;
            }
            if editor.scroll.is_animating() {
                editor.start_scroll_animation_on_window(window, cx);
            }
        });

        // Interactivity prepaint (hitbox & styling)
        let editor_entity = self.editor.clone();
        self.interactivity.prepaint(
            global_id,
            inspector_id,
            bounds,
            bounds.size,
            window,
            cx,
            |_style, _scroll_offset, hitbox, window, cx| {
                let (lines_to_prepaint, scrollbar, drop_indicator) = {
                    let editor = editor_entity.read(cx);
                    editor
                        .painted_lines
                        .borrow_mut()
                        .begin_frame(editor.current_painted_frame_key());
                    let layout = editor.layout_snapshot();
                    // Heights are already known: prepare only rows intersecting the viewport.
                    let scroll_top = editor.scroll.current_scroll_top;
                    let scroll_h = bounds.size.height;
                    let visible = if editor.scroll.anchor_line.is_some() {
                        layout.metrics.visible_lines_relative(
                            editor.scroll.anchor_line,
                            scroll_top,
                            scroll_h,
                        )
                    } else {
                        layout.visible_source_lines(scroll_top, scroll_h)
                    };
                    let visible_start = visible.start;
                    let visible_end = visible.end;

                    let is_focused = editor.focus_handle.is_focused(window);
                    let cursor_opacity = if is_focused {
                        editor.cursor_opacity
                    } else {
                        0.0
                    };
                    let cursor = editor.buffer.cursor_pos();
                    let cursor_line = cursor.line;
                    let cursor_col = cursor.col;
                    let disclosure_position = editor.disclosure_position();
                    let disclosure_col = disclosure_position.col;
                    let selection = editor.buffer.selection();
                    let is_dragging = editor.is_dragging;
                    let is_selection_disclosure = editor.is_selection_disclosure();
                    let mode = editor.render_mode;
                    let active_math_block = layout.active_math_block;
                    let active_mermaid_block = layout.active_mermaid_block;
                    let theme = cx.theme();
                    let catalog = cx
                        .try_global::<crate::i18n::LocaleManager>()
                        .map(|m| m.catalog())
                        .unwrap_or(&crate::i18n::DEFAULT_CATALOG);
                    let content_width = compute_content_width(bounds.size.width, mode);
                    let container_x =
                        bounds.origin.x + (bounds.size.width - content_width).max(px(0.0)) / 2.0;

                    let mut lines_to_prepaint =
                        Vec::with_capacity(visible_end.saturating_sub(visible_start));

                    match mode {
                        RenderMode::LivePreview => {
                            let quote_ranges =
                                layout.visible_quote_ranges(visible_start..visible_end);
                            let first_quote =
                                quote_ranges.partition_point(|range| range.end <= visible_start);
                            for quote_range in quote_ranges[first_quote..]
                                .iter()
                                .take_while(|range| range.start < visible_end)
                            {
                                let group_bottom = layout
                                    .metrics
                                    .top_relative(quote_range.end, editor.scroll.anchor_line)
                                    .unwrap_or(px((layout.metrics.total_height_precise()
                                        - 24.0
                                        - editor.scroll_origin_y())
                                        as f32));
                                let group_height = group_bottom
                                    - layout
                                        .metrics
                                        .top_relative(quote_range.start, editor.scroll.anchor_line)
                                        .unwrap();
                                let quote_indent = layout
                                    .indent_px(crate::buffer::SourceLine::new(quote_range.start));
                                let panel = div()
                                    .w_full()
                                    .h(group_height)
                                    .border_l_4()
                                    .border_color(theme.quote_border)
                                    .bg(theme.quote_bg);
                                let panel_y = bounds.origin.y
                                    + layout
                                        .metrics
                                        .top_relative(quote_range.start, editor.scroll.anchor_line)
                                        .unwrap()
                                    - scroll_top;
                                lines_to_prepaint.push((
                                    panel.into_any_element(),
                                    point(container_x + quote_indent, panel_y),
                                    size(
                                        AvailableSpace::Definite(
                                            (content_width - quote_indent).max(px(0.0)),
                                        ),
                                        AvailableSpace::Definite(group_height),
                                    ),
                                ));
                            }

                            if let Some((math_start, math_end)) = active_math_block
                                && math_end.index() > math_start.index() + 1
                            {
                                let content_start = math_start.index() + 1;
                                let content_end = math_end.index() - 1;
                                if content_start < visible_end
                                    && content_end >= visible_start
                                    && let (Some(block_top), Some(block_bottom)) = (
                                        layout
                                            .metrics
                                            .top_relative(content_start, editor.scroll.anchor_line),
                                        layout.metrics.top_relative(
                                            math_end.index(),
                                            editor.scroll.anchor_line,
                                        ),
                                    )
                                {
                                    let panel_h = (block_bottom - block_top).max(px(0.0));
                                    if panel_h > px(0.0) {
                                        let start_row =
                                            layout.parsed_lines.row(math_start.index());
                                        let (outer_left, inner_width) =
                                            if let Some(row) = start_row.as_deref()
                                                && let Some(quote) = row.quote
                                            {
                                                let quote_depth = quote.depth.max(1);
                                                let start_indent = layout.indent_px(math_start);
                                                (
                                                    crate::editor::shaping::QUOTE_GUTTER
                                                        * quote_depth as f32
                                                        + start_indent,
                                                    crate::editor::shaping::effective_content_width(
                                                        content_width,
                                                        row,
                                                        mode,
                                                        start_indent,
                                                    ),
                                                )
                                            } else {
                                                (px(0.0), content_width)
                                            };
                                        let left_pane_w = inner_width / 2.0;
                                        let panel_w = (inner_width - left_pane_w).max(px(0.0));
                                        let math_preview = start_row
                                            .as_deref()
                                            .and_then(|row| {
                                                row.spans
                                                    .iter()
                                                    .find_map(|span| span.math.as_ref())
                                            })
                                            .map(|math| &math.rendered);
                                        let fallback_error = if math_preview.is_none() {
                                            let (tex, err) =
                                                crate::math::resolve_math_block_fallback(
                                                    &editor.buffer,
                                                    math_start,
                                                    Some(math_end),
                                                );
                                            err.filter(|_| !tex.trim().is_empty())
                                        } else {
                                            None
                                        };
                                        let panel_y = bounds.origin.y + block_top - scroll_top;
                                        let panel = div()
                                            .w(panel_w)
                                            .h(panel_h)
                                            .border_l_1()
                                            .border_color(theme.code_block_border)
                                            .flex()
                                            .items_center()
                                            .justify_center()
                                            .px(crate::editor::geometry::MATH_EDITOR_PADDING_X)
                                            .py(crate::editor::geometry::MATH_EDITOR_PADDING_Y)
                                            .when_some(math_preview, |preview, math| {
                                                let preview_size =
                                                    crate::editor::geometry::math_preview_display_size(
                                                        math,
                                                        px(editor.font_size),
                                                        inner_width,
                                                    );
                                                preview.child(
                                                    svg()
                                                        .path(math.asset_path.clone())
                                                        .w(preview_size.width)
                                                        .h(preview_size.height)
                                                        .text_color(theme.text_primary),
                                                )
                                            })
                                            .when_some(fallback_error, |preview, err| {
                                                preview.child(
                                                    div()
                                                        .w_full()
                                                        .text_center()
                                                        .text_size(px(
                                                            (editor.font_size * (12.0 / 15.0))
                                                                .round(),
                                                        ))
                                                        .text_color(theme.text_muted)
                                                        .child(err),
                                                )
                                            });
                                        lines_to_prepaint.push((
                                            panel.into_any_element(),
                                            point(container_x + outer_left + left_pane_w, panel_y),
                                            size(
                                                AvailableSpace::Definite(panel_w),
                                                AvailableSpace::Definite(panel_h),
                                            ),
                                        ));
                                    }
                                }
                            }
                        }
                        RenderMode::Source => {}
                    }

                    for line_idx in visible_start..visible_end {
                        let source_line = crate::buffer::SourceLine::new(line_idx);
                        if layout.metrics.height(line_idx).unwrap() <= px(0.0) {
                            continue;
                        }
                        let row = layout.parsed_lines.row(line_idx).unwrap();
                        let parsed = &*row;

                        let is_active_line = source_line == cursor_line;
                        let is_disclosure_line = source_line == disclosure_position.line;
                        let is_math_block_editing =
                            editor.is_math_block_expanded(source_line, parsed, active_math_block);
                        let is_mermaid_block_editing = active_mermaid_block
                            .is_some_and(|(start, end)| source_line >= start && source_line <= end);
                        let mermaid_preview = parsed
                            .spans
                            .iter()
                            .find_map(|span| span.mermaid.as_ref())
                            .map(|mermaid| &mermaid.rendered);
                        let highlights = editor.search_highlights(source_line);
                        let lrc = LineRenderContext {
                            is_active_line,
                            is_math_block_editing,
                            is_mermaid_block_editing,
                            mermaid_preview,
                            is_disclosure_line,
                            is_selection_disclosure,
                            ime_preedit_cols: editor.ime_preedit_on_line(source_line),
                            is_focused,
                            cursor_col,
                            disclosure_col,
                            selection,
                            is_dragging,
                            theme,
                            catalog,
                            mode,
                            cursor_opacity,
                            hovered_link: editor.hovered_link.as_ref(),
                            content_width,
                            doc_path: editor.buffer.file_path(),
                            font_size: editor.font_size,
                            line_height: editor.line_height,
                            soft_wrap: editor.soft_wrap,
                            indent_px: layout.indent_px(source_line),
                            quote_content_indent_px:
                                crate::editor::shaping::quote_content_indent_px(
                                    parsed,
                                    layout.space_width,
                                ),
                            is_syntactic_separator: layout.separator(line_idx),
                            text_system: editor.text_system.as_ref(),
                            highlights: &highlights,
                            buffer_revision: editor.buffer.revision(),
                            painted_lines: Some(&editor.painted_lines),
                            shaped_rows: Some(&editor.shaped_rows),
                        };

                        let line_el = render_line_element(
                            source_line,
                            parsed,
                            &editor.buffer,
                            &lrc,
                            &editor_entity,
                        );

                        let line_y_in_doc = layout
                            .metrics
                            .top_relative(line_idx, editor.scroll.anchor_line)
                            .unwrap_or(px(24.0));
                        let line_screen_y = bounds.origin.y + line_y_in_doc - scroll_top;
                        let line_origin = point(container_x, line_screen_y);

                        let available_space = size(
                            AvailableSpace::Definite(content_width),
                            AvailableSpace::MinContent,
                        );

                        lines_to_prepaint.push((line_el, line_origin, available_space));
                    }

                    let track_h = bounds.size.height;
                    let metrics = editor.scrollbar_metrics(track_h, layout.metrics.total_height());

                    let last_scroll = editor.scroll.last_scroll_action;
                    let scroll_elapsed = std::time::Instant::now()
                        .duration_since(last_scroll)
                        .as_secs_f32();
                    let scrollbar_opacity =
                        if editor.scroll.is_scrollbar_dragging || scroll_elapsed < 0.8 {
                            1.0
                        } else if scroll_elapsed < 1.4 {
                            let t = (scroll_elapsed - 0.8) / 0.6;
                            (1.0 - t).max(0.0)
                        } else {
                            0.0
                        };

                    let scrollbar = if metrics.has_overflow && scrollbar_opacity > 0.01 {
                        let thumb_w = if editor.scroll.is_scrollbar_dragging {
                            SCROLLBAR_THUMB_WIDTH_DRAGGING
                        } else {
                            SCROLLBAR_THUMB_WIDTH_IDLE
                        };
                        let thumb_color = if editor.scroll.is_scrollbar_dragging {
                            theme.text_accent.opacity(0.85)
                        } else {
                            theme.border.opacity(0.6 * scrollbar_opacity)
                        };
                        let thumb_bounds = Bounds::new(
                            point(
                                bounds.right() - thumb_w - SCROLLBAR_THUMB_RIGHT_MARGIN,
                                bounds.top() + metrics.thumb_y,
                            ),
                            size(thumb_w, metrics.thumb_h),
                        );
                        Some(ScrollbarDrawInfo {
                            thumb_bounds,
                            thumb_color,
                        })
                    } else {
                        None
                    };

                    let drop_indicator = if cx.has_active_drag() {
                        editor.external_drop_target.map(|before_line| {
                            let y = editor.drop_indicator_screen_y(before_line, bounds);
                            let (left, width) = match mode {
                                RenderMode::LivePreview => (container_x, content_width),
                                RenderMode::Source => (
                                    container_x + crate::editor::layout::SOURCE_GUTTER_WIDTH,
                                    (content_width - crate::editor::layout::SOURCE_GUTTER_WIDTH)
                                        .max(px(0.0)),
                                ),
                            };
                            DropIndicatorDrawInfo {
                                y,
                                left,
                                width,
                                color: theme.text_accent,
                            }
                        })
                    } else {
                        None
                    };

                    (lines_to_prepaint, scrollbar, drop_indicator)
                };

                let mut lines = Vec::with_capacity(lines_to_prepaint.len());
                for (mut line_el, line_origin, available_space) in lines_to_prepaint {
                    line_el.prepaint_as_root(line_origin, available_space, window, cx);
                    lines.push(line_el);
                }

                EditorPrepaintState {
                    hitbox,
                    lines,
                    scrollbar,
                    drop_indicator,
                }
            },
        )
    }

    fn paint(
        &mut self,
        global_id: Option<&GlobalElementId>,
        inspector_id: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        _request_layout: &mut Self::RequestLayoutState,
        prepaint: &mut Self::PrepaintState,
        window: &mut Window,
        cx: &mut App,
    ) {
        // Register window-level mouse capture listeners during drag operations
        let (is_dragging, is_scrollbar_dragging, has_external_drop_target) = {
            let ed = self.editor.read(cx);
            (
                ed.is_dragging,
                ed.scroll.is_scrollbar_dragging,
                ed.external_drop_target.is_some(),
            )
        };
        if has_external_drop_target {
            let entity_drop_exit = self.editor.clone();
            window.on_mouse_event(
                move |event: &FileDropEvent,
                      phase: DispatchPhase,
                      _window: &mut Window,
                      cx: &mut App| {
                    if phase == DispatchPhase::Capture
                        && matches!(event, FileDropEvent::Exited)
                    {
                        entity_drop_exit.update(cx, |this, cx| {
                            this.clear_external_drop_target(cx);
                        });
                    }
                },
            );
        }
        if is_dragging || is_scrollbar_dragging {
            let entity_move = self.editor.clone();
            window.on_mouse_event(
                move |event: &MouseMoveEvent,
                      phase: DispatchPhase,
                      window: &mut Window,
                      cx: &mut App| {
                    if phase != DispatchPhase::Capture {
                        return;
                    }
                    let handled = entity_move.update(cx, |this, cx| {
                        if !this.is_dragging && !this.scroll.is_scrollbar_dragging {
                            return false;
                        }
                        if event.pressed_button != Some(MouseButton::Left) {
                            this.stop_drag(cx);
                            return true;
                        }
                        this.handle_drag_move(event.position, window, cx);
                        true
                    });
                    if handled {
                        cx.stop_propagation();
                    }
                },
            );

            let entity_up = self.editor.clone();
            window.on_mouse_event(
                move |event: &MouseUpEvent,
                      phase: DispatchPhase,
                      _window: &mut Window,
                      cx: &mut App| {
                    if phase != DispatchPhase::Capture {
                        return;
                    }
                    if event.button == MouseButton::Left {
                        let stopped = entity_up.update(cx, |this, cx| {
                            if this.is_dragging || this.scroll.is_scrollbar_dragging {
                                this.stop_drag(cx);
                                true
                            } else {
                                false
                            }
                        });
                        if stopped {
                            cx.stop_propagation();
                        }
                    }
                },
            );
        }

        // Bind native IME input handler during paint
        let focus_handle = self.editor.read(cx).focus_handle.clone();
        window.handle_input(
            &focus_handle,
            ElementInputHandler::new(bounds, self.editor.clone()),
            cx,
        );

        self.interactivity.paint(
            global_id,
            inspector_id,
            bounds,
            prepaint.hitbox.as_ref(),
            window,
            cx,
            |_style, window, cx| {
                // Viewport clipping mask for document contents
                window.with_content_mask(Some(ContentMask { bounds }), |window| {
                    for line in &mut prepaint.lines {
                        line.paint(window, cx);
                    }
                    if let Some(ind) = &prepaint.drop_indicator {
                        let line_y = ind.y - px(1.0);
                        let line_h = px(2.0);
                        let fade_w = px(24.0);
                        if ind.width > fade_w * 2.5 {
                            let mid_w = ind.width - fade_w * 2.0;
                            let transparent = ind.color.opacity(0.0);
                            window.paint_quad(fill(
                                Bounds::new(point(ind.left, line_y), size(fade_w, line_h)),
                                linear_gradient(
                                    90.0,
                                    linear_color_stop(transparent, 0.0),
                                    linear_color_stop(ind.color, 1.0),
                                ),
                            ));
                            window.paint_quad(fill(
                                Bounds::new(point(ind.left + fade_w, line_y), size(mid_w, line_h)),
                                ind.color,
                            ));
                            window.paint_quad(fill(
                                Bounds::new(
                                    point(ind.left + fade_w + mid_w, line_y),
                                    size(fade_w, line_h),
                                ),
                                linear_gradient(
                                    90.0,
                                    linear_color_stop(ind.color, 0.0),
                                    linear_color_stop(transparent, 1.0),
                                ),
                            ));
                        } else {
                            window.paint_quad(
                                fill(
                                    Bounds::new(point(ind.left, line_y), size(ind.width, line_h)),
                                    ind.color,
                                )
                                .corner_radii(px(1.0)),
                            );
                        }
                    }
                });

                // Overlay scrollbar drawn on top of document content
                if let Some(sb) = &prepaint.scrollbar {
                    window.paint_quad(fill(sb.thumb_bounds, sb.thumb_color).corner_radii(px(3.0)));
                }
            },
        );
    }
}

fn render_line_element(
    source_line: crate::buffer::SourceLine,
    parsed: &crate::markdown::ParsedLine,
    lines: &(impl LineSource + ?Sized),
    lrc: &LineRenderContext,
    editor: &Entity<Editor>,
) -> AnyElement {
    let line_idx = source_line.index();
    if lrc.mode == RenderMode::Source {
        render_source_line(source_line, parsed, lrc, Some(editor))
    } else {
        let mut content_lrc = *lrc;
        if parsed.quote.is_some() {
            content_lrc.content_width = crate::editor::shaping::effective_content_width(
                lrc.content_width,
                parsed,
                lrc.mode,
                lrc.indent_px,
            );
            content_lrc.indent_px = if parsed.kind.has_syntax_prefix() {
                lrc.quote_content_indent_px
            } else {
                px(0.0)
            };
            content_lrc.quote_content_indent_px = px(0.0);
        }
        let content_element =
            render_live_line_content(source_line, parsed, lines, &content_lrc, Some(editor));

        let content_element = if let Some(quote) = parsed.quote {
            let spec = super::geometry::resolve_line_text_spec(
                parsed,
                lrc.mode,
                lrc.content_width,
                lrc.indent_px,
                lrc.quote_content_indent_px,
                lrc.font_size,
                lrc.line_height,
                lrc.theme,
            );
            let quote_depth = quote.depth.max(1);
            let mut quote_line = div()
                .w_full()
                .pl(crate::editor::shaping::QUOTE_GUTTER * quote_depth as f32 + lrc.indent_px);
            if spec.quote_top_padding > px(0.0) {
                quote_line = quote_line.pt(spec.quote_top_padding);
            }
            if spec.quote_bottom_padding > px(0.0) {
                quote_line = quote_line.pb(spec.quote_bottom_padding);
            }
            quote_line.child(content_element).into_any_element()
        } else {
            content_element
        };

        div()
            .id(ElementId::NamedInteger("live_line".into(), line_idx as u64))
            .w_full()
            .flex()
            .flex_row()
            .items_start()
            .child(content_element)
            .into_any_element()
    }
}
