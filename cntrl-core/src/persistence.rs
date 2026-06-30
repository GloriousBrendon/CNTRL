//! Profile slot persistence under `dirs::config_dir()/cntrl`.

use crate::profile::{self, normalize, ProfileDocument};
use std::path::PathBuf;

fn config_dir() -> Result<PathBuf, String> {
    dirs::config_dir()
        .map(|p| p.join("cntrl"))
        .ok_or_else(|| "Could not resolve config directory".to_string())
}

fn profiles_dir() -> Result<PathBuf, String> {
    Ok(config_dir()?.join("profiles"))
}

fn slot_path(slot: u8) -> Result<PathBuf, String> {
    if slot > 3 {
        return Err("Slot must be 0..=3".into());
    }
    Ok(profiles_dir()?.join(format!("slot_{slot}.json")))
}

fn active_slot_path() -> Result<PathBuf, String> {
    Ok(config_dir()?.join("active_slot.txt"))
}

pub fn persist_active_slot(slot: u8) -> Result<(), String> {
    std::fs::create_dir_all(config_dir()?).map_err(|e| e.to_string())?;
    std::fs::write(active_slot_path()?, format!("{slot}")).map_err(|e| e.to_string())
}

pub fn read_startup_slot() -> u8 {
    active_slot_path()
        .ok()
        .and_then(|p| std::fs::read_to_string(p).ok())
        .and_then(|s| s.trim().parse().ok())
        .filter(|&s: &u8| s <= 3)
        .unwrap_or(0)
}

pub fn load_profile_slot(slot: u8) -> Result<ProfileDocument, String> {
    let path = slot_path(slot)?;
    if path.exists() {
        let s = std::fs::read_to_string(&path).map_err(|e| e.to_string())?;
        let mut p: ProfileDocument = serde_json::from_str(&s).map_err(|e| e.to_string())?;
        normalize(&mut p);
        Ok(p)
    } else {
        Ok(profile::default_profile_document())
    }
}

pub fn persist_profile_slot(slot: u8, doc: &ProfileDocument) -> Result<(), String> {
    std::fs::create_dir_all(profiles_dir()?).map_err(|e| e.to_string())?;
    let s = serde_json::to_string_pretty(doc).map_err(|e| e.to_string())?;
    let final_path = slot_path(slot)?;
    // Atomic write: a unique temp file + rename, so concurrent applies can't
    // interleave and corrupt the slot (which previously left a trailing `}`).
    let tmp = final_path.with_file_name(format!("slot_{slot}.json.tmp.{}", std::process::id()));
    std::fs::write(&tmp, s).map_err(|e| e.to_string())?;
    std::fs::rename(&tmp, &final_path).map_err(|e| e.to_string())
}
