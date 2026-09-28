use gpui::*;

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

pub struct ScrollbarDrawInfo {
    pub thumb_bounds: Bounds<Pixels>,
    pub thumb_color: Hsla,
}

pub struct EditorPrepaintState {
    pub hitbox: Option<Hitbox>,
    pub lines: Vec<AnyElement>,
    pub scrollbar: Option<ScrollbarDrawInfo>,
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
        self.editor.update(cx, |editor, _| {
            editor.viewport_bounds = bounds;
            if !is_focused && editor.blink_task.is_some() {
                editor.cursor_opacity = 0.0;
                editor.blink_task = None;
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
                let (lines_to_prepaint, scrollbar) = {
                    let editor = editor_entity.read(cx);
                    let layout = editor.layout_snapshot();
                    let total_lines = layout.parsed_lines.len();

                    // Calculate visible line slice based on ScrollState with 300px overscan
                    let scroll_top = editor.scroll.current_scroll_top;
                    let scroll_h = bounds.size.height;
                    let min_y = (scroll_top - px(300.0)).max(px(0.0));
                    let max_y = scroll_top + scroll_h + px(300.0);

                    let (visible_start, visible_end) = if layout.line_y_offsets.is_empty() {
                        (0, 0)
                    } else {
                        let start = layout
                            .line_y_offsets
                            .partition_point(|&y| y < min_y)
                            .saturating_sub(1);
                        let end = (layout.line_y_offsets.partition_point(|&y| y <= max_y) + 1)
                            .min(total_lines);
                        (start, end)
                    };

                    let is_focused = editor.focus_handle.is_focused(window);
                    let cursor_opacity = if is_focused {
                        editor.cursor_opacity
                    } else {
                        0.0
                    };
                    let cursor = editor.buffer.cursor_pos();
                    let cursor_line = cursor.line;
                    let cursor_col = cursor.col;
                    let disclosure_col = editor.disclosure_col();
                    let selection = editor.buffer.selection();
                    let is_dragging = editor.is_dragging;
                    let mode = editor.render_mode;
                    let theme = cx.theme();
                    let content_width = compute_content_width(bounds.size.width, mode);
                    let container_x =
                        bounds.origin.x + (bounds.size.width - content_width).max(px(0.0)) / 2.0;

                    let mut lines_to_prepaint =
                        Vec::with_capacity(visible_end.saturating_sub(visible_start));

                    for line_idx in visible_start..visible_end {
                        if editor.is_line_folded(line_idx) {
                            continue;
                        }
                        let parsed = &layout.parsed_lines[line_idx];
                        let is_active_line = line_idx == cursor_line;
                        let lrc = LineRenderContext {
                            is_active_line,
                            is_focused,
                            cursor_col,
                            disclosure_col,
                            selection,
                            is_dragging,
                            theme,
                            mode,
                            cursor_opacity,
                            hovered_link: editor.hovered_link.as_ref(),
                            content_width,
                            doc_path: editor.buffer.file_path(),
                            font_size: editor.font_size,
                            line_height: editor.line_height,
                            soft_wrap: editor.soft_wrap,
                            text_system: editor.text_system.as_ref(),
                        };

                        let line_el = render_line_element(
                            line_idx,
                            parsed,
                            editor.buffer.lines(),
                            &lrc,
                            &editor_entity,
                        );

                        let line_y_in_doc = layout
                            .line_y_offsets
                            .get(line_idx)
                            .copied()
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
                    let (max_scroll, thumb_y, thumb_h) =
                        editor.scrollbar_thumb_metrics(track_h, layout.total_height);
                    let has_overflow = max_scroll > px(2.0);

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

                    let scrollbar = if has_overflow && scrollbar_opacity > 0.01 {
                        let thumb_w = if editor.scroll.is_scrollbar_dragging {
                            px(8.0)
                        } else {
                            px(5.0)
                        };
                        let thumb_color = if editor.scroll.is_scrollbar_dragging {
                            theme.text_accent.opacity(0.85)
                        } else {
                            theme.border.opacity(0.6 * scrollbar_opacity)
                        };
                        let thumb_bounds = Bounds::new(
                            point(bounds.right() - thumb_w - px(2.0), bounds.top() + thumb_y),
                            size(thumb_w, thumb_h),
                        );
                        Some(ScrollbarDrawInfo {
                            thumb_bounds,
                            thumb_color,
                        })
                    } else {
                        None
                    };

                    (lines_to_prepaint, scrollbar)
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
        let (is_dragging, is_scrollbar_dragging) = {
            let ed = self.editor.read(cx);
            (ed.is_dragging, ed.scroll.is_scrollbar_dragging)
        };
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
    line_idx: usize,
    parsed: &crate::markdown::ParsedLine,
    lines: &[String],
    lrc: &LineRenderContext,
    editor: &Entity<Editor>,
) -> AnyElement {
    if lrc.mode == RenderMode::Source {
        render_source_line(line_idx, parsed, lrc, Some(editor))
    } else {
        let content_element = render_live_line_content(line_idx, parsed, lines, lrc, Some(editor));

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
