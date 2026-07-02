//! GPU RGB backend (M5.4): native Gigabyte Fusion2 I2C path, with optional
//! OpenRGB CLI fallback for resilience.

use crate::profile::ProfileDocument;

fn profile_rgb(doc: &ProfileDocument) -> Result<(u8, u8, u8), String> {
    let cfg = &doc.data.light.config;
    let t = cfg.hex_color.trim().trim_start_matches('#');
    if t.len() != 6 {
        return Err(format!("invalid hexColor {:?}, expected #rrggbb", cfg.hex_color));
    }
    let r = u8::from_str_radix(&t[0..2], 16).map_err(|e| e.to_string())?;
    let g = u8::from_str_radix(&t[2..4], 16).map_err(|e| e.to_string())?;
    let b = u8::from_str_radix(&t[4..6], 16).map_err(|e| e.to_string())?;
    Ok((r, g, b))
}

#[cfg(target_os = "linux")]
fn detect_default_gpu_i2c_bus() -> Option<String> {
    let base = std::path::Path::new("/sys/bus/i2c/devices");
    let entries = std::fs::read_dir(base).ok()?;
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        if !name.starts_with("i2c-") {
            continue;
        }
        let npath = entry.path().join("name");
        let Ok(raw) = std::fs::read_to_string(&npath) else {
            continue;
        };
        let lname = raw.to_ascii_lowercase();
        if lname.contains("amdgpu") && lname.contains("oem") {
            return Some(format!("/dev/{name}"));
        }
    }
    None
}

/// Presence/health of the GPU RGB target, for the Devices view.
pub enum GpuProbe {
    Ready(String),
    Blocked(String),
    Absent(String),
}

/// Read-only probe: locate the GPU's I2C bus and check it's openable. No writes.
#[cfg(target_os = "linux")]
pub fn probe() -> GpuProbe {
    let bus = std::env::var("CNTRL_GPU_I2C_BUS")
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .or_else(detect_default_gpu_i2c_bus);
    match bus {
        None => GpuProbe::Absent("no AMDGPU OEM i2c bus found".to_string()),
        Some(b) => match std::fs::OpenOptions::new().read(true).write(true).open(&b) {
            Ok(_) => GpuProbe::Ready(b),
            Err(e) if e.kind() == std::io::ErrorKind::PermissionDenied => {
                GpuProbe::Blocked(format!("{b}: permission denied"))
            }
            Err(e) => GpuProbe::Absent(format!("{b}: {e}")),
        },
    }
}

#[cfg(not(target_os = "linux"))]
pub fn probe() -> GpuProbe {
    GpuProbe::Absent("gpu_rgb requires Linux".to_string())
}

#[cfg(target_os = "linux")]
fn write_native_fusion2(r: u8, g: u8, b: u8) -> Result<String, String> {
    use i2cdev::core::I2CDevice;
    use i2cdev::linux::LinuxI2CDevice;

    let bus = std::env::var("CNTRL_GPU_I2C_BUS")
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .or_else(detect_default_gpu_i2c_bus)
        .unwrap_or_else(|| "/dev/i2c-10".to_string());
    let addr_raw = std::env::var("CNTRL_GPU_I2C_ADDR").unwrap_or_else(|_| "0x62".to_string());
    let addr = if let Some(h) = addr_raw
        .trim()
        .strip_prefix("0x")
        .or_else(|| addr_raw.trim().strip_prefix("0X"))
    {
        u16::from_str_radix(h, 16).map_err(|e| format!("invalid CNTRL_GPU_I2C_ADDR: {e}"))?
    } else {
        addr_raw
            .trim()
            .parse::<u16>()
            .map_err(|e| format!("invalid CNTRL_GPU_I2C_ADDR: {e}"))?
    };
    if addr > 0x7f {
        return Err(format!("CNTRL_GPU_I2C_ADDR out of 7-bit range: {addr:#x}"));
    }
    let brightness = std::env::var("CNTRL_GPU_I2C_BRIGHTNESS")
        .ok()
        .and_then(|s| s.trim().parse::<u8>().ok())
        .unwrap_or(0x63);
    let speed = std::env::var("CNTRL_GPU_I2C_SPEED")
        .ok()
        .and_then(|s| s.trim().parse::<u8>().ok())
        .unwrap_or(0x02);
    let zones = std::env::var("CNTRL_GPU_I2C_ZONES")
        .ok()
        .and_then(|s| s.trim().parse::<u8>().ok())
        .unwrap_or(5)
        .clamp(1, 8);
    let mode_static = 0x01u8;
    let mystery_flag = 0x00u8;

    let mut dev = LinuxI2CDevice::new(bus.as_str(), addr).map_err(|e| e.to_string())?;

    for zone in 0..zones {
        let pkt = [0x88, mode_static, speed, brightness, mystery_flag, zone + 1, 0x00, 0x00];
        dev.write(&pkt).map_err(|e| e.to_string())?;
    }

    if zones >= 2 {
        // OpenRGB fusion2 path: zone 0/1 share B0 register.
        let p = [0xB0, mode_static, r, g, b, r, g, b];
        dev.write(&p).map_err(|e| e.to_string())?;
    }
    if zones >= 3 {
        // OpenRGB fusion2 path: zone 2 uses B1 register.
        let p = [0xB1, mode_static, r, g, b, 0x00, 0x00, 0x00];
        dev.write(&p).map_err(|e| e.to_string())?;
    }
    for zone in 3..zones {
        // Remaining zones use REG_COLOR bank with zone selector.
        let p = [0x40, r, g, b, zone + 1, 0x00, 0x00, 0x00];
        dev.write(&p).map_err(|e| e.to_string())?;
    }
    // Persist/commit on device (mirrors OpenRGB SaveConfig).
    let save = [0xAA, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00];
    dev.write(&save).map_err(|e| e.to_string())?;

    Ok(format!(
        "gpu_rgb applied via native i2c (bus={bus}, addr={addr:#04x}, zones={zones})"
    ))
}

#[cfg(not(target_os = "linux"))]
fn write_native_fusion2(_r: u8, _g: u8, _b: u8) -> Result<String, String> {
    Err("native gpu i2c path is only supported on linux".to_string())
}

// ── canvas streaming (persistent direct session) ─────────────────────

/// Persistent session for canvas streaming: bus stays open, zone-mode setup
/// runs once, then `write_color` only touches the color registers per frame.
/// Deliberately never sends the 0xAA save/commit — that persists to device
/// flash and must not run tens of times per second.
#[cfg(target_os = "linux")]
pub struct GpuDirect {
    dev: i2cdev::linux::LinuxI2CDevice,
    zones: u8,
    bus: String,
    last: Option<(u8, u8, u8)>,
}

#[cfg(target_os = "linux")]
impl GpuDirect {
    pub fn open() -> Result<Self, String> {
        use i2cdev::core::I2CDevice;
        use i2cdev::linux::LinuxI2CDevice;

        let bus = std::env::var("CNTRL_GPU_I2C_BUS")
            .ok()
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .or_else(detect_default_gpu_i2c_bus)
            .ok_or_else(|| "no AMDGPU OEM i2c bus found".to_string())?;
        let addr = 0x62u16;
        let brightness = 0x63u8;
        let speed = 0x02u8;
        let zones = std::env::var("CNTRL_GPU_I2C_ZONES")
            .ok()
            .and_then(|s| s.trim().parse::<u8>().ok())
            .unwrap_or(5)
            .clamp(1, 8);

        let mut dev = LinuxI2CDevice::new(bus.as_str(), addr).map_err(|e| e.to_string())?;
        for zone in 0..zones {
            let pkt = [0x88, 0x01, speed, brightness, 0x00, zone + 1, 0x00, 0x00];
            dev.write(&pkt).map_err(|e| e.to_string())?;
        }
        Ok(Self {
            dev,
            zones,
            bus,
            last: None,
        })
    }

    pub fn bus(&self) -> &str {
        &self.bus
    }

    /// Update all zones to one color (skips the bus if unchanged).
    pub fn write_color(&mut self, r: u8, g: u8, b: u8) -> Result<(), String> {
        use i2cdev::core::I2CDevice;
        if self.last == Some((r, g, b)) {
            return Ok(());
        }
        if self.zones >= 2 {
            let p = [0xB0, 0x01, r, g, b, r, g, b];
            self.dev.write(&p).map_err(|e| e.to_string())?;
        }
        if self.zones >= 3 {
            let p = [0xB1, 0x01, r, g, b, 0x00, 0x00, 0x00];
            self.dev.write(&p).map_err(|e| e.to_string())?;
        }
        for zone in 3..self.zones {
            let p = [0x40, r, g, b, zone + 1, 0x00, 0x00, 0x00];
            self.dev.write(&p).map_err(|e| e.to_string())?;
        }
        self.last = Some((r, g, b));
        Ok(())
    }
}

pub fn apply_profile(doc: &ProfileDocument) -> Result<String, String> {
    use crate::lighting_modes::{EFFECT_ID_OFF, MODE_CUSTOM, MODE_STATIC};

    let effect = doc.data.light.config.effect;
    if effect != MODE_STATIC && effect != MODE_CUSTOM && effect != EFFECT_ID_OFF {
        return Err(format!(
            "unsupported effect id {effect} for gpu_rgb backend (supports static/custom/off only)"
        ));
    }

    let (mut r, mut g, mut b) = profile_rgb(doc)?;
    if effect == EFFECT_ID_OFF {
        r = 0;
        g = 0;
        b = 0;
    }

    write_native_fusion2(r, g, b).map_err(|native_err| format!("gpu native apply failed: {native_err}"))
}

