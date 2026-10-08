//! Persisted CNTRL settings (`~/.config/cntrl/settings.json`).
//!
//! Serde-tolerant: missing/unknown keys fall back to defaults so the file can
//! evolve without breaking older or newer binaries.

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    /// When true, the daemon watches the theme file and re-applies on change.
    pub theme_sync_enabled: bool,
    /// Profile slot the theme color is applied to (0..=3).
    pub theme_sync_slot: u8,
    /// Apply the theme color to the keyboard.
    pub theme_sync_keyboard: bool,
    /// Apply the theme color to PC RGB backends (ITE, etc.).
    pub theme_sync_pc: bool,
    /// The daemon starts the canvas stream (saved layout) when it launches.
    pub canvas_autostart: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            theme_sync_enabled: false,
            theme_sync_slot: 0,
            theme_sync_keyboard: true,
            theme_sync_pc: false,
            canvas_autostart: false,
        }
    }
}

fn config_dir() -> Result<PathBuf, String> {
    dirs::config_dir()
        .map(|p| p.join("cntrl"))
        .ok_or_else(|| "Could not resolve config directory".to_string())
}

pub fn settings_path() -> Result<PathBuf, String> {
    Ok(config_dir()?.join("settings.json"))
}

/// Load settings, returning defaults if the file is missing or unreadable.
pub fn load() -> Settings {
    let Ok(path) = settings_path() else {
        return Settings::default();
    };
    match std::fs::read_to_string(&path) {
        Ok(raw) => serde_json::from_str(&raw).unwrap_or_default(),
        Err(_) => Settings::default(),
    }
}

pub fn save(settings: &Settings) -> Result<(), String> {
    std::fs::create_dir_all(config_dir()?).map_err(|e| e.to_string())?;
    let raw = serde_json::to_string_pretty(settings).map_err(|e| e.to_string())?;
    std::fs::write(settings_path()?, raw).map_err(|e| e.to_string())
}
