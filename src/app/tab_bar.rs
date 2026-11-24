use crate::app::project_panel::DraggedProjectEntry;
use crate::theme::{DEFAULT_THEME, ThemeManager};
use crate::ui::{FontSize, Icon, IconName, IconSize, Radius, Spacing};
use gpui::prelude::*;
use gpui::*;
use std::path::PathBuf;

pub mod context_menu;
use context_menu::{TabContextMenuAction, TabContextMenuState};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TabMeta {
    pub is_loading: bool,
    pub title: String,
    pub is_dirty: bool,
    pub dir_hint: Option<String>,
    pub is_pinned: bool,
    pub is_read_only: bool,
    pub has_file_path: bool,
    pub supports_editing: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct DraggedTab {
    pub ix: usize,
    pub title: String,
    pub dir_hint: Option<String>,
    pub is_dirty: bool,
    pub is_pinned: bool,
    pub is_read_only: bool,
    pub is_search_tab: bool,
}

impl Render for DraggedTab {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx
            .try_global::<ThemeManager>()
            .map(|m| m.theme())
            .unwrap_or(&DEFAULT_THEME);
        div()
            .font_family(crate::platform::platform_ui_font())
            .h(px(28.0))
            .px(Spacing::MD)
            .bg(theme.bg_elevated)
            .border_1()
            .border_color(theme.border)
            .rounded(Radius::LG)
            .shadow_md()
            .flex()
            .flex_row()
            .items_center()
            .gap(Spacing::SMD)
            .when(self.is_read_only, |this| {
                this.child(
                    Icon::new(IconName::Lock)
                        .size(IconSize::Indicator)
                        .color(theme.text_secondary),
                )
            })
            .when(self.is_search_tab, |this| {
                this.child(
                    Icon::new(IconName::Search)
                        .size(IconSize::Indicator)
                        .color(theme.text_secondary),
                )
            })
            .child(
                div()
                    .text_size(FontSize::SM)
                    .line_height(relative(1.0))
                    .whitespace_nowrap()
                    .text_color(theme.text_primary)
                    .child(self.title.clone()),
            )
            .when_some(self.dir_hint.clone(), |this, hint| {
                this.child(
                    div()
                        .text_size(FontSize::CAPTION)
                        .line_height(relative(1.0))
                        .whitespace_nowrap()
                        .text_color(theme.text_muted)
                        .child(hint),
                )
            })
            .when(self.is_dirty, |this| {
                this.child(
                    div()
                        .w(px(5.0))
                        .h(px(5.0))
                        .rounded_full()
                        .bg(theme.inline_code_fg),
                )
            })
            .when(self.is_pinned, |this| {
                this.child(
                    Icon::new(IconName::Pin)
                        .size(IconSize::XSmall)
                        .color(theme.text_accent),
                )
            })
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TabBarEvent {
    Select(usize),
    Close(usize),
    CloseOthers(usize),
    CloseLeft(usize),
    CloseRight(usize),
    CloseClean,
    CloseAll,
    ToggleReadOnly(usize),
    CopyPath(usize),
    CopyRelativePath(usize),
    RevealInFileExplorer(usize),
    TogglePin(usize),
    RevealInProjectPanel(usize),
    MoveTab {
        from_idx: usize,
        to_idx: usize,
    },
    MoveTabToEnd(usize),
    DropProjectEntry {
        path: PathBuf,
        target_idx: Option<usize>,
    },
    EdgeFlushChanged,
}

impl EventEmitter<TabBarEvent> for TabBar {}

pub struct TabBar {
    tabs_meta: Vec<TabMeta>,
    duplicate_titles: std::collections::HashSet<String>,
    active_tab_idx: usize,
    has_workspace_root: bool,
    sidebar_offset: Pixels,
    active_context_menu: Option<TabContextMenuState>,
    scroll_handle: ScrollHandle,
    tab_scroll: crate::ui::SmoothScrollController,
    last_edge_flush: (bool, bool),
}

#[derive(Clone, Copy)]
enum FilletSide {
    Left,
    Right,
}

fn apply_tab_drop_indicator(
    style: StyleRefinement,
    side: FilletSide,
    hover_bg: Hsla,
    accent: Hsla,
) -> StyleRefinement {
    let style = style.bg(hover_bg).border_color(accent);
    match side {
        FilletSide::Left => style.border_l_2().rounded_tl(px(0.0)).rounded_bl(px(0.0)),
        FilletSide::Right => style.border_r_2().rounded_tr(px(0.0)).rounded_br(px(0.0)),
    }
}

/// Paints an outward concave corner fillet (`10x11`) whose bottom `1px` apron overlaps the
/// editor top edge to prevent subpixel seams at fractional display scaling.
fn render_tab_concave_fillet(side: FilletSide, color: Hsla) -> impl IntoElement {
    let offset = px(-10.0);
    canvas(
        |_bounds, _window, _cx| {},
        move |bounds, (), window, _cx| {
            let r = px(10.0);
            let x0 = bounds.origin.x;
            let y0 = bounds.origin.y;
            let x1 = x0 + r;
            let y_arc = y0 + r;
            let y1 = y0 + px(11.0);

            let mut builder = PathBuilder::fill();
            match side {
                FilletSide::Left => {
                    builder.move_to(point(x1, y0));
                    builder.line_to(point(x1, y1));
                    builder.line_to(point(x0, y1));
                    builder.line_to(point(x0, y_arc));
                    builder.arc_to(point(r, r), px(0.0), false, false, point(x1, y0));
                }
                FilletSide::Right => {
                    builder.move_to(point(x0, y0));
                    builder.line_to(point(x0, y1));
                    builder.line_to(point(x1, y1));
                    builder.line_to(point(x1, y_arc));
                    builder.arc_to(point(r, r), px(0.0), false, true, point(x0, y0));
                }
            }
            builder.close();
            if let Ok(path) = builder.build() {
                window.paint_path(path, color);
            }
        },
    )
    .absolute()
    .bottom_0()
    .when(matches!(side, FilletSide::Left), |this| this.left(offset))
    .when(matches!(side, FilletSide::Right), |this| this.right(offset))
    .w(px(10.0))
    .h(px(11.0))
}

impl TabBar {
    pub const HEIGHT: Pixels = px(35.0);

    /// Returns `(flush_left, flush_right)` indicating whether the active tab's left or right
    /// edge touches the tab viewport boundary, so the canvas drops the matching top corner radius.
    pub fn active_tab_edge_flush(&self) -> (bool, bool) {
        if self.tabs_meta.is_empty() {
            return (false, false);
        }
        let offset_x = self.scroll_handle.offset().x;
        let viewport = self.scroll_handle.bounds();
        if viewport.size.width > px(0.0)
            && let Some(item_bounds) = self.scroll_handle.bounds_for_item(self.active_tab_idx)
        {
            let vis_left = item_bounds.left() + offset_x;
            let vis_right = item_bounds.right() + offset_x;
            return (
                vis_left <= viewport.left() + px(1.0),
                vis_right >= viewport.right() - px(1.0),
            );
        }
        let max_x = self.scroll_handle.max_offset().width;
        let flush_left = self.active_tab_idx == 0 && (-offset_x) <= px(1.0);
        let flush_right = max_x > px(0.0)
            && self.active_tab_idx + 1 == self.tabs_meta.len()
            && (-offset_x) >= max_x - px(1.0);
        (flush_left, flush_right)
    }

    fn scroll_active_tab_into_view(&self) {
        self.scroll_handle.scroll_to_item(self.active_tab_idx);
        let viewport = self.scroll_handle.bounds();
        if viewport.size.width > px(0.0)
            && let Some(item_bounds) = self.scroll_handle.bounds_for_item(self.active_tab_idx)
        {
            let mut offset = self.scroll_handle.offset();
            if item_bounds.left() + offset.x < viewport.left() {
                offset.x = viewport.left() - item_bounds.left();
            } else if item_bounds.right() + offset.x > viewport.right() {
                offset.x = viewport.right() - item_bounds.right();
            }
            self.scroll_handle.set_offset(offset);
        }
    }

    fn sync_edge_flush(&mut self, cx: &mut Context<Self>) {
        let edge = self.active_tab_edge_flush();
        if edge != self.last_edge_flush {
            self.last_edge_flush = edge;
            cx.emit(TabBarEvent::EdgeFlushChanged);
            cx.notify();
        }
    }

    fn compute_duplicate_titles(tabs: &[TabMeta]) -> std::collections::HashSet<String> {
        let mut counts = std::collections::HashMap::new();
        for t in tabs {
            *counts.entry(&t.title).or_insert(0usize) += 1;
        }
        counts
            .into_iter()
            .filter_map(|(title, count)| if count > 1 { Some(title.clone()) } else { None })
            .collect()
    }

    pub fn new(tabs_meta: Vec<TabMeta>, active_tab_idx: usize) -> Self {
        let duplicate_titles = Self::compute_duplicate_titles(&tabs_meta);
        let scroll_handle = ScrollHandle::new();
        if !tabs_meta.is_empty() {
            scroll_handle.scroll_to_item(active_tab_idx);
        }
        let last_edge_flush = (active_tab_idx == 0 && !tabs_meta.is_empty(), false);
        Self {
            tabs_meta,
            duplicate_titles,
            active_tab_idx,
            has_workspace_root: false,
            sidebar_offset: px(0.0),
            active_context_menu: None,
            scroll_handle,
            tab_scroll: crate::ui::SmoothScrollController::horizontal(),
            last_edge_flush,
        }
    }

    pub fn sync(
        &mut self,
        tabs_meta: Vec<TabMeta>,
        active_tab_idx: usize,
        has_workspace_root: bool,
        sidebar_offset: Pixels,
        cx: &mut Context<Self>,
    ) {
        let active_changed = self.active_tab_idx != active_tab_idx;
        let tabs_changed = self.tabs_meta != tabs_meta;
        let tabs_len_changed = self.tabs_meta.len() != tabs_meta.len();
        let changed = tabs_changed
            || active_changed
            || self.has_workspace_root != has_workspace_root
            || self.sidebar_offset != sidebar_offset;
        if changed {
            if tabs_changed {
                self.duplicate_titles = Self::compute_duplicate_titles(&tabs_meta);
                self.tabs_meta = tabs_meta;
            }
            self.active_tab_idx = active_tab_idx;
            self.has_workspace_root = has_workspace_root;
            self.sidebar_offset = sidebar_offset;
            if let Some(ref menu) = self.active_context_menu
                && menu.tab_idx >= self.tabs_meta.len()
            {
                self.active_context_menu = None;
            }
            // Only scroll to the active item if active tab actually changed or tabs count changed.
            // Avoids resetting user's manual scroll position on sidebar drag or tab dirty changes.
            if active_changed || tabs_len_changed {
                if tabs_changed {
                    self.scroll_handle.scroll_to_item(active_tab_idx);
                } else {
                    self.scroll_active_tab_into_view();
                }
            }
            self.last_edge_flush = self.active_tab_edge_flush();
            cx.notify();
        }
    }

    pub fn set_sidebar_offset(&mut self, offset: Pixels, cx: &mut Context<Self>) {
        if self.sidebar_offset != offset {
            self.sidebar_offset = offset;
            cx.notify();
        }
    }

    pub fn active_context_menu(&self) -> Option<&TabContextMenuState> {
        self.active_context_menu.as_ref()
    }

    pub fn open_context_menu(&mut self, state: TabContextMenuState, cx: &mut Context<Self>) {
        self.active_context_menu = Some(state);
        cx.notify();
    }

    pub fn close_context_menu(&mut self, cx: &mut Context<Self>) {
        if self.active_context_menu.is_some() {
            self.active_context_menu = None;
            cx.notify();
        }
    }

    pub fn execute_context_menu_action(
        tab_bar: &Entity<Self>,
        action: TabContextMenuAction,
        _window: &mut Window,
        cx: &mut App,
    ) {
        tab_bar.update(cx, |this, cx| {
            let Some(menu) = this.active_context_menu.take() else {
                return;
            };
            cx.notify();
            let tab_idx = menu.tab_idx;
            let event = match action {
                TabContextMenuAction::Close => TabBarEvent::Close(tab_idx),
                TabContextMenuAction::CloseOthers => TabBarEvent::CloseOthers(tab_idx),
                TabContextMenuAction::CloseLeft => TabBarEvent::CloseLeft(tab_idx),
                TabContextMenuAction::CloseRight => TabBarEvent::CloseRight(tab_idx),
                TabContextMenuAction::CloseClean => TabBarEvent::CloseClean,
                TabContextMenuAction::CloseAll => TabBarEvent::CloseAll,
                TabContextMenuAction::ToggleReadOnly => TabBarEvent::ToggleReadOnly(tab_idx),
                TabContextMenuAction::CopyPath => TabBarEvent::CopyPath(tab_idx),
                TabContextMenuAction::CopyRelativePath => TabBarEvent::CopyRelativePath(tab_idx),
                TabContextMenuAction::RevealInFileExplorer => {
                    TabBarEvent::RevealInFileExplorer(tab_idx)
                }
                TabContextMenuAction::TogglePin => TabBarEvent::TogglePin(tab_idx),
                TabContextMenuAction::RevealInProjectPanel => {
                    TabBarEvent::RevealInProjectPanel(tab_idx)
                }
            };
            cx.emit(event);
        });
    }

    pub fn set_tab_dirty(&mut self, idx: usize, is_dirty: bool, cx: &mut Context<Self>) {
        if let Some(tab) = self.tabs_meta.get_mut(idx)
            && tab.is_dirty != is_dirty
        {
            tab.is_dirty = is_dirty;
            cx.notify();
        }
    }

    pub fn set_tab_title(
        &mut self,
        idx: usize,
        title: String,
        dir_hint: Option<String>,
        cx: &mut Context<Self>,
    ) {
        if let Some(tab) = self.tabs_meta.get_mut(idx)
            && (tab.title != title || tab.dir_hint != dir_hint)
        {
            tab.title = title;
            tab.dir_hint = dir_hint;
            self.duplicate_titles = Self::compute_duplicate_titles(&self.tabs_meta);
            cx.notify();
        }
    }

    pub fn tabs_meta(&self) -> &[TabMeta] {
        &self.tabs_meta
    }

    pub fn active_tab_idx(&self) -> usize {
        self.active_tab_idx
    }

    #[inline]
    pub fn scroll_handle(&self) -> &ScrollHandle {
        &self.scroll_handle
    }

    /// Returns whether the tab at `idx` should display its directory hint.
    /// A directory hint is only shown if there are multiple tabs sharing the same title.
    pub(crate) fn should_show_dir_hint(&self, idx: usize) -> bool {
        let Some(target) = self.tabs_meta.get(idx) else {
            return false;
        };
        if target.dir_hint.is_none() {
            return false;
        }
        self.duplicate_titles.contains(&target.title)
    }

    /// Returns the effective directory hint to display for the tab at `idx`.
    /// Returns `Some(hint)` only if the tab has a directory hint AND there are multiple tabs with the same title.
    pub fn effective_dir_hint(&self, idx: usize) -> Option<&str> {
        if self.should_show_dir_hint(idx) {
            self.tabs_meta.get(idx).and_then(|t| t.dir_hint.as_deref())
        } else {
            None
        }
    }

    /// Scrolls specified tab item into visible viewport
    pub fn scroll_to_tab(&mut self, idx: usize, cx: &mut Context<Self>) {
        self.scroll_handle.scroll_to_item(idx);
        cx.notify();
    }

    #[inline]
    fn scroll_access(&mut self) -> (&mut crate::ui::SmoothScrollController, ScrollHandle) {
        let handle = self.scroll_handle.clone();
        (&mut self.tab_scroll, handle)
    }

    pub fn handle_scroll_wheel(&mut self, event: &ScrollWheelEvent, cx: &mut Context<Self>) {
        crate::ui::SmoothScrollController::handle_wheel_with(
            self,
            event,
            None,
            cx,
            Self::scroll_access,
            Self::sync_edge_flush,
        );
    }

    pub fn handle_scroll_wheel_in_window(
        &mut self,
        event: &ScrollWheelEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        crate::ui::SmoothScrollController::handle_wheel_with(
            self,
            event,
            Some(window),
            cx,
            Self::scroll_access,
            Self::sync_edge_flush,
        );
    }
}

impl Render for TabBar {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        crate::ui::SmoothScrollController::continue_on_window_with(
            self,
            window,
            cx,
            Self::scroll_access,
            Self::sync_edge_flush,
        );
        let theme = cx
            .try_global::<ThemeManager>()
            .map(|m| m.theme())
            .unwrap_or(&DEFAULT_THEME);
        let active_tab_idx = self.active_tab_idx;

        let this_weak = cx.entity().downgrade();
        let on_action = {
            let this_weak = this_weak.clone();
            move |action, window: &mut Window, cx: &mut App| {
                if let Some(this) = this_weak.upgrade() {
                    TabBar::execute_context_menu_action(&this, action, window, cx);
                }
            }
        };
        let on_close = {
            let this_weak = this_weak.clone();
            move |_window: &mut Window, cx: &mut App| {
                if let Some(this) = this_weak.upgrade() {
                    this.update(cx, |this, cx| {
                        this.close_context_menu(cx);
                    });
                }
            }
        };

        let sidebar_width = self.sidebar_offset;
        let duplicate_titles = &self.duplicate_titles;
        let edge_flush = self.active_tab_edge_flush();
        self.last_edge_flush = edge_flush;
        let edge_watch_weak = this_weak.clone();

        div()
            .w_full()
            .h(Self::HEIGHT)
            .flex_none()
            .bg(transparent_black())
            .relative()
            .flex()
            .flex_row()
            .items_end()
            .justify_between()
            .child(crate::ui::SmoothScrollController::wrap_with(
                div()
                    .id("tab_bar_scroll")
                    .track_scroll(&self.scroll_handle)
                    .overflow_x_scroll()
                    .flex_1()
                    .h_full()
                    .flex()
                    .flex_row()
                    .items_end()
                    .gap(Spacing::XS)
                    .on_drop(cx.listener(|_this, dragged: &DraggedTab, _window, cx| {
                        cx.emit(TabBarEvent::MoveTabToEnd(dragged.ix));
                    }))
                    .on_drop(
                        cx.listener(|_this, entry: &DraggedProjectEntry, _window, cx| {
                            if !entry.is_dir {
                                cx.emit(TabBarEvent::DropProjectEntry {
                                    path: entry.path.clone(),
                                    target_idx: None,
                                });
                            }
                        }),
                    )
                    .children(self.tabs_meta.iter().enumerate().map(|(tab_idx, meta)| {
                        let is_active = tab_idx == active_tab_idx;
                        let is_pinned = meta.is_pinned;
                        let fname = meta.title.clone();
                        let is_d = meta.is_dirty;
                        let is_search_tab =
                            meta.title == "Search" && !meta.has_file_path && !meta.supports_editing;
                        let has_duplicate_title = duplicate_titles.contains(&meta.title);
                        let dir_hint = if has_duplicate_title {
                            meta.dir_hint.clone()
                        } else {
                            None
                        };
                        let inactive_hover = theme.btn_hover.opacity(0.55);
                        let hover_bg = if is_active {
                            theme.bg_editor
                        } else {
                            inactive_hover
                        };
                        let accent = theme.text_accent;
                        let dragged_tab = DraggedTab {
                            ix: tab_idx,
                            title: fname.clone(),
                            dir_hint: dir_hint.clone(),
                            is_dirty: is_d,
                            is_pinned,
                            is_read_only: meta.is_read_only,
                            is_search_tab,
                        };
                        div()
                            .id(ElementId::NamedInteger("tab_item".into(), tab_idx as u64))
                            .flex_none()
                            .group("tab")
                            .relative()
                            .flex()
                            .flex_row()
                            .items_center()
                            .justify_center()
                            .pl(Spacing::LG)
                            .pr(Spacing::MDS)
                            .gap(Spacing::SMD)
                            .when(is_active, |this| {
                                this.h(Self::HEIGHT)
                                    .pb(px(1.0))
                                    .rounded_tl(Radius::TAB)
                                    .rounded_tr(Radius::TAB)
                                    .bg(theme.bg_editor)
                                    .when(!edge_flush.0, |this| {
                                        this.child(render_tab_concave_fillet(
                                            FilletSide::Left,
                                            theme.bg_editor,
                                        ))
                                    })
                                    .when(!edge_flush.1, |this| {
                                        this.child(render_tab_concave_fillet(
                                            FilletSide::Right,
                                            theme.bg_editor,
                                        ))
                                    })
                            })
                            .when(!is_active, |this| {
                                this.h(px(24.0))
                                    .mb(px(6.0))
                                    .rounded(Radius::LG)
                                    .bg(transparent_black())
                                    .hover(move |s| s.bg(inactive_hover))
                            })
                            .cursor_pointer()
                            .on_drag(dragged_tab, |dragged, _offset, _window, cx| {
                                cx.new(|_cx| dragged.clone())
                            })
                            .drag_over::<DraggedTab>(move |style, dragged: &DraggedTab, _, _| {
                                if dragged.ix < tab_idx {
                                    apply_tab_drop_indicator(
                                        style,
                                        FilletSide::Right,
                                        hover_bg,
                                        accent,
                                    )
                                } else if dragged.ix > tab_idx {
                                    apply_tab_drop_indicator(
                                        style,
                                        FilletSide::Left,
                                        hover_bg,
                                        accent,
                                    )
                                } else {
                                    style
                                }
                            })
                            .drag_over::<DraggedProjectEntry>(move |style, entry, _, _| {
                                if !entry.is_dir {
                                    apply_tab_drop_indicator(
                                        style,
                                        FilletSide::Left,
                                        hover_bg,
                                        accent,
                                    )
                                } else {
                                    style
                                }
                            })
                            .on_drop(cx.listener(
                                move |_this, dragged: &DraggedTab, _window, cx| {
                                    if dragged.ix != tab_idx {
                                        cx.emit(TabBarEvent::MoveTab {
                                            from_idx: dragged.ix,
                                            to_idx: tab_idx,
                                        });
                                    }
                                },
                            ))
                            .on_drop(cx.listener(
                                move |_this, entry: &DraggedProjectEntry, _window, cx| {
                                    if !entry.is_dir {
                                        cx.emit(TabBarEvent::DropProjectEntry {
                                            path: entry.path.clone(),
                                            target_idx: Some(tab_idx),
                                        });
                                    }
                                },
                            ))
                            .on_click(cx.listener(move |_this, _event, _window, cx| {
                                cx.emit(TabBarEvent::Select(tab_idx));
                            }))
                            .on_mouse_down(
                                MouseButton::Middle,
                                cx.listener(move |_this, _event: &MouseDownEvent, _window, cx| {
                                    cx.emit(TabBarEvent::Close(tab_idx));
                                }),
                            )
                            .on_mouse_down(
                                MouseButton::Right,
                                cx.listener(move |this, event: &MouseDownEvent, _window, cx| {
                                    let tab_count = this.tabs_meta.len();
                                    let Some(target_meta) = this.tabs_meta.get(tab_idx) else {
                                        return;
                                    };
                                    let has_clean_tabs =
                                        this.tabs_meta.iter().any(|m| !m.is_dirty && !m.is_pinned);
                                    let state = context_menu::TabContextMenuState {
                                        tab_idx,
                                        position: event.position,
                                        is_pinned: target_meta.is_pinned,
                                        is_read_only: target_meta.is_read_only,
                                        has_file_path: target_meta.has_file_path,
                                        is_whitelisted: target_meta.supports_editing,
                                        tab_count,
                                        has_clean_tabs,
                                        has_workspace_root: this.has_workspace_root,
                                    };
                                    this.open_context_menu(state, cx);
                                }),
                            )
                            .when(meta.is_read_only, |this| {
                                this.child(
                                    Icon::new(IconName::Lock).size(IconSize::Indicator).color(
                                        if is_active {
                                            theme.text_secondary
                                        } else {
                                            theme.text_muted
                                        },
                                    ),
                                )
                            })
                            .when(is_search_tab, |this| {
                                this.child(
                                    Icon::new(IconName::Search).size(IconSize::Indicator).color(
                                        if is_active {
                                            theme.text_secondary
                                        } else {
                                            theme.text_muted
                                        },
                                    ),
                                )
                            })
                            .child(
                                div()
                                    .text_size(FontSize::SM)
                                    .line_height(relative(1.0))
                                    .whitespace_nowrap()
                                    .flex()
                                    .items_center()
                                    .text_color(if is_active {
                                        theme.text_primary
                                    } else {
                                        theme.text_muted
                                    })
                                    .child(fname),
                            )
                            .when(meta.is_loading, |this| {
                                this.child(
                                    div()
                                        .text_size(FontSize::CAPTION)
                                        .text_color(theme.text_muted)
                                        .child("Loading..."),
                                )
                            })
                            .children(dir_hint.map(|hint| {
                                div()
                                    .text_size(FontSize::CAPTION)
                                    .line_height(relative(1.0))
                                    .whitespace_nowrap()
                                    .flex()
                                    .items_center()
                                    .text_color(theme.text_muted)
                                    .child(hint)
                            }))
                            .when(is_d, |this| {
                                this.child(
                                    div()
                                        .w(px(5.0))
                                        .h(px(5.0))
                                        .rounded_full()
                                        .bg(theme.inline_code_fg),
                                )
                            })
                            .child(if is_pinned {
                                div()
                                    .id(ElementId::NamedInteger("tab_pin".into(), tab_idx as u64))
                                    .ml(Spacing::XS)
                                    .flex_none()
                                    .w(IconSize::Medium.pixels())
                                    .h(IconSize::Medium.pixels())
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .rounded(Radius::SM)
                                    .hover(|s| s.bg(theme.btn_hover))
                                    .active(|s| s.bg(theme.btn_active))
                                    .on_mouse_down(MouseButton::Left, |_, _, cx| {
                                        cx.stop_propagation();
                                    })
                                    .on_click(cx.listener(move |_this, _event, _window, cx| {
                                        cx.stop_propagation();
                                        cx.emit(TabBarEvent::TogglePin(tab_idx));
                                    }))
                                    .child(Icon::new(IconName::Pin).size(IconSize::XSmall).color(
                                        if is_active {
                                            theme.text_accent
                                        } else {
                                            theme.text_muted
                                        },
                                    ))
                            } else {
                                div()
                                    .id(ElementId::NamedInteger("tab_close".into(), tab_idx as u64))
                                    .ml(Spacing::XS)
                                    .flex_none()
                                    .w(IconSize::Medium.pixels())
                                    .h(IconSize::Medium.pixels())
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .rounded(Radius::SM)
                                    .opacity(0.0)
                                    .group_hover("tab", |s| s.opacity(1.0))
                                    .hover(|s| s.bg(theme.btn_hover))
                                    .active(|s| s.bg(theme.btn_active))
                                    .on_mouse_down(MouseButton::Left, |_, _, cx| {
                                        cx.stop_propagation();
                                    })
                                    .on_click(cx.listener(move |_this, _event, _window, cx| {
                                        cx.stop_propagation();
                                        cx.emit(TabBarEvent::Close(tab_idx));
                                    }))
                                    .child(
                                        Icon::new(IconName::Close)
                                            .size(IconSize::Indicator)
                                            .color(theme.text_secondary),
                                    )
                            })
                    })),
                cx,
                Self::scroll_access,
                Self::sync_edge_flush,
            ))
            .child(
                canvas(
                    |_bounds, _window, _cx| {},
                    move |_bounds, (), _window, cx| {
                        let Some(tb) = edge_watch_weak.upgrade() else {
                            return;
                        };
                        if tb.read(cx).active_tab_edge_flush() != edge_flush {
                            let tb_defer = edge_watch_weak.clone();
                            cx.defer(move |cx| {
                                if let Some(tb) = tb_defer.upgrade() {
                                    tb.update(cx, |this, cx| this.sync_edge_flush(cx));
                                }
                            });
                        }
                    },
                )
                .size_0()
                .absolute(),
            )
            .when_some(self.active_context_menu.clone(), |this, menu_state| {
                let safe_bounds = crate::app::workspace::workspace_safe_bounds(window.bounds());
                this.child(deferred(context_menu::render_tab_context_menu(
                    &menu_state,
                    safe_bounds,
                    sidebar_width,
                    on_action,
                    on_close,
                    cx,
                )))
            })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use core::prelude::v1::test;

    #[test]
    fn test_disambiguate_tab_directory_hint_only_on_duplicate_titles() {
        let tabs = vec![
            TabMeta {
                is_loading: false,
                title: "main.rs".to_string(),
                is_dirty: false,
                dir_hint: Some("src".to_string()),
                is_pinned: false,
                is_read_only: false,
                has_file_path: true,
                supports_editing: true,
            },
            TabMeta {
                is_loading: false,
                title: "lib.rs".to_string(),
                is_dirty: false,
                dir_hint: Some("src".to_string()),
                is_pinned: false,
                is_read_only: false,
                has_file_path: true,
                supports_editing: true,
            },
        ];
        let tab_bar = TabBar::new(tabs, 0);

        // Distinct titles: no directory hint shown
        assert!(!tab_bar.should_show_dir_hint(0));
        assert!(!tab_bar.should_show_dir_hint(1));
        assert_eq!(tab_bar.effective_dir_hint(0), None);
        assert_eq!(tab_bar.effective_dir_hint(1), None);

        // Add a duplicate "main.rs" from another folder
        let tabs_with_dup = vec![
            TabMeta {
                is_loading: false,
                title: "main.rs".to_string(),
                is_dirty: false,
                dir_hint: Some("src".to_string()),
                is_pinned: false,
                is_read_only: false,
                has_file_path: true,
                supports_editing: true,
            },
            TabMeta {
                is_loading: false,
                title: "lib.rs".to_string(),
                is_dirty: false,
                dir_hint: Some("src".to_string()),
                is_pinned: false,
                is_read_only: false,
                has_file_path: true,
                supports_editing: true,
            },
            TabMeta {
                is_loading: false,
                title: "main.rs".to_string(),
                is_dirty: false,
                dir_hint: Some("tests".to_string()),
                is_pinned: false,
                is_read_only: false,
                has_file_path: true,
                supports_editing: true,
            },
        ];
        let tab_bar_dup = TabBar::new(tabs_with_dup, 0);

        // "main.rs" tabs have duplicates: both show their directory hints
        assert!(tab_bar_dup.should_show_dir_hint(0));
        assert_eq!(tab_bar_dup.effective_dir_hint(0), Some("src"));

        // "lib.rs" is unique: does NOT show directory hint
        assert!(!tab_bar_dup.should_show_dir_hint(1));
        assert_eq!(tab_bar_dup.effective_dir_hint(1), None);

        // "main.rs" (tests) is duplicate: shows directory hint
        assert!(tab_bar_dup.should_show_dir_hint(2));
        assert_eq!(tab_bar_dup.effective_dir_hint(2), Some("tests"));

        // Tab with no dir_hint: should return None even if title is duplicated
        let tabs_no_hint = vec![
            TabMeta {
                is_loading: false,
                title: "Untitled.md".to_string(),
                is_dirty: false,
                dir_hint: None,
                is_pinned: false,
                is_read_only: false,
                has_file_path: false,
                supports_editing: true,
            },
            TabMeta {
                is_loading: false,
                title: "Untitled.md".to_string(),
                is_dirty: false,
                dir_hint: None,
                is_pinned: false,
                is_read_only: false,
                has_file_path: false,
                supports_editing: true,
            },
        ];
        let tab_bar_no_hint = TabBar::new(tabs_no_hint, 0);
        assert!(!tab_bar_no_hint.should_show_dir_hint(0));
        assert_eq!(tab_bar_no_hint.effective_dir_hint(0), None);
    }

    #[gpui::test]
    fn test_tab_bar_set_tab_title_updates_duplicate_titles(cx: &mut gpui::TestAppContext) {
        let tabs = vec![
            TabMeta {
                is_loading: false,
                title: "foo.rs".to_string(),
                is_dirty: false,
                dir_hint: Some("dir_a".to_string()),
                is_pinned: false,
                is_read_only: false,
                has_file_path: true,
                supports_editing: true,
            },
            TabMeta {
                is_loading: false,
                title: "bar.rs".to_string(),
                is_dirty: false,
                dir_hint: Some("dir_b".to_string()),
                is_pinned: false,
                is_read_only: false,
                has_file_path: true,
                supports_editing: true,
            },
        ];
        let tab_bar = cx.new(|_cx| TabBar::new(tabs, 0));

        // Initially distinct: no directory hints shown
        tab_bar.update(cx, |tb, _cx| {
            assert!(!tb.should_show_dir_hint(0));
            assert!(!tb.should_show_dir_hint(1));
        });

        // Rename second tab to "foo.rs": duplicate title detected and hints displayed
        tab_bar.update(cx, |tb, cx| {
            tb.set_tab_title(1, "foo.rs".to_string(), Some("dir_b".to_string()), cx);
            assert!(tb.should_show_dir_hint(0));
            assert!(tb.should_show_dir_hint(1));
            assert_eq!(tb.effective_dir_hint(0), Some("dir_a"));
            assert_eq!(tb.effective_dir_hint(1), Some("dir_b"));
        });

        // Rename back to distinct: duplicate hints cleared
        tab_bar.update(cx, |tb, cx| {
            tb.set_tab_title(1, "bar.rs".to_string(), Some("dir_b".to_string()), cx);
            assert!(!tb.should_show_dir_hint(0));
            assert!(!tb.should_show_dir_hint(1));
            assert_eq!(tb.effective_dir_hint(0), None);
            assert_eq!(tb.effective_dir_hint(1), None);
        });
    }

    #[gpui::test]
    fn test_tab_bar_sync_preserves_scroll_when_active_tab_unchanged(cx: &mut gpui::TestAppContext) {
        let tabs = vec![
            TabMeta {
                is_loading: false,
                title: "a.rs".to_string(),
                is_dirty: false,
                dir_hint: None,
                is_pinned: false,
                is_read_only: false,
                has_file_path: true,
                supports_editing: true,
            },
            TabMeta {
                is_loading: false,
                title: "b.rs".to_string(),
                is_dirty: false,
                dir_hint: None,
                is_pinned: false,
                is_read_only: false,
                has_file_path: true,
                supports_editing: true,
            },
        ];
        let tab_bar = cx.new(|_cx| TabBar::new(tabs.clone(), 0));

        // When syncing with only sidebar_offset or dirty state changed (active_tab_idx and count unchanged)
        tab_bar.update(cx, |tb, cx| {
            let mut updated_tabs = tabs.clone();
            updated_tabs[0].is_dirty = true;
            tb.sync(updated_tabs, 0, false, px(100.0), cx);
            assert_eq!(tb.active_tab_idx(), 0);
            assert!(tb.tabs_meta()[0].is_dirty);
        });

        // When switching active tab, active_tab_idx updates
        tab_bar.update(cx, |tb, cx| {
            tb.sync(tabs.clone(), 1, false, px(100.0), cx);
            assert_eq!(tb.active_tab_idx(), 1);
            assert!(!tb.active_tab_edge_flush().0);
        });

        // Switching back to the first tab restores flush-left connection
        tab_bar.update(cx, |tb, cx| {
            tb.sync(tabs.clone(), 0, false, px(100.0), cx);
            assert!(tb.active_tab_edge_flush().0);
        });
    }
}
