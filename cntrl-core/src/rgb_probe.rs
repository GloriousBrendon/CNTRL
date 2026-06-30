//! Native PC-RGB device probe — no external tools (no OpenRGB).
//!
//! Detects controllable non-keyboard RGB hardware with `hidapi`/sysfs and
//! persists a small mapping to `~/.config/cntrl/pc_rgb_probe.json`. Backends
//! self-discover by VID/PID, so this mapping is mostly informational + an
//! override hint; it is intentionally serde-tolerant so old/new files load.

use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::sync::OnceLock;

static PROBE_ONCE: OnceLock<()> = OnceLock::new();

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct RgbProbeConfig {
    /// hidraw node for the ITE motherboard/case controller, if detected.
    pub ite_hid_path: Option<String>,
    /// GPU I2C bus (e.g. `/dev/i2c-8`) — deferred backend, recorded if known.
    pub gpu_i2c_bus: Option<String>,
    pub gpu_i2c_addr: Option<String>,
    /// Candidate Corsair DRAM SMBus buses — deferred backend.
    pub corsair_i2c_buses: Vec<String>,
}

fn config_dir() -> Result<PathBuf, String> {
    dirs::config_dir()
        .map(|p| p.join("cntrl"))
        .ok_or_else(|| "Could not resolve config directory".to_string())
}

fn probe_path() -> Result<PathBuf, String> {
    Ok(config_dir()?.join("pc_rgb_probe.json"))
}

pub fn probe_config_path() -> Result<PathBuf, String> {
    probe_path()
}

pub fn read_probe_config() -> Result<Option<RgbProbeConfig>, String> {
    let path = probe_path()?;
    if !path.exists() {
        return Ok(None);
    }
    let raw = std::fs::read_to_string(path).map_err(|e| e.to_string())?;
    // Tolerant: unknown keys ignored, missing keys default.
    let cfg = serde_json::from_str::<RgbProbeConfig>(&raw).map_err(|e| e.to_string())?;
    Ok(Some(cfg))
}

pub fn write_probe_config(cfg: &RgbProbeConfig) -> Result<(), String> {
    std::fs::create_dir_all(config_dir()?).map_err(|e| e.to_string())?;
    let raw = serde_json::to_string_pretty(cfg).map_err(|e| e.to_string())?;
    std::fs::write(probe_path()?, raw).map_err(|e| e.to_string())
}

fn env_missing(key: &str) -> bool {
    std::env::var(key)
        .ok()
        .map(|v| v.trim().is_empty())
        .unwrap_or(true)
}

fn set_if_missing(key: &str, value: &str) {
    if env_missing(key) {
        unsafe {
            std::env::set_var(key, value);
        }
    }
}

/// Feed detected mapping into the env the backends read (only when not already set).
pub fn apply_probe_env(cfg: &RgbProbeConfig) {
    if let Some(p) = &cfg.ite_hid_path {
        set_if_missing("CNTRL_ITE_HID_PATH", p);
    }
    if let Some(bus) = &cfg.gpu_i2c_bus {
        set_if_missing("CNTRL_GPU_I2C_BUS", bus);
    }
    if let Some(addr) = &cfg.gpu_i2c_addr {
        set_if_missing("CNTRL_GPU_I2C_ADDR", addr);
    }
    if !cfg.corsair_i2c_buses.is_empty() {
        set_if_missing("CNTRL_CORSAIR_I2C_BUSES", &cfg.corsair_i2c_buses.join(","));
    }
}

/// Native probe: discover the ITE controller via hidapi. GPU/Corsair are deferred
/// backends, so their fields are left as previously persisted (not guessed here).
pub fn probe_native() -> Result<RgbProbeConfig, String> {
    let mut cfg = read_probe_config()?.unwrap_or_default();

    match crate::ite_rgb::discover_candidates() {
        Ok(cands) => {
            cfg.ite_hid_path = cands.into_iter().max_by_key(|c| c.score).map(|c| c.path);
        }
        Err(_) => {
            cfg.ite_hid_path = None;
        }
    }

    Ok(cfg)
}

pub fn ensure_runtime_probe(force_reprobe: bool) -> Result<String, String> {
    if !force_reprobe && PROBE_ONCE.get().is_some() {
        return Ok("rgb probe already initialized in this process".to_string());
    }

    if !force_reprobe {
        if let Some(cfg) = read_probe_config()? {
            apply_probe_env(&cfg);
            let _ = PROBE_ONCE.set(());
            return Ok("rgb probe loaded from persisted config".to_string());
        }
    }

    let cfg = probe_native()?;
    write_probe_config(&cfg)?;
    apply_probe_env(&cfg);
    let _ = PROBE_ONCE.set(());
    Ok("rgb probe refreshed via native detection".to_string())
}
