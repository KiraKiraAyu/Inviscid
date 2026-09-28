use serde::{Deserialize, Serialize};
use std::fs;
use std::ops::{Deref, DerefMut};
use std::path::{Path, PathBuf};

use crate::editor::RenderMode;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct WorkspaceState {
    #[serde(default)]
    pub root_dir: Option<PathBuf>,
    #[serde(default)]
    pub active_file: Option<PathBuf>,
    #[serde(default)]
    pub open_files: Vec<PathBuf>,
    #[serde(default = "default_true")]
    pub sidebar_visible: bool,
    #[serde(default)]
    pub sidebar_width: Option<f32>,
}

fn default_true() -> bool {
    true
}

fn default_false() -> bool {
    false
}

fn default_ui_font_size() -> f32 {
    13.0
}

fn default_editor_font_size() -> f32 {
    15.0
}

fn default_line_height() -> f32 {
    1.6
}

fn default_tab_size() -> usize {
    4
}

impl Default for WorkspaceState {
    fn default() -> Self {
        Self {
            root_dir: None,
            active_file: None,
            open_files: Vec::new(),
            sidebar_visible: true,
            sidebar_width: None,
        }
    }
}

impl WorkspaceState {
    pub fn is_empty(&self) -> bool {
        self.root_dir.is_none() && self.active_file.is_none() && self.open_files.is_empty()
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

/// User preferences persisted in `config.toml`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct UserPreferences {
    #[serde(default = "default_theme")]
    pub theme: String,
    #[serde(default = "default_ui_font_size")]
    pub ui_font_size: f32,
    #[serde(default = "default_true")]
    pub restore_workspace: bool,
    #[serde(default = "default_false")]
    pub auto_save: bool,
    #[serde(default = "default_editor_font_size")]
    pub editor_font_size: f32,
    #[serde(default = "default_line_height")]
    pub line_height: f32,
    #[serde(default = "default_true")]
    pub soft_wrap: bool,
    #[serde(default = "default_tab_size")]
    pub tab_size: usize,
    #[serde(default = "default_true")]
    pub cursor_blink: bool,
    #[serde(default = "default_true")]
    pub cursor_breathing: bool,
    #[serde(default)]
    pub default_render_mode: RenderMode,
    #[serde(default)]
    pub custom_keybindings: std::collections::HashMap<String, String>,
}

/// Must match the `name` inside `assets/themes/catppuccin-mocha.toml`.
fn default_theme() -> String {
    "Catppuccin Mocha".to_string()
}

impl Default for UserPreferences {
    fn default() -> Self {
        Self {
            theme: default_theme(),
            ui_font_size: default_ui_font_size(),
            restore_workspace: default_true(),
            auto_save: default_false(),
            editor_font_size: default_editor_font_size(),
            line_height: default_line_height(),
            soft_wrap: default_true(),
            tab_size: default_tab_size(),
            cursor_blink: default_true(),
            cursor_breathing: default_true(),
            default_render_mode: RenderMode::default(),
            custom_keybindings: std::collections::HashMap::new(),
        }
    }
}

/// Runtime session state persisted in `state.toml`.
#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
pub struct SessionState {
    #[serde(default)]
    pub last_workspace: Option<WorkspaceState>,
    #[serde(default)]
    pub recent_workspaces: Vec<WorkspaceState>,
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
}

/// Writes `content` to `target_path` via a sibling temporary file, flushing to disk before
/// renaming with short retries to handle transient file locks on Windows.
pub fn atomic_write(target_path: &Path, content: &str) -> std::io::Result<()> {
    if let Some(parent) = target_path.parent() {
        fs::create_dir_all(parent)?;
    }

    let rand_suffix: u64 = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos() as u64;
    let tmp_path =
        target_path.with_extension(format!("tmp.{}.{}", std::process::id(), rand_suffix));

    {
        use std::io::Write;
        let mut file = fs::OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .open(&tmp_path)?;
        file.write_all(content.as_bytes())?;
        file.sync_all()?;
    }

    // Windows anti-virus or search indexers can momentarily lock files; retry a few times.
    let mut last_err = None;
    for attempt in 0..3 {
        match fs::rename(&tmp_path, target_path) {
            Ok(()) => return Ok(()),
            Err(e) => {
                last_err = Some(e);
                std::thread::sleep(std::time::Duration::from_millis(5 * (attempt + 1)));
            }
        }
    }

    if let Some(e) = last_err {
        let _ = fs::remove_file(&tmp_path);
        return Err(e);
    }

    Ok(())
}

impl AppConfig {
    /// Standard system config path (`%APPDATA%/inviscid/config.toml` or `~/.config/inviscid/config.toml`).
    pub fn config_path() -> Option<PathBuf> {
        dirs::config_dir().map(|p| p.join("inviscid").join("config.toml"))
    }

    /// Standard system state path (`%LOCALAPPDATA%/inviscid/state.toml` or `~/.local/share/inviscid/state.toml`).
    pub fn state_path() -> Option<PathBuf> {
        dirs::data_local_dir()
            .or_else(dirs::data_dir)
            .or_else(dirs::config_dir)
            .map(|p| p.join("inviscid").join("state.toml"))
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
                    Ok(parsed) => {
                        config.preferences = parsed;
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
            .retain(|ws| ws.has_existing_path());

        if let Some(ref ws) = config.session.last_workspace {
            if !ws.has_existing_path() {
                config.session.last_workspace = None;
            }
        }

        config
    }

    /// Persists user preferences to `config.toml` atomically when `config_file_path` is set.
    pub fn save_preferences(&self) {
        if let Some(ref path) = self.config_file_path {
            if let Ok(content) = toml::to_string_pretty(&self.preferences) {
                let _ = atomic_write(path, &content);
            }
        }
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
        self.save_preferences();
    }

    pub fn reset_keybinding(&mut self, action: &str) {
        self.custom_keybindings.remove(action);
        self.save_preferences();
    }

    pub fn reset_all_keybindings(&mut self) {
        self.custom_keybindings.clear();
        self.save_preferences();
    }

    pub fn clear_recent_workspaces(&mut self) {
        self.session.recent_workspaces.clear();
    }

    /// Persists session state to `state.toml` atomically when `state_file_path` is set.
    pub fn save_session(&self) {
        if let Some(ref path) = self.state_file_path {
            if let Ok(content) = toml::to_string_pretty(&self.session) {
                let _ = atomic_write(path, &content);
            }
        }
    }

    /// Records the current workspace state and adds it to recent workspaces in memory.
    pub fn update_last_workspace(&mut self, state: WorkspaceState) {
        if !state.is_empty() {
            self.session.recent_workspaces.retain(|w| {
                if let (Some(a), Some(b)) = (&w.root_dir, &state.root_dir) {
                    a != b
                } else if w.root_dir.is_none()
                    && state.root_dir.is_none()
                    && state.active_file.is_some()
                {
                    w.active_file != state.active_file
                } else {
                    w != &state
                }
            });
            self.session.recent_workspaces.insert(0, state.clone());
            if self.session.recent_workspaces.len() > 10 {
                self.session.recent_workspaces.truncate(10);
            }
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
            sidebar_width: None,
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
        };
        config.update_last_workspace(state_10);
        assert_eq!(config.recent_workspaces().len(), 10);
        assert_eq!(
            config.recent_workspaces()[0].active_file,
            Some(PathBuf::from("/doc_10.md"))
        );
    }

    #[test]
    fn test_workspace_single_file_deduplication() {
        let mut config = AppConfig::default();

        let state_file_a = WorkspaceState {
            root_dir: None,
            active_file: Some(PathBuf::from("/notes/today.md")),
            open_files: vec![PathBuf::from("/notes/today.md")],
            sidebar_visible: true,
            sidebar_width: None,
        };
        config.update_last_workspace(state_file_a);

        let state_file_a_updated = WorkspaceState {
            root_dir: None,
            active_file: Some(PathBuf::from("/notes/today.md")),
            open_files: vec![PathBuf::from("/notes/today.md")],
            sidebar_visible: false,
            sidebar_width: Some(250.0),
        };
        config.update_last_workspace(state_file_a_updated.clone());

        assert_eq!(config.recent_workspaces().len(), 1);
        assert_eq!(config.recent_workspaces()[0], state_file_a_updated);
    }

    #[test]
    fn test_atomic_write_safety() {
        let temp_dir =
            std::env::temp_dir().join(format!("inviscid_test_atomic_{}", std::process::id()));
        let target_file = temp_dir.join("sub").join("test_config.toml");

        let content = "theme = \"One Dark Pro\"\n";
        atomic_write(&target_file, content).expect("atomic_write should succeed");

        let read_back = fs::read_to_string(&target_file).expect("file must exist and be readable");
        assert_eq!(read_back, content);

        let updated_content = "theme = \"Nord\"\n";
        atomic_write(&target_file, updated_content).expect("atomic overwrite should succeed");
        let read_back_updated = fs::read_to_string(&target_file).expect("file must be readable");
        assert_eq!(read_back_updated, updated_content);

        let _ = fs::remove_dir_all(temp_dir);
    }

    #[test]
    fn test_preferences_and_session_separation() {
        let temp_dir =
            std::env::temp_dir().join(format!("inviscid_test_split_{}", std::process::id()));
        let cfg_path = temp_dir.join("config.toml");
        let state_path = temp_dir.join("state.toml");

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
        };
        config.update_last_workspace(ws);
        config.save_session();

        assert!(state_path.exists());
        let state_content = fs::read_to_string(&state_path).unwrap();
        assert!(state_content.contains("/a/b.md"));

        let _ = fs::remove_dir_all(temp_dir);
    }

    #[test]
    fn test_corrupted_config_repaired_on_ui_save() {
        let temp_dir = std::env::temp_dir().join(format!(
            "inviscid_test_corrupt_repair_{}",
            std::process::id()
        ));
        let target_file = temp_dir.join("config.toml");
        let _ = fs::create_dir_all(&temp_dir);

        fs::write(&target_file, "theme = [invalid syntax here").unwrap();

        let config = AppConfig {
            config_file_path: Some(target_file.clone()),
            ..AppConfig::default().with_theme("CustomTheme")
        };

        config.save_preferences();

        let current_content = fs::read_to_string(&target_file).unwrap();
        assert!(current_content.contains("CustomTheme"));
        assert!(!current_content.contains("invalid syntax here"));

        let _ = fs::remove_dir_all(temp_dir);
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
        assert!(!parsed.auto_save);
        assert_eq!(parsed.editor_font_size, 15.0);
        assert_eq!(parsed.line_height, 1.6);
        assert!(parsed.soft_wrap);
        assert_eq!(parsed.tab_size, 4);
        assert!(parsed.cursor_blink);
        assert!(parsed.cursor_breathing);
        assert_eq!(parsed.default_render_mode, RenderMode::LivePreview);
        assert!(parsed.custom_keybindings.is_empty());

        let mut full_config = AppConfig::default().with_theme("Dracula");
        full_config.ui_font_size = 14.0;
        full_config.editor_font_size = 18.0;
        full_config.line_height = 1.8;
        full_config.auto_save = true;
        full_config.tab_size = 2;
        full_config.soft_wrap = false;
        full_config.set_keybinding("NewTab", "ctrl-t");

        let serialized = toml::to_string_pretty(&full_config.preferences).unwrap();
        let deserialized: UserPreferences = toml::from_str(&serialized).unwrap();

        assert_eq!(deserialized.theme, "Dracula");
        assert_eq!(deserialized.ui_font_size, 14.0);
        assert_eq!(deserialized.editor_font_size, 18.0);
        assert_eq!(deserialized.line_height, 1.8);
        assert!(deserialized.auto_save);
        assert_eq!(deserialized.tab_size, 2);
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
        });
        assert_eq!(config.recent_workspaces().len(), 1);

        config.clear_recent_workspaces();
        assert!(config.recent_workspaces().is_empty());
    }

    #[test]
    fn test_load_filters_nonexistent_workspaces() {
        let temp_dir =
            std::env::temp_dir().join(format!("inviscid_test_filter_ws_{}", std::process::id()));
        let _ = fs::create_dir_all(&temp_dir);
        let valid_folder = temp_dir.join("real_project");
        let _ = fs::create_dir_all(&valid_folder);

        let config_path = temp_dir.join("config.toml");
        let state_path = temp_dir.join("state.toml");
        let session = SessionState {
            last_workspace: Some(WorkspaceState {
                root_dir: Some(PathBuf::from("/fake/nonexistent_last")),
                active_file: None,
                open_files: Vec::new(),
                sidebar_visible: true,
                sidebar_width: None,
            }),
            recent_workspaces: vec![
                WorkspaceState {
                    root_dir: Some(PathBuf::from("/fake/project_5")),
                    active_file: None,
                    open_files: Vec::new(),
                    sidebar_visible: true,
                    sidebar_width: None,
                },
                WorkspaceState {
                    root_dir: Some(valid_folder.clone()),
                    active_file: None,
                    open_files: Vec::new(),
                    sidebar_visible: true,
                    sidebar_width: None,
                },
            ],
        };
        let toml_str = toml::to_string_pretty(&session).unwrap();
        fs::write(&state_path, toml_str).unwrap();

        let config = AppConfig::load_from_paths(config_path, state_path);

        assert_eq!(config.last_workspace(), None);
        assert_eq!(config.recent_workspaces().len(), 1);
        assert_eq!(config.recent_workspaces()[0].root_dir, Some(valid_folder));

        let _ = fs::remove_dir_all(temp_dir);
    }
}
