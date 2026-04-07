mod profile;

#[cfg(target_os = "linux")]
mod real_hid;
#[cfg(not(target_os = "linux"))]
mod mock_hid;

#[cfg(target_os = "linux")]
use real_hid as hid;
#[cfg(not(target_os = "linux"))]
use mock_hid as hid;

use profile::ProfileDocument;
use serde::Serialize;
use std::sync::Mutex;
use tauri::State;

pub struct AppState {
    pub profile: Mutex<ProfileDocument>,
    /// 0..=3 onboard slots (UI); device sync comes later.
    pub active_slot: Mutex<u8>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DeviceStatus {
    pub connected: bool,
    pub profile_name: String,
    pub firmware_version: String,
    pub active_slot: u8,
    pub mock_mode: bool,
}

fn config_dir() -> Result<std::path::PathBuf, String> {
    dirs::config_dir()
        .map(|p| p.join("cntrl"))
        .ok_or_else(|| "Could not resolve config directory".to_string())
}

#[tauri::command]
fn get_profile(state: State<'_, AppState>) -> Result<ProfileDocument, String> {
    state
        .profile
        .lock()
        .map_err(|e| e.to_string())
        .map(|g| g.clone())
}

#[tauri::command]
fn set_lighting(
    hex_color: String,
    effect: i32,
    brightness: i32,
    speed: i32,
    state: State<'_, AppState>,
) -> Result<ProfileDocument, String> {
    let mut p = state.profile.lock().map_err(|e| e.to_string())?;
    p.data.light.config.hex_color = hex_color;
    p.data.light.config.effect = effect;
    p.data.light.config.brightness = brightness.clamp(0, 100);
    p.data.light.config.speed = speed;
    // Static preset id in exports is often 3
    if effect == 3 {
        let c = p.data.light.config.hex_color.clone();
        for x in &mut p.data.light.colors {
            *x = c.clone();
        }
    }
    let out = p.clone();
    drop(p);
    hid::write_profile_to_device(&out)?;
    Ok(out)
}

#[tauri::command]
fn set_selected_light_effect(
    effect_id: String,
    display_name: String,
    state: State<'_, AppState>,
) -> Result<ProfileDocument, String> {
    let mut p = state.profile.lock().map_err(|e| e.to_string())?;
    p.data.selected_light_effect = vec![effect_id, display_name];
    let out = p.clone();
    drop(p);
    hid::write_profile_to_device(&out)?;
    Ok(out)
}

#[tauri::command]
fn refresh_profile_from_device(state: State<'_, AppState>) -> Result<ProfileDocument, String> {
    let read = hid::read_profile_from_device()?;
    let mut g = state.profile.lock().map_err(|e| e.to_string())?;
    *g = read;
    Ok(g.clone())
}

#[tauri::command]
fn get_device_status(state: State<'_, AppState>) -> Result<DeviceStatus, String> {
    let profile = state.profile.lock().map_err(|e| e.to_string())?;
    let slot = *state.active_slot.lock().map_err(|e| e.to_string())?;
    Ok(DeviceStatus {
        connected: hid::keyboard_connected(),
        profile_name: profile.data.name.clone(),
        firmware_version: profile.data.extra.fw_ver.clone(),
        active_slot: slot,
        mock_mode: !cfg!(target_os = "linux"),
    })
}

#[tauri::command]
fn set_active_slot(slot: u8, state: State<'_, AppState>) -> Result<(), String> {
    if slot > 3 {
        return Err("Slot must be 0..=3".into());
    }
    *state.active_slot.lock().map_err(|e| e.to_string())? = slot;
    Ok(())
}

#[tauri::command]
fn get_config_dir_cmd() -> Result<String, String> {
    config_dir().map(|p| p.to_string_lossy().into_owned())
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let initial = hid::read_profile_from_device().unwrap_or_else(|_| profile::default_profile_document());

    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .manage(AppState {
            profile: Mutex::new(initial),
            active_slot: Mutex::new(0),
        })
        .invoke_handler(tauri::generate_handler![
            get_profile,
            set_lighting,
            set_selected_light_effect,
            refresh_profile_from_device,
            get_device_status,
            set_active_slot,
            get_config_dir_cmd
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
