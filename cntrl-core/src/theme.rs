//! Pywal-style theme file: `~/.config/themes/colors.conf` with lines like `$key=rgba(rrggbbaa)`.
//! Shared by GTK and ICED (no UI dependencies).

use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;

/// Canonical keys written by common pywal / rice templates (subset used by CNTRL GTK).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ThemeColors {
    pub bg1: String,
    pub bg2: String,
    pub surface: String,
    pub accent: String,
    pub text: String,
}

impl Default for ThemeColors {
    fn default() -> Self {
        Self {
            bg1: "#0f1822".to_string(),
            bg2: "#080c10".to_string(),
            surface: "#1f252e".to_string(),
            accent: "#d88f92".to_string(),
            text: "#a8b0be".to_string(),
        }
    }
}

/// Resolved path: `dirs::config_dir()/themes/colors.conf` (typically `~/.config/themes/colors.conf`).
pub fn theme_file_path() -> PathBuf {
    dirs::config_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("themes")
        .join("colors.conf")
}

/// Reads and parses the theme file; on missing file or parse failure returns [`ThemeColors::default`].
pub fn load_theme_from_disk() -> ThemeColors {
    let path = theme_file_path();
    let content = fs::read_to_string(path).unwrap_or_default();
    parse_theme_colors(&content).unwrap_or_default()
}

pub fn parse_theme_colors(content: &str) -> Result<ThemeColors, String> {
    let mut out = ThemeColors::default();
    let mut found = 0usize;

    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        let Some((raw_key, raw_val)) = trimmed.split_once('=') else {
            continue;
        };
        let key = raw_key.trim().trim_start_matches('$');
        let val = raw_val.trim();
        let Some(hex) = rgba_to_hex(val) else {
            continue;
        };

        match key {
            "bg1" => {
                out.bg1 = hex;
                found += 1;
            }
            "bg2" => {
                out.bg2 = hex;
                found += 1;
            }
            "surface" => {
                out.surface = hex;
                found += 1;
            }
            "accent" => {
                out.accent = hex;
                found += 1;
            }
            "text" => {
                out.text = hex;
                found += 1;
            }
            _ => {}
        }
    }

    if found == 0 {
        return Err("No theme keys found in colors.conf".to_string());
    }

    Ok(out)
}

fn rgba_to_hex(value: &str) -> Option<String> {
    let inner = value
        .strip_prefix("rgba(")?
        .strip_suffix(')')?
        .trim()
        .to_ascii_lowercase();
    if inner.len() != 8 || !inner.chars().all(|c| c.is_ascii_hexdigit()) {
        return None;
    }
    Some(format!("#{}", &inner[..6]))
}
