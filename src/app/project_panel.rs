use gpui::prelude::*;
use gpui::*;
use std::collections::HashSet;
use std::path::{Path, PathBuf};

use crate::app::actions::{CopyEntry, CutEntry, DeleteEntry, PasteEntry, RenameEntry, TrashEntry};
use crate::theme::ThemeManager;

pub mod confirm_modal;
pub mod context_menu;
pub mod fs_ops;
pub mod inline_input;
pub mod tree_view;

pub use confirm_modal::{ConfirmModalKind, ConfirmModalState};
use context_menu::{ContextMenuAction, ContextMenuState, FileClipboard};
pub use inline_input::{InlineInput, InlineInputEvent, InlineInputKind};

/// The type and expansion state of an entry in the file tree.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FileTreeEntryKind {
    File,
    Directory { is_expanded: bool },
}

/// A single entry (file or folder) in the flattened visible file tree.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FileTreeEntry {
    pub path: PathBuf,
    pub name: String,
    pub depth: usize,
    pub kind: FileTreeEntryKind,
}

impl FileTreeEntry {
    /// Creates a file tree entry for a regular file.
    pub fn new_file(path: PathBuf, name: String, depth: usize) -> Self {
        Self {
            path,
            name,
            depth,
            kind: FileTreeEntryKind::File,
        }
    }

    /// Creates a file tree entry for a directory.
    pub fn new_dir(path: PathBuf, name: String, depth: usize, is_expanded: bool) -> Self {
        Self {
            path,
            name,
            depth,
            kind: FileTreeEntryKind::Directory { is_expanded },
        }
    }

    /// Returns `true` if this entry represents a directory.
    #[inline]
    pub fn is_dir(&self) -> bool {
        matches!(self.kind, FileTreeEntryKind::Directory { .. })
    }

    /// Returns `true` if this entry is an expanded directory. Files always return `false`.
    #[inline]
    pub fn is_expanded(&self) -> bool {
        matches!(
            self.kind,
            FileTreeEntryKind::Directory { is_expanded: true }
        )
    }
}

/// Smooth scroll physics state for the file tree (re-exports unified SmoothScrollPhysics)
pub type TreeScrollState = crate::ui::SmoothScrollPhysics;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ProjectPanelEvent {
    OpenFile(PathBuf),
    PathRenamed { old: PathBuf, new: PathBuf },
    PathDeleted(PathBuf),
}

impl EventEmitter<ProjectPanelEvent> for ProjectPanel {}

/// Project Panel managing worktree file hierarchy, context menu, and navigation.
pub struct ProjectPanel {
    root_dir: Option<PathBuf>,
    expanded_dirs: HashSet<PathBuf>,
    entries: Vec<FileTreeEntry>,
    focus_handle: FocusHandle,
    active_context_menu: Option<ContextMenuState>,
    active_confirm_modal: Option<ConfirmModalState>,
    clipboard: FileClipboard,
    inline_input: Option<Entity<InlineInput>>,
    selected_path: Option<PathBuf>,
    /// Keeps the focus-lost handler alive; it can only be built during render, which needs a window.
    blur_subscription: Option<Subscription>,
    scroll_handle: ScrollHandle,
    tree_scroll: TreeScrollState,
    scroll_task: Option<Task<()>>,
    panel_width: Pixels,
    active_file: Option<PathBuf>,
}

impl Focusable for ProjectPanel {
    fn focus_handle(&self, _cx: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl ProjectPanel {
    pub fn new(root_dir: Option<PathBuf>, focus_handle: FocusHandle) -> Self {
        let mut expanded_dirs = HashSet::new();
        if let Some(ref root) = root_dir {
            expanded_dirs.insert(root.clone());
        }

        let mut panel = Self {
            root_dir,
            expanded_dirs,
            entries: Vec::new(),
            focus_handle,
            active_context_menu: None,
            active_confirm_modal: None,
            clipboard: FileClipboard::None,
            inline_input: None,
            selected_path: None,
            blur_subscription: None,
            scroll_handle: ScrollHandle::new(),
            tree_scroll: TreeScrollState::new(),
            scroll_task: None,
            panel_width: px(crate::app::workspace::DEFAULT_SIDEBAR_WIDTH),
            active_file: None,
        };
        panel.rebuild_entries();
        panel
    }

    /// Accessor for the panel's scroll handle.
    #[inline]
    pub fn scroll_handle(&self) -> &ScrollHandle {
        &self.scroll_handle
    }

    /// Accessor for the tree scroll state.
    #[inline]
    pub fn tree_scroll(&self) -> &TreeScrollState {
        &self.tree_scroll
    }

    /// Mutable accessor for the tree scroll state.
    #[inline]
    pub fn tree_scroll_mut(&mut self) -> &mut TreeScrollState {
        &mut self.tree_scroll
    }

    /// Accessor for current root directory.
    #[inline]
    pub fn root_dir(&self) -> Option<&Path> {
        self.root_dir.as_deref()
    }

    /// Sets the panel width.
    pub fn set_panel_width(&mut self, width: Pixels, cx: &mut Context<Self>) {
        if self.panel_width != width {
            self.panel_width = width;
            cx.notify();
        }
    }

    /// Sets the currently active file path.
    pub fn set_active_file(&mut self, active_file: Option<PathBuf>, cx: &mut Context<Self>) {
        if self.active_file != active_file {
            self.active_file = active_file;
            cx.notify();
        }
    }

    /// Accessor for visible flattened tree entries.
    #[inline]
    pub fn entries(&self) -> &[FileTreeEntry] {
        &self.entries
    }

    /// Returns true if the directory at `path` is currently expanded.
    #[inline]
    pub fn is_expanded(&self, path: &Path) -> bool {
        self.expanded_dirs.contains(path)
    }

    /// Accessor for current active context menu state, if any.
    #[inline]
    pub fn active_context_menu(&self) -> Option<&ContextMenuState> {
        self.active_context_menu.as_ref()
    }

    /// Accessor for current active confirmation modal state, if any.
    #[inline]
    pub fn active_confirm_modal(&self) -> Option<&ConfirmModalState> {
        self.active_confirm_modal.as_ref()
    }

    /// Accessor for current clipboard state.
    #[inline]
    pub fn clipboard(&self) -> &FileClipboard {
        &self.clipboard
    }

    /// Accessor for current inline input entity, if any.
    #[inline]
    pub fn inline_input(&self) -> Option<&Entity<InlineInput>> {
        self.inline_input.as_ref()
    }

    /// Accessor for currently selected path.
    #[inline]
    pub fn selected_path(&self) -> Option<&Path> {
        self.selected_path.as_deref()
    }

    /// Sets the currently selected path.
    pub fn set_selected_path(&mut self, path: Option<PathBuf>, cx: &mut Context<Self>) {
        if self.selected_path != path {
            self.selected_path = path;
            cx.notify();
        }
    }

    /// Clears the currently selected path, if any.
    pub fn clear_selection(&mut self, cx: &mut Context<Self>) {
        if self.selected_path.is_some() {
            self.selected_path = None;
            cx.notify();
        }
    }

    /// Handles ProjectPanel blur (focus lost) event.
    pub fn handle_blur(&mut self, cx: &mut Context<Self>) {
        if self.inline_input.is_some() || self.active_confirm_modal.is_some() {
            return;
        }
        if self.active_context_menu.is_some() {
            self.active_context_menu = None;
        }
        self.clear_selection(cx);
    }

    /// Confirms the active delete/trash modal, performing the deletion.
    pub fn confirm_delete_modal(&mut self, cx: &mut Context<Self>) {
        let modal = match self.active_confirm_modal.take() {
            Some(m) => m,
            None => return,
        };
        let to_trash = modal.kind == ConfirmModalKind::Trash;
        let target = modal.target_path;
        match fs_ops::delete_entry(&target, to_trash) {
            Ok(()) => {
                cx.emit(ProjectPanelEvent::PathDeleted(target.clone()));
                if self.selected_path.as_ref() == Some(&target) {
                    self.selected_path = None;
                }
                self.rebuild_entries();
                cx.notify();
            }
            Err(e) => {
                eprintln!("Failed to delete {}: {}", target.display(), e);
            }
        }
    }

    /// Cancels and dismisses the active delete/trash modal.
    pub fn cancel_delete_modal(&mut self, cx: &mut Context<Self>) {
        if self.active_confirm_modal.is_some() {
            self.active_confirm_modal = None;
            cx.notify();
        }
    }

    /// Sets a new workspace root directory and refreshes the tree.
    pub fn set_root_dir(&mut self, root: Option<PathBuf>, cx: &mut Context<Self>) {
        self.root_dir = root;
        self.expanded_dirs.clear();
        self.active_context_menu = None;
        self.inline_input = None;
        self.selected_path = None;
        if let Some(ref r) = self.root_dir {
            self.expanded_dirs.insert(r.clone());
        }
        self.rebuild_entries();
        cx.notify();
    }

    /// Toggles directory expanded/collapsed state and updates the visible list.
    pub fn toggle_expand(&mut self, dir_path: &Path, cx: &mut Context<Self>) {
        if self.expanded_dirs.contains(dir_path) {
            self.expanded_dirs.remove(dir_path);
        } else {
            self.expanded_dirs.insert(dir_path.to_path_buf());
        }
        self.rebuild_entries();
        cx.notify();
    }

    /// Expands all ancestor directories leading to `path` so the file is visible in the tree.
    pub fn reveal_path(&mut self, path: &Path, cx: &mut Context<Self>) {
        let mut changed = false;
        if let Some(parent) = path.parent() {
            for ancestor in parent.ancestors() {
                if let Some(root) = &self.root_dir
                    && !ancestor.starts_with(root)
                {
                    break;
                }
                if self.expanded_dirs.insert(ancestor.to_path_buf()) {
                    changed = true;
                }
            }
        }
        if changed {
            self.rebuild_entries();
            cx.notify();
        }
    }

    /// Collapses all open subdirectories in the tree.
    pub fn collapse_all(&mut self, cx: &mut Context<Self>) {
        self.expanded_dirs.clear();
        if let Some(ref root) = self.root_dir {
            self.expanded_dirs.insert(root.clone());
        }
        self.rebuild_entries();
        cx.notify();
    }

    /// Closes the active context menu.
    pub fn close_context_menu(&mut self, cx: &mut Context<Self>) {
        if self.active_context_menu.is_some() {
            self.active_context_menu = None;
            cx.notify();
        }
    }

    pub(crate) fn start_scroll_animation(&mut self, cx: &mut Context<Self>) {
        crate::ui::start_scroll_animation(
            self,
            cx,
            |panel| &mut panel.scroll_task,
            |panel| {
                let still_animating = panel.tree_scroll.step_interpolation();
                panel
                    .scroll_handle
                    .set_offset(Point::new(px(0.0), -panel.tree_scroll.current_scroll_top));
                still_animating
            },
        );
    }

    /// Handles mouse wheel and precision touchpad events with smooth scroll physics
    pub(crate) fn handle_scroll_wheel(&mut self, event: &ScrollWheelEvent, cx: &mut Context<Self>) {
        let max_scroll = self.scroll_handle.max_offset().height;
        if max_scroll <= px(0.0) {
            return;
        }

        // Sync with actual offset if not animating (handles window resize / collapse)
        let actual_top = (-self.scroll_handle.offset().y).clamp(px(0.0), max_scroll);
        if !self.tree_scroll.is_animating()
            && (self.tree_scroll.current_scroll_top - actual_top).abs() > px(1.0)
        {
            self.tree_scroll.current_scroll_top = actual_top;
            self.tree_scroll.target_scroll_top = actual_top;
            self.tree_scroll.anim_start_scroll_top = actual_top;
        }

        match event.delta {
            ScrollDelta::Pixels(p) => {
                if p.y.abs() > px(0.01) {
                    self.scroll_task = None;
                    self.tree_scroll.scroll_direct(p.y, max_scroll);
                    self.scroll_handle
                        .set_offset(Point::new(px(0.0), -self.tree_scroll.current_scroll_top));
                    cx.notify();
                }
            }
            ScrollDelta::Lines(l) => {
                let delta_y = px(l.y * crate::ui::WHEEL_LINE_STEP_PX);
                if delta_y.abs() > px(0.01) {
                    self.tree_scroll.scroll_by_wheel(delta_y, max_scroll);
                    self.start_scroll_animation(cx);
                    cx.notify();
                }
            }
        }
    }

    /// Opens context menu manually (useful for testing or programmatic triggering).
    pub fn open_context_menu(
        &mut self,
        position: Point<Pixels>,
        target_path: PathBuf,
        is_dir: bool,
        is_root: bool,
        cx: &mut Context<Self>,
    ) {
        self.selected_path = Some(target_path.clone());
        self.active_context_menu = Some(ContextMenuState {
            position,
            target_path,
            is_dir,
            is_root,
        });
        cx.notify();
    }

    /// Starts an inline rename for the specified target.
    pub fn start_rename(&mut self, target: &Path, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(ref root) = self.root_dir
            && target == root
        {
            return;
        }
        let is_dir = target.is_dir();
        let name = target
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("")
            .to_string();
        let depth = self.entry_depth(target);
        let kind = InlineInputKind::Rename {
            target: target.to_path_buf(),
            is_dir,
        };
        let input = cx.new(|cx| InlineInput::new(kind, &name, depth, cx));
        self.attach_inline_input(input, window, cx);
    }

    fn attach_inline_input(
        &mut self,
        input: Entity<InlineInput>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let focus_handle = input.read(cx).focus_handle(cx);
        cx.subscribe(
            &input,
            move |this, _emitter, event: &InlineInputEvent, cx| match event {
                InlineInputEvent::Commit(text) => this.handle_inline_commit(text.clone(), cx),
                InlineInputEvent::Cancel => this.handle_inline_cancel(cx),
            },
        )
        .detach();

        cx.on_blur(&focus_handle, window, move |this, _window, cx| {
            this.commit_or_cancel_inline_input(cx);
        })
        .detach();

        window.focus(&focus_handle);
        self.inline_input = Some(input);
        cx.notify();
    }

    /// Commits active inline input if non-empty, otherwise cancels it.
    pub fn commit_or_cancel_inline_input(&mut self, cx: &mut Context<Self>) {
        let text = match &self.inline_input {
            Some(input) => input.read(cx).text(),
            None => return,
        };
        let trimmed = text.trim();
        if trimmed.is_empty() {
            self.handle_inline_cancel(cx);
        } else {
            self.handle_inline_commit(text, cx);
        }
    }

    pub fn handle_inline_commit(&mut self, text: String, cx: &mut Context<Self>) {
        let input = match self.inline_input.take() {
            Some(i) => i,
            None => return,
        };
        let kind = input.read(cx).kind().clone();
        let trimmed = text.trim();
        if trimmed.is_empty() {
            cx.notify();
            return;
        }

        match kind {
            InlineInputKind::NewFile { parent } => match fs_ops::create_file(&parent, trimmed) {
                Ok(new_path) => {
                    self.expanded_dirs.insert(parent);
                    self.rebuild_entries();
                    self.selected_path = Some(new_path.clone());
                    cx.emit(ProjectPanelEvent::OpenFile(new_path));
                }
                Err(e) => {
                    eprintln!(
                        "Failed to create file \"{}\" in {}: {}",
                        trimmed,
                        parent.display(),
                        e
                    );
                }
            },
            InlineInputKind::NewFolder { parent } => {
                match fs_ops::create_folder(&parent, trimmed) {
                    Ok(new_path) => {
                        self.expanded_dirs.insert(parent);
                        self.expanded_dirs.insert(new_path.clone());
                        self.rebuild_entries();
                        self.selected_path = Some(new_path);
                    }
                    Err(e) => {
                        eprintln!(
                            "Failed to create folder \"{}\" in {}: {}",
                            trimmed,
                            parent.display(),
                            e
                        );
                    }
                }
            }
            InlineInputKind::Rename { target, is_dir } => {
                match fs_ops::rename_entry(&target, trimmed) {
                    Ok(new_path) => {
                        if is_dir && self.expanded_dirs.remove(&target) {
                            self.expanded_dirs.insert(new_path.clone());
                        }
                        cx.emit(ProjectPanelEvent::PathRenamed {
                            old: target,
                            new: new_path.clone(),
                        });
                        self.rebuild_entries();
                        self.selected_path = Some(new_path);
                    }
                    Err(e) => {
                        eprintln!(
                            "Failed to rename {} to \"{}\": {}",
                            target.display(),
                            trimmed,
                            e
                        );
                    }
                }
            }
        }
        cx.notify();
    }

    pub fn handle_inline_cancel(&mut self, cx: &mut Context<Self>) {
        if self.inline_input.is_some() {
            self.inline_input = None;
            cx.notify();
        }
    }

    /// Handles all actions selected from the context menu.
    pub fn execute_context_menu_action(
        &mut self,
        action: ContextMenuAction,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let state = match self.active_context_menu.take() {
            Some(s) => s,
            None => return,
        };

        let target = state.target_path.clone();
        let is_dir = state.is_dir;

        match action {
            ContextMenuAction::NewFile | ContextMenuAction::NewFolder => {
                let (parent, depth) = self.new_entry_slot(&state);
                self.expanded_dirs.insert(parent.clone());
                self.rebuild_entries();
                let kind = if action == ContextMenuAction::NewFolder {
                    InlineInputKind::NewFolder { parent }
                } else {
                    InlineInputKind::NewFile { parent }
                };
                let input = cx.new(|cx| InlineInput::new(kind, "", depth, cx));
                self.attach_inline_input(input, window, cx);
            }
            ContextMenuAction::RevealInFileManager => {
                crate::platform::reveal_in_file_manager(&target);
            }
            ContextMenuAction::Cut => self.cut_entry(target, cx),
            ContextMenuAction::Copy => self.copy_entry(target, cx),
            ContextMenuAction::Duplicate => match fs_ops::duplicate_entry(&target) {
                Ok(new_path) => {
                    if let Some(parent) = new_path.parent() {
                        self.expanded_dirs.insert(parent.to_path_buf());
                    }
                    self.rebuild_entries();
                    cx.notify();
                }
                Err(e) => fs_ops::report_error("duplicate", &target, &e),
            },
            ContextMenuAction::Paste => self.paste_into(directory_of(&target, is_dir), cx),
            ContextMenuAction::CopyPath => {
                fs_ops::copy_path_to_clipboard(&target, cx);
            }
            ContextMenuAction::CopyRelativePath => {
                fs_ops::copy_relative_path_to_clipboard(&target, self.root_dir.as_deref(), cx);
            }
            ContextMenuAction::Rename => self.start_rename(&target, window, cx),
            ContextMenuAction::Trash => {
                self.request_delete(ConfirmModalKind::Trash, target, is_dir, window, cx);
            }
            ContextMenuAction::Delete => {
                self.request_delete(ConfirmModalKind::Delete, target, is_dir, window, cx);
            }
        }
    }

    /// Runs a file-tree command triggered from the keyboard.
    ///
    /// Keyboard and context menu differ only in how the target is resolved: the menu carries the
    /// clicked entry in its state, the keyboard uses the selection. Everything past that point is
    /// shared, which is why these commands live in the keybinding table instead of in a
    /// hand-rolled `key == "f2"` chain.
    pub fn handle_entry_action(
        &mut self,
        action: ContextMenuAction,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        // No guard for an open rename box is needed here: the keymap scopes all six commands to
        // `PROJECT_PANEL_TREE`, which excludes `InlineInput`, so they cannot fire while the box
        // owns the keyboard.
        let Some(target) = self.keyboard_target() else {
            return;
        };
        let is_dir = target.is_dir();
        // The root itself can be pasted into but not renamed, trashed or deleted.
        let is_root = self.root_dir.as_deref() == Some(target.as_path());

        match action {
            ContextMenuAction::Copy => self.copy_entry(target, cx),
            ContextMenuAction::Cut => self.cut_entry(target, cx),
            ContextMenuAction::Paste => self.paste_into(directory_of(&target, is_dir), cx),
            ContextMenuAction::Rename if !is_root => self.start_rename(&target, window, cx),
            ContextMenuAction::Trash if !is_root => {
                self.request_delete(ConfirmModalKind::Trash, target, is_dir, window, cx);
            }
            ContextMenuAction::Delete if !is_root => {
                self.request_delete(ConfirmModalKind::Delete, target, is_dir, window, cx);
            }
            _ => {}
        }
    }

    /// Target for a keyboard-triggered command: the selection, else the workspace root so that
    /// paste still has somewhere to go when nothing is selected.
    fn keyboard_target(&self) -> Option<PathBuf> {
        self.selected_path.clone().or_else(|| self.root_dir.clone())
    }

    /// Where a new file or folder goes, and the indent its inline row renders at.
    fn new_entry_slot(&self, state: &ContextMenuState) -> (PathBuf, usize) {
        let on_root = state.is_root || self.root_dir.as_deref() == Some(&state.target_path);
        if on_root {
            (state.target_path.clone(), 0)
        } else if state.is_dir {
            (
                state.target_path.clone(),
                self.entry_depth(&state.target_path) + 1,
            )
        } else {
            (
                state
                    .target_path
                    .parent()
                    .unwrap_or(&state.target_path)
                    .to_path_buf(),
                self.entry_depth(&state.target_path),
            )
        }
    }

    fn cut_entry(&mut self, path: PathBuf, cx: &mut Context<Self>) {
        self.clipboard = FileClipboard::Cut(path);
        cx.notify();
    }

    fn copy_entry(&mut self, path: PathBuf, cx: &mut Context<Self>) {
        self.clipboard = FileClipboard::Copy(path);
        cx.notify();
    }

    fn paste_into(&mut self, dest_dir: PathBuf, cx: &mut Context<Self>) {
        let (src, is_cut) = match &self.clipboard {
            FileClipboard::Cut(p) => (p.clone(), true),
            FileClipboard::Copy(p) => (p.clone(), false),
            FileClipboard::None => return,
        };
        match fs_ops::perform_paste(&src, is_cut, &dest_dir) {
            Ok(target) => {
                if is_cut {
                    self.clipboard = FileClipboard::None;
                    cx.emit(ProjectPanelEvent::PathRenamed {
                        old: src,
                        new: target,
                    });
                }
                self.expanded_dirs.insert(dest_dir);
                self.rebuild_entries();
                cx.notify();
            }
            Err(e) => fs_ops::report_error("paste", &src, &e),
        }
    }

    fn request_delete(
        &mut self,
        kind: ConfirmModalKind,
        target: PathBuf,
        is_dir: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.active_confirm_modal = Some(ConfirmModalState::new(kind, target, is_dir));
        self.focus_handle.focus(window);
        cx.notify();
    }

    /// Handles the keys the panel owns itself, as opposed to the file-tree commands that come in
    /// through the keybinding table.
    ///
    /// These two stay as `on_key_down` handling rather than bindings on purpose: `escape` has to
    /// mean different things depending on what is open here, and a binding is claimed globally
    /// (see `key_context::MENU_OPEN`).
    ///
    /// The rename box is not handled here: it is deeper on the dispatch path and stops
    /// propagation, so `escape`/`enter` reach it first and never bubble up to this listener.
    pub fn handle_key_down(
        &mut self,
        event: &KeyDownEvent,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let key = event.keystroke.key.to_lowercase();

        if self.active_confirm_modal.is_some() {
            if key == "escape" {
                self.cancel_delete_modal(cx);
            } else if key == "enter" {
                self.confirm_delete_modal(cx);
            }
            return;
        }

        if key == "escape" {
            if self.active_context_menu.is_some() {
                self.close_context_menu(cx);
            } else {
                self.clear_selection(cx);
            }
        }
    }

    fn rebuild_entries(&mut self) {
        self.entries.clear();
        if let Some(ref root) = self.root_dir
            && root.is_dir()
        {
            scan_dir_recursive(root, 0, &self.expanded_dirs, &mut self.entries);
        }
    }

    /// Indent depth of `path` in the flattened tree. The root and anything currently hidden by a
    /// collapsed ancestor sit at 0.
    ///
    /// No fallback for a path that is absent from `entries`: every call site passes something the
    /// user picked out of the tree (a context-menu target or the rename target), so it is visible
    /// by construction. The pre-merge version estimated a depth from `strip_prefix` components for
    /// that unreachable case.
    fn entry_depth(&self, path: &Path) -> usize {
        if self.root_dir.as_deref() == Some(path) {
            return 0;
        }
        self.entries
            .iter()
            .find(|e| e.path == path)
            .map(|e| e.depth)
            .unwrap_or(0)
    }

    pub(crate) fn select_and_activate_entry(
        &mut self,
        path: PathBuf,
        is_dir: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.commit_or_cancel_inline_input(cx);
        self.active_context_menu = None;
        self.selected_path = Some(path.clone());
        if is_dir {
            self.toggle_expand(&path, cx);
            window.focus(&self.focus_handle);
        } else {
            cx.emit(ProjectPanelEvent::OpenFile(path));
        }
    }

    pub(crate) fn open_entry_context_menu(
        &mut self,
        path: PathBuf,
        is_dir: bool,
        position: Point<Pixels>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.commit_or_cancel_inline_input(cx);
        self.selected_path = Some(path.clone());
        self.active_context_menu = Some(ContextMenuState {
            position,
            target_path: path,
            is_dir,
            is_root: false,
        });
        window.focus(&self.focus_handle);
        cx.notify();
    }

    /// Computes the safe bounds for positioning context menus within the window workspace.
    pub fn safe_menu_bounds(&self, window_bounds: Bounds<Pixels>) -> Bounds<Pixels> {
        crate::app::workspace::workspace_safe_bounds(window_bounds)
    }
}

/// The directory a paste lands in: `path` itself when it is one, otherwise its parent.
fn directory_of(path: &Path, is_dir: bool) -> PathBuf {
    if is_dir {
        path.to_path_buf()
    } else {
        path.parent().unwrap_or(path).to_path_buf()
    }
}

/// Recursively scans directory contents, ignoring hidden and transient files.
fn scan_dir_recursive(
    dir: &Path,
    depth: usize,
    expanded: &HashSet<PathBuf>,
    out: &mut Vec<FileTreeEntry>,
) {
    let mut items = Vec::new();
    if let Ok(read_dir) = std::fs::read_dir(dir) {
        for entry in read_dir.flatten() {
            let path = entry.path();
            let file_name = entry.file_name().to_string_lossy().to_string();

            // Hidden entries (.git, .vscode, .DS_Store) plus the build directories that would
            // otherwise dominate the tree.
            if file_name.starts_with('.') || file_name == "target" || file_name == "node_modules" {
                continue;
            }

            let is_dir = path.is_dir();
            items.push((path, file_name, is_dir));
        }
    }

    // Sort: directories first, then alphabetically case-insensitive
    items.sort_by(|a, b| match (a.2, b.2) {
        (true, false) => std::cmp::Ordering::Less,
        (false, true) => std::cmp::Ordering::Greater,
        _ => a.1.to_lowercase().cmp(&b.1.to_lowercase()),
    });

    for (path, name, is_dir) in items {
        if is_dir {
            let is_expanded = expanded.contains(&path);
            out.push(FileTreeEntry::new_dir(
                path.clone(),
                name,
                depth,
                is_expanded,
            ));
            if is_expanded {
                scan_dir_recursive(&path, depth + 1, expanded, out);
            }
        } else {
            out.push(FileTreeEntry::new_file(path, name, depth));
        }
    }
}

impl Render for ProjectPanel {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if self.blur_subscription.is_none() {
            let sub = cx.on_blur(&self.focus_handle, window, |this, _window, cx| {
                this.handle_blur(cx);
            });
            self.blur_subscription = Some(sub);
        }

        let theme = cx.global::<ThemeManager>().theme().clone();

        let panel_width = self.panel_width;
        let active_file = self.active_file.as_deref();

        let entries_count = self.entries.len();
        let has_root = self.root_dir.is_some();

        let panel_weak = cx.entity().downgrade();

        let on_action = {
            let panel_weak = panel_weak.clone();
            move |action: ContextMenuAction, window: &mut Window, cx: &mut App| {
                if let Some(panel) = panel_weak.upgrade() {
                    panel.update(cx, |this, cx| {
                        this.execute_context_menu_action(action, window, cx);
                    });
                }
            }
        };

        let on_close = {
            let panel_weak = panel_weak.clone();
            move |_window: &mut Window, cx: &mut App| {
                if let Some(panel) = panel_weak.upgrade() {
                    panel.update(cx, |this, cx| {
                        this.close_context_menu(cx);
                    });
                }
            }
        };

        let on_confirm_modal = {
            let panel_weak = panel_weak.clone();
            move |_window: &mut Window, cx: &mut App| {
                if let Some(panel) = panel_weak.upgrade() {
                    panel.update(cx, |this, cx| {
                        this.confirm_delete_modal(cx);
                    });
                }
            }
        };

        let on_cancel_modal = {
            let panel_weak = panel_weak.clone();
            move |_window: &mut Window, cx: &mut App| {
                if let Some(panel) = panel_weak.upgrade() {
                    panel.update(cx, |this, cx| {
                        this.cancel_delete_modal(cx);
                    });
                }
            }
        };

        let item_elements = tree_view::build_tree_items(
            tree_view::TreeItemList {
                entries: &self.entries,
                inline_input: self.inline_input.as_ref(),
                selected_path: self.selected_path.as_deref(),
                active_file,
                root_dir: self.root_dir.as_deref(),
                panel: &panel_weak,
            },
            &theme,
            cx,
        );

        // Right-click on the empty area below the tree targets the workspace root.
        let bg_right_click = cx.listener(|this, event: &MouseDownEvent, window, cx| {
            let Some(root) = this.root_dir.clone() else {
                return;
            };
            this.commit_or_cancel_inline_input(cx);
            this.selected_path = None;
            this.active_context_menu = Some(ContextMenuState {
                position: event.position,
                target_path: root,
                is_dir: true,
                is_root: true,
            });
            window.focus(&this.focus_handle);
            cx.notify();
        });

        let bg_left_click = cx.listener(|this, _event: &MouseDownEvent, window, cx| {
            this.commit_or_cancel_inline_input(cx);
            this.clear_selection(cx);
            this.close_context_menu(cx);
            window.focus(&this.focus_handle);
        });

        // File-tree commands arrive as actions, so the keyboard path and the context-menu path
        // reach the same code. The handlers sit on this element because that is where the
        // `ProjectPanel` key context lives; dispatch starts at the focused node and bubbles, so
        // this node sees them whenever the panel or anything inside it has focus.
        div()
            .id("project_panel")
            .key_context(crate::ui::key_context::PROJECT_PANEL)
            .track_focus(&self.focus_handle)
            .on_key_down(cx.listener(|this, event: &KeyDownEvent, window, cx| {
                this.handle_key_down(event, window, cx);
            }))
            .on_action(cx.listener(|this, _: &CopyEntry, window, cx| {
                this.handle_entry_action(ContextMenuAction::Copy, window, cx);
            }))
            .on_action(cx.listener(|this, _: &CutEntry, window, cx| {
                this.handle_entry_action(ContextMenuAction::Cut, window, cx);
            }))
            .on_action(cx.listener(|this, _: &PasteEntry, window, cx| {
                this.handle_entry_action(ContextMenuAction::Paste, window, cx);
            }))
            .on_action(cx.listener(|this, _: &RenameEntry, window, cx| {
                this.handle_entry_action(ContextMenuAction::Rename, window, cx);
            }))
            .on_action(cx.listener(|this, _: &TrashEntry, window, cx| {
                this.handle_entry_action(ContextMenuAction::Trash, window, cx);
            }))
            .on_action(cx.listener(|this, _: &DeleteEntry, window, cx| {
                this.handle_entry_action(ContextMenuAction::Delete, window, cx);
            }))
            .w(panel_width)
            .h_full()
            .bg(theme.bg_toolbar)
            .border_r_1()
            .border_color(theme.border_subtle)
            .flex()
            .flex_col()
            .overflow_hidden()
            .on_mouse_down(MouseButton::Right, bg_right_click)
            .on_mouse_down(MouseButton::Left, bg_left_click)
            .when(!has_root || entries_count == 0, |this| {
                this.child(tree_view::render_empty_state(
                    has_root,
                    self.inline_input.as_ref(),
                    &theme,
                    cx,
                ))
            })
            .when(has_root && entries_count > 0, |this| {
                this.child(
                    div()
                        .id("project_panel_tree_scroll")
                        .track_scroll(&self.scroll_handle)
                        .overflow_y_scroll()
                        .on_scroll_wheel(cx.listener(
                            |this, event: &ScrollWheelEvent, _window, cx| {
                                cx.stop_propagation();
                                this.handle_scroll_wheel(event, cx);
                            },
                        ))
                        .flex_1()
                        .w_full()
                        .py(px(4.0))
                        .children(item_elements),
                )
            })
            .when_some(self.active_context_menu.clone(), |this, menu_state| {
                let safe_bounds = self.safe_menu_bounds(window.bounds());
                this.child(deferred(context_menu::render_context_menu(
                    &menu_state,
                    &self.clipboard,
                    safe_bounds,
                    on_action,
                    on_close,
                    cx,
                )))
            })
            .when_some(self.active_confirm_modal.clone(), |this, modal_state| {
                this.child(deferred(confirm_modal::render_confirm_modal(
                    &modal_state,
                    window.bounds(),
                    on_confirm_modal,
                    on_cancel_modal,
                    &theme,
                )))
            })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    // Load-bearing: `use super::*` also brings in `gpui::test`, so this explicit import
    // shadows it and keeps bare `#[test]` resolving to the built-in test attribute.
    use crate::config::AppConfig;
    use core::prelude::v1::test;
    use gpui::{AppContext, ScrollDelta, ScrollWheelEvent, TestAppContext, px};

    /// Hosts the panel inside a deliberately short viewport so the file tree overflows.
    struct PanelHost {
        panel: Entity<ProjectPanel>,
    }

    impl Render for PanelHost {
        fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
            div().w(px(240.0)).h(px(120.0)).child(self.panel.clone())
        }
    }

    fn init_test_globals(cx: &mut TestAppContext) {
        let config = AppConfig::default();
        let theme_manager = ThemeManager::from_config(&config);
        cx.update(|cx| {
            cx.set_global(theme_manager);
            cx.set_global(config);
        });
    }

    #[gpui::test]
    fn test_handle_scroll_wheel_moves_tree_scroll(cx: &mut TestAppContext) {
        init_test_globals(cx);

        let temp_dir =
            std::env::temp_dir().join(format!("inviscid_panel_wheel_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&temp_dir);
        std::fs::create_dir_all(&temp_dir).unwrap();
        for i in 0..40 {
            std::fs::write(temp_dir.join(format!("file_{:03}.md", i)), "x").unwrap();
        }

        let (host, cx) = cx.add_window_view(|_window, cx| {
            let panel = cx.new(|cx| ProjectPanel::new(Some(temp_dir.clone()), cx.focus_handle()));
            PanelHost { panel }
        });
        cx.run_until_parked();

        // Probe: the host caps the panel at 120px, so 40 entries must overflow the scroll area.
        // Without a real non-zero max_offset `handle_scroll_wheel` would bail out early and
        // this test would be vacuous.
        let max_scroll = cx.update(|_window, cx| {
            host.read(cx)
                .panel
                .read(cx)
                .scroll_handle()
                .max_offset()
                .height
        });
        assert!(
            max_scroll > px(0.0),
            "file tree must overflow the 120px host viewport, got max_offset = {max_scroll:?}"
        );

        // A mouse wheel notch becomes an animated impulse on the shared physics state.
        cx.update(|_window, cx| {
            let event = ScrollWheelEvent {
                delta: ScrollDelta::Lines(gpui::Point::new(0.0, -3.0)),
                ..Default::default()
            };
            host.update(cx, |host, cx| {
                host.panel
                    .update(cx, |panel, cx| panel.handle_scroll_wheel(&event, cx));
            });
        });

        cx.update(|_window, cx| {
            let panel = host.read(cx).panel.read(cx);
            assert_eq!(
                panel.tree_scroll().target_scroll_top,
                px(100.0),
                "one wheel notch must advance the target by 100px"
            );
            assert!(panel.tree_scroll().is_animating());
        });

        let _ = std::fs::remove_dir_all(&temp_dir);
    }

    #[test]
    fn test_file_tree_entry_invariants() {
        let file = FileTreeEntry::new_file(PathBuf::from("/ws/main.rs"), "main.rs".to_string(), 1);
        assert_eq!(file.kind, FileTreeEntryKind::File);
        assert!(!file.is_dir());
        assert!(!file.is_expanded());

        let collapsed_dir =
            FileTreeEntry::new_dir(PathBuf::from("/ws/src"), "src".to_string(), 0, false);
        assert_eq!(
            collapsed_dir.kind,
            FileTreeEntryKind::Directory { is_expanded: false }
        );
        assert!(collapsed_dir.is_dir());
        assert!(!collapsed_dir.is_expanded());

        let expanded_dir =
            FileTreeEntry::new_dir(PathBuf::from("/ws/src"), "src".to_string(), 0, true);
        assert_eq!(
            expanded_dir.kind,
            FileTreeEntryKind::Directory { is_expanded: true }
        );
        assert!(expanded_dir.is_dir());
        assert!(expanded_dir.is_expanded());
    }
}
