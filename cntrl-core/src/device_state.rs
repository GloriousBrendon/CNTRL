//! Per-device saved lighting, for restoring RGB after a reboot.
//!
//! Each backend (`keyboard`, `ite_rgb`, `gpu_rgb`, `corsair_ram`) remembers its
//! own last color/effect in `~/.config/cntrl/device_state.json`, so devices that
//! were given different colors all come back correctly on the next login.

use crate::ipc::{push_to_devices, ApplyOptions};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct LightState {
    pub hex: String,
    pub effect: i32,
    pub brightness: i32,
    pub speed: i32,
}

impl Default for LightState {
    fn default() -> Self {
        Self {
            hex: "#ffffff".to_string(),
            effect: crate::lighting_modes::MODE_STATIC,
            brightness: 100,
            speed: 1,
        }
    }
}

/// Backend id → its last lighting.
pub type DeviceStates = BTreeMap<String, LightState>;

fn config_dir() -> Result<PathBuf, String> {
    dirs::config_dir()
        .map(|p| p.join("cntrl"))
        .ok_or_else(|| "Could not resolve config directory".to_string())
}

pub fn state_path() -> Result<PathBuf, String> {
    Ok(config_dir()?.join("device_state.json"))
}

pub fn load() -> DeviceStates {
    let Ok(path) = state_path() else {
        return DeviceStates::new();
    };
    match std::fs::read_to_string(&path) {
        Ok(raw) => serde_json::from_str(&raw).unwrap_or_default(),
        Err(_) => DeviceStates::new(),
    }
}

pub fn save(states: &DeviceStates) -> Result<(), String> {
    std::fs::create_dir_all(config_dir()?).map_err(|e| e.to_string())?;
    let raw = serde_json::to_string_pretty(states).map_err(|e| e.to_string())?;
    let final_path = state_path()?;
    let tmp = final_path.with_file_name(format!("device_state.json.tmp.{}", std::process::id()));
    std::fs::write(&tmp, raw).map_err(|e| e.to_string())?;
    std::fs::rename(&tmp, &final_path).map_err(|e| e.to_string())
}

/// Record the same lighting for each backend id (e.g. `["keyboard", "gpu_rgb"]`).
pub fn remember(backends: &[String], hex: &str, effect: i32, brightness: i32, speed: i32) {
    let mut states = load();
    for b in backends {
        states.insert(
            b.clone(),
            LightState {
                hex: hex.to_string(),
                effect,
                brightness,
                speed,
            },
        );
    }
    if let Err(e) = save(&states) {
        eprintln!("cntrl: failed to save device_state.json: {e}");
    }
}

/// Record the lighting a document actually applied, for the backends named by
/// `options` (PC fan-out resolved via `CNTRL_PC_RGB_BACKENDS`, as at apply time).
///
/// Callers must pass the document they *applied*, not the one they loaded from
/// disk — e.g. theme sync promotes an Off slot to Static before pushing, and
/// saving the pre-promotion effect is what made the keyboard come back dark
/// after every reboot.
pub fn remember_applied(doc: &crate::profile::ProfileDocument, options: ApplyOptions) {
    let mut backends: Vec<String> = Vec::new();
    if options.keyboard {
        backends.push("keyboard".into());
    }
    if options.pc_rgb {
        backends.extend(crate::rgb_backends::BackendRegistry::from_env().enabled_ids);
    }
    if backends.is_empty() {
        return;
    }
    let cfg = &doc.data.light.config;
    remember(
        &backends,
        &cfg.hex_color,
        cfg.effect,
        cfg.brightness,
        cfg.speed,
    );
}

pub(crate) fn doc_for(state: &LightState) -> crate::profile::ProfileDocument {
    let mut doc = crate::profile::default_profile_document();
    doc.data.light.config.hex_color = state.hex.clone();
    doc.data.light.config.effect = state.effect;
    doc.data.light.config.brightness = state.brightness;
    doc.data.light.config.speed = state.speed;
    doc.data.light.colors = vec![state.hex.clone()];
    doc
}

/// Re-apply every saved device's lighting (used by `cntrl restore` on login).
pub fn restore_all() -> Result<String, String> {
    let states = load();
    if states.is_empty() {
        return Ok("no saved device state to restore".to_string());
    }
    let mut ok = 0usize;
    let mut errors: Vec<String> = Vec::new();
    for (backend, state) in &states {
        let doc = doc_for(state);
        let result = if backend == "keyboard" {
            push_to_devices(
                &doc,
                ApplyOptions {
                    keyboard: true,
                    pc_rgb: false,
                },
            )
        } else {
            // Restrict the PC fan-out to just this backend.
            unsafe {
                std::env::set_var("CNTRL_PC_RGB_BACKENDS", backend);
            }
            push_to_devices(
                &doc,
                ApplyOptions {
                    keyboard: false,
                    pc_rgb: true,
                },
            )
        };
        match result {
            Ok(()) => ok += 1,
            Err(e) => errors.push(format!("{backend}: {e}")),
        }
    }
    if errors.is_empty() {
        Ok(format!("restored {ok} device(s)"))
    } else {
        Err(format!("restored {ok}; errors: {}", errors.join(" | ")))
    }
}
