use gpui::prelude::*;
use gpui::*;

use crate::buffer::SourceLine;
use crate::markdown::{OutlineItem, collect_outline};
use crate::theme::ThemeManager;
use crate::ui::dock_panel::{self, LIST_VERTICAL_PADDING};
use crate::ui::tokens::FontSize;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum OutlinePanelEvent {
    Select(SourceLine),
}

impl EventEmitter<OutlinePanelEvent> for OutlinePanel {}

pub struct OutlinePanel {
    focus_handle: FocusHandle,
    entries: Vec<OutlineItem>,
    active_line: Option<SourceLine>,
    editor_focus_handle: Option<FocusHandle>,
    scroll_handle: UniformListScrollHandle,
}

impl Focusable for OutlinePanel {
    fn focus_handle(&self, _cx: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl OutlinePanel {
    pub fn new(focus_handle: FocusHandle) -> Self {
        Self {
            focus_handle,
            entries: Vec::new(),
            active_line: None,
            editor_focus_handle: None,
            scroll_handle: UniformListScrollHandle::default(),
        }
    }

    pub fn entries(&self) -> &[OutlineItem] {
        &self.entries
    }

    pub fn active_line(&self) -> Option<SourceLine> {
        self.active_line
    }

    pub fn set_document(
        &mut self,
        lines: &[String],
        active_line: Option<SourceLine>,
        editor_focus_handle: Option<FocusHandle>,
        cx: &mut Context<Self>,
    ) {
        let entries = collect_outline(lines);
        let active_heading = active_line.and_then(|line| {
            entries
                .iter()
                .rev()
                .find(|entry| entry.source_line <= line)
                .map(|entry| entry.source_line)
        });
        if self.entries == entries
            && self.active_line == active_heading
            && self.editor_focus_handle == editor_focus_handle
        {
            return;
        }
        self.entries = entries;
        self.active_line = active_heading;
        self.editor_focus_handle = editor_focus_handle;
        cx.notify();
    }

    fn select(&mut self, line: SourceLine, cx: &mut Context<Self>) {
        cx.emit(OutlinePanelEvent::Select(line));
    }

    fn render_entries(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let panel = cx.entity().downgrade();
        uniform_list(
            "outline_entries",
            self.entries.len(),
            move |range, _window, cx| {
                let Some(panel) = panel.upgrade() else {
                    return Vec::new();
                };
                let theme = cx.global::<ThemeManager>().theme();
                let this = panel.read(cx);
                range
                    .filter_map(|index| {
                        let entry = this.entries.get(index)?.clone();
                        let selected = this
                            .active_line
                            .is_some_and(|line| line == entry.source_line);
                        let indent = px(10.0 + (entry.level.saturating_sub(1) as f32 * 14.0));
                        let panel_click = panel.downgrade();
                        let editor_focus_handle = this.editor_focus_handle.clone();
                        let label = if entry.title.is_empty() {
                            "Untitled heading".to_string()
                        } else {
                            entry.title.clone()
                        };
                        Some(
                            dock_panel::row(indent, theme)
                                .id(("outline_entry", index))
                                .on_click(move |_, window, cx| {
                                    if let Some(focus_handle) = &editor_focus_handle {
                                        window.focus(focus_handle);
                                    }
                                    if let Some(panel) = panel_click.upgrade() {
                                        panel.update(cx, |this, cx| {
                                            this.select(entry.source_line, cx);
                                        });
                                    }
                                })
                                .child(
                                    div()
                                        .min_w_0()
                                        .overflow_hidden()
                                        .whitespace_nowrap()
                                        .text_size(FontSize::SM)
                                        .text_color(if selected {
                                            theme.text_accent
                                        } else {
                                            theme.text_secondary
                                        })
                                        .flex_1()
                                        .child(label),
                                )
                                .into_any_element(),
                        )
                    })
                    .collect()
            },
        )
        .track_scroll(self.scroll_handle.clone())
        .flex_1()
        .w_full()
        .py(LIST_VERTICAL_PADDING)
    }
}

impl Render for OutlinePanel {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.global::<ThemeManager>().theme().clone();
        let entries = if self.entries.is_empty() {
            dock_panel::empty_state("No headings", &theme).into_any_element()
        } else {
            self.render_entries(cx).into_any_element()
        };

        div()
            .id("outline_panel")
            .key_context(crate::ui::key_context::WORKSPACE)
            .track_focus(&self.focus_handle)
            .size_full()
            .bg(theme.bg_toolbar)
            .flex()
            .flex_col()
            .overflow_hidden()
            .child(entries)
    }
}
