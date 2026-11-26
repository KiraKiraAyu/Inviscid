use std::cell::RefCell;
use std::collections::HashMap;
use std::time::Duration;

use gpui::prelude::*;
use gpui::*;

use crate::buffer::{LineSource, SourceLine, SourceSnapshot, StructureIndex};
use crate::i18n::{ActiveLocale, TextKey};
use crate::markdown::{OutlineItem, collect_outline_from_source, parse_heading_line};
use crate::theme::ThemeManager;
use crate::ui::dock_panel::{self, LIST_VERTICAL_PADDING};
use crate::ui::tokens::FontSize;

const LOADING_INDICATOR_DELAY: Duration = Duration::from_millis(150);
const MAX_CACHED_VISIBLE_ROWS: usize = 256;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum OutlinePanelEvent {
    Select(SourceLine),
}

impl EventEmitter<OutlinePanelEvent> for OutlinePanel {}

struct VirtualOutline {
    source: SourceSnapshot,
    index: StructureIndex,
    heading_count: usize,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct CachedHeadingRow {
    level: u8,
    title: String,
    source_line: SourceLine,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum LoadingIndicatorState {
    Idle,
    Pending,
    Visible,
}

pub struct OutlinePanel {
    focus_handle: FocusHandle,
    document: Option<VirtualOutline>,
    row_cache: RefCell<HashMap<usize, CachedHeadingRow>>,
    loading_state: LoadingIndicatorState,
    loading_delay_task: Option<Task<()>>,
    active_line: Option<SourceLine>,
    editor_focus_handle: Option<FocusHandle>,
    scroll_handle: UniformListScrollHandle,
    outline_scroll: crate::ui::SmoothScrollController,
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
            document: None,
            row_cache: RefCell::new(HashMap::new()),
            loading_state: LoadingIndicatorState::Idle,
            loading_delay_task: None,
            active_line: None,
            editor_focus_handle: None,
            scroll_handle: UniformListScrollHandle::default(),
            outline_scroll: crate::ui::SmoothScrollController::vertical(),
        }
    }

    #[inline]
    fn scroll_access(&mut self) -> (&mut crate::ui::SmoothScrollController, ScrollHandle) {
        let handle = self.scroll_handle.0.borrow().base_handle.clone();
        (&mut self.outline_scroll, handle)
    }

    pub fn heading_count(&self) -> usize {
        self.document.as_ref().map_or(0, |doc| doc.heading_count)
    }

    pub fn entries(&self) -> Vec<OutlineItem> {
        self.document
            .as_ref()
            .map(|doc| collect_outline_from_source(&doc.source))
            .unwrap_or_default()
    }

    pub fn active_line(&self) -> Option<SourceLine> {
        self.active_line
    }

    pub fn is_loading(&self) -> bool {
        self.loading_state == LoadingIndicatorState::Visible
    }

    fn clear_loading(&mut self) {
        self.loading_state = LoadingIndicatorState::Idle;
        self.loading_delay_task = None;
    }

    fn begin_deferred_loading(&mut self, cx: &mut Context<Self>) {
        if self.loading_state != LoadingIndicatorState::Idle {
            return;
        }
        self.loading_state = LoadingIndicatorState::Pending;
        self.loading_delay_task = Some(cx.spawn(async move |panel, cx| {
            cx.background_executor().timer(LOADING_INDICATOR_DELAY).await;
            let _ = panel.update(cx, |panel, cx| {
                if panel.loading_state == LoadingIndicatorState::Pending {
                    panel.loading_state = LoadingIndicatorState::Visible;
                    panel.loading_delay_task = None;
                    cx.notify();
                }
            });
        }));
    }

    pub(crate) fn set_loading_pending(&mut self, cx: &mut Context<Self>) {
        if self.document.is_none()
            && self.loading_state != LoadingIndicatorState::Idle
            && self.active_line.is_none()
            && self.editor_focus_handle.is_none()
        {
            return;
        }
        self.document = None;
        self.row_cache.borrow_mut().clear();
        self.active_line = None;
        self.editor_focus_handle = None;
        self.begin_deferred_loading(cx);
        cx.notify();
    }

    pub(crate) fn clear(&mut self, cx: &mut Context<Self>) {
        if self.document.is_none()
            && self.loading_state == LoadingIndicatorState::Idle
            && self.active_line.is_none()
            && self.editor_focus_handle.is_none()
        {
            return;
        }
        self.document = None;
        self.row_cache.borrow_mut().clear();
        self.active_line = None;
        self.editor_focus_handle = None;
        self.clear_loading();
        cx.notify();
    }

    pub fn set_document(
        &mut self,
        lines: &[String],
        active_line: Option<SourceLine>,
        editor_focus_handle: Option<FocusHandle>,
        cx: &mut Context<Self>,
    ) {
        self.set_snapshot(
            lines.source_snapshot(),
            false,
            active_line,
            editor_focus_handle,
            cx,
        );
    }

    pub(crate) fn set_snapshot(
        &mut self,
        source: SourceSnapshot,
        is_document_loading: bool,
        active_line: Option<SourceLine>,
        editor_focus_handle: Option<FocusHandle>,
        cx: &mut Context<Self>,
    ) {
        let index = source.structure_index();
        let heading_count = index.heading_count();
        let active_heading = active_line.and_then(|line| index.heading_at_or_before(line));
        self.document = Some(VirtualOutline {
            source,
            index,
            heading_count,
        });
        self.row_cache.borrow_mut().clear();
        self.active_line = active_heading;
        self.editor_focus_handle = editor_focus_handle;
        if heading_count == 0 && is_document_loading {
            self.begin_deferred_loading(cx);
        } else {
            self.clear_loading();
        }
        cx.notify();
    }

    pub fn set_active_line(&mut self, line: Option<SourceLine>, cx: &mut Context<Self>) {
        let active_heading = line.and_then(|line| {
            self.document
                .as_ref()?
                .index
                .heading_at_or_before(line)
        });
        if self.active_line != active_heading {
            self.active_line = active_heading;
            cx.notify();
        }
    }

    fn rows_for_range(&self, range: std::ops::Range<usize>) -> Vec<(usize, CachedHeadingRow)> {
        let Some(doc) = self.document.as_ref() else {
            return Vec::new();
        };
        let start = range.start.min(doc.heading_count);
        let end = range.end.min(doc.heading_count);
        if start >= end {
            return Vec::new();
        }

        let needs_populate = {
            let cache = self.row_cache.borrow();
            (start..end).any(|ordinal| !cache.contains_key(&ordinal))
        };
        if needs_populate {
            let mut cache = self.row_cache.borrow_mut();
            if cache.len() + (end - start) > MAX_CACHED_VISIBLE_ROWS {
                cache.clear();
            }
            for (offset, (source_line, quote_prefix)) in
                doc.index.heading_entries_in_range(start..end).into_iter().enumerate()
            {
                let ordinal = start + offset;
                if cache.contains_key(&ordinal) {
                    continue;
                }
                let raw_line = doc.source.source_line(source_line);
                let (level, title) = parse_heading_line(&raw_line, quote_prefix).unwrap_or_else(|| {
                    let fallback = raw_line
                        .get(quote_prefix..)
                        .unwrap_or(raw_line.as_ref())
                        .trim()
                        .to_string();
                    (1, fallback)
                });
                cache.insert(
                    ordinal,
                    CachedHeadingRow {
                        level,
                        title,
                        source_line,
                    },
                );
            }
        }

        let cache = self.row_cache.borrow();
        (start..end)
            .filter_map(|ordinal| cache.get(&ordinal).cloned().map(|row| (ordinal, row)))
            .collect()
    }

    fn select(&mut self, line: SourceLine, cx: &mut Context<Self>) {
        cx.emit(OutlinePanelEvent::Select(line));
    }

    fn render_entries(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let panel = cx.entity().downgrade();
        crate::ui::SmoothScrollController::wrap(
            uniform_list(
                "outline_entries",
                self.heading_count(),
                move |range, _window, cx| {
                    let Some(panel) = panel.upgrade() else {
                        return Vec::new();
                    };
                    let theme = cx.global::<ThemeManager>().theme();
                    let this = panel.read(cx);
                    let editor_focus_handle = this.editor_focus_handle.clone();
                    let active_line = this.active_line;
                    this.rows_for_range(range)
                        .into_iter()
                        .map(|(index, entry)| {
                            let selected = active_line.is_some_and(|line| line == entry.source_line);
                            let indent = px(10.0 + (entry.level.saturating_sub(1) as f32 * 14.0));
                            let panel_click = panel.downgrade();
                            let editor_focus_handle = editor_focus_handle.clone();
                            let label = if entry.title.is_empty() {
                                cx.t(TextKey::OutlineUntitledHeading).to_string()
                            } else {
                                entry.title
                            };
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
                                .into_any_element()
                        })
                        .collect()
                },
            )
            .track_scroll(self.scroll_handle.clone())
            .flex_1()
            .w_full()
            .px(crate::ui::tokens::Spacing::SM)
            .py(LIST_VERTICAL_PADDING),
            cx,
            Self::scroll_access,
        )
    }
}

impl Render for OutlinePanel {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        crate::ui::SmoothScrollController::continue_on_window(
            self,
            window,
            cx,
            Self::scroll_access,
        );
        let theme = cx.global::<ThemeManager>().theme().clone();
        let entries = if self.heading_count() > 0 {
            self.render_entries(cx).into_any_element()
        } else {
            match self.loading_state {
                LoadingIndicatorState::Visible => {
                    dock_panel::loading_state("outline_loading_metaball", &theme).into_any_element()
                }
                LoadingIndicatorState::Pending => div().w_full().h_full().into_any_element(),
                LoadingIndicatorState::Idle => {
                    dock_panel::empty_state(cx.t(TextKey::OutlineNoHeadings).to_string(), &theme)
                        .into_any_element()
                }
            }
        };

        div()
            .id("outline_panel")
            .key_context(crate::ui::key_context::WORKSPACE)
            .track_focus(&self.focus_handle)
            .size_full()
            .bg(theme.bg_app)
            .flex()
            .flex_col()
            .overflow_hidden()
            .child(dock_panel::section_header(
                cx.t(TextKey::OutlineHeader).to_string(),
                &theme,
            ))
            .child(entries)
    }
}
