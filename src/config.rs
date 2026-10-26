//! Persisted, non-secret settings.
//!
//! Nothing in here may ever hold the lockfile password, Riot tokens or any
//! session JWT: `Settings` only contains plain preferences.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum Theme {
    #[default]
    Light,
    Dark,
}

/// Which renderer to use.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum RendererPref {
    /// GPU via wgpu (Direct3D 12 / Vulkan), falling back to OpenGL.
    #[default]
    Auto,
    /// OpenGL only (for drivers where wgpu misbehaves).
    OpenGl,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CrosshairPreset {
    pub name: String,
    pub code: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub window_size: [f32; 2],
    /// Restore the window maximized (window_size is then the size to un-maximize to).
    pub window_maximized: bool,
    /// Stored as "appearance" (the old "theme" key, from before the redesign,
    /// is ignored so everyone starts on the new light look).
    #[serde(rename = "appearance")]
    pub theme: Theme,
    /// Live-match polling interval while in agent select / in game.
    pub poll_interval_secs: u64,
    pub crosshairs: Vec<CrosshairPreset>,
    /// Optional social backend (Phase 3). `None` = tracker-only mode.
    pub backend_url: Option<String>,
    /// Share "in menus / agent select / in game" with backend friends.
    pub share_presence: bool,
    /// Share party size + queue with backend friends (never the party id).
    pub share_lobby: bool,
    pub disclaimer_accepted: bool,
    pub check_for_updates: bool,
    /// Renderer preference (takes effect on restart).
    pub renderer: RendererPref,
    /// Interface animations (page fades, sliding highlights, count-ups).
    pub animations: bool,
    /// Use the OS title bar instead of the app's own (takes effect on restart).
    pub system_title_bar: bool,
    /// GitHub "latest release" API URL (or any JSON with a `tag_name`).
    pub update_url: String,
}

pub const DEFAULT_UPDATE_URL: &str = "https://api.github.com/repos/tacobellerontop-sudo/tracked/releases/latest";

impl Default for Settings {
    fn default() -> Self {
        Self {
            window_size: [1440.0, 900.0],
            window_maximized: false,
            theme: Theme::Light,
            poll_interval_secs: 5,
            crosshairs: Vec::new(),
            backend_url: None,
            share_presence: true,
            share_lobby: false,
            disclaimer_accepted: false,
            check_for_updates: true,
            renderer: RendererPref::Auto,
            animations: true,
            system_title_bar: false,
            update_url: DEFAULT_UPDATE_URL.to_string(),
        }
    }
}

pub const MIN_POLL_SECS: u64 = 3;
pub const MAX_POLL_SECS: u64 = 30;

impl Settings {
    pub fn poll_interval(&self) -> std::time::Duration {
        std::time::Duration::from_secs(self.poll_interval_secs.clamp(MIN_POLL_SECS, MAX_POLL_SECS))
    }
}

fn project_dirs() -> Option<directories::ProjectDirs> {
    directories::ProjectDirs::from("dev", "tracked", "tracked")
}

pub fn config_path() -> Option<PathBuf> {
    if let Some(dir) = std::env::var_os("TRACKED_CONFIG_DIR") {
        return Some(PathBuf::from(dir).join("settings.json"));
    }
    project_dirs().map(|d| d.config_dir().join("settings.json"))
}

pub fn cache_dir() -> Option<PathBuf> {
    if let Some(dir) = std::env::var_os("TRACKED_CACHE_DIR") {
        return Some(PathBuf::from(dir));
    }
    project_dirs().map(|d| d.cache_dir().to_path_buf())
}

/// Load settings, falling back to defaults on any error. Settings saved
/// under the app's old name (Ascend-Lite) are picked up once and then
/// saved under the new one.
pub fn load() -> Settings {
    let read = |p: PathBuf| {
        std::fs::read_to_string(p)
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
    };
    if let Some(settings) = config_path().and_then(read) {
        return settings;
    }
    if std::env::var_os("TRACKED_CONFIG_DIR").is_none() {
        let legacy = directories::ProjectDirs::from("dev", "ascend-lite", "ascend-lite")
            .map(|d| d.config_dir().join("settings.json"));
        if let Some(settings) = legacy.and_then(read) {
            let _ = save(&settings);
            return settings;
        }
    }
    Settings::default()
}

pub fn save(settings: &Settings) -> std::io::Result<()> {
    let path = config_path().ok_or_else(|| std::io::Error::other("no config dir"))?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, serde_json::to_vec_pretty(settings)?)?;
    std::fs::rename(tmp, path)
}
