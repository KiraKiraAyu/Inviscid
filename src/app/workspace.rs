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

use crate::app::{status_bar, tab_bar};
use crate::config::{AppConfig, WorkspaceState};
use crate::editor::Editor;
use crate::theme::{DEFAULT_THEME, ThemeManager};

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
    root_dir: Option<PathBuf>,
    sidebar_visible: bool,
    sidebar_width: Pixels,
    is_resizing_sidebar: bool,
    sidebar_drag_start_x: Option<Pixels>,
    sidebar_drag_start_width: Option<Pixels>,
}

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
            }
            crate::app::project_panel::ProjectPanelEvent::PathDeleted(path) => {
                this.handle_path_deleted(path, cx);
            }
        })
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
            let (editor, subscription) = Self::init_editor(Some(path), cx);
            let meta = editor.read(cx).status_meta();
            let tab_meta = tab_bar::TabMeta {
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
                    editor,
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

        let status_bar = cx.new(|_cx| status_bar::StatusBar::new(status_meta, true));
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
        project_panel.update(cx, |p, cx| p.set_active_file(initial_active_file, cx));
        let project_panel_sub = Self::subscribe_project_panel(&project_panel, cx);
        Self {
            focus_handle,
            tabs,
            active_tab_idx,
            tab_bar,
            _tab_bar_sub: tab_bar_sub,
            status_bar,
            project_panel,
            _project_panel_sub: project_panel_sub,
            root_dir,
            sidebar_visible: true,
            sidebar_width: px(DEFAULT_SIDEBAR_WIDTH),
            is_resizing_sidebar: false,
            sidebar_drag_start_x: None,
            sidebar_drag_start_width: None,
        }
    }

    fn load_tabs_from_state(ws: &WorkspaceState, cx: &mut Context<Self>) -> (Vec<TabItem>, usize) {
        let mut tabs = Vec::new();
        let mut active_idx = 0;

        for file_path in &ws.open_files {
            if file_path.exists() {
                let (editor, subscription) = Self::init_editor(Some(file_path.clone()), cx);
                tabs.push(TabItem {
                    editor,
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
        let status_bar = cx.new(|_cx| status_bar::StatusBar::new(status_meta, ws.sidebar_visible));
        let tabs_meta = tabs::collect_tabs_meta(&tabs, cx);
        let sidebar_width = px(ws
            .sidebar_width
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
            p.set_panel_width(sidebar_width, cx);
            p.set_active_file(ws.active_file.clone(), cx);
        });
        let project_panel_sub = Self::subscribe_project_panel(&project_panel, cx);
        Self {
            focus_handle,
            tabs,
            active_tab_idx,
            tab_bar,
            _tab_bar_sub: tab_bar_sub,
            status_bar,
            project_panel,
            _project_panel_sub: project_panel_sub,
            root_dir: ws.root_dir.clone(),
            sidebar_visible: ws.sidebar_visible,
            sidebar_width,
            is_resizing_sidebar: false,
            sidebar_drag_start_x: None,
            sidebar_drag_start_width: None,
        }
    }

    pub fn restore_from_state(&mut self, ws: &WorkspaceState, cx: &mut Context<Self>) {
        self.root_dir = ws.root_dir.clone();
        self.sidebar_visible = ws.sidebar_visible;
        let sidebar_width = px(ws
            .sidebar_width
            .unwrap_or(DEFAULT_SIDEBAR_WIDTH)
            .clamp(MIN_SIDEBAR_WIDTH, MAX_SIDEBAR_WIDTH));
        self.sidebar_width = sidebar_width;
        self.is_resizing_sidebar = false;
        self.sidebar_drag_start_x = None;
        self.sidebar_drag_start_width = None;

        let (tabs, active_tab_idx) = Self::load_tabs_from_state(ws, cx);
        self.tabs = tabs;
        self.active_tab_idx = active_tab_idx;

        self.project_panel.update(cx, |p, cx| {
            p.set_root_dir(ws.root_dir.clone(), cx);
            p.set_panel_width(sidebar_width, cx);
            p.set_active_file(ws.active_file.clone(), cx);
        });

        self.status_bar.update(cx, |sb, cx| {
            sb.set_sidebar_visible(ws.sidebar_visible, cx);
        });

        self.sync_active_editor_state(cx);
    }

    pub fn init_editor(
        path: Option<PathBuf>,
        cx: &mut Context<Self>,
    ) -> (Entity<Editor>, Subscription) {
        let editor = cx.new(|cx| match path {
            Some(p) => Editor::new_with_path(p, cx),
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
                        } else {
                            last_status_version = current_version;
                            let meta = tab_editor.read(cx).status_meta();
                            this.status_bar.update(cx, |sb, cx| sb.set_meta(meta, cx));
                        }
                    }
                }
            },
        );
        (editor, subscription)
    }

    pub fn active_editor(&self) -> Option<&Entity<Editor>> {
        if self.tabs.is_empty() {
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
        cx.emit(WorkspaceEvent::TitleChanged);
        cx.notify();
    }

    pub fn new_tab(&mut self, cx: &mut Context<Self>) {
        let (editor, subscription) = Self::init_editor(None, cx);
        self.tabs.push(TabItem {
            editor,
            is_pinned: false,
            _subscription: subscription,
        });
        self.active_tab_idx = self.tabs.len() - 1;
        self.sync_active_editor_state(cx);
    }

    pub fn close_tab(&mut self, index: usize, cx: &mut Context<Self>) {
        if index < self.tabs.len() {
            self.tabs.remove(index);
            if self.tabs.is_empty() {
                self.active_tab_idx = 0;
            } else if self.active_tab_idx >= self.tabs.len() {
                self.active_tab_idx = self.tabs.len() - 1;
            } else if self.active_tab_idx > index {
                self.active_tab_idx -= 1;
            }
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
        tabs::filter_close_other_tabs(&mut self.tabs, &target_editor);
        self.active_tab_idx = self
            .tabs
            .iter()
            .position(|t| t.editor == target_editor)
            .unwrap_or(0);
        self.sync_active_editor_state(cx);
    }

    pub fn close_tabs_to_left(&mut self, target_idx: usize, cx: &mut Context<Self>) {
        if target_idx >= self.tabs.len() {
            return;
        }
        let target_editor = self.tabs[target_idx].editor.clone();
        tabs::filter_close_tabs_to_left(&mut self.tabs, target_idx);
        self.active_tab_idx = self
            .tabs
            .iter()
            .position(|t| t.editor == target_editor)
            .unwrap_or(0);
        self.sync_active_editor_state(cx);
    }

    pub fn close_tabs_to_right(&mut self, target_idx: usize, cx: &mut Context<Self>) {
        if target_idx >= self.tabs.len() {
            return;
        }
        let target_editor = self.tabs[target_idx].editor.clone();
        tabs::filter_close_tabs_to_right(&mut self.tabs, target_idx);
        self.active_tab_idx = self
            .tabs
            .iter()
            .position(|t| t.editor == target_editor)
            .unwrap_or(0);
        self.sync_active_editor_state(cx);
    }

    pub fn close_clean_tabs(&mut self, cx: &mut Context<Self>) {
        let active_editor = self.active_editor().cloned();
        tabs::filter_close_clean_tabs(&mut self.tabs, cx);
        if let Some(ed) = active_editor {
            self.active_tab_idx = self.tabs.iter().position(|t| t.editor == ed).unwrap_or(0);
        } else {
            self.active_tab_idx = 0;
        }
        if !self.tabs.is_empty() && self.active_tab_idx >= self.tabs.len() {
            self.active_tab_idx = self.tabs.len() - 1;
        }
        self.sync_active_editor_state(cx);
    }

    pub fn close_all_tabs(&mut self, cx: &mut Context<Self>) {
        self.tabs.retain(|t| t.is_pinned);
        if self.tabs.is_empty() {
            self.active_tab_idx = 0;
        } else if self.active_tab_idx >= self.tabs.len() {
            self.active_tab_idx = self.tabs.len() - 1;
        }
        self.sync_active_editor_state(cx);
    }

    pub fn toggle_tab_read_only(&mut self, tab_idx: usize, cx: &mut Context<Self>) {
        if let Some(tab) = self.tabs.get(tab_idx) {
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
            self.set_sidebar_visible(true, cx);
            self.defer_reveal_path(path, cx);
        }
    }

    pub fn switch_tab(&mut self, index: usize, cx: &mut Context<Self>) {
        if index < self.tabs.len() && self.active_tab_idx != index {
            self.active_tab_idx = index;
            self.sync_active_editor_state(cx);
        }
    }

    pub fn next_tab(&mut self, cx: &mut Context<Self>) {
        if !self.tabs.is_empty() {
            self.active_tab_idx = (self.active_tab_idx + 1) % self.tabs.len();
            self.sync_active_editor_state(cx);
        }
    }

    pub fn prev_tab(&mut self, cx: &mut Context<Self>) {
        if !self.tabs.is_empty() {
            self.active_tab_idx = (self.active_tab_idx + self.tabs.len() - 1) % self.tabs.len();
            self.sync_active_editor_state(cx);
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
            if let Some(active_editor) = self.active_editor() {
                active_editor.update(cx, |editor, cx| {
                    let _ = editor.load_file(path.clone(), cx);
                });
                self.sync_active_editor_state(cx);
            }
        } else {
            let (editor, subscription) = Self::init_editor(Some(path.clone()), cx);
            self.tabs.push(TabItem {
                editor,
                is_pinned: false,
                _subscription: subscription,
            });
            self.active_tab_idx = self.tabs.len() - 1;
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
            if let Some(path) = tab.editor.read(cx).file_path_buf() {
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

    #[inline]
    pub fn is_resizing_sidebar(&self) -> bool {
        self.is_resizing_sidebar
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
            self.project_panel.update(cx, |panel, cx| {
                panel.set_panel_width(clamped, cx);
            });
            cx.notify();
        }
    }

    pub fn start_sidebar_resize(&mut self, mouse_x: Pixels, cx: &mut Context<Self>) {
        self.is_resizing_sidebar = true;
        self.sidebar_drag_start_x = Some(mouse_x);
        self.sidebar_drag_start_width = Some(self.sidebar_width);
        cx.notify();
    }

    pub fn handle_sidebar_resize_drag(&mut self, current_x: Pixels, cx: &mut Context<Self>) {
        if let (Some(start_x), Some(start_w)) =
            (self.sidebar_drag_start_x, self.sidebar_drag_start_width)
        {
            let delta = current_x - start_x;
            let new_width = start_w + delta;
            self.set_sidebar_width(new_width, cx);
        }
    }

    pub fn finish_sidebar_resize(&mut self, cx: &mut Context<Self>) {
        self.is_resizing_sidebar = false;
        self.sidebar_drag_start_x = None;
        self.sidebar_drag_start_width = None;
        cx.notify();
    }

    pub fn reset_sidebar_width(&mut self, cx: &mut Context<Self>) {
        self.is_resizing_sidebar = false;
        self.sidebar_drag_start_x = None;
        self.sidebar_drag_start_width = None;
        self.set_sidebar_width(px(DEFAULT_SIDEBAR_WIDTH), cx);
    }

    pub fn toggle_sidebar(&mut self, cx: &mut Context<Self>) {
        self.set_sidebar_visible(!self.sidebar_visible, cx);
    }

    pub fn set_sidebar_visible(&mut self, visible: bool, cx: &mut Context<Self>) {
        if self.sidebar_visible != visible {
            self.sidebar_visible = visible;
            let offset = if visible { self.sidebar_width } else { px(0.0) };
            self.tab_bar
                .update(cx, |tb, cx| tb.set_sidebar_offset(offset, cx));
            self.status_bar.update(cx, |sb, cx| {
                sb.set_sidebar_visible(visible, cx);
            });
            cx.notify();
        }
    }

    pub fn active_file_path<'a>(&'a self, cx: &'a App) -> Option<&'a std::path::Path> {
        self.active_editor().and_then(|ed| ed.read(cx).file_path())
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
}

impl Render for Workspace {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let active_editor = self.active_editor().cloned();
        let project_panel = self.project_panel.clone();
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
            .when(self.is_resizing_sidebar, |this| {
                this.child(sidebar::render_sidebar_resize_canvas(
                    cx.entity().downgrade(),
                ))
            })
            .child(
                div()
                    .flex_1()
                    .min_h_0()
                    .w_full()
                    .flex()
                    .flex_row()
                    .when(sidebar_visible, |this| this.child(project_panel))
                    .when(sidebar_visible, |this| {
                        this.child(sidebar::render_sidebar_resize_handle(
                            self.is_resizing_sidebar,
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
                                if let Some(editor) = active_editor {
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
                    ),
            )
            .child(self.status_bar.clone())
    }
}
