use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::ops::{Deref, DerefMut};
use std::path::PathBuf;
use std::sync::{
    Arc, OnceLock,
    atomic::{AtomicU64, Ordering},
};

struct PathPersistSlot {
    latest_seq: AtomicU64,
    write_lock: Mutex<()>,
}

fn persist_slot_for(path: &PathBuf) -> Arc<PathPersistSlot> {
    static SLOTS: OnceLock<Mutex<HashMap<PathBuf, Arc<PathPersistSlot>>>> = OnceLock::new();
    let mut slots = SLOTS.get_or_init(Default::default).lock();
    slots
        .entry(path.clone())
        .or_insert_with(|| {
            Arc::new(PathPersistSlot {
                latest_seq: AtomicU64::new(0),
                write_lock: Mutex::new(()),
            })
        })
        .clone()
}

fn write_atomic_toml_sync<T: Serialize>(path: Option<&PathBuf>, value: &T) {
    let Some(path) = path else {
        return;
    };
    let slot = persist_slot_for(path);
    let _seq = slot.latest_seq.fetch_add(1, Ordering::AcqRel) + 1;
    let _guard = slot.write_lock.lock();
    if let Ok(content) = toml::to_string_pretty(value) {
        let _ = crate::fs::atomic_write(path, &content);
    }
}

fn spawn_atomic_toml_write<T: Serialize + Send + 'static>(
    path: Option<PathBuf>,
    value: T,
    executor: &gpui::BackgroundExecutor,
) {
    let Some(path) = path else {
        return;
    };
    let slot = persist_slot_for(&path);
    let seq = slot.latest_seq.fetch_add(1, Ordering::AcqRel) + 1;
    executor
        .spawn(async move {
            let _guard = slot.write_lock.lock();
            if slot.latest_seq.load(Ordering::Acquire) != seq {
                return;
            }
            if let Ok(content) = toml::to_string_pretty(&value) {
                let _ = crate::fs::atomic_write(&path, &content);
            }
        })
        .detach();
}

use crate::app::SidebarView;
use crate::editor::RenderMode;

pub const DEFAULT_WINDOW_WIDTH: f32 = 1080.0;
pub const DEFAULT_WINDOW_HEIGHT: f32 = 780.0;
pub const MIN_WINDOW_WIDTH: f32 = 360.0;
pub const MIN_WINDOW_HEIGHT: f32 = 240.0;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum WindowMode {
    #[default]
    Windowed,
    Maximized,
    Fullscreen,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
pub struct SerializedBounds {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
}

impl Default for SerializedBounds {
    fn default() -> Self {
        Self {
            x: 0.0,
            y: 0.0,
            width: DEFAULT_WINDOW_WIDTH,
            height: DEFAULT_WINDOW_HEIGHT,
        }
    }
}

impl SerializedBounds {
    fn from_gpui_bounds(bounds: gpui::Bounds<gpui::Pixels>) -> Self {
        Self {
            x: f32::from(bounds.origin.x),
            y: f32::from(bounds.origin.y),
            width: f32::from(bounds.size.width),
            height: f32::from(bounds.size.height),
        }
    }

    fn to_gpui_bounds(self) -> gpui::Bounds<gpui::Pixels> {
        gpui::Bounds::new(
            gpui::point(gpui::px(self.x), gpui::px(self.y)),
            gpui::size(gpui::px(self.width), gpui::px(self.height)),
        )
    }

    fn is_valid(self) -> bool {
        self.x.is_finite()
            && self.y.is_finite()
            && self.width.is_finite()
            && self.height.is_finite()
            && self.width >= MIN_WINDOW_WIDTH
            && self.height >= MIN_WINDOW_HEIGHT
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
#[serde(default)]
pub struct WindowLayoutState {
    pub mode: WindowMode,
    pub bounds: SerializedBounds,
    pub display_uuid: Option<String>,
}

impl WindowLayoutState {
    /// Captures the current window bounds, mode, and display UUID if the window has valid non-minimized geometry.
    pub fn from_window(window: &gpui::Window, cx: &gpui::App) -> Option<Self> {
        let live_bounds = window.bounds();
        // On Windows, minimizing moves the live window rect to (-32000, -32000) and GetWindowPlacement
        // clears SW_SHOWMAXIMIZED even when WPF_RESTORETOMAXIMIZED is set; avoid clobbering state.
        if f32::from(live_bounds.origin.x) <= -16000.0
            || f32::from(live_bounds.origin.y) <= -16000.0
            || f32::from(live_bounds.size.width) < MIN_WINDOW_WIDTH
            || f32::from(live_bounds.size.height) < MIN_WINDOW_HEIGHT
        {
            return None;
        }

        let (mode, gpui_bounds) = match window.window_bounds() {
            gpui::WindowBounds::Windowed(bounds) => (WindowMode::Windowed, bounds),
            gpui::WindowBounds::Maximized(bounds) => (WindowMode::Maximized, bounds),
            gpui::WindowBounds::Fullscreen(bounds) => (WindowMode::Fullscreen, bounds),
        };
        let bounds = SerializedBounds::from_gpui_bounds(gpui_bounds);
        if !bounds.is_valid() {
            return None;
        }
        let display_uuid = window
            .display(cx)
            .and_then(|display| display.uuid().ok())
            .map(|uuid| uuid.to_string());

        Some(Self {
            mode,
            bounds,
            display_uuid,
        })
    }

    pub fn default_window_bounds(cx: &gpui::App) -> (Option<gpui::DisplayId>, gpui::WindowBounds) {
        let size = gpui::size(
            gpui::px(DEFAULT_WINDOW_WIDTH),
            gpui::px(DEFAULT_WINDOW_HEIGHT),
        );
        (
            None,
            gpui::WindowBounds::Windowed(gpui::Bounds::centered(None, size, cx)),
        )
    }

    /// Resolves persisted window geometry onto currently connected displays, falling back safely if
    /// the original monitor was disconnected or the saved bounds are invalid/off-screen.
    pub fn resolve(&self, cx: &gpui::App) -> (Option<gpui::DisplayId>, gpui::WindowBounds) {
        if !self.bounds.is_valid() {
            return Self::default_window_bounds(cx);
        }

        let saved = self.bounds.to_gpui_bounds();
        let displays = cx.displays();
        let matched_display = self
            .display_uuid
            .as_deref()
            .and_then(|uuid| {
                displays.iter().find(|display| {
                    display
                        .uuid()
                        .ok()
                        .is_some_and(|candidate| candidate.to_string() == uuid)
                })
            })
            .or_else(|| {
                if self.display_uuid.is_none() {
                    let center = saved.center();
                    displays.iter().find(|d| d.bounds().contains(&center))
                } else {
                    None
                }
            });

        let target_display = matched_display.cloned().or_else(|| cx.primary_display());
        let display_id = target_display.as_ref().map(|d| d.id());

        let clamped_size = if let Some(ref display) = target_display {
            let max_size = display.bounds().size;
            gpui::size(
                saved
                    .size
                    .width
                    .min(max_size.width)
                    .max(gpui::px(MIN_WINDOW_WIDTH).min(max_size.width)),
                saved
                    .size
                    .height
                    .min(max_size.height)
                    .max(gpui::px(MIN_WINDOW_HEIGHT).min(max_size.height)),
            )
        } else {
            saved.size
        };

        let resolved_bounds = if let Some(display) = matched_display
            && display.bounds().contains(&saved.center())
        {
            gpui::Bounds::new(saved.origin, clamped_size)
        } else {
            gpui::Bounds::centered(display_id, clamped_size, cx)
        };

        let window_bounds = match self.mode {
            WindowMode::Windowed => gpui::WindowBounds::Windowed(resolved_bounds),
            WindowMode::Maximized => gpui::WindowBounds::Maximized(resolved_bounds),
            WindowMode::Fullscreen => gpui::WindowBounds::Fullscreen(resolved_bounds),
        };

        (display_id, window_bounds)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct UiLayoutState {
    pub window_bounds: Option<WindowLayoutState>,
    pub sidebar_visible: bool,
    pub sidebar_width: Option<f32>,
    pub sidebar_view: SidebarView,
    pub outline_visible: bool,
    pub outline_width: Option<f32>,
}

impl Default for UiLayoutState {
    fn default() -> Self {
        Self {
            window_bounds: None,
            sidebar_visible: true,
            sidebar_width: None,
            sidebar_view: SidebarView::default(),
            outline_visible: false,
            outline_width: None,
        }
    }
}

impl UiLayoutState {
    fn from_workspace_and_fallback(ws: &WorkspaceState, fallback: Option<&UiLayoutState>) -> Self {
        Self {
            window_bounds: ws
                .window_bounds
                .clone()
                .or_else(|| fallback.and_then(|f| f.window_bounds.clone())),
            sidebar_visible: ws.sidebar_visible,
            sidebar_width: ws.sidebar_width,
            sidebar_view: ws.sidebar_view,
            outline_visible: ws.outline_visible,
            outline_width: ws.outline_width,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct WorkspaceState {
    pub root_dir: Option<PathBuf>,
    pub active_file: Option<PathBuf>,
    pub open_files: Vec<PathBuf>,
    pub sidebar_visible: bool,
    pub sidebar_width: Option<f32>,
    pub sidebar_view: SidebarView,
    pub outline_visible: bool,
    pub outline_width: Option<f32>,
    pub window_bounds: Option<WindowLayoutState>,
}

impl Default for WorkspaceState {
    fn default() -> Self {
        Self {
            root_dir: None,
            active_file: None,
            open_files: Vec::new(),
            sidebar_visible: true,
            sidebar_width: None,
            sidebar_view: SidebarView::default(),
            outline_visible: false,
            outline_width: None,
            window_bounds: None,
        }
    }
}

impl WorkspaceState {
    pub fn is_empty(&self) -> bool {
        self.root_dir.is_none() && self.active_file.is_none() && self.open_files.is_empty()
    }

    pub fn has_workspace_root(&self) -> bool {
        self.root_dir.is_some()
    }

    pub fn has_existing_workspace_root(&self) -> bool {
        self.root_dir.as_ref().is_some_and(|root| root.exists())
    }

    pub fn has_existing_path(&self) -> bool {
        if let Some(ref root) = self.root_dir {
            root.exists()
        } else if let Some(ref file) = self.active_file {
            file.exists()
        } else if let Some(first) = self.open_files.first() {
            first.exists()
        } else {
            false
        }
    }
}

/// User-configurable preferences persisted in `config.toml`.
///
/// # Note
///
/// The file is serialized in full on save; manual comments or custom formatting are not preserved.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct UserPreferences {
    pub theme: String,
    pub ui_font_size: f32,
    pub restore_workspace: bool,
    pub collocated_mode: bool,
    pub auto_save: bool,
    pub editor_font_size: f32,
    pub line_height: f32,
    pub soft_wrap: bool,
    pub cursor_blink: bool,
    pub cursor_breathing: bool,
    pub default_render_mode: RenderMode,
    pub show_fps: bool,
    pub grammar_base_url: String,
    pub custom_keybindings: std::collections::HashMap<String, String>,
}

impl UserPreferences {
    /// Must match the `name` inside `assets/themes/catppuccin-mocha.toml`.
    pub const DEFAULT_THEME: &'static str = "Catppuccin Mocha";
    pub const DEFAULT_UI_FONT_SIZE: f32 = 13.0;
    pub const DEFAULT_EDITOR_FONT_SIZE: f32 = 15.0;
    pub const DEFAULT_LINE_HEIGHT: f32 = 1.6;
}

impl Default for UserPreferences {
    fn default() -> Self {
        Self {
            theme: Self::DEFAULT_THEME.to_string(),
            ui_font_size: Self::DEFAULT_UI_FONT_SIZE,
            restore_workspace: true,
            collocated_mode: false,
            auto_save: false,
            editor_font_size: Self::DEFAULT_EDITOR_FONT_SIZE,
            line_height: Self::DEFAULT_LINE_HEIGHT,
            soft_wrap: true,
            cursor_blink: true,
            cursor_breathing: true,
            default_render_mode: RenderMode::default(),
            show_fps: false,
            grammar_base_url: crate::syntax::GrammarCdnPreset::DEFAULT_URL.to_string(),
            custom_keybindings: std::collections::HashMap::new(),
        }
    }
}

/// Runtime session state persisted in `state.toml`.
#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
#[serde(default)]
pub struct SessionState {
    pub last_workspace: Option<WorkspaceState>,
    pub recent_workspaces: Vec<WorkspaceState>,
    pub last_layout: Option<UiLayoutState>,
}

/// Application configuration and session state stored as a GPUI global.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct AppConfig {
    pub preferences: UserPreferences,
    pub session: SessionState,
    pub config_file_path: Option<PathBuf>,
    pub state_file_path: Option<PathBuf>,
}

impl gpui::Global for AppConfig {}

impl Deref for AppConfig {
    type Target = UserPreferences;

    #[inline]
    fn deref(&self) -> &Self::Target {
        &self.preferences
    }
}

impl DerefMut for AppConfig {
    #[inline]
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.preferences
    }
}

impl AppConfig {
    pub fn with_theme(mut self, theme: impl Into<String>) -> Self {
        self.preferences.theme = theme.into();
        self
    }

    #[inline]
    pub fn last_workspace(&self) -> Option<&WorkspaceState> {
        self.session.last_workspace.as_ref()
    }

    #[inline]
    pub fn recent_workspaces(&self) -> &[WorkspaceState] {
        &self.session.recent_workspaces
    }

    #[inline]
    pub fn last_layout(&self) -> Option<&UiLayoutState> {
        self.session.last_layout.as_ref()
    }

    /// Resolves initial window geometry for app launch, preferring the restored workspace's
    /// saved window bounds (when `restore_workspace` is active and no CLI file was passed)
    /// and falling back to the global `last_layout` window bounds or default centered size.
    pub fn resolve_initial_window_bounds(
        &self,
        has_initial_path: bool,
        cx: &gpui::App,
    ) -> (Option<gpui::DisplayId>, gpui::WindowBounds) {
        let workspace_bounds = if !has_initial_path && self.restore_workspace {
            self.session
                .last_workspace
                .as_ref()
                .and_then(|ws| ws.window_bounds.as_ref())
        } else {
            None
        };
        let fallback_bounds = self
            .session
            .last_layout
            .as_ref()
            .and_then(|layout| layout.window_bounds.as_ref());

        workspace_bounds
            .or(fallback_bounds)
            .map(|wb| wb.resolve(cx))
            .unwrap_or_else(|| WindowLayoutState::default_window_bounds(cx))
    }

    /// Resolves window geometry for opening a new window: offsets from the active window when one
    /// exists, or falls back to the persisted layout window bounds.
    pub fn resolve_new_window_bounds(
        &self,
        cx: &mut gpui::App,
    ) -> (Option<gpui::DisplayId>, gpui::WindowBounds) {
        if let Some(active_window) = cx.active_window()
            && let Ok((active_bounds, display)) = active_window.update(cx, |_, window, cx| {
                (window.window_bounds(), window.display(cx))
            })
        {
            let display_id = display.as_ref().map(|d| d.id());
            let resolved = match active_bounds {
                gpui::WindowBounds::Windowed(mut b) => {
                    b.origin.x += gpui::px(24.0);
                    b.origin.y += gpui::px(24.0);
                    if let Some(d) = display
                        && !d.bounds().contains(&b.center())
                    {
                        b = gpui::Bounds::centered(Some(d.id()), b.size, cx);
                    }
                    gpui::WindowBounds::Windowed(b)
                }
                gpui::WindowBounds::Maximized(b) => gpui::WindowBounds::Maximized(b),
                gpui::WindowBounds::Fullscreen(b) => gpui::WindowBounds::Fullscreen(b),
            };
            return (display_id, resolved);
        }

        self.resolve_initial_window_bounds(true, cx)
    }

    /// Returns previously saved `window_bounds` for a workspace matching `state`, or from `last_layout`.
    pub fn existing_window_bounds_for(&self, state: &WorkspaceState) -> Option<WindowLayoutState> {
        self.session
            .recent_workspaces
            .iter()
            .find(|w| Self::same_workspace_identity(w, state))
            .and_then(|w| w.window_bounds.clone())
            .or_else(|| {
                self.session
                    .last_layout
                    .as_ref()
                    .and_then(|l| l.window_bounds.clone())
            })
    }

    fn same_workspace_identity(a: &WorkspaceState, b: &WorkspaceState) -> bool {
        match (&a.root_dir, &b.root_dir) {
            (Some(dir_a), Some(dir_b)) => dir_a == dir_b,
            _ => false,
        }
    }
}

impl AppConfig {
    /// Standard system config path (`%APPDATA%/inviscid/config.toml` or `~/.config/inviscid/config.toml`).
    fn config_path() -> Option<PathBuf> {
        dirs::config_dir().map(|p| p.join("inviscid").join("config.toml"))
    }

    /// Standard system state path (`%LOCALAPPDATA%/inviscid/state.toml` or `~/.local/share/inviscid/state.toml`).
    fn state_path() -> Option<PathBuf> {
        dirs::data_local_dir()
            .or_else(dirs::data_dir)
            .or_else(dirs::config_dir)
            .map(|p| p.join("inviscid").join("state.toml"))
    }

    /// Local cache directory for downloaded `.wasm` grammars and `highlights.scm` queries.
    pub fn grammars_cache_dir() -> Option<PathBuf> {
        dirs::cache_dir().map(|p| p.join("inviscid").join("grammars"))
    }

    /// Loads application configuration from `config.toml` and session state from `state.toml`.
    pub fn load() -> Self {
        let config_path = Self::config_path().unwrap_or_else(|| PathBuf::from("config.toml"));
        let state_path = Self::state_path().unwrap_or_else(|| PathBuf::from("state.toml"));
        Self::load_from_paths(config_path, state_path)
    }

    pub fn load_from_paths(config_path: PathBuf, state_path: PathBuf) -> Self {
        let mut config = AppConfig {
            config_file_path: Some(config_path.clone()),
            state_file_path: Some(state_path.clone()),
            ..AppConfig::default()
        };

        if config_path.exists() {
            match fs::read_to_string(&config_path) {
                Ok(content) => match toml::from_str::<UserPreferences>(&content) {
                    Ok(mut parsed) => {
                        let normalized_url =
                            crate::syntax::normalize_grammar_base_url(&parsed.grammar_base_url);
                        let changed = normalized_url != parsed.grammar_base_url;
                        parsed.grammar_base_url = normalized_url;
                        config.preferences = parsed;
                        if changed {
                            config.save_preferences();
                        }
                    }
                    Err(e) => {
                        eprintln!(
                            "Warning: Failed to parse config file {:?}: {}. Using default preferences.",
                            config_path, e
                        );
                    }
                },
                Err(e) => {
                    eprintln!(
                        "Warning: Failed to read config file {:?}: {}",
                        config_path, e
                    );
                }
            }
        }

        if state_path.exists() {
            match fs::read_to_string(&state_path) {
                Ok(content) => match toml::from_str::<SessionState>(&content) {
                    Ok(parsed) => {
                        config.session = parsed;
                    }
                    Err(e) => {
                        eprintln!(
                            "Warning: Failed to parse state file {:?}: {}. Using empty session state.",
                            state_path, e
                        );
                    }
                },
                Err(e) => {
                    eprintln!("Warning: Failed to read state file {:?}: {}", state_path, e);
                }
            }
        }

        config
            .session
            .recent_workspaces
            .retain(|ws| ws.has_existing_workspace_root());

        if let Some(ref ws) = config.session.last_workspace {
            if !ws.has_existing_path() {
                config.session.last_workspace = None;
            }
        }

        config
    }

    /// Persists user preferences to `config.toml` atomically when `config_file_path` is set.
    pub fn save_preferences(&self) {
        write_atomic_toml_sync(self.config_file_path.as_ref(), &self.preferences);
    }

    /// Serializes and persists user preferences on a background thread in latest-snapshot order.
    pub fn persist_preferences_async(&self, executor: &gpui::BackgroundExecutor) {
        spawn_atomic_toml_write(
            self.config_file_path.clone(),
            self.preferences.clone(),
            executor,
        );
    }

    pub fn get_keybinding(&self, action: &str, default_key: &str) -> String {
        self.custom_keybindings
            .get(action)
            .cloned()
            .unwrap_or_else(|| default_key.to_string())
    }

    pub fn set_keybinding(&mut self, action: impl Into<String>, keystroke: impl Into<String>) {
        self.custom_keybindings
            .insert(action.into(), keystroke.into());
    }

    pub fn reset_keybinding(&mut self, action: &str) {
        self.custom_keybindings.remove(action);
    }

    pub fn reset_all_keybindings(&mut self) {
        self.custom_keybindings.clear();
    }

    pub fn clear_recent_workspaces(&mut self) {
        self.session.recent_workspaces.clear();
    }

    /// Persists session state to `state.toml` atomically when `state_file_path` is set.
    pub fn save_session(&self) {
        write_atomic_toml_sync(self.state_file_path.as_ref(), &self.session);
    }

    /// Serializes and persists session state on a background thread in latest-snapshot order.
    pub fn persist_session_async(&self, executor: &gpui::BackgroundExecutor) {
        spawn_atomic_toml_write(self.state_file_path.clone(), self.session.clone(), executor);
    }

    /// Records the current workspace state and updates both recent workspaces and global UI layout.
    pub fn update_last_workspace(&mut self, state: WorkspaceState) {
        let layout =
            UiLayoutState::from_workspace_and_fallback(&state, self.session.last_layout.as_ref());
        self.session.last_layout = Some(layout);

        if state.has_workspace_root() {
            self.session
                .recent_workspaces
                .retain(|w| !Self::same_workspace_identity(w, &state));
            self.session.recent_workspaces.insert(0, state.clone());
            if self.session.recent_workspaces.len() > 10 {
                self.session.recent_workspaces.truncate(10);
            }
        }

        if !state.is_empty() {
            self.session.last_workspace = Some(state);
        } else {
            self.session.last_workspace = None;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_workspace_state_serialization() {
        let state = WorkspaceState {
            root_dir: Some(PathBuf::from("/project")),
            active_file: Some(PathBuf::from("/project/doc.md")),
            open_files: vec![
                PathBuf::from("/project/doc.md"),
                PathBuf::from("/project/notes.md"),
            ],
            sidebar_visible: true,
            sidebar_width: Some(260.0),
            sidebar_view: SidebarView::Search,
            outline_visible: true,
            outline_width: Some(300.0),
            window_bounds: Some(WindowLayoutState {
                mode: WindowMode::Maximized,
                bounds: SerializedBounds {
                    x: 120.0,
                    y: 80.0,
                    width: 1280.0,
                    height: 820.0,
                },
                display_uuid: Some("00000000-0000-0000-0000-000000000001".to_string()),
            }),
        };

        let mut config = AppConfig::default();
        config.update_last_workspace(state.clone());

        assert_eq!(config.last_workspace(), Some(&state));
        assert_eq!(config.recent_workspaces().len(), 1);
        assert_eq!(config.recent_workspaces()[0], state);

        // Test TOML roundtrip
        let serialized = toml::to_string_pretty(&config.session).unwrap();
        let deserialized: SessionState = toml::from_str(&serialized).unwrap();
        assert_eq!(deserialized.last_workspace, Some(state));
        assert!(deserialized.last_layout.is_some());
    }

    #[test]
    fn test_workspace_state_serde_defaults() {
        let minimal_toml = r#"
            [last_workspace]
            active_file = "/project/doc.md"
            open_files = ["/project/doc.md"]
        "#;

        let session: SessionState = toml::from_str(minimal_toml).unwrap();
        let ws = session.last_workspace.unwrap();
        assert_eq!(ws.active_file, Some(PathBuf::from("/project/doc.md")));
        assert_eq!(ws.open_files, vec![PathBuf::from("/project/doc.md")]);
        assert_eq!(ws.root_dir, None);
        assert!(ws.sidebar_visible);
        assert_eq!(ws.sidebar_width, None);
        assert_eq!(ws.sidebar_view, SidebarView::Files);
        assert!(!ws.outline_visible);
        assert_eq!(ws.outline_width, None);
        assert_eq!(ws.window_bounds, None);
    }

    #[test]
    fn test_workspace_deduplication_and_limit() {
        let mut config = AppConfig::default();
        for i in 0..15 {
            let state = WorkspaceState {
                root_dir: Some(PathBuf::from(format!("/project_{}", i))),
                active_file: Some(PathBuf::from(format!("/doc_{}.md", i))),
                open_files: vec![PathBuf::from(format!("/doc_{}.md", i))],
                sidebar_visible: true,
                sidebar_width: None,
                outline_width: None,
                ..WorkspaceState::default()
            };
            config.update_last_workspace(state);
        }

        assert_eq!(config.recent_workspaces().len(), 10);
        assert_eq!(
            config.recent_workspaces()[0].active_file,
            Some(PathBuf::from("/doc_14.md"))
        );

        let state_10 = WorkspaceState {
            root_dir: Some(PathBuf::from("/project_10")),
            active_file: Some(PathBuf::from("/doc_10.md")),
            open_files: vec![PathBuf::from("/doc_10.md")],
            sidebar_visible: true,
            sidebar_width: None,
            outline_width: None,
            ..WorkspaceState::default()
        };
        config.update_last_workspace(state_10);
        assert_eq!(config.recent_workspaces().len(), 10);
        assert_eq!(
            config.recent_workspaces()[0].active_file,
            Some(PathBuf::from("/doc_10.md"))
        );
    }

    #[test]
    fn test_single_file_without_workspace_does_not_record_recent_workspace() {
        let mut config = AppConfig::default();

        let loose_file_state = WorkspaceState {
            root_dir: None,
            active_file: Some(PathBuf::from("/notes/today.md")),
            open_files: vec![PathBuf::from("/notes/today.md")],
            sidebar_visible: false,
            sidebar_width: Some(250.0),
            outline_width: None,
            ..WorkspaceState::default()
        };
        config.update_last_workspace(loose_file_state.clone());

        assert!(config.recent_workspaces().is_empty());
        assert_eq!(config.last_workspace(), Some(&loose_file_state));
        assert!(config.last_layout().is_some());

        let workspace_state = WorkspaceState {
            root_dir: Some(PathBuf::from("/project")),
            active_file: Some(PathBuf::from("/project/README.md")),
            open_files: vec![PathBuf::from("/project/README.md")],
            ..WorkspaceState::default()
        };
        config.update_last_workspace(workspace_state.clone());
        assert_eq!(config.recent_workspaces(), std::slice::from_ref(&workspace_state));

        let another_loose_file_state = WorkspaceState {
            root_dir: None,
            active_file: Some(PathBuf::from("/scratch/other.md")),
            open_files: vec![PathBuf::from("/scratch/other.md")],
            ..WorkspaceState::default()
        };
        config.update_last_workspace(another_loose_file_state.clone());

        assert_eq!(config.recent_workspaces(), std::slice::from_ref(&workspace_state));
        assert_eq!(config.last_workspace(), Some(&another_loose_file_state));
    }

    #[test]
    fn test_atomic_write_safety() {
        let temp_dir = tempfile::tempdir().expect("create temp dir");
        let target_file = temp_dir.path().join("sub").join("test_config.toml");

        let content = "theme = \"One Dark Pro\"\n";
        crate::fs::atomic_write(&target_file, content).expect("atomic_write should succeed");

        let read_back = fs::read_to_string(&target_file).expect("file must exist and be readable");
        assert_eq!(read_back, content);

        let updated_content = "theme = \"Nord\"\n";
        crate::fs::atomic_write(&target_file, updated_content)
            .expect("atomic overwrite should succeed");
        let read_back_updated = fs::read_to_string(&target_file).expect("file must be readable");
        assert_eq!(read_back_updated, updated_content);
    }

    #[test]
    fn test_preferences_and_session_separation() {
        let temp_dir = tempfile::tempdir().expect("create temp dir");
        let cfg_path = temp_dir.path().join("config.toml");
        let state_path = temp_dir.path().join("state.toml");

        let mut config = AppConfig {
            config_file_path: Some(cfg_path.clone()),
            state_file_path: Some(state_path.clone()),
            ..AppConfig::default().with_theme("Solarized Dark")
        };

        config.save_preferences();
        assert!(cfg_path.exists());
        assert!(!state_path.exists());

        let cfg_content = fs::read_to_string(&cfg_path).unwrap();
        assert!(cfg_content.contains("Solarized Dark"));
        assert!(!cfg_content.contains("last_workspace"));
        assert!(!cfg_content.contains("recent_workspaces"));

        let ws = WorkspaceState {
            root_dir: Some(PathBuf::from("/a")),
            active_file: Some(PathBuf::from("/a/b.md")),
            open_files: vec![PathBuf::from("/a/b.md")],
            sidebar_visible: true,
            sidebar_width: None,
            outline_width: None,
            ..WorkspaceState::default()
        };
        config.update_last_workspace(ws);
        config.save_session();

        assert!(state_path.exists());
        let state_content = fs::read_to_string(&state_path).unwrap();
        assert!(state_content.contains("/a/b.md"));
    }

    #[test]
    fn test_corrupted_config_repaired_on_ui_save() {
        let temp_dir = tempfile::tempdir().expect("create temp dir");
        let target_file = temp_dir.path().join("config.toml");

        fs::write(&target_file, "theme = [invalid syntax here").unwrap();

        let config = AppConfig {
            config_file_path: Some(target_file.clone()),
            ..AppConfig::default().with_theme("CustomTheme")
        };

        config.save_preferences();

        let current_content = fs::read_to_string(&target_file).unwrap();
        assert!(current_content.contains("CustomTheme"));
        assert!(!current_content.contains("invalid syntax here"));
    }

    #[test]
    fn test_user_preferences_defaults_and_roundtrip() {
        let minimal_toml = r#"
            theme = "Nord"
        "#;
        let parsed: UserPreferences = toml::from_str(minimal_toml).unwrap();
        assert_eq!(parsed.theme, "Nord");
        assert_eq!(parsed.ui_font_size, 13.0);
        assert!(parsed.restore_workspace);
        assert!(!parsed.collocated_mode);
        assert!(!parsed.auto_save);
        assert_eq!(parsed.editor_font_size, 15.0);
        assert_eq!(parsed.line_height, 1.6);
        assert!(parsed.soft_wrap);
        assert!(parsed.cursor_blink);
        assert!(parsed.cursor_breathing);
        assert_eq!(parsed.default_render_mode, RenderMode::LivePreview);
        assert!(parsed.custom_keybindings.is_empty());

        let mut full_config = AppConfig::default().with_theme("Dracula");
        full_config.ui_font_size = 14.0;
        full_config.editor_font_size = 18.0;
        full_config.line_height = 1.8;
        full_config.collocated_mode = true;
        full_config.auto_save = true;
        full_config.soft_wrap = false;
        full_config.set_keybinding("NewTab", "ctrl-t");

        let serialized = toml::to_string_pretty(&full_config.preferences).unwrap();
        let deserialized: UserPreferences = toml::from_str(&serialized).unwrap();

        assert_eq!(deserialized.theme, "Dracula");
        assert_eq!(deserialized.ui_font_size, 14.0);
        assert_eq!(deserialized.editor_font_size, 18.0);
        assert_eq!(deserialized.line_height, 1.8);
        assert!(deserialized.collocated_mode);
        assert!(deserialized.auto_save);
        assert!(!deserialized.soft_wrap);
        assert!(deserialized.cursor_blink);
        assert!(deserialized.cursor_breathing);
        assert_eq!(
            deserialized.custom_keybindings.get("NewTab"),
            Some(&"ctrl-t".to_string())
        );
    }

    #[test]
    fn test_default_config_has_no_persisted_paths() {
        let mut config = AppConfig::default();
        assert_eq!(config.config_file_path, None);
        assert_eq!(config.state_file_path, None);

        config.theme = "Custom".to_string();
        config.save_preferences();
        config.update_last_workspace(WorkspaceState {
            root_dir: Some(PathBuf::from("/nonexistent/test")),
            active_file: None,
            open_files: Vec::new(),
            sidebar_visible: true,
            sidebar_width: None,
            outline_width: None,
            ..WorkspaceState::default()
        });
        assert_eq!(config.recent_workspaces().len(), 1);

        config.clear_recent_workspaces();
        assert!(config.recent_workspaces().is_empty());
    }

    #[test]
    fn test_load_filters_nonexistent_workspaces() {
        let temp_dir = tempfile::tempdir().expect("create temp dir");
        let valid_folder = temp_dir.path().join("real_project");
        let _ = fs::create_dir_all(&valid_folder);

        let config_path = temp_dir.path().join("config.toml");
        let state_path = temp_dir.path().join("state.toml");
        let session = SessionState {
            last_workspace: Some(WorkspaceState {
                root_dir: Some(PathBuf::from("/fake/nonexistent_last")),
                active_file: None,
                open_files: Vec::new(),
                sidebar_visible: true,
                sidebar_width: None,
                outline_width: None,
                ..WorkspaceState::default()
            }),
            recent_workspaces: vec![
                WorkspaceState {
                    root_dir: Some(PathBuf::from("/fake/project_5")),
                    active_file: None,
                    open_files: Vec::new(),
                    sidebar_visible: true,
                    sidebar_width: None,
                    outline_width: None,
                    ..WorkspaceState::default()
                },
                WorkspaceState {
                    root_dir: Some(valid_folder.clone()),
                    active_file: None,
                    open_files: Vec::new(),
                    sidebar_visible: true,
                    sidebar_width: None,
                    outline_width: None,
                    ..WorkspaceState::default()
                },
            ],
            last_layout: None,
        };
        let toml_str = toml::to_string_pretty(&session).unwrap();
        fs::write(&state_path, toml_str).unwrap();

        let config = AppConfig::load_from_paths(config_path, state_path);

        assert_eq!(config.last_workspace(), None);
        assert_eq!(config.recent_workspaces().len(), 1);
        assert_eq!(config.recent_workspaces()[0].root_dir, Some(valid_folder));
    }

    #[test]
    fn test_grammar_base_url_normalization_on_load() {
        let temp_dir = tempfile::tempdir().expect("create temp dir");
        let config_path = temp_dir.path().join("config.toml");
        let state_path = temp_dir.path().join("state.toml");

        fs::write(&config_path, "grammar_base_url = \"https://unpkg.com/\"\n").unwrap();
        let config = AppConfig::load_from_paths(config_path.clone(), state_path.clone());
        assert_eq!(
            config.grammar_base_url,
            crate::syntax::GrammarCdnPreset::Unpkg.url()
        );
        let saved_toml = fs::read_to_string(&config_path).unwrap();
        assert!(saved_toml.contains("grammar_base_url = \"https://unpkg.com\""));

        fs::write(&config_path, "grammar_base_url = \"\"\n").unwrap();
        let offline_config = AppConfig::load_from_paths(config_path.clone(), state_path);
        assert_eq!(offline_config.grammar_base_url, "");
    }

    #[gpui::test]
    fn test_async_persist_writes_latest_preferences_snapshot(cx: &mut gpui::TestAppContext) {
        let temp_dir = tempfile::tempdir().expect("create temp dir");
        let config_path = temp_dir.path().join("config.toml");
        let state_path = temp_dir.path().join("state.toml");
        let mut config = AppConfig {
            config_file_path: Some(config_path.clone()),
            state_file_path: Some(state_path),
            ..AppConfig::default()
        };

        cx.update(|cx| {
            for size in [14.0, 16.0, 18.0, 21.0] {
                config.editor_font_size = size;
                config.persist_preferences_async(cx.background_executor());
            }
        });
        cx.run_until_parked();

        let loaded = AppConfig::load_from_paths(config_path, temp_dir.path().join("state.toml"));
        assert_eq!(loaded.editor_font_size, 21.0);
    }

    #[gpui::test]
    fn test_window_layout_state_resolve_handles_valid_and_offscreen_bounds(
        cx: &mut gpui::TestAppContext,
    ) {
        cx.update(|cx| {
            let display = cx.primary_display().expect("test display");
            let uuid = display.uuid().unwrap().to_string();

            // 1. Valid bounds on matching display UUID preserve origin, size, and mode.
            let valid = WindowLayoutState {
                mode: WindowMode::Maximized,
                bounds: SerializedBounds {
                    x: 100.0,
                    y: 80.0,
                    width: 1200.0,
                    height: 760.0,
                },
                display_uuid: Some(uuid),
            };
            let (resolved_display, resolved_wb) = valid.resolve(cx);
            assert_eq!(resolved_display, Some(display.id()));
            assert_eq!(
                resolved_wb,
                gpui::WindowBounds::Maximized(gpui::Bounds::new(
                    gpui::point(gpui::px(100.0), gpui::px(80.0)),
                    gpui::size(gpui::px(1200.0), gpui::px(760.0)),
                ))
            );

            // 2. Disconnected display UUID falls back to primary display keeping clamped size.
            let disconnected = WindowLayoutState {
                mode: WindowMode::Windowed,
                bounds: SerializedBounds {
                    x: 3200.0,
                    y: 200.0,
                    width: 1400.0,
                    height: 900.0,
                },
                display_uuid: Some("disconnected-monitor-uuid".to_string()),
            };
            let (fallback_display, fallback_wb) = disconnected.resolve(cx);
            assert_eq!(fallback_display, Some(display.id()));
            let gpui::WindowBounds::Windowed(centered_bounds) = fallback_wb else {
                panic!("expected windowed bounds");
            };
            assert_eq!(
                centered_bounds.size,
                gpui::size(gpui::px(1400.0), gpui::px(900.0))
            );
            assert!(display.bounds().contains(&centered_bounds.center()));

            // 3. Invalid undersized bounds fall back to default centered size.
            let invalid = WindowLayoutState {
                mode: WindowMode::Windowed,
                bounds: SerializedBounds {
                    x: f32::NAN,
                    y: 0.0,
                    width: 100.0,
                    height: 50.0,
                },
                display_uuid: None,
            };
            let (_, default_wb) = invalid.resolve(cx);
            assert_eq!(
                default_wb.get_bounds().size,
                gpui::size(
                    gpui::px(DEFAULT_WINDOW_WIDTH),
                    gpui::px(DEFAULT_WINDOW_HEIGHT)
                )
            );
        });
    }

    #[test]
    fn test_empty_workspace_updates_last_layout_without_setting_last_workspace() {
        let mut config = AppConfig::default();
        let initial_wb = WindowLayoutState {
            mode: WindowMode::Windowed,
            bounds: SerializedBounds {
                x: 50.0,
                y: 60.0,
                width: 1280.0,
                height: 800.0,
            },
            display_uuid: None,
        };

        config.update_last_workspace(WorkspaceState {
            root_dir: Some(PathBuf::from("/proj")),
            active_file: None,
            open_files: Vec::new(),
            sidebar_visible: true,
            sidebar_width: Some(300.0),
            sidebar_view: SidebarView::Search,
            outline_visible: true,
            outline_width: Some(260.0),
            window_bounds: Some(initial_wb.clone()),
        });
        assert!(config.last_workspace().is_some());

        // Closing workspace or toggling sidebar on welcome view sends an empty WorkspaceState
        // without window_bounds; last_workspace becomes None, while last_layout keeps window_bounds
        // and updates sidebar/outline state.
        config.update_last_workspace(WorkspaceState {
            root_dir: None,
            active_file: None,
            open_files: Vec::new(),
            sidebar_visible: false,
            sidebar_width: Some(220.0),
            sidebar_view: SidebarView::Files,
            outline_visible: true,
            outline_width: Some(340.0),
            window_bounds: None,
        });

        assert_eq!(config.last_workspace(), None);
        let layout = config.last_layout().expect("last_layout must be kept");
        assert_eq!(layout.window_bounds.as_ref(), Some(&initial_wb));
        assert!(!layout.sidebar_visible);
        assert_eq!(layout.sidebar_width, Some(220.0));
        assert!(layout.outline_visible);
        assert_eq!(layout.outline_width, Some(340.0));
    }

    #[gpui::test]
    fn test_resolve_initial_window_bounds_respects_restore_workspace(
        cx: &mut gpui::TestAppContext,
    ) {
        cx.update(|cx| {
            let ws_wb = WindowLayoutState {
                mode: WindowMode::Maximized,
                bounds: SerializedBounds {
                    x: 40.0,
                    y: 40.0,
                    width: 1300.0,
                    height: 850.0,
                },
                display_uuid: None,
            };
            let layout_wb = WindowLayoutState {
                mode: WindowMode::Windowed,
                bounds: SerializedBounds {
                    x: 120.0,
                    y: 120.0,
                    width: 960.0,
                    height: 680.0,
                },
                display_uuid: None,
            };

            let mut config = AppConfig::default();
            config.session.last_workspace = Some(WorkspaceState {
                root_dir: Some(PathBuf::from("/proj")),
                window_bounds: Some(ws_wb),
                ..WorkspaceState::default()
            });
            config.session.last_layout = Some(UiLayoutState {
                window_bounds: Some(layout_wb),
                ..UiLayoutState::default()
            });

            config.restore_workspace = true;
            let (_, wb_restored) = config.resolve_initial_window_bounds(false, cx);
            assert!(matches!(wb_restored, gpui::WindowBounds::Maximized(_)));

            // With CLI initial file or restore_workspace = false, uses global last_layout bounds.
            let (_, wb_cli) = config.resolve_initial_window_bounds(true, cx);
            assert_eq!(
                wb_cli.get_bounds().size,
                gpui::size(gpui::px(960.0), gpui::px(680.0))
            );

            config.restore_workspace = false;
            let (_, wb_no_restore) = config.resolve_initial_window_bounds(false, cx);
            assert_eq!(
                wb_no_restore.get_bounds().size,
                gpui::size(gpui::px(960.0), gpui::px(680.0))
            );
        });
    }
}
