use gpui::prelude::*;
use gpui::*;
use std::path::PathBuf;

pub mod fs;
pub mod sidebar;
pub mod tabs;
pub mod welcome;

pub use fs::{extract_workspace_name_from_paths, workspace_safe_bounds};
pub use sidebar::{
    DEFAULT_SIDEBAR_WIDTH, MAX_SIDEBAR_WIDTH, MIN_SIDEBAR_WIDTH, clamp_sidebar_width,
};
pub use tabs::TabItem;
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
    outline_source: Option<(EntityId, u64)>,
    outline_task: Option<Task<()>>,
    outline_width: Pixels,
    search_panel: Entity<SearchPanel>,
    resize_target: Option<PanelResizeTarget>,
    panel_drag_start_x: Option<Pixels>,
    panel_drag_start_width: Option<Pixels>,
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
                this.search_panel.update(cx, |sp, cx| sp.refresh_files(cx));
            }
            crate::app::project_panel::ProjectPanelEvent::PathDeleted(path) => {
                this.handle_path_deleted(path, cx);
                this.search_panel.update(cx, |sp, cx| sp.refresh_files(cx));
            }
            crate::app::project_panel::ProjectPanelEvent::PathCreated(_) => {
                this.search_panel.update(cx, |sp, cx| sp.refresh_files(cx));
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

    pub fn new(initial_path: Option<PathBuf>, cx: &mut Context<Self>) -> Self {
        let focus_handle = cx.focus_handle();
        let (root_dir, file_path) = match initial_path {
            Some(p) if p.is_dir() => {
                let target = Self::find_folder_target_file(&p);
                (Some(p), target)
            }
            Some(p) => {
                let parent = p.parent().map(|d| d.to_path_buf());
                (parent, Some(p))
            }
            None => (None, None),
        };

        let (tabs, active_tab_idx, tabs_meta, status_meta) = if let Some(path) = file_path.clone() {
            let (editor, subscription, load_task) = Self::init_editor(Some(path), cx);
            let meta = editor.read(cx).status_meta();
            let tab_meta = tab_bar::TabMeta {
                is_loading: editor.read(cx).is_loading(),
                title: editor.read(cx).title(),
                is_dirty: editor.read(cx).is_dirty(),
                dir_hint: editor.read(cx).dir_hint(),
                is_pinned: false,
                is_read_only: editor.read(cx).is_read_only(),
                has_file_path: editor.read(cx).file_path().is_some(),
                supports_editing: editor.read(cx).is_editable(),
            };
            (
                vec![TabItem {
                    _load_task: load_task,
                    editor,
                    kind: TabKind::Editor,
                    is_pinned: false,
                    _subscription: subscription,
                }],
                0,
                vec![tab_meta],
                meta,
            )
        } else {
            (Vec::new(), 0, Vec::new(), None)
        };

        let status_bar =
            cx.new(|_cx| status_bar::StatusBar::new(status_meta, Some(SidebarView::default())));
        let sidebar_offset = px(DEFAULT_SIDEBAR_WIDTH);
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
        let tab_bar_sub = Self::subscribe_tab_bar(&tab_bar, cx);
        let project_panel = cx.new(|cx| {
            crate::app::project_panel::ProjectPanel::new(root_dir.clone(), cx.focus_handle())
        });
        let initial_active_file = file_path.clone();
        project_panel.update(cx, |p, cx| {
            p.set_active_file(initial_active_file, cx);
            p.prewarm_filter_index(cx);
        });
        let project_panel_sub = Self::subscribe_project_panel(&project_panel, cx);
        let outline_panel = cx.new(|cx| OutlinePanel::new(cx.focus_handle()));
        let outline_panel_sub = Self::subscribe_outline_panel(&outline_panel, cx);
        let search_panel = cx.new(SearchPanel::new);
        search_panel.update(cx, |sp, cx| sp.set_root_dir(root_dir.clone(), cx));
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
            sidebar_visible: true,
            sidebar_width: px(DEFAULT_SIDEBAR_WIDTH),
            sidebar_view: SidebarView::default(),
            outline_visible: false,
            outline_source: None,
            outline_task: None,
            outline_width: px(DEFAULT_SIDEBAR_WIDTH),
            search_panel,
            resize_target: None,
            panel_drag_start_x: None,
            panel_drag_start_width: None,
        };
        workspace.sync_outline_panel(cx);
        workspace
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
        let focus_handle = cx.focus_handle();
        let (tabs, active_tab_idx) = Self::load_tabs_from_state(ws, cx);

        let status_meta = if tabs.is_empty() {
            None
        } else {
            tabs[active_tab_idx].editor.read(cx).status_meta()
        };
        let active_sidebar = ws.sidebar_visible.then_some(SidebarView::default());
        let status_bar = cx.new(|_cx| status_bar::StatusBar::new(status_meta, active_sidebar));
        let tabs_meta = tabs::collect_tabs_meta(&tabs, cx);
        let sidebar_width = px(ws
            .sidebar_width
            .unwrap_or(DEFAULT_SIDEBAR_WIDTH)
            .clamp(MIN_SIDEBAR_WIDTH, MAX_SIDEBAR_WIDTH));
        let outline_width = px(ws
            .outline_width
            .unwrap_or(DEFAULT_SIDEBAR_WIDTH)
            .clamp(MIN_SIDEBAR_WIDTH, MAX_SIDEBAR_WIDTH));
        let sidebar_offset = if ws.sidebar_visible {
            sidebar_width
        } else {
            px(0.0)
        };
        let has_workspace_root = ws.root_dir.is_some();
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
        let tab_bar_sub = Self::subscribe_tab_bar(&tab_bar, cx);
        let project_panel = cx.new(|cx| {
            crate::app::project_panel::ProjectPanel::new(ws.root_dir.clone(), cx.focus_handle())
        });
        project_panel.update(cx, |p, cx| {
            p.set_active_file(ws.active_file.clone(), cx);
            p.prewarm_filter_index(cx);
        });
        let project_panel_sub = Self::subscribe_project_panel(&project_panel, cx);
        let outline_panel = cx.new(|cx| OutlinePanel::new(cx.focus_handle()));
        let outline_panel_sub = Self::subscribe_outline_panel(&outline_panel, cx);
        let search_panel = cx.new(SearchPanel::new);
        search_panel.update(cx, |sp, cx| sp.set_root_dir(ws.root_dir.clone(), cx));
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
            root_dir: ws.root_dir.clone(),
            sidebar_visible: ws.sidebar_visible,
            sidebar_width,
            sidebar_view: SidebarView::default(),
            outline_visible: false,
            outline_source: None,
            outline_task: None,
            outline_width,
            search_panel,
            resize_target: None,
            panel_drag_start_x: None,
            panel_drag_start_width: None,
        };
        workspace.sync_outline_panel(cx);
        workspace
    }

    pub fn restore_from_state(&mut self, ws: &WorkspaceState, cx: &mut Context<Self>) {
        self.root_dir = ws.root_dir.clone();
        self.sidebar_visible = ws.sidebar_visible;
        self.sidebar_view = SidebarView::Files;
        self.outline_visible = false;
        self.status_bar.update(cx, |status_bar, cx| {
            status_bar.set_outline_visible(false, cx)
        });
        let sidebar_width = px(ws
            .sidebar_width
            .unwrap_or(DEFAULT_SIDEBAR_WIDTH)
            .clamp(MIN_SIDEBAR_WIDTH, MAX_SIDEBAR_WIDTH));
        self.sidebar_width = sidebar_width;
        self.outline_width = px(ws
            .outline_width
            .unwrap_or(DEFAULT_SIDEBAR_WIDTH)
            .clamp(MIN_SIDEBAR_WIDTH, MAX_SIDEBAR_WIDTH));
        self.resize_target = None;
        self.panel_drag_start_x = None;
        self.panel_drag_start_width = None;

        let (tabs, active_tab_idx) = Self::load_tabs_from_state(ws, cx);
        self.tabs = tabs;
        self.active_tab_idx = active_tab_idx;

        self.project_panel.update(cx, |p, cx| {
            p.set_root_dir(ws.root_dir.clone(), cx);
            p.set_active_file(ws.active_file.clone(), cx);
            p.set_filter_visible(false, cx);
        });

        self.sync_sidebar_state(cx);

        self.search_panel.update(cx, |sp, cx| {
            sp.set_root_dir(ws.root_dir.clone(), cx);
        });

        self.sync_active_editor_state(cx);
    }

    pub fn init_editor(
        path: Option<PathBuf>,
        cx: &mut Context<Self>,
    ) -> (Entity<Editor>, Subscription, Option<Task<()>>) {
        let editor = cx.new(|cx| match path.as_ref() {
            Some(p) => Editor::new_loading(p.clone(), cx),
            None => Editor::new(cx),
        });
        let tab_editor = editor.clone();
        let mut last_status_version = tab_editor.read(cx).buffer_version();
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
                        last_status_version = tab_editor.read(cx).buffer_version();
                        this.status_bar.update(cx, |sb, cx| sb.set_meta(meta, cx));
                        this.sync_outline_panel(cx);
                        cx.emit(WorkspaceEvent::TitleChanged);
                    }
                }
                crate::editor::EditorEvent::CursorMoved => {
                    if this.active_editor() == Some(&tab_editor) {
                        let current_version = tab_editor.read(cx).buffer_version();
                        if current_version == last_status_version {
                            let pos = tab_editor.read(cx).cursor_pos();
                            this.status_bar
                                .update(cx, |sb, cx| sb.set_cursor_pos(pos, cx));
                            if this.outline_visible {
                                this.outline_panel.update(cx, |panel, cx| {
                                    panel.set_active_line(Some(pos.line), cx)
                                });
                            }
                        } else {
                            last_status_version = current_version;
                            let meta = tab_editor.read(cx).status_meta();
                            this.status_bar.update(cx, |sb, cx| sb.set_meta(meta, cx));
                            this.sync_outline_panel(cx);
                        }
                    }
                }
                crate::editor::EditorEvent::ScrollChanged => {
                    if this.active_editor() == Some(&tab_editor) {
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
        let reader_session = session.clone();
        let reader_path = path.clone();
        let read_work = cx
            .background_executor()
            .spawn(async move { reader_session.run_reader(reader_path) });
        let worker_session = session.clone();
        let prepare_work = cx
            .background_executor()
            .spawn(async move { worker_session.run_worker(path).await });
        let cancellation = crate::editor::loading::CancelSession(session.clone());
        cx.spawn(async move |this, cx| {
            use futures::StreamExt;
            let _work = (read_work, prepare_work);
            let _cancellation = cancellation;
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

    fn sync_active_editor_state(&mut self, cx: &mut Context<Self>) {
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
        let tabs_meta = self.get_tabs_meta(cx);
        let sidebar_offset = if self.sidebar_visible {
            self.sidebar_width
        } else {
            px(0.0)
        };
        let has_workspace_root = self.root_dir.is_some();
        self.tab_bar.update(cx, |tb, cx| {
            tb.sync(
                tabs_meta,
                self.active_tab_idx,
                has_workspace_root,
                sidebar_offset,
                cx,
            )
        });
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

    pub fn toggle_tab_read_only(&mut self, tab_idx: usize, cx: &mut Context<Self>) {
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
            let path = tab.editor.read(cx).file_path().map(|p| p.to_path_buf());
            if let Some(path) = path {
                crate::app::project_panel::fs_ops::copy_path_to_clipboard(&path, cx);
            }
        }
    }

    pub fn copy_tab_relative_path(&self, tab_idx: usize, cx: &mut App) {
        if let Some(tab) = self.tabs.get(tab_idx) {
            let path = tab.editor.read(cx).file_path().map(|p| p.to_path_buf());
            if let Some(path) = path {
                crate::app::project_panel::fs_ops::copy_relative_path_to_clipboard(
                    &path,
                    self.root_dir.as_deref(),
                    cx,
                );
            }
        }
    }

    pub fn reveal_tab_in_file_explorer(&self, tab_idx: usize, cx: &App) {
        if let Some(tab) = self.tabs.get(tab_idx)
            && let Some(path) = tab.editor.read(cx).file_path()
        {
            crate::platform::reveal_in_file_manager(path);
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
    pub fn handle_path_renamed(
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

    pub fn workspace_name(&self, cx: &App) -> String {
        if let Some(ref root) = self.root_dir
            && let Some(name) = root.file_name().and_then(|n| n.to_str())
            && !name.is_empty()
        {
            return name.to_string();
        }

        let active_path = self.active_editor().and_then(|ed| ed.read(cx).file_path());
        let other_paths = self.tabs.iter().map(|t| t.editor.read(cx).file_path());

        fs::extract_workspace_name_from_paths(std::iter::once(active_path).chain(other_paths))
    }

    pub fn has_workspace(&self, cx: &App) -> bool {
        self.workspace_name(cx) != "Open Workspace"
    }

    /// Locates an initial markdown entry file (preferring README.md, then index.md) in a directory
    #[inline]
    pub fn find_folder_target_file(folder: &std::path::Path) -> Option<PathBuf> {
        fs::find_folder_target_file(folder)
    }

    pub fn open_folder(&mut self, folder: PathBuf, cx: &mut Context<Self>) {
        self.root_dir = Some(folder.clone());
        let folder_clone = folder.clone();
        self.project_panel.update(cx, |panel, cx| {
            panel.set_root_dir(Some(folder_clone), cx);
        });
        self.search_panel.update(cx, |sp, cx| {
            sp.set_root_dir(Some(folder.clone()), cx);
        });

        if let Some(file) = Self::find_folder_target_file(&folder) {
            self.open_file(file, cx);
        }
        cx.emit(WorkspaceEvent::TitleChanged);
        cx.notify();
    }

    pub fn close_workspace(&mut self, cx: &mut Context<Self>) {
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

        config.update_last_workspace(WorkspaceState {
            root_dir: self.root_dir.clone(),
            active_file,
            open_files,
            sidebar_visible: self.sidebar_visible,
            sidebar_width: Some(f32::from(self.sidebar_width)),
            outline_width: Some(f32::from(self.outline_width)),
        });
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
        self.resize_target = None;
        self.panel_drag_start_x = None;
        self.panel_drag_start_width = None;
        cx.notify();
    }

    pub fn finish_sidebar_resize(&mut self, cx: &mut Context<Self>) {
        if self.is_resizing_sidebar() {
            self.finish_panel_resize(cx);
        }
    }

    pub fn reset_sidebar_width(&mut self, cx: &mut Context<Self>) {
        if self.is_resizing_sidebar() {
            self.finish_panel_resize(cx);
        }
        self.set_sidebar_width(px(DEFAULT_SIDEBAR_WIDTH), cx);
    }

    pub fn reset_outline_width(&mut self, cx: &mut Context<Self>) {
        if self.is_resizing_outline() {
            self.finish_panel_resize(cx);
        }
        self.set_outline_width(px(DEFAULT_SIDEBAR_WIDTH), cx);
    }

    pub fn set_sidebar_visible(&mut self, visible: bool, cx: &mut Context<Self>) {
        if self.sidebar_visible != visible {
            self.sidebar_visible = visible;
            self.sync_sidebar_state(cx);
            cx.notify();
        }
    }

    /// Shows the sidebar with `view` on screen.
    pub fn show_sidebar(&mut self, view: SidebarView, cx: &mut Context<Self>) {
        self.project_panel.update(cx, |panel, cx| {
            panel.set_filter_visible(view == SidebarView::Search, cx);
        });
        if !self.sidebar_visible || self.sidebar_view != view {
            self.sidebar_visible = true;
            self.sidebar_view = view;
            self.sync_sidebar_state(cx);
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

    pub fn get_tabs_meta(&self, cx: &App) -> Vec<tab_bar::TabMeta> {
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

    fn sync_outline_panel(&mut self, cx: &mut Context<Self>) {
        // Hidden outlines catch up when opened; input events should not parse
        // an invisible panel's document on the UI thread.
        if !self.outline_visible {
            self.outline_task = None;
            self.outline_source = None;
            return;
        }
        if let Some(entity) = self.active_editor() {
            let editor = entity.read(cx);
            use crate::buffer::LineSource;
            if editor.is_loading() {
                self.outline_task = None;
                if self.outline_source.take().is_some() {
                    self.outline_panel.update(cx, |panel, cx| {
                        panel.set_entries(Vec::new(), None, None, cx)
                    });
                }
                return;
            }
            let key = (entity.entity_id(), editor.buffer().revision());
            let active = Some(editor.cursor_pos().line);
            if self.outline_source == Some(key) {
                self.outline_panel
                    .update(cx, |panel, cx| panel.set_active_line(active, cx));
                return;
            }
            let source = editor.buffer().source_snapshot();
            let focus = Some(editor.focus_handle(cx));
            self.outline_source = Some(key);
            self.outline_panel.update(cx, |panel, cx| {
                panel.set_entries(Vec::new(), active, focus, cx)
            });
            let job = cx
                .background_executor()
                .spawn(async move { crate::markdown::collect_outline_from_source(&source) });
            self.outline_task = Some(cx.spawn(async move |workspace, cx| {
                let entries = job.await;
                let _ = workspace.update(cx, |workspace, cx| {
                    if workspace.outline_source != Some(key) {
                        return;
                    }
                    let Some(entity) = workspace
                        .active_editor()
                        .filter(|entity| entity.entity_id() == key.0)
                    else {
                        return;
                    };
                    let editor = entity.read(cx);
                    if editor.buffer().revision() != key.1 {
                        return;
                    }
                    let active = Some(editor.cursor_pos().line);
                    let focus = Some(editor.focus_handle(cx));
                    workspace.outline_panel.update(cx, |panel, cx| {
                        panel.set_entries(entries, active, focus, cx)
                    });
                    workspace.outline_task = None;
                });
            }));
            return;
        }
        self.outline_source = None;
        self.outline_task = None;
        self.outline_panel.update(cx, |panel, cx| {
            panel.set_entries(Vec::new(), None, None, cx)
        });
    }

    pub fn jump_to_outline(&mut self, line: SourceLine, cx: &mut Context<Self>) {
        if let Some(editor) = self.active_editor() {
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
        self.search_panel.update(cx, |sp, cx| {
            sp.refresh_files(cx);
            sp.focus_input(window, cx);
        });
    }
}

impl Render for Workspace {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let active_editor = self.active_editor().cloned();
        let active_is_search = self.active_tab_kind() == Some(TabKind::WorkspaceSearch);
        let sidebar_visible = self.sidebar_visible;
        let has_tabs = !self.tabs.is_empty();
        let has_workspace = self.root_dir.is_some();
        let theme = cx
            .try_global::<ThemeManager>()
            .map(|m| m.theme())
            .unwrap_or(&DEFAULT_THEME)
            .clone();

        div()
            .key_context(crate::ui::key_context::WORKSPACE)
            .track_focus(&self.focus_handle)
            .font_family(crate::platform::platform_ui_font())
            .flex_1()
            .min_h_0()
            .w_full()
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
                            .flex()
                            .flex_col()
                            .when(has_tabs, |this| this.child(self.tab_bar.clone()))
                            .child(div().flex_1().min_h_0().w_full().overflow_hidden().child(
                                if active_is_search {
                                    self.search_panel.clone().into_any_element()
                                } else if let Some(editor) = active_editor {
                                    editor.into_any_element()
                                } else if has_workspace {
                                    div()
                                        .flex_1()
                                        .w_full()
                                        .h_full()
                                        .bg(theme.bg_editor)
                                        .into_any_element()
                                } else {
                                    welcome::render_welcome_view(&self.focus_handle, &theme, cx)
                                        .into_any_element()
                                },
                            )),
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
    use crate::buffer::Position;
    use crate::config::AppConfig;
    use crate::theme::ThemeManager;
    use gpui::TestAppContext;

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
}
