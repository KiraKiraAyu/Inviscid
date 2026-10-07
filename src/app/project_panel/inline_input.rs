use gpui::prelude::*;
use gpui::*;
use std::ops::Range;
use std::path::PathBuf;

use crate::buffer::TextBuffer;
use crate::ui::text_input::{TextInput, TextInputHost};

/// Target type and parameters for inline file-tree text input (New File, New Folder, Rename).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum InlineInputKind {
    NewFile { parent: PathBuf },
    NewFolder { parent: PathBuf },
    Rename { target: PathBuf, is_dir: bool },
}

/// Events emitted by [`InlineInput`] on user completion or cancellation.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum InlineInputEvent {
    Commit(String),
    Cancel,
}

fn is_valid_filename_char(c: char) -> bool {
    c != '\n' && c != '\r' && c != '/' && c != '\\'
}

/// Project panel inline text input specialized for file-tree entry creation and renaming.
pub struct InlineInput {
    pub(crate) input: TextInput,
    pub(crate) kind: InlineInputKind,
    pub(crate) depth: usize,
}

impl Focusable for InlineInput {
    fn focus_handle(&self, _cx: &App) -> FocusHandle {
        self.input.focus_handle.clone()
    }
}

impl EventEmitter<InlineInputEvent> for InlineInput {}

impl TextInputHost for InlineInput {
    fn text_input(&self) -> &TextInput {
        &self.input
    }

    fn text_input_mut(&mut self) -> &mut TextInput {
        &mut self.input
    }

    fn on_commit(&mut self, cx: &mut Context<Self>) {
        self.commit(cx);
    }

    fn on_cancel(&mut self, cx: &mut Context<Self>) {
        self.cancel(cx);
    }
}

impl EntityInputHandler for InlineInput {
    fn text_for_range(
        &mut self,
        range_utf16: Range<usize>,
        adjusted_range: &mut Option<Range<usize>>,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<String> {
        self.input.text_for_utf16_range(range_utf16, adjusted_range)
    }

    fn selected_text_range(
        &mut self,
        _ignore_disabled_input: bool,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<UTF16Selection> {
        self.input.selected_utf16_range()
    }

    fn marked_text_range(
        &self,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<Range<usize>> {
        self.input.marked_text_range()
    }

    fn unmark_text(&mut self, _window: &mut Window, cx: &mut Context<Self>) {
        self.input.unmark_text();
        cx.notify();
    }

    fn replace_text_in_range(
        &mut self,
        range_utf16: Option<Range<usize>>,
        text: &str,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.input.replace_text_in_utf16_range(range_utf16, text);
        self.on_mutation(cx);
    }

    fn replace_and_mark_text_in_range(
        &mut self,
        range_utf16: Option<Range<usize>>,
        new_text: &str,
        new_selected_range: Option<Range<usize>>,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.input
            .replace_and_mark_utf16_range(range_utf16, new_text, new_selected_range);
        self.on_mutation(cx);
    }

    fn bounds_for_range(
        &mut self,
        range_utf16: Range<usize>,
        element_bounds: Bounds<Pixels>,
        window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<Bounds<Pixels>> {
        self.input
            .bounds_for_utf16_range(range_utf16, element_bounds, window)
    }

    fn character_index_for_point(
        &mut self,
        point: Point<Pixels>,
        window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<usize> {
        self.input.character_index_for_pixel_point(point, window)
    }
}

impl InlineInput {
    pub fn new(
        kind: InlineInputKind,
        initial_text: &str,
        depth: usize,
        cx: &mut Context<Self>,
    ) -> Self {
        let char_count = initial_text.chars().count();
        // For file Rename, pre-select the filename stem (excluding extension).
        let selection = match &kind {
            InlineInputKind::Rename { is_dir, .. } => {
                let stem_len = if *is_dir {
                    char_count
                } else if let Some(dot_idx) = initial_text.rfind('.')
                    && dot_idx > 0
                {
                    initial_text[..dot_idx].chars().count()
                } else {
                    char_count
                };
                0..stem_len
            }
            _ => char_count..char_count,
        };

        let input = TextInput::new_unstarted(initial_text, cx.focus_handle())
            .id("inline_input_box")
            .key_context(crate::ui::key_context::INLINE_INPUT)
            .char_filter(is_valid_filename_char)
            .selection(selection);

        let mut this = Self { input, kind, depth };
        this.input.reset_blink_host(cx);
        this
    }

    #[inline]
    pub fn kind(&self) -> &InlineInputKind {
        &self.kind
    }

    #[inline]
    pub fn depth(&self) -> usize {
        self.depth
    }

    #[inline]
    pub fn text(&self) -> String {
        self.input.text()
    }

    #[inline]
    pub fn input(&self) -> &TextInput {
        &self.input
    }

    #[inline]
    pub fn input_mut(&mut self) -> &mut TextInput {
        &mut self.input
    }

    #[inline]
    pub fn buffer(&self) -> &TextBuffer {
        self.input.buffer()
    }

    #[inline]
    pub fn buffer_mut(&mut self) -> &mut TextBuffer {
        self.input.buffer_mut()
    }

    pub fn commit(&mut self, cx: &mut Context<Self>) {
        let trimmed = self.input.text().trim().to_string();
        cx.emit(InlineInputEvent::Commit(trimmed));
    }

    pub fn cancel(&mut self, cx: &mut Context<Self>) {
        cx.emit(InlineInputEvent::Cancel);
    }
}

impl Render for InlineInput {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let entity = cx.entity().clone();
        self.input.render_hosted(entity, window, cx)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use core::prelude::v1::test;

    #[gpui::test]
    fn test_inline_input_stem_preselection(cx: &mut gpui::TestAppContext) {
        // File with extension: stem should be pre-selected (0..4 for "test.rs")
        let (input, cx) = cx.add_window_view(|_window, cx| {
            InlineInput::new(
                InlineInputKind::Rename {
                    target: PathBuf::from("test.rs"),
                    is_dir: false,
                },
                "test.rs",
                1,
                cx,
            )
        });

        cx.update(|_window, cx| {
            let sel = input.read(cx).buffer().selection();
            assert_eq!(sel.start().col, 0);
            assert_eq!(sel.end().col, 4);
        });

        // Directory: whole name should be selected (0..6 for "folder")
        let (dir_input, cx) = cx.add_window_view(|_window, cx| {
            InlineInput::new(
                InlineInputKind::Rename {
                    target: PathBuf::from("folder"),
                    is_dir: true,
                },
                "folder",
                1,
                cx,
            )
        });

        cx.update(|_window, cx| {
            let sel = dir_input.read(cx).buffer().selection();
            assert_eq!(sel.start().col, 0);
            assert_eq!(sel.end().col, 6);
        });

        // New file: cursor placed at the end
        let (new_file_input, cx) = cx.add_window_view(|_window, cx| {
            InlineInput::new(
                InlineInputKind::NewFile {
                    parent: PathBuf::from("."),
                },
                "untitled.md",
                0,
                cx,
            )
        });

        cx.update(|_window, cx| {
            let sel = new_file_input.read(cx).buffer().selection();
            assert_eq!(sel.start().col, 11);
            assert_eq!(sel.end().col, 11);
        });
    }

    #[test]
    fn test_inline_input_filename_char_filter() {
        assert!(is_valid_filename_char('a'));
        assert!(is_valid_filename_char('-'));
        assert!(is_valid_filename_char('_'));
        assert!(is_valid_filename_char('.'));
        assert!(!is_valid_filename_char('/'));
        assert!(!is_valid_filename_char('\\'));
        assert!(!is_valid_filename_char('\n'));
        assert!(!is_valid_filename_char('\r'));
    }

    #[gpui::test]
    fn test_inline_input_commit_and_cancel(cx: &mut gpui::TestAppContext) {
        let (input, cx) = cx.add_window_view(|_window, cx| {
            InlineInput::new(
                InlineInputKind::NewFile {
                    parent: PathBuf::from("."),
                },
                "  new_file.txt  ",
                0,
                cx,
            )
        });

        let committed = std::rc::Rc::new(std::cell::RefCell::new(None));
        let committed_sub = committed.clone();
        let cancelled = std::rc::Rc::new(std::cell::Cell::new(false));
        let cancelled_sub = cancelled.clone();

        let _sub = cx.update(|_window, cx| {
            cx.subscribe(
                &input,
                move |_emitter, event: &InlineInputEvent, _cx| match event {
                    InlineInputEvent::Commit(text) => {
                        *committed_sub.borrow_mut() = Some(text.clone())
                    }
                    InlineInputEvent::Cancel => cancelled_sub.set(true),
                },
            )
        });

        cx.update(|_window, cx| {
            input.update(cx, |this, cx| {
                this.commit(cx);
            });
        });
        cx.run_until_parked();
        assert_eq!(*committed.borrow(), Some("new_file.txt".to_string()));

        cx.update(|_window, cx| {
            input.update(cx, |this, cx| {
                this.cancel(cx);
            });
        });
        cx.run_until_parked();
        assert!(cancelled.get());
    }
}
