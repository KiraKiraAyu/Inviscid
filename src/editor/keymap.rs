use crate::buffer::Position;
use crate::editor::element::EditorElement;
use gpui::*;

use super::actions::*;
use super::{Editor, RenderMode};

const PAGE_SCROLL_LINES: usize = 15;

impl Editor {
    pub fn perform_edit<F>(&mut self, cx: &mut Context<Self>, f: F)
    where
        F: FnOnce(&mut Editor),
    {
        self.normalize_cursor_to_projection(false);
        self.scroll_to_cursor();
        self.start_scroll_animation_if_needed(cx);
        self.reset_cursor_blink(cx);
        cx.notify();
        let target_line = target_line.index();
        let col = self.buffer.line_len(target_line).min(pos.col).max(
            crate::markdown::get_block_prefix_len(self.buffer.line(target_line).unwrap_or("")),
        );
        self.buffer
            .update_cursor(select, Position::new(target_line, col));
    }

    pub(crate) fn navigate_vertical(
        &mut self,
        is_up: bool,
        is_shift: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let prev_head = self.buffer.cursor_pos();
        if is_up {
            self.move_up(is_shift, window);
        } else {
            self.move_down(is_shift, window);
        }
        if self.buffer.cursor_pos() != prev_head {
            cx.emit(super::EditorEvent::CursorMoved);
        }
        self.scroll_to_cursor();
        self.start_scroll_animation_if_needed(cx);
        self.reset_cursor_blink(cx);
        cx.notify();
    }

    pub(crate) fn register_actions(
        element: Stateful<EditorElement>,
        cx: &Context<Editor>,
    ) -> Stateful<EditorElement> {
        element
            .on_action(cx.listener(|this, _: &UndoAction, _, cx| this.undo(cx)))
            .on_action(cx.listener(|this, _: &RedoAction, _, cx| this.redo(cx)))
            .on_action(cx.listener(|this, _: &CutAction, _, cx| this.cut(cx)))
            this.perform_edit(cx, |this| {
                    this.move_horizontally(true, false, |buffer, is_source| {
                        buffer.move_left_mode(true, is_source)
                    });
                });
            }))
            .on_action(cx.listener(|this, _: &MoveRight, _, cx| {
                this.perform_edit(cx, |this| {
                    this.move_horizontally(false, true, |buffer, is_source| {
                        buffer.move_right_mode(false, is_source)
                    });
                });
            }))
            .on_action(cx.listener(|this, _: &SelectRight, _, cx| {
                this.perform_edit(cx, |this| {
                    this.move_horizontally(true, true, |buffer, is_source| {
                        buffer.move_right_mode(true, is_source)
                    });
                });
            }))
            .on_action(cx.listener(|this, _: &MoveUp, window, cx| {
                this.navigate_vertical(true, false, window, cx);
            }))
            .on_action(cx.listener(|this, _: &SelectUp, window, cx| {
                this.navigate_vertical(true, true, window, cx);
            }))
            .on_action(cx.listener(|this, _: &MoveDown, window, cx| {
                this.navigate_vertical(false, false, window, cx);
            }))
            this.perform_edit(cx, |this| {
                    this.move_horizontally(true, false, |buffer, _| {
                        buffer.move_to_previous_word(true)
                    });
                });
            }))
            .on_action(cx.listener(|this, _: &MoveToNextWord, _, cx| {
                this.perform_edit(cx, |this| {
                    this.move_horizontally(false, true, |buffer, _| {
                        buffer.move_to_next_word(false)
                    });
                });
            }))
            .on_action(cx.listener(|this, _: &SelectToNextWord, _, cx| {
                this.perform_edit(cx, |this| {
                    this.move_horizontally(true, true, |buffer, _| buffer.move_to_next_word(true));
                });
            }))
            .on_action(cx.listener(|this, _: &MoveToBeginningOfLine, _, cx| {
                let is_source = this.render_mode == RenderMode::Source;
                this.perform_edit(cx, |this| {
                    this.buffer.move_to_line_start_mode(false, is_source)
                });
            }))
            .on_action(cx.listener(|this, _: &SelectToBeginningOfLine, _, cx| {
                let is_source = this.render_mode == RenderMode::Source;
                this.perform_edit(cx, |this| {
                    this.buffer.move_to_line_start_mode(true, is_source)
                });
            }))
            .on_action(cx.listener(|this, _: &MoveToEndOfLine, _, cx| {
                this.perform_edit(cx, |this| this.buffer.move_to_line_end(false));
            }))
            this.perform_edit(cx, |this| {
                    this.buffer.move_to_doc_start(true);
                    this.normalize_cursor_to_projection(true);
                });
            }))
            .on_action(cx.listener(|this, _: &MoveToEndOfDocument, _, cx| {
                this.perform_edit(cx, |this| {
                    this.buffer.move_to_doc_end(false);
                    this.normalize_cursor_to_projection(false);
                });
            }))
            .on_action(cx.listener(|this, _: &SelectToEndOfDocument, _, cx| {
                this.perform_edit(cx, |this| {
                    this.buffer.move_to_doc_end(true);
                    this.normalize_cursor_to_projection(true);
                });
            }))
            .on_action(cx.listener(|this, _: &PageUp, _, cx| {
                this.perform_edit(cx, |this| this.move_by_page(false, false));
            }))
            .on_action(cx.listener(|this, _: &SelectPageUp, _, cx| {
                this.perform_edit(cx, |this| this.move_by_page(true, false));
            }))
            .on_action(cx.listener(|this, _: &PageDown, _, cx| {
                this.perform_edit(cx, |this| this.move_by_page(false, true));
            }))
            .on_action(cx.listener(|this, _: &SelectPageDown, _, cx| {
                this.perform_edit(cx, |this| this.move_by_page(true, true));
            }))
            .on_action(cx.listener(|this, _: &MoveLineUp, _, cx| {
                this.perform_edit(cx, |this| this.buffer.move_lines_up());
            }))
            .on_action(cx.listener(|this, _: &MoveLineDown, _, cx| {
                this.perform_edit(cx, |this| this.buffer.move_lines_down());
            }))
            .on_action(cx.listener(|this, _: &DuplicateLineUp, _, cx| {
                this.perform_edit(cx, |this| this.buffer.duplicate_lines_up());
            }))
            .on_action(cx.listener(|this, _: &DuplicateLineDown, _, cx| {
                this.perform_edit(cx, |this| this.buffer.duplicate_lines_down());
            }))
            .on_action(cx.listener(|this, _: &DeleteLine, _, cx| {
                this.perform_edit(cx, |this| this.buffer.delete_lines());
            }))
            .on_action(cx.listener(|this, _: &ToggleComment, _, cx| {
                this.perform_edit(cx, |this| this.buffer.toggle_comment());
            }))
            .on_action(cx.listener(|this, _: &Indent, _, cx| {
                let is_source = this.render_mode == RenderMode::Source;
                this.perform_edit(cx, |this| {
                    if is_source || !this.buffer.table_tab_forward() {
                        this.buffer.indent_lines_with_tab_size(4);
                    }
                });
            }))
            .on_action(cx.listener(|this, _: &Outdent, _, cx| {
                let is_source = this.render_mode == RenderMode::Source;
                this.perform_edit(cx, |this| {
                    if is_source || !this.buffer.table_tab_backward() {
                        this.buffer.outdent_lines_with_tab_size(4);
                    }
                });
            }))
            .on_action(cx.listener(|this, _: &Newline, _, cx| {
                let is_source = this.render_mode == RenderMode::Source;
                this.perform_edit(cx, |this| {
                    if is_source || !this.buffer.table_enter_next_row() {
                        this.buffer.insert_newline_mode(is_source);
                    }
                });
            }))
            .on_action(cx.listener(|this, _: &NewlineBelow, _, cx| {
                this.perform_edit(cx, |this| {
                    this.buffer.insert_newline_below();
                });
            }))
            .on_action(cx.listener(|this, _: &NewlineBelowBlock, _, cx| {
                this.perform_edit(cx, |this| {
                    this.buffer.insert_line_below_block();
                });
            }))
            .on_action(cx.listener(|this, _: &NewlineAbove, _, cx| {
                this.perform_edit(cx, |this| {
                    this.buffer.insert_newline_above();
                });
            }))
            .on_action(cx.listener(|this, _: &Backspace, _, cx| {
                if this.edit_key_belongs_to_ime() {
                    return;
                }
                let is_source = this.render_mode == RenderMode::Source;
                this.perform_edit(cx, |this| {
                    if is_source || !this.buffer.table_backspace() {
                        this.buffer.backspace_mode(is_source);
                    }
                });
            }))
            .on_action(cx.listener(|this, _: &Delete, _, cx| {
                if this.edit_key_belongs_to_ime() {
                    return;
                }
                let is_source = this.render_mode == RenderMode::Source;
                this.perform_edit(cx, |this| {
                    this.buffer.delete_forward_mode(is_source);
                });
            }))
            .on_action(cx.listener(|this, _: &DeleteToPreviousWord, _, cx| {
                this.perform_edit(cx, |this| {
                    this.buffer.delete_to_previous_word();
                });
            }))
            .on_action(cx.listener(|this, _: &DeleteToNextWord, _, cx| {
                this.perform_edit(cx, |this| {
                    this.buffer.delete_to_next_word();
                });
            }))
    }
}
