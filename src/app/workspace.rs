use gpui::prelude::*;
use gpui::*;
use std::path::PathBuf;

mod fs;
mod sidebar;
mod tabs;
mod welcome;

pub(crate) use fs::{extract_workspace_name_from_paths, workspace_safe_bounds};
pub use sidebar::{DEFAULT_SIDEBAR_WIDTH, MAX_SIDEBAR_WIDTH, MIN_SIDEBAR_WIDTH};
pub(crate) use tabs::TabItem;
use tabs::TabKind;

use crate::app::search_panel::SearchPanel;
use crate::app::{OutlinePanel, OutlinePanelEvent, SidebarView, status_bar, tab_bar};
use crate::buffer::{Position, SourceLine};
use crate::config::{AppConfig, WorkspaceState};
use crate::editor::Editor;
use crate::editor::find_bar::FindBar;
use crate::theme::{DEFAULT_THEME, ThemeManager};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum PanelResizeTarget {
    Sidebar,
    Outline,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum WorkspaceEvent {
    TitleChanged,
    LayoutChanged,
}

#[derive(Clone, Copy, Debug)]
struct PanelLayoutValues {
    sidebar_visible: bool,
    sidebar_width: Pixels,
    sidebar_view: SidebarView,
    outline_visible: bool,
    outline_width: Pixels,
}

impl PanelLayoutValues {
    fn from_workspace_state(ws: &WorkspaceState) -> Self {
        Self {
            sidebar_visible: ws.sidebar_visible,
            sidebar_width: sidebar::clamp_sidebar_width(px(ws
                .sidebar_width
                .unwrap_or(DEFAULT_SIDEBAR_WIDTH))),
            sidebar_view: ws.sidebar_view,
            outline_visible: ws.outline_visible,
            outline_width: sidebar::clamp_sidebar_width(px(ws
                .outline_width
                .unwrap_or(DEFAULT_SIDEBAR_WIDTH))),
        }
    }

    fn from_app_config(cx: &App) -> Self {
        match cx.try_global::<AppConfig>().and_then(|c| c.last_layout()) {
            Some(layout) => Self {
                sidebar_visible: layout.sidebar_visible,
                sidebar_width: sidebar::clamp_sidebar_width(px(layout
                    .sidebar_width
                    .unwrap_or(DEFAULT_SIDEBAR_WIDTH))),
                sidebar_view: layout.sidebar_view,
                outline_visible: layout.outline_visible,
                outline_width: sidebar::clamp_sidebar_width(px(layout
                    .outline_width
                    .unwrap_or(DEFAULT_SIDEBAR_WIDTH))),
            },
            None => Self {
                sidebar_visible: true,
                sidebar_width: px(DEFAULT_SIDEBAR_WIDTH),
                sidebar_view: SidebarView::default(),
                outline_visible: false,
                outline_width: px(DEFAULT_SIDEBAR_WIDTH),
            },
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum OutlineActiveLineSource {
    Cursor,
    Viewport,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum OutlineDisplayed {
    EmptyLoading(EntityId),
    Snapshot {
        editor_id: EntityId,
        outline_revision: u64,
        is_loading: bool,
    },
}

impl OutlineDisplayed {
    fn editor_id(self) -> EntityId {
        match self {
            Self::EmptyLoading(id) => id,
            Self::Snapshot { editor_id, .. } => editor_id,
        }
    }
}

impl EventEmitter<WorkspaceEvent> for Workspace {}

pub struct Workspace {
    focus_handle: FocusHandle,
    tabs: Vec<TabItem>,
    active_tab_idx: usize,
    tab_bar: Entity<tab_bar::TabBar>,
    _tab_bar_sub: Subscription,
    status_bar: Entity<status_bar::StatusBar>,
    project_panel: Entity<crate::app::project_panel::ProjectPanel>,
    _project_panel_sub: Subscription,
    outline_panel: Entity<OutlinePanel>,
    _outline_panel_sub: Subscription,
    root_dir: Option<PathBuf>,
    sidebar_visible: bool,
    sidebar_width: Pixels,
    sidebar_view: SidebarView,
    outline_visible: bool,
    outline_displayed: Option<OutlineDisplayed>,
    outline_active_line_source: OutlineActiveLineSource,
    outline_width: Pixels,
    search_panel: Entity<SearchPanel>,
    resize_target: Option<PanelResizeTarget>,
    panel_drag_start_x: Option<Pixels>,
    panel_drag_start_width: Option<Pixels>,
    collocated_mode: bool,
    persists_session: bool,
    file_index: crate::fs::WorkspaceFileIndex,
    image_cache: Entity<crate::app::BoundedImageCache>,
    _config_subscription: Subscription,
}

#[cfg(test)]
mod loading_tests;

impl Focusable for Workspace {
    fn focus_handle(&self, _cx: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl Workspace {
    fn subscribe_tab_bar(
        tab_bar: &Entity<tab_bar::TabBar>,
        cx: &mut Context<Self>,
    ) -> Subscription {
        cx.subscribe(tab_bar, |this, _tab_bar, event, cx| match event {
            tab_bar::TabBarEvent::Select(idx) => {
                this.switch_tab(*idx, cx);
            }
            tab_bar::TabBarEvent::Close(idx) => {
                this.close_tab(*idx, cx);
            }
            tab_bar::TabBarEvent::CloseOthers(idx) => {
                this.close_other_tabs(*idx, cx);
            }
            tab_bar::TabBarEvent::CloseLeft(idx) => {
                this.close_tabs_to_left(*idx, cx);
            }
            tab_bar::TabBarEvent::CloseRight(idx) => {
                this.close_tabs_to_right(*idx, cx);
            }
            tab_bar::TabBarEvent::CloseClean => {
                this.close_clean_tabs(cx);
            }
            tab_bar::TabBarEvent::CloseAll => {
                this.close_all_tabs(cx);
            }
            tab_bar::TabBarEvent::ToggleReadOnly(idx) => {
                this.toggle_tab_read_only(*idx, cx);
            }
            tab_bar::TabBarEvent::CopyPath(idx) => {
                this.copy_tab_path(*idx, cx);
            }
            tab_bar::TabBarEvent::CopyRelativePath(idx) => {
                this.copy_tab_relative_path(*idx, cx);
            }
            tab_bar::TabBarEvent::RevealInFileExplorer(idx) => {
                this.reveal_tab_in_file_explorer(*idx, cx);
            }
            tab_bar::TabBarEvent::TogglePin(idx) => {
                this.toggle_tab_pin(*idx, cx);
            }
            tab_bar::TabBarEvent::RevealInProjectPanel(idx) => {
                this.reveal_tab_in_project_panel(*idx, cx);
            }
            tab_bar::TabBarEvent::MoveTab { from_idx, to_idx } => {
                this.move_tab(*from_idx, *to_idx, cx);
            }
            tab_bar::TabBarEvent::MoveTabToEnd(from_idx) => {
                this.move_tab_to_end(*from_idx, cx);
            }
            tab_bar::TabBarEvent::DropProjectEntry { path, target_idx } => {
                this.handle_dropped_project_file(path.clone(), *target_idx, cx);
            }
            tab_bar::TabBarEvent::EdgeFlushChanged => {
                cx.notify();
            }
        })
    }

    fn subscribe_project_panel(
        project_panel: &Entity<crate::app::project_panel::ProjectPanel>,
        cx: &mut Context<Self>,
    ) -> Subscription {
        cx.subscribe(project_panel, |this, _panel, event, cx| match event {
            crate::app::project_panel::ProjectPanelEvent::OpenFile(path) => {
                this.open_file(path.clone(), cx);
            }
            crate::app::project_panel::ProjectPanelEvent::PathRenamed { old, new } => {
                this.handle_path_renamed(old, new, cx);
                this.invalidate_file_index_and_search(cx);
            }
            crate::app::project_panel::ProjectPanelEvent::PathDeleted(path) => {
                this.handle_path_deleted(path, cx);
                this.invalidate_file_index_and_search(cx);
            }
            crate::app::project_panel::ProjectPanelEvent::PathCreated(_) => {
                this.invalidate_file_index_and_search(cx);
            }
            crate::app::project_panel::ProjectPanelEvent::FilterClosed => {
                this.release_idle_file_index(cx);
            }
        })
    }

    fn subscribe_outline_panel(
        outline_panel: &Entity<OutlinePanel>,
        cx: &mut Context<Self>,
    ) -> Subscription {
        cx.subscribe(
            outline_panel,
            |this, _panel, event: &OutlinePanelEvent, cx| match event {
                OutlinePanelEvent::Select(line) => this.jump_to_outline(*line, cx),
            },
        )
    }

    fn build_with_layout(
        root_dir: Option<PathBuf>,
        active_file: Option<PathBuf>,
        tabs: Vec<TabItem>,
        active_tab_idx: usize,
        layout: PanelLayoutValues,
        persists_session: bool,
        cx: &mut Context<Self>,
    ) -> Self {
        let focus_handle = cx.focus_handle();
        let status_meta = if tabs.is_empty() {
            None
        } else {
            tabs[active_tab_idx].editor.read(cx).status_meta()
        };
        let active_sidebar = layout.sidebar_visible.then_some(layout.sidebar_view);
        let status_bar = cx.new(|_cx| {
            let mut sb = status_bar::StatusBar::new(status_meta, active_sidebar);
            if layout.outline_visible {
                sb.set_outline_visible(true, _cx);
            }
            sb
        });
        let tabs_meta = tabs::collect_tabs_meta(&tabs, cx);
        let sidebar_offset = if layout.sidebar_visible {
            layout.sidebar_width
        } else {
            px(0.0)
        };
        let has_workspace_root = root_dir.is_some();
        let tab_bar = cx.new(|_cx| tab_bar::TabBar::new(tabs_meta.clone(), active_tab_idx));
        tab_bar.update(cx, |tb, cx| {
            tb.sync(
                tabs_meta,
                active_tab_idx,
                has_workspace_root,
                sidebar_offset,
                cx,
            );
        });
        let collocated_mode = cx
            .try_global::<AppConfig>()
            .is_some_and(|cfg| cfg.collocated_mode);
        let _config_subscription = cx.observe_global::<AppConfig>(|this, cx| {
            let mode = cx
                .try_global::<AppConfig>()
                .is_some_and(|cfg| cfg.collocated_mode);
            this.set_collocated_mode(mode, cx);
        });
        let tab_bar_sub = Self::subscribe_tab_bar(&tab_bar, cx);
        let file_index = crate::fs::WorkspaceFileIndex::default();
        let project_panel = cx.new(|cx| {
            crate::app::project_panel::ProjectPanel::with_file_index(
                root_dir.clone(),
                file_index.clone(),
                cx,
            )
        });
        project_panel.update(cx, |p, cx| {
            if let Some(ref active_path) = active_file {
                p.reveal_path(active_path, cx);
            }
            p.set_active_file(active_file, cx);
            p.set_filter_visible(layout.sidebar_view == SidebarView::Search, cx);
        });
        let project_panel_sub = Self::subscribe_project_panel(&project_panel, cx);
        let outline_panel = cx.new(|cx| OutlinePanel::new(cx.focus_handle()));
        let outline_panel_sub = Self::subscribe_outline_panel(&outline_panel, cx);
        let search_panel = cx.new(|cx| SearchPanel::with_file_index(file_index.clone(), cx));
        search_panel.update(cx, |sp, cx| sp.set_root_dir(root_dir.clone(), cx));
        let image_cache = crate::app::BoundedImageCache::new(cx);
        let mut workspace = Self {
            focus_handle,
            tabs,
            active_tab_idx,
            tab_bar,
            _tab_bar_sub: tab_bar_sub,
            status_bar,
            project_panel,
            _project_panel_sub: project_panel_sub,
            outline_panel,
            _outline_panel_sub: outline_panel_sub,
            root_dir,
            sidebar_visible: layout.sidebar_visible,
            sidebar_width: layout.sidebar_width,
            sidebar_view: layout.sidebar_view,
            outline_visible: layout.outline_visible,
            outline_displayed: None,
            outline_active_line_source: OutlineActiveLineSource::Cursor,
            outline_width: layout.outline_width,
            search_panel,
            resize_target: None,
            panel_drag_start_x: None,
            panel_drag_start_width: None,
            collocated_mode,
            persists_session,
            file_index,
            image_cache,
            _config_subscription,
        };
        workspace.sync_outline_panel(cx);
        workspace
    }

    pub fn new(initial_path: Option<PathBuf>, cx: &mut Context<Self>) -> Self {
        let collocated_mode = cx
            .try_global::<AppConfig>()
            .is_some_and(|cfg| cfg.collocated_mode);
        let (root_dir, file_path, persists_session) = match initial_path {
            Some(p) if p.is_dir() => {
                if collocated_mode
                    && let Some(index_file) = crate::fs::find_collocated_index_file(&p)
                {
                    let parent = p.parent().map(|d| d.to_path_buf());
                    (parent, Some(index_file), false)
                } else {
                    let target = fs::find_folder_target_file(&p, collocated_mode);
                    (Some(p), target, true)
                }
            }
            Some(p) => {
                let parent = crate::fs::resolve_document_parent_dir(&p, collocated_mode, None)
                    .map(|d| d.to_path_buf())
                    .or_else(|| p.parent().map(|d| d.to_path_buf()));
                (parent, Some(p), false)
            }
            None => (None, None, true),
        };

        let (tabs, active_tab_idx) = if let Some(path) = file_path.clone() {
            let (editor, subscription, load_task) = Self::init_editor(Some(path), cx);
            (
                vec![TabItem {
                    _load_task: load_task,
                    editor,
                    kind: TabKind::Editor,
                    is_pinned: false,
                    _subscription: subscription,
                }],
                0,
            )
        } else {
            (Vec::new(), 0)
        };

        let layout = PanelLayoutValues::from_app_config(cx);
        Self::build_with_layout(
            root_dir,
            file_path,
            tabs,
            active_tab_idx,
            layout,
            persists_session,
            cx,
        )
    }

    fn load_tabs_from_state(ws: &WorkspaceState, cx: &mut Context<Self>) -> (Vec<TabItem>, usize) {
        let mut tabs = Vec::new();
        let mut active_idx = 0;

        for file_path in &ws.open_files {
            if file_path.exists() {
                let (editor, subscription, load_task) =
                    Self::init_editor(Some(file_path.clone()), cx);
                tabs.push(TabItem {
                    _load_task: load_task,
                    editor,
                    kind: TabKind::Editor,
                    is_pinned: false,
                    _subscription: subscription,
                });

                if let Some(active_file) = &ws.active_file
                    && active_file == file_path
                {
                    // Track actual tabs length to avoid skew if previous files did not exist
                    active_idx = tabs.len() - 1;
                }
            }
        }

        let final_idx = if tabs.is_empty() {
            0
        } else {
            active_idx.min(tabs.len() - 1)
        };

        (tabs, final_idx)
    }

    pub fn from_state(ws: &WorkspaceState, cx: &mut Context<Self>) -> Self {
        let (tabs, active_tab_idx) = Self::load_tabs_from_state(ws, cx);
        let layout = PanelLayoutValues::from_workspace_state(ws);
        Self::build_with_layout(
            ws.root_dir.clone(),
            ws.active_file.clone(),
            tabs,
            active_tab_idx,
            layout,
            true,
            cx,
        )
    }

    pub fn restore_from_state(&mut self, ws: &WorkspaceState, cx: &mut Context<Self>) {
        self.persists_session = true;
        let layout = PanelLayoutValues::from_workspace_state(ws);
        self.root_dir = ws.root_dir.clone();
        self.sidebar_visible = layout.sidebar_visible;
        self.sidebar_view = layout.sidebar_view;
        self.outline_visible = layout.outline_visible;
        self.status_bar.update(cx, |status_bar, cx| {
            status_bar.set_outline_visible(layout.outline_visible, cx)
        });
        self.sidebar_width = layout.sidebar_width;
        self.outline_width = layout.outline_width;
        self.resize_target = None;
        self.panel_drag_start_x = None;
        self.panel_drag_start_width = None;

        let (tabs, active_tab_idx) = Self::load_tabs_from_state(ws, cx);
        self.tabs = tabs;
        self.active_tab_idx = active_tab_idx;

        self.project_panel.update(cx, |p, cx| {
            p.set_root_dir(ws.root_dir.clone(), cx);
            if let Some(ref active_path) = ws.active_file {
                p.reveal_path(active_path, cx);
            }
            p.set_active_file(ws.active_file.clone(), cx);
            p.set_filter_visible(
                layout.sidebar_visible && layout.sidebar_view == SidebarView::Search,
                cx,
            );
        });

        self.sync_sidebar_state(cx);

        self.search_panel.update(cx, |sp, cx| {
            sp.set_root_dir(ws.root_dir.clone(), cx);
        });
        self.image_cache.update(cx, |cache, cx| cache.clear(cx));

        self.sync_active_editor_state(cx);
    }

    pub(crate) fn init_editor(
        path: Option<PathBuf>,
        cx: &mut Context<Self>,
    ) -> (Entity<Editor>, Subscription, Option<Task<()>>) {
        let editor = cx.new(|cx| match path.as_ref() {
            Some(p) => Editor::new_loading(p.clone(), cx),
            None => Editor::new(cx),
        });
        let tab_editor = editor.clone();
        let mut last_status_revision = tab_editor.read(cx).buffer().revision();
        let subscription = cx.subscribe(
            &editor,
            move |this, _emitter, event: &crate::editor::EditorEvent, cx| match event {
                crate::editor::EditorEvent::DirtyChanged(is_dirty) => {
                    if let Some(idx) = this.tabs.iter().position(|t| t.editor == tab_editor) {
                        this.tab_bar
                            .update(cx, |tb, cx| tb.set_tab_dirty(idx, *is_dirty, cx));
                    }
                    if this.active_editor() == Some(&tab_editor) {
                        this.status_bar
                            .update(cx, |sb, cx| sb.set_dirty(*is_dirty, cx));
                        this.sync_outline_panel(cx);
                    }
                }
                crate::editor::EditorEvent::ExternalChangeChanged(_) => {
                    if this.active_editor() == Some(&tab_editor) {
                        let meta = tab_editor.read(cx).status_meta();
                        this.status_bar.update(cx, |sb, cx| sb.set_meta(meta, cx));
                    }
                }
                crate::editor::EditorEvent::TitleChanged => {
                    if let Some(idx) = this.tabs.iter().position(|t| t.editor == tab_editor) {
                        let (title, dir_hint) = {
                            let ed = tab_editor.read(cx);
                            (ed.title(), ed.dir_hint())
                        };
                        this.tab_bar
                            .update(cx, |tb, cx| tb.set_tab_title(idx, title, dir_hint, cx));
                    }
                    if this.active_editor() == Some(&tab_editor) {
                        let meta = tab_editor.read(cx).status_meta();
                        last_status_revision = tab_editor.read(cx).buffer().revision();
                        this.outline_active_line_source = OutlineActiveLineSource::Cursor;
                        this.status_bar.update(cx, |sb, cx| sb.set_meta(meta, cx));
                        this.sync_outline_panel(cx);
                        cx.emit(WorkspaceEvent::TitleChanged);
                    }
                }
                crate::editor::EditorEvent::CursorMoved => {
                    if this.active_editor() == Some(&tab_editor) {
                        this.outline_active_line_source = OutlineActiveLineSource::Cursor;
                        let current_revision = tab_editor.read(cx).buffer().revision();
                        if current_revision == last_status_revision {
                            let pos = tab_editor.read(cx).cursor_pos();
                            this.status_bar
                                .update(cx, |sb, cx| sb.set_cursor_pos(pos, cx));
                            if this.outline_visible {
                                this.outline_panel.update(cx, |panel, cx| {
                                    panel.set_active_line(Some(pos.line), cx)
                                });
                            }
                        } else {
                            last_status_revision = current_revision;
                            let meta = tab_editor.read(cx).status_meta();
                            this.status_bar.update(cx, |sb, cx| sb.set_meta(meta, cx));
                            this.sync_outline_panel(cx);
                        }
                    }
                }
                crate::editor::EditorEvent::ScrollChanged => {
                    if this.active_editor() == Some(&tab_editor) && this.outline_visible {
                        this.outline_active_line_source = OutlineActiveLineSource::Viewport;
                        let visible_line = {
                            let editor = tab_editor.read(cx);
                            editor.first_visible_source_line()
                        };
                        this.outline_panel
                            .update(cx, |panel, cx| panel.set_active_line(visible_line, cx));
                    }
                }
            },
        );
        let task = path.map(|path| Self::spawn_file_load(path, editor.downgrade(), cx));
        (editor, subscription, task)
    }

    fn spawn_file_load(
        path: PathBuf,
        editor: WeakEntity<Editor>,
        cx: &mut Context<Self>,
    ) -> Task<()> {
        let requested_path = path.clone();
        let session = crate::editor::loading::DocumentSession::new();
        let mut updates = session.updates.subscribe();
        let session_work =
            crate::editor::loading::spawn_session(&session, path, cx.background_executor());
        cx.spawn(async move |this, cx| {
            use futures::StreamExt;
            let _session_work = session_work;
            while updates.next().await.is_some() {
                let Some(result) = session.take() else {
                    continue;
                };
                let keep_running = this
                    .update(cx, |workspace, cx| {
                        let Some(editor) = editor.upgrade() else {
                            return false;
                        };
                        let Some(index) =
                            workspace.tabs.iter().position(|tab| tab.editor == editor)
                        else {
                            return false;
                        };
                        if editor.read(cx).file_path() != Some(requested_path.as_path()) {
                            if let Some(path) = editor.read(cx).file_path_buf() {
                                workspace.tabs[index]._load_task =
                                    Some(Self::spawn_file_load(path, editor.downgrade(), cx));
                            }
                            return false;
                        }
                        editor.update(cx, |editor, cx| {
                            editor.install_publication(&session, result, cx)
                        });
                        workspace.sync_active_editor_state(cx);
                        true
                    })
                    .unwrap_or(false);
                if !keep_running {
                    break;
                }
            }
        })
    }

    pub fn active_editor(&self) -> Option<&Entity<Editor>> {
        if self.tabs.is_empty() || self.active_tab_kind() != Some(TabKind::Editor) {
            None
        } else {
            let idx = self.active_tab_idx.min(self.tabs.len() - 1);
            Some(&self.tabs[idx].editor)
        }
    }

    pub fn focus_active_editor(&self, window: &mut Window, cx: &App) {
        if let Some(editor) = self.active_editor() {
            window.focus(&editor.focus_handle(cx));
        } else {
            window.focus(&self.focus_handle);
        }
    }

    #[inline]
    pub fn tab_count(&self) -> usize {
        self.tabs.len()
    }

    #[inline]
    pub fn active_tab_idx(&self) -> usize {
        self.active_tab_idx
    }

    #[inline]
    fn active_tab_kind(&self) -> Option<TabKind> {
        self.tabs.get(self.active_tab_idx).map(|tab| tab.kind)
    }

    fn sync_tab_bar(&mut self, cx: &mut Context<Self>) {
        let tabs_meta = self.get_tabs_meta(cx);
        let sidebar_offset = if self.sidebar_visible {
            self.sidebar_width
        } else {
            px(0.0)
        };
        let has_workspace_root = self.root_dir.is_some();
        let active_tab_idx = self.active_tab_idx;
        self.tab_bar.update(cx, |tb, cx| {
            tb.sync(
                tabs_meta,
                active_tab_idx,
                has_workspace_root,
                sidebar_offset,
                cx,
            )
        });
    }

    fn set_collocated_mode(&mut self, collocated_mode: bool, cx: &mut Context<Self>) {
        if self.collocated_mode == collocated_mode {
            return;
        }
        self.collocated_mode = collocated_mode;
        for tab in &self.tabs {
            if tab.kind == TabKind::Editor {
                tab.editor
                    .update(cx, |ed, _| ed.set_collocated_mode(collocated_mode));
            }
        }
        self.project_panel.update(cx, |panel, cx| {
            panel.set_collocated_mode(collocated_mode, cx);
        });
        self.search_panel.update(cx, |sp, cx| {
            sp.set_collocated_mode(collocated_mode, cx);
        });
        self.sync_tab_bar(cx);
        cx.emit(WorkspaceEvent::TitleChanged);
        cx.notify();
    }

    fn invalidate_file_index_and_search(&mut self, cx: &mut Context<Self>) {
        self.search_panel.update(cx, |sp, cx| sp.refresh_files(cx));
    }

    pub fn refresh_panels(&mut self, cx: &mut Context<Self>) {
        self.file_index.invalidate();
        self.project_panel.update(cx, |panel, cx| {
            panel.refresh_tree(cx);
        });
        self.invalidate_file_index_and_search(cx);
    }

    fn release_idle_file_index(&mut self, cx: &mut Context<Self>) {
        let has_search_tab = self
            .tabs
            .iter()
            .any(|tab| tab.kind == TabKind::WorkspaceSearch);
        if !has_search_tab {
            self.search_panel.update(cx, |sp, _| sp.cancel_prewarm());
        }
        let search_idle = !has_search_tab;
        let filter_idle = !self.project_panel.read(cx).has_filter_index_activity();
        if search_idle && filter_idle {
            self.file_index.invalidate();
        }
    }

    fn sync_active_editor_state(&mut self, cx: &mut Context<Self>) {
        let has_search_tab = self
            .tabs
            .iter()
            .any(|tab| tab.kind == TabKind::WorkspaceSearch);
        self.search_panel
            .update(cx, |sp, cx| sp.set_active(has_search_tab, cx));
        self.release_idle_file_index(cx);
        for (i, tab) in self.tabs.iter().enumerate() {
            if i != self.active_tab_idx {
                tab.editor.update(cx, |ed, cx| ed.on_blur(cx));
            }
        }
        if let Some(editor) = self.active_editor() {
            let meta = editor.read(cx).status_meta();
            self.status_bar.update(cx, |sb, cx| sb.set_meta(meta, cx));
            editor.update(cx, |ed, cx| ed.reset_cursor_blink(cx));
        } else {
            self.status_bar.update(cx, |sb, cx| sb.set_meta(None, cx));
        }
        self.sync_tab_bar(cx);
        let active_file = self.active_file_path(cx).map(|p| p.to_path_buf());
        self.project_panel
            .update(cx, |p, cx| p.set_active_file(active_file, cx));
        self.sync_outline_panel(cx);
        cx.emit(WorkspaceEvent::TitleChanged);
        cx.notify();
    }

    fn transfer_find_bar(&mut self, cx: &mut Context<Self>) -> Option<Entity<FindBar>> {
        self.active_editor()
            .and_then(|ed| ed.update(cx, |ed, cx| ed.detach_find_bar(cx)))
    }

    fn restore_find_bar(&mut self, bar: Option<Entity<FindBar>>, cx: &mut Context<Self>) {
        if let Some(bar) = bar {
            if let Some(active_ed) = self.active_editor() {
                active_ed.update(cx, |ed, cx| ed.attach_find_bar(bar, cx));
            }
        }
    }

    pub fn new_tab(&mut self, cx: &mut Context<Self>) {
        let find_bar = self.transfer_find_bar(cx);
        let (editor, subscription, load_task) = Self::init_editor(None, cx);
        self.tabs.push(TabItem {
            _load_task: load_task,
            editor,
            kind: TabKind::Editor,
            is_pinned: false,
            _subscription: subscription,
        });
        self.active_tab_idx = self.tabs.len() - 1;
        self.restore_find_bar(find_bar, cx);
        self.sync_active_editor_state(cx);
    }

    pub fn close_tab(&mut self, index: usize, cx: &mut Context<Self>) {
        if index < self.tabs.len() {
            let is_closing_active = index == self.active_tab_idx;
            let next_active_is_editor = is_closing_active
                && self.tabs.len() > 1
                && self.tabs[if index + 1 < self.tabs.len() {
                    index + 1
                } else {
                    index - 1
                }]
                .kind
                    == TabKind::Editor;
            let find_bar = if next_active_is_editor {
                self.transfer_find_bar(cx)
            } else {
                None
            };

            self.tabs.remove(index);
            if self.tabs.is_empty() {
                self.active_tab_idx = 0;
            } else if self.active_tab_idx >= self.tabs.len() {
                self.active_tab_idx = self.tabs.len() - 1;
            } else if self.active_tab_idx > index {
                self.active_tab_idx -= 1;
            }

            self.restore_find_bar(find_bar, cx);
            self.sync_active_editor_state(cx);
        }
    }

    pub fn close_active_tab(&mut self, cx: &mut Context<Self>) {
        let idx = self.active_tab_idx;
        self.close_tab(idx, cx);
    }

    pub fn tabs(&self) -> &[TabItem] {
        &self.tabs
    }

    pub fn close_other_tabs(&mut self, keep_idx: usize, cx: &mut Context<Self>) {
        if keep_idx >= self.tabs.len() {
            return;
        }
        let target_editor = self.tabs[keep_idx].editor.clone();
        let find_bar = if self.active_tab_kind() == Some(TabKind::Editor)
            && self.tabs[keep_idx].kind == TabKind::Editor
        {
            self.transfer_find_bar(cx)
        } else {
            None
        };
        tabs::filter_close_other_tabs(&mut self.tabs, &target_editor);
        self.active_tab_idx = self
            .tabs
            .iter()
            .position(|t| t.editor == target_editor)
            .unwrap_or(0);
        self.restore_find_bar(find_bar, cx);
        self.sync_active_editor_state(cx);
    }

    pub fn close_tabs_to_left(&mut self, target_idx: usize, cx: &mut Context<Self>) {
        if target_idx >= self.tabs.len() {
            return;
        }
        let target_editor = self.tabs[target_idx].editor.clone();
        let find_bar = if self.active_tab_kind() == Some(TabKind::Editor)
            && self.tabs[target_idx].kind == TabKind::Editor
        {
            self.transfer_find_bar(cx)
        } else {
            None
        };
        tabs::filter_close_tabs_to_left(&mut self.tabs, target_idx);
        self.active_tab_idx = self
            .tabs
            .iter()
            .position(|t| t.editor == target_editor)
            .unwrap_or(0);
        self.restore_find_bar(find_bar, cx);
        self.sync_active_editor_state(cx);
    }

    pub fn close_tabs_to_right(&mut self, target_idx: usize, cx: &mut Context<Self>) {
        if target_idx >= self.tabs.len() {
            return;
        }
        let target_editor = self.tabs[target_idx].editor.clone();
        let find_bar = if self.active_tab_kind() == Some(TabKind::Editor)
            && self.tabs[target_idx].kind == TabKind::Editor
        {
            self.transfer_find_bar(cx)
        } else {
            None
        };
        tabs::filter_close_tabs_to_right(&mut self.tabs, target_idx);
        self.active_tab_idx = self
            .tabs
            .iter()
            .position(|t| t.editor == target_editor)
            .unwrap_or(0);
        self.restore_find_bar(find_bar, cx);
        self.sync_active_editor_state(cx);
    }

    pub fn close_clean_tabs(&mut self, cx: &mut Context<Self>) {
        let active_editor = self.active_editor().cloned();
        let active_is_retained = self.active_tab_kind() == Some(TabKind::Editor)
            && self
                .tabs
                .get(self.active_tab_idx)
                .is_some_and(|tab| tab.is_pinned || tab.editor.read(cx).is_dirty());
        let find_bar = if active_is_retained {
            self.transfer_find_bar(cx)
        } else {
            None
        };
        tabs::filter_close_clean_tabs(&mut self.tabs, cx);
        if let Some(ed) = active_editor {
            self.active_tab_idx = self.tabs.iter().position(|t| t.editor == ed).unwrap_or(0);
        } else {
            self.active_tab_idx = 0;
        }
        if !self.tabs.is_empty() && self.active_tab_idx >= self.tabs.len() {
            self.active_tab_idx = self.tabs.len() - 1;
        }
        self.restore_find_bar(find_bar, cx);
        self.sync_active_editor_state(cx);
    }

    pub fn close_all_tabs(&mut self, cx: &mut Context<Self>) {
        let find_bar = if self.active_tab_kind() == Some(TabKind::Editor)
            && self
                .tabs
                .get(self.active_tab_idx)
                .is_some_and(|tab| tab.is_pinned)
        {
            self.transfer_find_bar(cx)
        } else {
            None
        };
        self.tabs.retain(|t| t.is_pinned);
        if self.tabs.is_empty() {
            self.active_tab_idx = 0;
        } else if self.active_tab_idx >= self.tabs.len() {
            self.active_tab_idx = self.tabs.len() - 1;
        }
        self.restore_find_bar(find_bar, cx);
        self.sync_active_editor_state(cx);
    }

    pub(crate) fn toggle_tab_read_only(&mut self, tab_idx: usize, cx: &mut Context<Self>) {
        if let Some(tab) = self.tabs.get(tab_idx)
            && tab.kind == TabKind::Editor
        {
            tab.editor.update(cx, |ed, _cx| {
                ed.toggle_read_only();
            });
            self.sync_active_editor_state(cx);
        }
    }

    pub fn copy_tab_path(&self, tab_idx: usize, cx: &mut App) {
        if let Some(tab) = self.tabs.get(tab_idx) {
            let path = tab.editor.read(cx).file_path().map(|p| {
                crate::fs::resolve_document_external_path(
                    p,
                    self.collocated_mode,
                    self.root_dir.as_deref(),
                )
                .to_path_buf()
            });
            if let Some(path) = path {
                crate::app::project_panel::fs_ops::copy_path_to_clipboard(&path, cx);
            }
        }
    }

    pub fn copy_tab_relative_path(&self, tab_idx: usize, cx: &mut App) {
        if let Some(tab) = self.tabs.get(tab_idx) {
            let path = tab.editor.read(cx).file_path().map(|p| {
                crate::fs::resolve_document_external_path(
                    p,
                    self.collocated_mode,
                    self.root_dir.as_deref(),
                )
                .to_path_buf()
            });
            if let Some(path) = path {
                crate::app::project_panel::fs_ops::copy_relative_path_to_clipboard(
                    &path,
                    self.root_dir.as_deref(),
                    cx,
                );
            }
        }
    }

    pub(crate) fn reveal_tab_in_file_explorer(&self, tab_idx: usize, cx: &App) {
        if let Some(tab) = self.tabs.get(tab_idx)
            && let Some(path) = tab.editor.read(cx).file_path()
        {
            let external = crate::fs::resolve_document_external_path(
                path,
                self.collocated_mode,
                self.root_dir.as_deref(),
            );
            crate::platform::reveal_in_file_manager(external);
        }
    }

    pub fn toggle_tab_pin(&mut self, tab_idx: usize, cx: &mut Context<Self>) {
        if tab_idx >= self.tabs.len() {
            return;
        }
        let target_editor = self.tabs[tab_idx].editor.clone();
        tabs::toggle_tab_pin(&mut self.tabs, tab_idx);

        self.active_tab_idx = self
            .tabs
            .iter()
            .position(|t| t.editor == target_editor)
            .unwrap_or(0);
        self.sync_active_editor_state(cx);
    }

    fn reorder_tab_with(
        &mut self,
        from_idx: usize,
        reorder: impl FnOnce(&mut Vec<TabItem>) -> Option<usize>,
        cx: &mut Context<Self>,
    ) {
        if from_idx >= self.tabs.len() {
            return;
        }
        let find_bar = if self.active_tab_idx != from_idx
            && self.active_tab_kind() == Some(TabKind::Editor)
            && self.tabs[from_idx].kind == TabKind::Editor
        {
            self.transfer_find_bar(cx)
        } else {
            None
        };
        if let Some(new_idx) = reorder(&mut self.tabs) {
            self.active_tab_idx = new_idx;
            self.restore_find_bar(find_bar, cx);
            self.sync_active_editor_state(cx);
            cx.emit(WorkspaceEvent::LayoutChanged);
        } else if self.active_tab_idx != from_idx {
            self.restore_find_bar(find_bar, cx);
            self.switch_tab(from_idx, cx);
        } else {
            self.restore_find_bar(find_bar, cx);
        }
    }

    /// Reorders the tab at `from_idx` to `to_idx`, adopting `to_idx`'s pinned state when
    /// crossing the pinned/unpinned boundary (matching Zed) and keeping it active.
    pub fn move_tab(&mut self, from_idx: usize, to_idx: usize, cx: &mut Context<Self>) {
        if to_idx >= self.tabs.len() {
            return;
        }
        self.reorder_tab_with(from_idx, |tabs| tabs::move_tab(tabs, from_idx, to_idx), cx);
    }

    /// Moves the tab at `from_idx` to the end of the tab strip and unpins it.
    pub fn move_tab_to_end(&mut self, from_idx: usize, cx: &mut Context<Self>) {
        self.reorder_tab_with(from_idx, |tabs| tabs::move_tab_to_end(tabs, from_idx), cx);
    }

    /// Opens (or moves if already open) a file dropped onto the tab bar at `target_idx`.
    pub fn handle_dropped_project_file(
        &mut self,
        path: PathBuf,
        target_idx: Option<usize>,
        cx: &mut Context<Self>,
    ) {
        let Some(path) = crate::fs::resolve_openable_document_path(&path, self.collocated_mode)
        else {
            return;
        };
        if let Some(existing_idx) = self.tabs.iter().position(|t| {
            t.kind == TabKind::Editor && t.editor.read(cx).file_path() == Some(path.as_path())
        }) {
            match target_idx {
                Some(to_idx) => self.move_tab(existing_idx, to_idx, cx),
                None => self.move_tab_to_end(existing_idx, cx),
            }
            self.defer_reveal_path(path, cx);
            return;
        }

        match target_idx {
            None => self.open_file(path, cx),
            Some(to_idx) => {
                let is_single_untitled = self.tabs.len() == 1
                    && self.active_editor().is_some_and(|ed| {
                        let ed = ed.read(cx);
                        ed.file_path().is_none() && !ed.is_dirty() && ed.is_empty()
                    });
                if is_single_untitled {
                    self.open_file(path, cx);
                    return;
                }

                let is_pinned = self.tabs.get(to_idx).is_some_and(|t| t.is_pinned);
                let insert_idx = to_idx.min(self.tabs.len());
                let find_bar = self.transfer_find_bar(cx);
                let (editor, subscription, load_task) = Self::init_editor(Some(path.clone()), cx);
                self.tabs.insert(
                    insert_idx,
                    TabItem {
                        _load_task: load_task,
                        editor,
                        kind: TabKind::Editor,
                        is_pinned,
                        _subscription: subscription,
                    },
                );
                self.active_tab_idx = insert_idx;
                self.restore_find_bar(find_bar, cx);
                self.sync_active_editor_state(cx);
                self.defer_reveal_path(path, cx);
                cx.emit(WorkspaceEvent::LayoutChanged);
            }
        }
    }

    pub fn reveal_tab_in_project_panel(&mut self, tab_idx: usize, cx: &mut Context<Self>) {
        if let Some(tab) = self.tabs.get(tab_idx)
            && let Some(path) = tab.editor.read(cx).file_path().map(|p| p.to_path_buf())
        {
            self.show_sidebar(SidebarView::Files, cx);
            self.defer_reveal_path(path, cx);
        }
    }

    pub fn switch_tab(&mut self, index: usize, cx: &mut Context<Self>) {
        if index < self.tabs.len() && self.active_tab_idx != index {
            let find_bar = if self.active_tab_kind() == Some(TabKind::Editor)
                && self.tabs[index].kind == TabKind::Editor
            {
                self.transfer_find_bar(cx)
            } else {
                None
            };
            self.active_tab_idx = index;
            self.restore_find_bar(find_bar, cx);
            self.sync_active_editor_state(cx);
        }
    }

    pub fn next_tab(&mut self, cx: &mut Context<Self>) {
        if !self.tabs.is_empty() {
            let next_idx = (self.active_tab_idx + 1) % self.tabs.len();
            self.switch_tab(next_idx, cx);
        }
    }

    pub fn prev_tab(&mut self, cx: &mut Context<Self>) {
        if !self.tabs.is_empty() {
            let prev_idx = (self.active_tab_idx + self.tabs.len() - 1) % self.tabs.len();
            self.switch_tab(prev_idx, cx);
        }
    }

    pub fn open_file(&mut self, path: PathBuf, cx: &mut Context<Self>) {
        let path = if path.is_dir() {
            if self.collocated_mode {
                let Some(index_path) = crate::fs::find_collocated_index_file(&path) else {
                    return;
                };
                index_path
            } else {
                return;
            }
        } else {
            path
        };

        for (i, tab) in self.tabs.iter().enumerate() {
            if tab.editor.read(cx).file_path() == Some(&path) {
                self.switch_tab(i, cx);
                self.defer_reveal_path(path, cx);
                return;
            }
        }

        let is_current_empty_and_untitled = self.active_editor().is_some_and(|ed| {
            let ed = ed.read(cx);
            ed.file_path().is_none() && !ed.is_dirty() && ed.is_empty()
        });

        if is_current_empty_and_untitled {
            if let Some(active_editor) = self.active_editor().cloned() {
                active_editor.update(cx, |editor, _| {
                    editor.begin_loading(path.clone());
                });
                let task = Self::spawn_file_load(path.clone(), active_editor.downgrade(), cx);
                self.tabs[self.active_tab_idx]._load_task = Some(task);
                self.sync_active_editor_state(cx);
            }
        } else {
            let find_bar = self.transfer_find_bar(cx);
            let (editor, subscription, load_task) = Self::init_editor(Some(path.clone()), cx);
            self.tabs.push(TabItem {
                _load_task: load_task,
                editor,
                kind: TabKind::Editor,
                is_pinned: false,
                _subscription: subscription,
            });
            self.active_tab_idx = self.tabs.len() - 1;
            self.restore_find_bar(find_bar, cx);
            self.sync_active_editor_state(cx);
        }

        self.defer_reveal_path(path, cx);
    }

    fn defer_reveal_path(&self, path: PathBuf, cx: &mut Context<Self>) {
        let project_panel = self.project_panel.clone();
        cx.defer(move |cx| {
            project_panel.update(cx, |panel, cx| {
                panel.reveal_path(&path, cx);
            });
        });
    }

    /// Closes any open tabs associated with the deleted path or any file inside it.
    pub fn handle_path_deleted(&mut self, path: &std::path::Path, cx: &mut Context<Self>) {
        let mut i = 0;
        let mut closed_any = false;
        while i < self.tabs.len() {
            let is_match = self.tabs[i]
                .editor
                .read(cx)
                .file_path()
                .is_some_and(|p| p == path || p.starts_with(path));
            if is_match {
                self.close_tab(i, cx);
                closed_any = true;
            } else {
                i += 1;
            }
        }
        if closed_any {
            self.sync_active_editor_state(cx);
        }
    }

    /// Updates paths for any open tabs matching the renamed file or directory.
    pub(crate) fn handle_path_renamed(
        &mut self,
        old_path: &std::path::Path,
        new_path: &std::path::Path,
        cx: &mut Context<Self>,
    ) {
        let mut renamed_any = false;
        for tab in &self.tabs {
            let current_path = tab.editor.read(cx).file_path().map(|p| p.to_path_buf());
            if let Some(ref p) = current_path {
                if p == old_path {
                    tab.editor.update(cx, |ed, cx| {
                        ed.update_file_path(new_path.to_path_buf(), cx);
                    });
                    renamed_any = true;
                } else if let Ok(suffix) = p.strip_prefix(old_path) {
                    let updated = new_path.join(suffix);
                    tab.editor.update(cx, |ed, cx| {
                        ed.update_file_path(updated, cx);
                    });
                    renamed_any = true;
                }
            }
        }
        if renamed_any {
            self.sync_active_editor_state(cx);
        }
    }

    pub fn workspace_name(&self, _cx: &App) -> String {
        match &self.root_dir {
            Some(root) => root
                .file_name()
                .and_then(|n| n.to_str())
                .filter(|n| !n.is_empty())
                .map(str::to_string)
                .unwrap_or_else(|| root.to_string_lossy().into_owned()),
            None => "Open Workspace".to_string(),
        }
    }

    pub fn has_workspace(&self, _cx: &App) -> bool {
        self.root_dir.is_some()
    }

    pub fn open_folder(&mut self, folder: PathBuf, cx: &mut Context<Self>) {
        self.persists_session = true;
        self.image_cache.update(cx, |cache, cx| cache.clear(cx));
        self.root_dir = Some(folder.clone());
        let folder_clone = folder.clone();
        self.project_panel.update(cx, |panel, cx| {
            panel.set_root_dir(Some(folder_clone), cx);
        });
        self.search_panel.update(cx, |sp, cx| {
            sp.set_root_dir(Some(folder.clone()), cx);
        });

        if let Some(file) = fs::find_folder_target_file(&folder, self.collocated_mode) {
            self.open_file(file, cx);
        }
        cx.emit(WorkspaceEvent::TitleChanged);
        cx.notify();
    }

    pub fn close_workspace(&mut self, cx: &mut Context<Self>) {
        self.persists_session = true;
        self.image_cache.update(cx, |cache, cx| cache.clear(cx));
        self.root_dir = None;
        self.project_panel.update(cx, |panel, cx| {
            panel.set_root_dir(None, cx);
            panel.set_filter_visible(false, cx);
        });
        self.search_panel.update(cx, |sp, cx| {
            sp.set_root_dir(None, cx);
        });
        self.tabs.clear();
        self.active_tab_idx = 0;
        self.sync_active_editor_state(cx);
        cx.emit(WorkspaceEvent::TitleChanged);
        cx.notify();
    }

    pub fn sync_workspace_state(&self, config: &mut AppConfig, cx: &App) {
        self.sync_workspace_state_with_window(config, None, cx);
    }

    pub fn sync_workspace_state_with_window(
        &self,
        config: &mut AppConfig,
        window_bounds: Option<crate::config::WindowLayoutState>,
        cx: &App,
    ) {
        let mut open_files = Vec::new();
        let mut active_file = None;

        for (i, tab) in self.tabs.iter().enumerate() {
            if tab.kind == TabKind::Editor
                && let Some(path) = tab.editor.read(cx).file_path_buf()
            {
                if i == self.active_tab_idx {
                    active_file = Some(path.clone());
                }
                open_files.push(path);
            }
        }

        let mut state = WorkspaceState {
            root_dir: self.root_dir.clone(),
            active_file,
            open_files,
            sidebar_visible: self.sidebar_visible,
            sidebar_width: Some(f32::from(self.sidebar_width)),
            sidebar_view: self.sidebar_view,
            outline_visible: self.outline_visible,
            outline_width: Some(f32::from(self.outline_width)),
            window_bounds,
        };
        if state.window_bounds.is_none() {
            state.window_bounds = config.existing_window_bounds_for(&state);
        }

        config.update_last_workspace(state);
    }

    #[inline]
    pub fn persists_session(&self) -> bool {
        self.persists_session
    }

    pub fn root_dir(&self) -> Option<&std::path::Path> {
        self.root_dir.as_deref()
    }

    pub fn project_panel(&self) -> &Entity<crate::app::project_panel::ProjectPanel> {
        &self.project_panel
    }

    pub fn sidebar_visible(&self) -> bool {
        self.sidebar_visible
    }

    pub fn sidebar_width(&self) -> Pixels {
        self.sidebar_width
    }

    pub fn outline_width(&self) -> Pixels {
        self.outline_width
    }

    #[inline]
    pub fn is_resizing_sidebar(&self) -> bool {
        self.resize_target == Some(PanelResizeTarget::Sidebar)
    }

    #[inline]
    pub fn is_resizing_outline(&self) -> bool {
        self.resize_target == Some(PanelResizeTarget::Outline)
    }

    #[inline]
    pub fn is_resizing_panel(&self) -> bool {
        self.resize_target.is_some()
    }

    pub fn set_sidebar_width(&mut self, width: Pixels, cx: &mut Context<Self>) {
        let clamped = sidebar::clamp_sidebar_width(width);
        if self.sidebar_width != clamped {
            self.sidebar_width = clamped;
            let offset = if self.sidebar_visible {
                clamped
            } else {
                px(0.0)
            };
            self.tab_bar
                .update(cx, |tb, cx| tb.set_sidebar_offset(offset, cx));
            cx.notify();
        }
    }

    pub fn set_outline_width(&mut self, width: Pixels, cx: &mut Context<Self>) {
        let clamped = sidebar::clamp_sidebar_width(width);
        if self.outline_width != clamped {
            self.outline_width = clamped;
            cx.notify();
        }
    }

    pub fn start_sidebar_resize(&mut self, mouse_x: Pixels, cx: &mut Context<Self>) {
        self.start_panel_resize(PanelResizeTarget::Sidebar, mouse_x, cx);
    }

    pub fn start_outline_resize(&mut self, mouse_x: Pixels, cx: &mut Context<Self>) {
        self.start_panel_resize(PanelResizeTarget::Outline, mouse_x, cx);
    }

    fn start_panel_resize(
        &mut self,
        target: PanelResizeTarget,
        mouse_x: Pixels,
        cx: &mut Context<Self>,
    ) {
        self.resize_target = Some(target);
        self.panel_drag_start_x = Some(mouse_x);
        self.panel_drag_start_width = Some(match target {
            PanelResizeTarget::Sidebar => self.sidebar_width,
            PanelResizeTarget::Outline => self.outline_width,
        });
        cx.notify();
    }

    pub fn handle_sidebar_resize_drag(&mut self, current_x: Pixels, cx: &mut Context<Self>) {
        if self.is_resizing_sidebar() {
            self.handle_panel_resize_drag(current_x, cx);
        }
    }

    pub fn handle_outline_resize_drag(&mut self, current_x: Pixels, cx: &mut Context<Self>) {
        if self.is_resizing_outline() {
            self.handle_panel_resize_drag(current_x, cx);
        }
    }

    pub fn handle_panel_resize_drag(&mut self, current_x: Pixels, cx: &mut Context<Self>) {
        if let (Some(target), Some(start_x), Some(start_w)) = (
            self.resize_target,
            self.panel_drag_start_x,
            self.panel_drag_start_width,
        ) {
            let delta = current_x - start_x;
            match target {
                PanelResizeTarget::Sidebar => self.set_sidebar_width(start_w + delta, cx),
                PanelResizeTarget::Outline => self.set_outline_width(start_w - delta, cx),
            }
        }
    }

    pub fn finish_panel_resize(&mut self, cx: &mut Context<Self>) {
        let target = self.resize_target.take();
        self.panel_drag_start_x = None;
        let start_width = self.panel_drag_start_width.take();
        let changed = match (target, start_width) {
            (Some(PanelResizeTarget::Sidebar), Some(start)) => start != self.sidebar_width,
            (Some(PanelResizeTarget::Outline), Some(start)) => start != self.outline_width,
            _ => false,
        };
        if changed {
            cx.emit(WorkspaceEvent::LayoutChanged);
        }
        cx.notify();
    }

    pub fn finish_sidebar_resize(&mut self, cx: &mut Context<Self>) {
        if self.is_resizing_sidebar() {
            self.finish_panel_resize(cx);
        }
    }

    pub fn reset_sidebar_width(&mut self, cx: &mut Context<Self>) {
        let was_resizing = self.is_resizing_sidebar();
        let start_width = self.panel_drag_start_width;
        if was_resizing {
            self.resize_target = None;
            self.panel_drag_start_x = None;
            self.panel_drag_start_width = None;
        }
        let prev = start_width.unwrap_or(self.sidebar_width);
        self.set_sidebar_width(px(DEFAULT_SIDEBAR_WIDTH), cx);
        if prev != self.sidebar_width {
            cx.emit(WorkspaceEvent::LayoutChanged);
        } else if was_resizing {
            cx.notify();
        }
    }

    pub fn reset_outline_width(&mut self, cx: &mut Context<Self>) {
        let was_resizing = self.is_resizing_outline();
        let start_width = self.panel_drag_start_width;
        if was_resizing {
            self.resize_target = None;
            self.panel_drag_start_x = None;
            self.panel_drag_start_width = None;
        }
        let prev = start_width.unwrap_or(self.outline_width);
        self.set_outline_width(px(DEFAULT_SIDEBAR_WIDTH), cx);
        if prev != self.outline_width {
            cx.emit(WorkspaceEvent::LayoutChanged);
        } else if was_resizing {
            cx.notify();
        }
    }

    pub fn set_sidebar_visible(&mut self, visible: bool, cx: &mut Context<Self>) {
        if self.sidebar_visible != visible {
            self.sidebar_visible = visible;
            if self.sidebar_view == SidebarView::Search {
                self.project_panel.update(cx, |panel, cx| {
                    panel.set_filter_visible(visible, cx);
                });
                if !visible {
                    self.release_idle_file_index(cx);
                }
            }
            self.sync_sidebar_state(cx);
            cx.emit(WorkspaceEvent::LayoutChanged);
            cx.notify();
        }
    }

    /// Shows the sidebar with `view` on screen.
    pub fn show_sidebar(&mut self, view: SidebarView, cx: &mut Context<Self>) {
        self.project_panel.update(cx, |panel, cx| {
            panel.set_filter_visible(view == SidebarView::Search, cx);
        });
        if view != SidebarView::Search {
            self.release_idle_file_index(cx);
        }
        if !self.sidebar_visible || self.sidebar_view != view {
            self.sidebar_visible = true;
            self.sidebar_view = view;
            self.sync_sidebar_state(cx);
            cx.emit(WorkspaceEvent::LayoutChanged);
            cx.notify();
        }
    }

    /// The sidebar panel on screen; `None` while the sidebar is hidden.
    pub fn active_sidebar(&self) -> Option<SidebarView> {
        self.sidebar_visible.then_some(self.sidebar_view)
    }

    pub fn outline_visible(&self) -> bool {
        self.outline_visible
    }

    pub fn toggle_outline_panel(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.outline_visible = !self.outline_visible;
        self.status_bar.update(cx, |status_bar, cx| {
            status_bar.set_outline_visible(self.outline_visible, cx)
        });
        if self.outline_visible {
            self.sync_outline_panel(cx);
        } else {
            self.focus_active_editor(window, cx);
        }
        cx.emit(WorkspaceEvent::LayoutChanged);
        cx.notify();
    }

    /// Pushes the sidebar state to the views that mirror it.
    fn sync_sidebar_state(&mut self, cx: &mut Context<Self>) {
        let offset = if self.sidebar_visible {
            self.sidebar_width
        } else {
            px(0.0)
        };
        let active = self.active_sidebar();
        self.tab_bar
            .update(cx, |tb, cx| tb.set_sidebar_offset(offset, cx));
        self.status_bar
            .update(cx, |sb, cx| sb.set_active_sidebar(active, cx));
    }

    pub fn active_file_path<'a>(&'a self, cx: &'a App) -> Option<&'a std::path::Path> {
        self.active_editor().and_then(|ed| ed.read(cx).file_path())
    }

    pub(crate) fn is_workspace_search_active(&self) -> bool {
        self.active_tab_kind() == Some(TabKind::WorkspaceSearch)
    }

    pub(crate) fn get_tabs_meta(&self, cx: &App) -> Vec<tab_bar::TabMeta> {
        tabs::collect_tabs_meta(&self.tabs, cx)
    }

    pub fn tab_bar(&self) -> &Entity<tab_bar::TabBar> {
        &self.tab_bar
    }

    pub fn status_bar(&self) -> &Entity<status_bar::StatusBar> {
        &self.status_bar
    }

    pub fn search_panel(&self) -> &Entity<SearchPanel> {
        &self.search_panel
    }

    pub fn outline_panel(&self) -> &Entity<OutlinePanel> {
        &self.outline_panel
    }

    fn resolve_outline_active_line(&self, editor: &Editor) -> Option<SourceLine> {
        match self.outline_active_line_source {
            OutlineActiveLineSource::Cursor => Some(editor.cursor_pos().line),
            OutlineActiveLineSource::Viewport => editor
                .first_visible_source_line()
                .or_else(|| Some(editor.cursor_pos().line)),
        }
    }

    fn sync_outline_panel(&mut self, cx: &mut Context<Self>) {
        // Hidden outlines catch up when opened; input events should not build
        // an invisible panel's structure index.
        if !self.outline_visible {
            return;
        }
        let Some(entity) = self.active_editor().cloned() else {
            self.outline_displayed = None;
            self.outline_panel.update(cx, |panel, cx| panel.clear(cx));
            return;
        };

        let entity_id = entity.entity_id();
        let same_document = self
            .outline_displayed
            .is_some_and(|displayed| displayed.editor_id() == entity_id);
        if !same_document {
            self.outline_active_line_source = OutlineActiveLineSource::Cursor;
        }

        let is_loading = entity.read(cx).is_loading();
        let is_empty_loading = is_loading && entity.read(cx).buffer().is_empty();
        if is_empty_loading {
            let key = OutlineDisplayed::EmptyLoading(entity_id);
            if self.outline_displayed != Some(key) {
                self.outline_displayed = Some(key);
                self.outline_panel
                    .update(cx, |panel, cx| panel.set_loading_pending(cx));
            }
            return;
        }

        enum Action {
            UpdateActiveLine(Option<SourceLine>),
            InstallSnapshot {
                key: OutlineDisplayed,
                source: crate::buffer::SourceSnapshot,
                is_loading: bool,
                active: Option<SourceLine>,
                focus: Option<FocusHandle>,
            },
        }

        let action = {
            use crate::buffer::LineSource;
            let editor = entity.read(cx);
            let key = OutlineDisplayed::Snapshot {
                editor_id: entity_id,
                outline_revision: editor.buffer().outline_revision(),
                is_loading,
            };
            let active = if same_document {
                self.resolve_outline_active_line(editor)
            } else {
                Some(editor.cursor_pos().line)
            };
            if self.outline_displayed == Some(key) {
                Action::UpdateActiveLine(active)
            } else {
                Action::InstallSnapshot {
                    key,
                    source: editor.buffer().source_snapshot(),
                    is_loading,
                    active,
                    focus: Some(editor.focus_handle(cx)),
                }
            }
        };

        match action {
            Action::UpdateActiveLine(active) => {
                self.outline_panel
                    .update(cx, |panel, cx| panel.set_active_line(active, cx));
            }
            Action::InstallSnapshot {
                key,
                source,
                is_loading,
                active,
                focus,
            } => {
                self.outline_displayed = Some(key);
                self.outline_panel.update(cx, |panel, cx| {
                    panel.set_snapshot(source, is_loading, active, focus, cx)
                });
            }
        }
    }

    pub fn jump_to_outline(&mut self, line: SourceLine, cx: &mut Context<Self>) {
        if let Some(editor) = self.active_editor().cloned() {
            self.outline_active_line_source = OutlineActiveLineSource::Cursor;
            editor.update(cx, |editor, cx| {
                editor.buffer_mut().set_cursor(Position::new(line, 0));
                editor.scroll_to_cursor();
                editor.start_scroll_animation_if_needed(cx);
                editor.reset_cursor_blink(cx);
            });
            self.sync_outline_panel(cx);
        }
    }

    /// Hides the sidebar when `view` is already on screen, otherwise shows it.
    ///
    /// Hiding hands focus back to the editor, since the focused panel may be the one going away.
    pub fn toggle_sidebar_view(
        &mut self,
        view: SidebarView,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.active_sidebar() == Some(view) {
            self.set_sidebar_visible(false, cx);
            self.focus_active_editor(window, cx);
        } else {
            self.show_sidebar(view, cx);
            if view == SidebarView::Search {
                self.project_panel.update(cx, |panel, cx| {
                    panel.focus_filter(window, cx);
                });
            }
        }
    }

    /// Opens `path` with the cursor at `position`. Focus is left to the caller.
    pub fn open_file_at(&mut self, path: PathBuf, position: Position, cx: &mut Context<Self>) {
        self.open_file(path, cx);
        if let Some(active_editor) = self.active_editor() {
            active_editor.update(cx, |ed, cx| {
                ed.open_at_position(position, cx);
            });
        }
    }

    /// Shows the search panel and focuses its input. The file listing is refreshed because
    /// files may have changed outside the app while the panel was closed.
    pub fn open_workspace_search(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.show_sidebar(SidebarView::Search, cx);
        if let Some(index) = self
            .tabs
            .iter()
            .position(|tab| tab.kind == TabKind::WorkspaceSearch)
        {
            self.switch_tab(index, cx);
        } else {
            let (editor, subscription, load_task) = Self::init_editor(None, cx);
            self.tabs.push(TabItem {
                _load_task: load_task,
                editor,
                kind: TabKind::WorkspaceSearch,
                is_pinned: false,
                _subscription: subscription,
            });
            self.active_tab_idx = self.tabs.len() - 1;
            self.sync_active_editor_state(cx);
        }
        let reuse_active_index = self.project_panel.read(cx).has_filter_index_activity();
        self.search_panel.update(cx, |sp, cx| {
            sp.set_active(true, cx);
            if !reuse_active_index {
                sp.refresh_files(cx);
            }
            sp.focus_input(window, cx);
        });
    }
}

impl Render for Workspace {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.image_cache
            .update(cx, |cache, cx| cache.begin_frame(window, cx));
        let active_editor = self.active_editor().cloned();
        let active_is_search = self.active_tab_kind() == Some(TabKind::WorkspaceSearch);
        let sidebar_visible = self.sidebar_visible;
        let has_tabs = !self.tabs.is_empty();
        let has_workspace = self.root_dir.is_some();
        let (flush_left, flush_right) = if has_tabs {
            self.tab_bar.read(cx).active_tab_edge_flush()
        } else {
            (false, false)
        };
        let canvas_tl_radius = if flush_left {
            px(0.0)
        } else {
            crate::ui::Radius::CANVAS
        };
        let canvas_tr_radius = if flush_right {
            px(0.0)
        } else {
            crate::ui::Radius::CANVAS
        };
        let theme = cx
            .try_global::<ThemeManager>()
            .map(|m| m.theme())
            .unwrap_or(&DEFAULT_THEME)
            .clone();

        div()
            .image_cache(self.image_cache.clone())
            .key_context(crate::ui::key_context::WORKSPACE)
            .track_focus(&self.focus_handle)
            .font_family(crate::platform::platform_ui_font())
            .flex_1()
            .min_h_0()
            .size_full()
            .flex()
            .flex_col()
            .when(self.is_resizing_panel(), |this| {
                this.child(sidebar::render_panel_resize_canvas(cx.entity().downgrade()))
            })
            .child(
                div()
                    .flex_1()
                    .min_h_0()
                    .w_full()
                    .flex()
                    .flex_row()
                    .when(sidebar_visible, |this| {
                        let content = match self.sidebar_view {
                            SidebarView::Files => self.project_panel.clone().into_any_element(),
                            SidebarView::Search => self.project_panel.clone().into_any_element(),
                        };
                        this.child(sidebar::render_sidebar_panel(
                            self.sidebar_width,
                            content,
                            &theme,
                        ))
                    })
                    .when(sidebar_visible, |this| {
                        this.child(sidebar::render_sidebar_resize_handle(
                            self.is_resizing_sidebar(),
                            cx.entity().downgrade(),
                            &theme,
                        ))
                    })
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .h_full()
                            .when(!sidebar_visible, |this| this.pl(crate::ui::Spacing::SM))
                            .when(!self.outline_visible, |this| {
                                this.pr(crate::ui::Spacing::SM)
                            })
                            .flex()
                            // Reverse column order paints TabBar after the editor so its 1px
                            // bottom apron and concave fillets seal cleanly over the canvas edge.
                            .flex_col_reverse()
                            .child(
                                div()
                                    .flex_1()
                                    .min_h_0()
                                    .w_full()
                                    .when(has_tabs, |this| this.mt(px(-1.0)))
                                    .bg(theme.bg_editor)
                                    .rounded_tl(canvas_tl_radius)
                                    .rounded_tr(canvas_tr_radius)
                                    .rounded_bl(crate::ui::Radius::CANVAS)
                                    .rounded_br(crate::ui::Radius::CANVAS)
                                    .overflow_hidden()
                                    .on_drop(cx.listener(
                                        |this,
                                         entry: &crate::app::project_panel::DraggedProjectEntry,
                                         _window,
                                         cx| {
                                            if !entry.is_dir
                                                && let Some(open_path) =
                                                    crate::fs::resolve_openable_document_path(
                                                        &entry.path,
                                                        this.collocated_mode,
                                                    )
                                            {
                                                this.open_file(open_path, cx);
                                            }
                                        },
                                    ))
                                    .child(if active_is_search {
                                        self.search_panel.clone().into_any_element()
                                    } else if let Some(editor) = active_editor {
                                        editor.into_any_element()
                                    } else if has_workspace {
                                        div().flex_1().w_full().h_full().into_any_element()
                                    } else {
                                        welcome::render_welcome_view(&self.focus_handle, &theme, cx)
                                            .into_any_element()
                                    }),
                            )
                            .when(has_tabs, |this| this.child(self.tab_bar.clone())),
                    )
                    .when(self.outline_visible, |this| {
                        this.child(sidebar::render_outline_resize_handle(
                            self.is_resizing_outline(),
                            cx.entity().downgrade(),
                            &theme,
                        ))
                        .child(sidebar::render_right_panel(
                            self.outline_width,
                            self.outline_panel.clone().into_any_element(),
                            &theme,
                        ))
                    }),
            )
            .child(self.status_bar.clone())
    }
}

#[cfg(test)]
mod tests {
    use super::Workspace;
    use crate::app::SidebarView;
    use crate::buffer::{Position, SourceLine};
    use crate::config::AppConfig;
    use crate::theme::ThemeManager;
    use gpui::{TestAppContext, px};

    fn init_globals(cx: &mut TestAppContext) {
        let config = AppConfig::default().with_theme("Dracula");
        let theme_manager = ThemeManager::from_config(&config);
        cx.update(|cx| {
            cx.set_global(theme_manager);
            cx.set_global(config);
            crate::app::actions::bind_default_bindings(cx);
        });
    }

    #[gpui::test]
    fn test_workspace_tab_switch_preserves_find_bar(cx: &mut TestAppContext) {
        init_globals(cx);

        let (ws, cx) = cx.add_window_view(|_window, cx| Workspace::new(None, cx));

        ws.update(cx, |ws, cx| {
            ws.new_tab(cx);
            ws.new_tab(cx);
        });

        let (ed0, ed1) = ws.read_with(cx, |ws, _| {
            (ws.tabs[0].editor.clone(), ws.tabs[1].editor.clone())
        });
        ed0.update(cx, |ed, _| {
            ed.buffer_mut().insert_text("first doc with match");
        });
        ed1.update(cx, |ed, _| {
            ed.buffer_mut().insert_text("second doc match match");
        });

        ws.update(cx, |ws, cx| ws.switch_tab(0, cx));
        cx.update(|window, cx| {
            ed0.update(cx, |ed, cx| {
                ed.open_find_bar(window, cx);
                let bar = ed.find_bar().cloned().unwrap();
                bar.update(cx, |bar, cx| bar.focus_query(Some("match"), window, cx));
            });
        });

        cx.run_until_parked();
        ed0.read_with(cx, |ed, _| {
            assert!(ed.is_find_bar_visible());
            assert_eq!(ed.find_state().match_count(), 1);
        });
        ed1.read_with(cx, |ed, _| {
            assert!(!ed.is_find_bar_visible());
        });

        ws.update(cx, |ws, cx| ws.switch_tab(1, cx));
        cx.run_until_parked();
        ed0.read_with(cx, |ed, _| {
            assert!(!ed.is_find_bar_visible());
            assert_eq!(ed.find_state().match_count(), 0);
        });
        ed1.read_with(cx, |ed, cx| {
            assert!(ed.is_find_bar_visible());
            assert_eq!(ed.find_state().match_count(), 2);
            assert_eq!(ed.find_bar().unwrap().read(cx).query_text(cx), "match");
        });

        ws.update(cx, |ws, cx| ws.switch_tab(0, cx));
        cx.run_until_parked();
        ed0.read_with(cx, |ed, cx| {
            assert!(ed.is_find_bar_visible());
            assert_eq!(ed.find_state().match_count(), 1);
            assert_eq!(ed.find_bar().unwrap().read(cx).query_text(cx), "match");
        });
        ed1.read_with(cx, |ed, _| {
            assert!(!ed.is_find_bar_visible());
        });
    }

    #[gpui::test]
    fn tail_edit_hidden_outline_does_not_materialize_document(cx: &mut TestAppContext) {
        init_globals(cx);
        let (ws, cx) = cx.add_window_view(|_, cx| Workspace::new(None, cx));
        ws.update(cx, |ws, cx| ws.new_tab(cx));
        let editor = ws.read_with(cx, |ws, _| ws.active_editor().unwrap().clone());
        editor.update(cx, |editor, _| {
            editor
                .buffer_mut()
                .insert_text(&format!("# Heading\n\n{}Tail", "paragraph\n".repeat(500)));
        });
        let before = editor.read_with(cx, |editor, _| editor.buffer().full_line_materializations());
        ws.update(cx, |ws, cx| {
            assert!(!ws.outline_visible);
            ws.sync_outline_panel(cx);
        });
        assert_eq!(
            editor.read_with(cx, |editor, _| editor.buffer().full_line_materializations()),
            before
        );
    }

    #[gpui::test]
    fn tail_edit_input_handler_does_not_copy_document(cx: &mut TestAppContext) {
        use gpui::EntityInputHandler;
        init_globals(cx);
        let (ws, cx) = cx.add_window_view(|_, cx| Workspace::new(None, cx));
        ws.update(cx, |ws, cx| ws.new_tab(cx));
        let editor = ws.read_with(cx, |ws, _| ws.active_editor().unwrap().clone());
        editor.update(cx, |editor, _| {
            editor
                .buffer_mut()
                .insert_text(&format!("# Heading\n\n{}Tail", "paragraph\n\n".repeat(500)));
            editor.ensure_layout_cache();
        });
        let before = editor.read_with(cx, |editor, _| editor.buffer().full_line_materializations());
        for text in ["x", "界", "🙂"] {
            cx.update(|window, cx| {
                editor.update(cx, |editor, cx| {
                    editor.replace_text_in_range(None, text, window, cx);
                })
            });
        }
        assert_eq!(
            editor.read_with(cx, |editor, _| editor.buffer().full_line_materializations()),
            before
        );
    }

    #[gpui::test]
    fn tail_edit_visible_outline_uses_background_source_snapshot(cx: &mut TestAppContext) {
        init_globals(cx);
        let (ws, cx) = cx.add_window_view(|_, cx| Workspace::new(None, cx));
        ws.update(cx, |ws, cx| ws.new_tab(cx));
        let editor = ws.read_with(cx, |ws, _| ws.active_editor().unwrap().clone());
        editor.update(cx, |editor, _| {
            editor
                .buffer_mut()
                .insert_text("# First\n\nparagraph\n\n## Last");
            editor.ensure_layout_cache();
        });
        let before = editor.read_with(cx, |editor, _| editor.buffer().full_line_materializations());
        cx.update(|window, cx| ws.update(cx, |ws, cx| ws.toggle_outline_panel(window, cx)));
        cx.run_until_parked();
        ws.read_with(cx, |ws, cx| {
            assert_eq!(ws.outline_panel.read(cx).entries().len(), 2);
        });
        editor.update(cx, |editor, _| {
            editor.buffer_mut().insert_text(" edited");
            editor.ensure_layout_cache();
        });
        ws.update(cx, |ws, cx| ws.sync_outline_panel(cx));
        cx.run_until_parked();
        ws.read_with(cx, |ws, cx| {
            assert_eq!(ws.outline_panel.read(cx).entries()[1].title, "Last edited");
        });
        assert_eq!(
            editor.read_with(cx, |editor, _| editor.buffer().full_line_materializations()),
            before
        );
    }

    #[gpui::test]
    fn outline_skips_body_edits_and_updates_immediately_without_loading_flash(
        cx: &mut TestAppContext,
    ) {
        init_globals(cx);
        let (ws, cx) = cx.add_window_view(|_, cx| Workspace::new(None, cx));
        ws.update(cx, |ws, cx| ws.new_tab(cx));
        let editor = ws.read_with(cx, |ws, _| ws.active_editor().unwrap().clone());
        editor.update(cx, |editor, _| {
            editor
                .buffer_mut()
                .insert_text("# First\nbody\n## Second");
            editor.ensure_layout_cache();
        });
        cx.update(|window, cx| ws.update(cx, |ws, cx| ws.toggle_outline_panel(window, cx)));

        let initial_displayed = ws.read_with(cx, |ws, cx| {
            let panel = ws.outline_panel.read(cx);
            assert!(!panel.is_loading());
            assert_eq!(panel.heading_count(), 2);
            assert_eq!(panel.entries().len(), 2);
            ws.outline_displayed
        });

        // Single-line body edit does not bump outline_revision or replace the snapshot.
        editor.update(cx, |editor, cx| {
            editor
                .buffer_mut()
                .record_single_line_edit(SourceLine::new(1), |line, _| *line = "body edited".into());
            cx.emit(crate::editor::EditorEvent::CursorMoved);
        });
        ws.read_with(cx, |ws, cx| {
            let panel = ws.outline_panel.read(cx);
            assert_eq!(ws.outline_displayed, initial_displayed);
            assert!(!panel.is_loading());
            assert_eq!(panel.heading_count(), 2);
        });

        // Heading edit updates synchronously on the virtual snapshot with zero loading state.
        editor.update(cx, |editor, cx| {
            editor
                .buffer_mut()
                .record_single_line_edit(SourceLine::new(0), |line, _| *line = "# First edited".into());
            cx.emit(crate::editor::EditorEvent::CursorMoved);
        });
        ws.read_with(cx, |ws, cx| {
            let panel = ws.outline_panel.read(cx);
            assert!(!panel.is_loading());
            assert_eq!(panel.heading_count(), 2);
            assert_eq!(panel.entries()[0].title, "First edited");
        });

        // Toggling outline off and back on without heading edits preserves snapshot immediately.
        cx.update(|window, cx| ws.update(cx, |ws, cx| ws.toggle_outline_panel(window, cx)));
        cx.update(|window, cx| ws.update(cx, |ws, cx| ws.toggle_outline_panel(window, cx)));
        ws.read_with(cx, |ws, cx| {
            let panel = ws.outline_panel.read(cx);
            assert!(!panel.is_loading());
            assert_eq!(panel.entries()[0].title, "First edited");
        });
    }

    #[gpui::test]
    fn outline_switches_tabs_instantly_and_preserves_viewport_highlight_and_undo_branch(
        cx: &mut TestAppContext,
    ) {
        init_globals(cx);
        let (ws, cx) = cx.add_window_view(|_, cx| Workspace::new(None, cx));
        ws.update(cx, |ws, cx| {
            ws.new_tab(cx);
            ws.new_tab(cx);
        });
        let (ed0, ed1) = ws.read_with(cx, |ws, _| {
            (ws.tabs[0].editor.clone(), ws.tabs[1].editor.clone())
        });
        ed0.update(cx, |ed, _| {
            ed.buffer_mut().insert_text(&format!(
                "# Top\n{}\n## Lower\n{}",
                "line\n".repeat(40),
                "tail\n".repeat(60)
            ));
            ed.ensure_layout_cache();
        });
        ed1.update(cx, |ed, _| {
            ed.buffer_mut().insert_text("# Other Tab");
            ed.ensure_layout_cache();
        });

        ws.update(cx, |ws, cx| ws.switch_tab(0, cx));
        cx.update(|window, cx| ws.update(cx, |ws, cx| ws.toggle_outline_panel(window, cx)));
        ws.read_with(cx, |ws, cx| {
            let panel = ws.outline_panel.read(cx);
            assert!(!panel.is_loading());
            assert_eq!(panel.heading_count(), 2);
        });

        // Switching to another already-loaded tab shows its outline immediately with zero loading flash.
        ws.update(cx, |ws, cx| ws.switch_tab(1, cx));
        ws.read_with(cx, |ws, cx| {
            let panel = ws.outline_panel.read(cx);
            assert!(!panel.is_loading());
            assert_eq!(panel.heading_count(), 1);
            assert_eq!(panel.entries()[0].title, "Other Tab");
        });

        // Switch back to tab 0 and scroll: viewport highlight updates and stays untouched when hidden.
        ws.update(cx, |ws, cx| ws.switch_tab(0, cx));
        let lower_y = ed0.read_with(cx, |ed, _| ed.layout_snapshot().metrics.top(42).unwrap());
        let wheel_event = gpui::ScrollWheelEvent {
            delta: gpui::ScrollDelta::Pixels(gpui::Point::new(px(0.0), -lower_y)),
            ..Default::default()
        };
        ed0.update(cx, |ed, cx| ed.handle_scroll_wheel(&wheel_event, cx));
        ws.read_with(cx, |ws, cx| {
            assert_eq!(
                ws.outline_panel.read(cx).active_line(),
                Some(SourceLine::new(42))
            );
        });

        // ScrollChanged while outline is hidden must not update outline_panel.
        cx.update(|window, cx| ws.update(cx, |ws, cx| ws.toggle_outline_panel(window, cx)));
        let top_scroll = gpui::ScrollWheelEvent {
            delta: gpui::ScrollDelta::Pixels(gpui::Point::new(px(0.0), lower_y)),
            ..Default::default()
        };
        ed0.update(cx, |ed, cx| ed.handle_scroll_wheel(&top_scroll, cx));
        ws.read_with(cx, |ws, cx| {
            assert_eq!(
                ws.outline_panel.read(cx).active_line(),
                Some(SourceLine::new(42))
            );
        });

        // Undo + new branch edit reusing buffer_version() still refreshes outline via revision().
        cx.update(|window, cx| ws.update(cx, |ws, cx| ws.toggle_outline_panel(window, cx)));
        ed0.update(cx, |ed, cx| {
            ed.buffer_mut().set_cursor(Position::new(SourceLine::new(0), 0));
            ed.buffer_mut()
                .record_single_line_edit(SourceLine::new(0), |line, _| *line = "# Top v2".into());
            cx.emit(crate::editor::EditorEvent::CursorMoved);
        });
        let cached_version = ed0.read_with(cx, |ed, _| ed.buffer_version());
        ed0.update(cx, |ed, cx| {
            ed.buffer_mut().undo();
            ed.buffer_mut()
                .record_single_line_edit(SourceLine::new(0), |line, _| {
                    *line = "# Top branched".into()
                });
            assert_eq!(ed.buffer_version(), cached_version);
            cx.emit(crate::editor::EditorEvent::CursorMoved);
        });
        ws.read_with(cx, |ws, cx| {
            assert_eq!(
                ws.outline_panel.read(cx).entries()[0].title,
                "Top branched"
            );
        });
    }

    #[gpui::test]
    fn outline_delays_loading_indicator_and_streams_headings_while_file_loads(
        cx: &mut TestAppContext,
    ) {
        init_globals(cx);
        let dir = tempfile::tempdir().unwrap();
        let missing_or_slow = dir.path().join("slow.md");
        let (ws, cx) = cx.add_window_view(|_, cx| Workspace::new(None, cx));
        ws.update(cx, |ws, cx| ws.new_tab(cx));
        let editor = ws.read_with(cx, |ws, _| ws.active_editor().unwrap().clone());

        // Simulate an editor that is still streaming from disk with an empty buffer.
        editor.update(cx, |ed, _| {
            ed.begin_loading(missing_or_slow);
        });
        cx.update(|window, cx| ws.update(cx, |ws, cx| ws.toggle_outline_panel(window, cx)));

        // Before 150ms, the loading indicator stays pending (not visible) to avoid flashes.
        ws.read_with(cx, |ws, cx| {
            let panel = ws.outline_panel.read(cx);
            assert!(!panel.is_loading());
            assert_eq!(panel.heading_count(), 0);
        });

        cx.background_executor
            .advance_clock(std::time::Duration::from_millis(160));
        cx.run_until_parked();
        ws.read_with(cx, |ws, cx| {
            let panel = ws.outline_panel.read(cx);
            assert!(panel.is_loading());
        });

        // Streaming delivers headings while editor.is_loading() is still true:
        // outline renders them immediately and clears the loading animation.
        editor.update(cx, |ed, _| {
            ed.buffer_mut().insert_text("# Streamed Heading\nbody");
        });
        ws.update(cx, |ws, cx| ws.sync_outline_panel(cx));
        ws.read_with(cx, |ws, cx| {
            let panel = ws.outline_panel.read(cx);
            assert!(!panel.is_loading());
            assert_eq!(panel.heading_count(), 1);
            assert_eq!(panel.entries()[0].title, "Streamed Heading");
        });
    }

    #[gpui::test]
    fn test_workspace_close_tab_preserves_find_bar_to_remaining_tab(cx: &mut TestAppContext) {
        init_globals(cx);

        let (ws, cx) = cx.add_window_view(|_window, cx| Workspace::new(None, cx));
        ws.update(cx, |ws, cx| {
            ws.new_tab(cx);
            ws.new_tab(cx);
        });

        let (ed0, ed1) = ws.read_with(cx, |ws, _| {
            (ws.tabs[0].editor.clone(), ws.tabs[1].editor.clone())
        });
        ed0.update(cx, |ed, _| {
            ed.buffer_mut().insert_text("item in zero");
        });
        ed1.update(cx, |ed, _| {
            ed.buffer_mut().insert_text("item in one");
        });

        cx.update(|window, cx| {
            ed1.update(cx, |ed, cx| {
                ed.open_find_bar(window, cx);
                let bar = ed.find_bar().cloned().unwrap();
                bar.update(cx, |bar, cx| bar.focus_query(Some("item"), window, cx));
            });
        });

        assert!(ed1.read_with(cx, |ed, _| ed.is_find_bar_visible()));

        ws.update(cx, |ws, cx| ws.close_tab(1, cx));
        cx.run_until_parked();
        ed0.read_with(cx, |ed, cx| {
            assert!(ed.is_find_bar_visible());
            assert_eq!(ed.find_state().match_count(), 1);
            assert_eq!(ed.find_bar().unwrap().read(cx).query_text(cx), "item");
        });
    }

    #[gpui::test]
    fn test_workspace_open_workspace_search(cx: &mut TestAppContext) {
        init_globals(cx);

        let (ws, cx) = cx.add_window_view(|_window, cx| Workspace::new(None, cx));
        assert_eq!(
            ws.read_with(cx, |ws, _| ws.active_sidebar()),
            Some(SidebarView::Files)
        );

        cx.update(|window, cx| {
            ws.update(cx, |ws, cx| {
                ws.open_workspace_search(window, cx);
            });
        });

        ws.read_with(cx, |ws, cx| {
            assert_eq!(ws.tab_count(), 1);
            assert!(ws.active_editor().is_none());
            assert_eq!(ws.tab_bar().read(cx).tabs_meta()[0].title, "Search");
            assert_eq!(ws.active_sidebar(), Some(SidebarView::Search));
            assert_eq!(
                ws.status_bar().read(cx).active_sidebar(),
                Some(SidebarView::Search)
            );
        });
    }

    #[gpui::test]
    fn test_workspace_search_tab_is_reused_and_preserves_editor_tabs(cx: &mut TestAppContext) {
        init_globals(cx);

        let (ws, cx) = cx.add_window_view(|_window, cx| Workspace::new(None, cx));
        cx.update(|window, cx| {
            ws.update(cx, |ws, cx| ws.open_workspace_search(window, cx));
        });
        ws.update(cx, |ws, cx| ws.new_tab(cx));
        assert_eq!(ws.read_with(cx, |ws, _| ws.tab_count()), 2);

        cx.update(|window, cx| {
            ws.update(cx, |ws, cx| ws.open_workspace_search(window, cx));
        });
        ws.read_with(cx, |ws, _| {
            assert_eq!(ws.tab_count(), 2);
            assert!(ws.active_editor().is_none());
        });
    }

    #[gpui::test]
    fn test_workspace_open_file_at_position(cx: &mut TestAppContext) {
        init_globals(cx);
        let temp_dir = tempfile::tempdir().unwrap();
        let file = temp_dir.path().join("target.txt");
        std::fs::write(&file, "first line\nsecond target line\nthird line").unwrap();

        let (ws, cx) = cx.add_window_view(|_window, cx| Workspace::new(None, cx));

        ws.update(cx, |ws, cx| {
            ws.open_file_at(
                file.clone(),
                Position::new(crate::buffer::SourceLine::new(1), 7),
                cx,
            );
        });

        cx.run_until_parked();

        ws.read_with(cx, |ws, cx| {
            let active = ws.active_editor().expect("active editor");
            let ed = active.read(cx);
            assert_eq!(ed.file_path(), Some(file.as_path()));
            assert_eq!(
                ed.buffer().cursor_pos(),
                Position::new(crate::buffer::SourceLine::new(1), 7)
            );
        });
    }

    #[gpui::test]
    fn test_reveal_in_project_panel_switches_from_search_to_files(cx: &mut TestAppContext) {
        init_globals(cx);
        let temp_dir = tempfile::tempdir().unwrap();
        let file = temp_dir.path().join("doc.md");
        std::fs::write(&file, "text").unwrap();

        let (ws, cx) = cx.add_window_view(|_window, cx| Workspace::new(Some(file.clone()), cx));
        ws.update(cx, |ws, cx| {
            ws.show_sidebar(SidebarView::Search, cx);
            ws.reveal_tab_in_project_panel(0, cx);
        });

        assert_eq!(
            ws.read_with(cx, |ws, _| ws.active_sidebar()),
            Some(SidebarView::Files)
        );
    }

    #[gpui::test]
    fn test_workspace_startup_defers_recursive_search_and_filter_scans(cx: &mut TestAppContext) {
        init_globals(cx);
        let temp_dir = tempfile::tempdir().unwrap();
        let root = temp_dir.path().to_path_buf();
        std::fs::create_dir_all(root.join("sub")).unwrap();
        std::fs::write(root.join("sub/doc.md"), "needle\n").unwrap();

        let state = crate::config::WorkspaceState {
            root_dir: Some(root),
            active_file: None,
            open_files: vec![],
            sidebar_visible: true,
            sidebar_width: None,
            sidebar_view: SidebarView::Files,
            outline_visible: false,
            outline_width: None,
            window_bounds: None,
        };
        let (ws, cx) = cx.add_window_view(|_window, cx| Workspace::from_state(&state, cx));
        cx.background_executor.run_until_parked();
        cx.run_until_parked();

        ws.read_with(cx, |ws, cx| {
            assert!(!ws.project_panel.read(cx).has_filter_index_activity());
            assert!(!ws.search_panel.read(cx).has_cached_files());
        });

        cx.update(|window, cx| {
            ws.update(cx, |ws, cx| ws.open_workspace_search(window, cx));
        });
        cx.background_executor.run_until_parked();
        cx.run_until_parked();

        ws.read_with(cx, |ws, cx| {
            assert!(!ws.project_panel.read(cx).has_filter_index_activity());
            assert!(ws.search_panel.read(cx).has_cached_files());
        });

        ws.update(cx, |ws, cx| ws.close_active_tab(cx));
        ws.read_with(cx, |ws, cx| {
            assert!(!ws.search_panel.read(cx).has_cached_files());
        });

        cx.update(|window, cx| {
            ws.update(cx, |ws, cx| {
                ws.show_sidebar(SidebarView::Files, cx);
                ws.toggle_sidebar_view(SidebarView::Search, window, cx);
            });
        });
        cx.background_executor.run_until_parked();
        cx.run_until_parked();

        ws.read_with(cx, |ws, cx| {
            assert!(ws.project_panel.read(cx).has_filter_index_activity());
        });
    }

    #[gpui::test]
    fn test_workspace_editor_prepaint_routes_markdown_images_through_bounded_cache(
        cx: &mut TestAppContext,
    ) {
        init_globals(cx);
        let temp_dir = tempfile::tempdir().unwrap();
        let img_path = temp_dir.path().join("shot.png");
        let doc_path = temp_dir.path().join("note.md");
        // Valid 1x1 RGBA PNG.
        #[rustfmt::skip]
        let png_bytes: &[u8] = &[
            0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A,
            0x00, 0x00, 0x00, 0x0D, b'I', b'H', b'D', b'R',
            0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01,
            0x08, 0x06, 0x00, 0x00, 0x00, 0x1F, 0x15, 0xC4, 0x89,
            0x00, 0x00, 0x00, 0x0D, b'I', b'D', b'A', b'T',
            0x78, 0x9C, 0x63, 0xF8, 0xCF, 0xC0, 0xF0, 0x1F, 0x00, 0x05, 0x00, 0x01, 0xFF,
            0x89, 0x99, 0x3D, 0x1D,
            0x00, 0x00, 0x00, 0x00, b'I', b'E', b'N', b'D',
            0xAE, 0x42, 0x60, 0x82,
        ];
        std::fs::write(&img_path, png_bytes).unwrap();
        std::fs::write(&doc_path, "![shot](shot.png)\n").unwrap();

        let (ws, cx) = cx.add_window_view(|_window, cx| Workspace::new(Some(doc_path.clone()), cx));
        cx.run_until_parked();
        cx.update(|window, cx| {
            window.refresh();
            window.draw(cx).clear();
        });
        cx.run_until_parked();
        cx.update(|window, cx| {
            window.refresh();
            window.draw(cx).clear();
        });
        cx.run_until_parked();

        ws.read_with(cx, |ws, cx| {
            let ed = ws.active_editor().expect("active editor").read(cx);
            assert!(!ed.is_loading(), "editor still loading");
            assert!(
                ed.viewport_bounds.size.height > gpui::px(0.0),
                "viewport bounds empty: {:?}",
                ed.viewport_bounds
            );
            assert_eq!(ws.image_cache.read(cx).entry_count(), 1);
        });

        let resource: gpui::Resource = img_path.into();
        cx.update(|window, cx| {
            assert!(
                window
                    .get_asset::<gpui::ImgResourceLoader>(&resource, cx)
                    .is_none(),
                "image must be cached in Workspace's BoundedImageCache, not global App assets"
            );
            cx.remove_asset::<gpui::ImgResourceLoader>(&resource);
        });
    }

    #[gpui::test]
    fn test_workspace_lifecycle_clears_image_cache(cx: &mut TestAppContext) {
        init_globals(cx);
        let temp_dir = tempfile::tempdir().unwrap();
        let img_path = temp_dir.path().join("shot.png");
        let doc_path = temp_dir.path().join("note.md");
        #[rustfmt::skip]
        let png_bytes: &[u8] = &[
            0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A,
            0x00, 0x00, 0x00, 0x0D, b'I', b'H', b'D', b'R',
            0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01,
            0x08, 0x06, 0x00, 0x00, 0x00, 0x1F, 0x15, 0xC4, 0x89,
            0x00, 0x00, 0x00, 0x0D, b'I', b'D', b'A', b'T',
            0x78, 0x9C, 0x63, 0xF8, 0xCF, 0xC0, 0xF0, 0x1F, 0x00, 0x05, 0x00, 0x01, 0xFF,
            0x89, 0x99, 0x3D, 0x1D,
            0x00, 0x00, 0x00, 0x00, b'I', b'E', b'N', b'D',
            0xAE, 0x42, 0x60, 0x82,
        ];
        std::fs::write(&img_path, png_bytes).unwrap();
        std::fs::write(&doc_path, "![shot](shot.png)\n").unwrap();

        let (ws, cx) = cx.add_window_view(|_window, cx| Workspace::new(Some(doc_path.clone()), cx));
        let draw_once = |cx: &mut gpui::VisualTestContext| {
            cx.run_until_parked();
            cx.update(|window, cx| {
                window.refresh();
                window.draw(cx).clear();
            });
            cx.run_until_parked();
        };
        draw_once(cx);
        draw_once(cx);
        ws.read_with(cx, |ws, cx| {
            assert_eq!(ws.image_cache.read(cx).entry_count(), 1);
        });

        let folder_b = temp_dir.path().join("folder_b");
        std::fs::create_dir_all(&folder_b).unwrap();
        // Switch to a blank tab first so `note.md` does not re-populate the cache on redraw;
        // `shot.png` remains cached across the tab switch until `open_folder` clears it.
        ws.update(cx, |ws, cx| ws.new_tab(cx));
        draw_once(cx);
        ws.read_with(cx, |ws, cx| {
            assert_eq!(ws.image_cache.read(cx).entry_count(), 1);
        });
        ws.update(cx, |ws, cx| ws.open_folder(folder_b.clone(), cx));
        ws.read_with(cx, |ws, cx| {
            assert_eq!(ws.image_cache.read(cx).entry_count(), 0);
        });

        ws.update(cx, |ws, cx| ws.open_file(doc_path.clone(), cx));
        draw_once(cx);
        draw_once(cx);
        ws.read_with(cx, |ws, cx| {
            assert_eq!(ws.image_cache.read(cx).entry_count(), 1);
        });

        ws.update(cx, |ws, cx| ws.close_workspace(cx));
        ws.read_with(cx, |ws, cx| {
            assert_eq!(ws.image_cache.read(cx).entry_count(), 0);
        });

        ws.update(cx, |ws, cx| ws.open_file(doc_path.clone(), cx));
        draw_once(cx);
        draw_once(cx);
        ws.read_with(cx, |ws, cx| {
            assert_eq!(ws.image_cache.read(cx).entry_count(), 1);
        });

        let restored_state = crate::config::WorkspaceState {
            root_dir: Some(folder_b),
            active_file: None,
            open_files: Vec::new(),
            sidebar_visible: true,
            sidebar_width: None,
            sidebar_view: crate::app::SidebarView::Files,
            outline_visible: false,
            outline_width: None,
            window_bounds: None,
        };
        ws.update(cx, |ws, cx| ws.restore_from_state(&restored_state, cx));
        ws.read_with(cx, |ws, cx| {
            assert_eq!(ws.image_cache.read(cx).entry_count(), 0);
        });
    }

    #[gpui::test]
    fn test_workspace_shares_single_file_index_between_search_and_filter(cx: &mut TestAppContext) {
        init_globals(cx);
        let temp_dir = tempfile::tempdir().unwrap();
        let root = temp_dir.path().to_path_buf();
        std::fs::create_dir_all(root.join("sub")).unwrap();
        std::fs::write(root.join("sub/doc.md"), "needle\n").unwrap();

        let (ws, cx) = cx.add_window_view(|_window, cx| Workspace::new(Some(root.clone()), cx));
        cx.run_until_parked();

        ws.read_with(cx, |ws, _| {
            assert_eq!(ws.file_index.walk_count(), 0);
            assert!(!ws.file_index.is_populated());
        });

        // 1. Open Search -> single walk populates shared index
        cx.update(|window, cx| {
            ws.update(cx, |ws, cx| ws.open_workspace_search(window, cx));
        });
        cx.background_executor.run_until_parked();
        cx.run_until_parked();

        ws.read_with(cx, |ws, cx| {
            assert_eq!(ws.file_index.walk_count(), 1);
            assert!(ws.search_panel.read(cx).has_cached_files());
            assert!(!ws.project_panel.read(cx).has_filter_index_activity());
        });

        // 2. Focus sidebar filter -> reuses shared snapshot with zero extra walk
        cx.update(|window, cx| {
            ws.update(cx, |ws, cx| {
                ws.project_panel
                    .update(cx, |panel, cx| panel.focus_filter(window, cx));
            });
        });
        cx.background_executor.run_until_parked();
        cx.run_until_parked();

        ws.read_with(cx, |ws, cx| {
            assert!(ws.project_panel.read(cx).has_filter_index_activity());
            assert_eq!(ws.file_index.walk_count(), 1);
        });

        // 3. Closing Search tab while filter remains active keeps shared snapshot alive
        ws.update(cx, |ws, cx| ws.close_active_tab(cx));
        ws.read_with(cx, |ws, _| {
            assert!(ws.file_index.is_populated());
        });

        // 4. Closing sidebar filter releases the idle shared snapshot once both are idle
        ws.update(cx, |ws, cx| ws.show_sidebar(SidebarView::Files, cx));
        cx.run_until_parked();
        ws.read_with(cx, |ws, _| {
            assert!(!ws.file_index.is_populated());
        });

        // 5. Reverse order: focus sidebar filter first, then open Search -> still one walk total for both
        cx.update(|window, cx| {
            ws.update(cx, |ws, cx| {
                ws.show_sidebar(SidebarView::Files, cx);
                ws.toggle_sidebar_view(SidebarView::Search, window, cx);
            });
        });
        cx.background_executor.run_until_parked();
        cx.run_until_parked();

        ws.read_with(cx, |ws, cx| {
            assert!(ws.project_panel.read(cx).has_filter_index_activity());
            assert_eq!(ws.file_index.walk_count(), 2);
        });

        cx.update(|window, cx| {
            ws.update(cx, |ws, cx| ws.open_workspace_search(window, cx));
        });
        cx.background_executor.run_until_parked();
        cx.run_until_parked();

        ws.read_with(cx, |ws, cx| {
            assert!(ws.search_panel.read(cx).has_cached_files());
            assert_eq!(ws.file_index.walk_count(), 2);
        });
    }

    #[gpui::test]
    fn test_workspace_releases_file_index_on_close_with_query_and_hidden_sidebar(
        cx: &mut TestAppContext,
    ) {
        init_globals(cx);
        let temp_dir = tempfile::tempdir().unwrap();
        let root = temp_dir.path().to_path_buf();
        std::fs::write(root.join("doc.md"), "needle\n").unwrap();

        let (ws, cx) = cx.add_window_view(|_window, cx| Workspace::new(Some(root.clone()), cx));
        cx.run_until_parked();

        // 1. Open Search, type a query, let the debounced search finish, then close Search tab
        cx.update(|window, cx| {
            ws.update(cx, |ws, cx| {
                ws.open_workspace_search(window, cx);
                ws.search_panel
                    .update(cx, |sp, cx| sp.set_query_text("needle", cx));
            });
        });
        cx.background_executor
            .advance_clock(std::time::Duration::from_millis(300));
        cx.background_executor.run_until_parked();
        cx.run_until_parked();

        ws.update(cx, |ws, cx| ws.close_active_tab(cx));
        cx.run_until_parked();
        ws.read_with(cx, |ws, _| {
            assert!(!ws.file_index.is_populated());
        });

        // 2. FS invalidation while Search tab is closed (with non-empty query) does not run a search
        let walks_before = ws.read_with(cx, |ws, _| ws.file_index.walk_count());
        ws.update(cx, |ws, cx| ws.invalidate_file_index_and_search(cx));
        cx.background_executor.run_until_parked();
        cx.run_until_parked();

        ws.read_with(cx, |ws, cx| {
            assert_ne!(
                ws.search_panel.read(cx).status(),
                crate::app::search_panel::SearchStatus::Searching
            );
            assert_eq!(ws.file_index.walk_count(), walks_before);
            assert!(!ws.file_index.is_populated());
        });

        // 3. Hiding sidebar via toggle_sidebar_view(SidebarView::Search) clears filter & releases index
        cx.update(|window, cx| {
            ws.update(cx, |ws, cx| {
                ws.show_sidebar(SidebarView::Files, cx);
                ws.toggle_sidebar_view(SidebarView::Search, window, cx);
            });
        });
        cx.background_executor.run_until_parked();
        cx.run_until_parked();

        ws.read_with(cx, |ws, cx| {
            assert!(ws.project_panel.read(cx).has_filter_index_activity());
            assert!(ws.file_index.is_populated());
        });

        cx.update(|window, cx| {
            ws.update(cx, |ws, cx| {
                ws.toggle_sidebar_view(SidebarView::Search, window, cx);
            });
        });
        cx.run_until_parked();

        ws.read_with(cx, |ws, cx| {
            assert!(!ws.project_panel.read(cx).has_filter_index_activity());
            assert!(!ws.file_index.is_populated());
        });
    }
}
