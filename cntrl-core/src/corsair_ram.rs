//! Corsair DDR5 RAM RGB backend — native SMBus/I2C only.
//!
//! Detection: reads register 0x43 (expects 0x1A/0x1B/0x1C) and 0x44
//! (expects 0x03 or 0x04) on each candidate address to confirm Corsair
//! DRAM RGB controller presence.
//!
//! Protocol: For protocol_version ≥ 4 (DDR5 Vengeance/Dominator), uses
//! direct block writes to registers 0x31/0x32. Falls back to byte-at-a-time
//! binary buffer writes (0x0B/0x21/0x20/0x42/0x82) when block writes are
//! unsupported by the SMBus adapter.
//!
//! Bus discovery: scans /sys/bus/i2c/devices/i2c-*/name for "SMBus PIIX4"
//! adapters (AMD chipset SMBus), then probes addresses 0x18–0x1F on each.
//! Override with CNTRL_CORSAIR_RAM_I2C_BUS=/dev/i2c-18 if needed.

use crate::profile::ProfileDocument;
use std::path::Path;

/// Corsair DDR5 DRAM RGB candidate addresses (0x18–0x1F).
/// DDR4 used 0x58–0x5F; DDR5 shifted down.
const DDR5_ADDR_LO: u16 = 0x18;
const DDR5_ADDR_HI: u16 = 0x1F;

/// Default LED count for Corsair Vengeance DDR5.
const DEFAULT_LED_COUNT: u8 = 10;

// ── helpers ──────────────────────────────────────────────────────────

fn profile_rgb(doc: &ProfileDocument) -> Result<(u8, u8, u8), String> {
    let t = doc.data.light.config.hex_color.trim().trim_start_matches('#');
    if t.len() != 6 {
        return Err(format!(
            "invalid hexColor {:?}, expected #rrggbb",
            doc.data.light.config.hex_color
        ));
    }
    let r = u8::from_str_radix(&t[0..2], 16).map_err(|e| e.to_string())?;
    let g = u8::from_str_radix(&t[2..4], 16).map_err(|e| e.to_string())?;
    let b = u8::from_str_radix(&t[4..6], 16).map_err(|e| e.to_string())?;
    Ok((r, g, b))
}

// ── Linux SMBus implementation ───────────────────────────────────────

#[cfg(target_os = "linux")]
fn crc8(data: &[u8]) -> u8 {
    let mut crc = 0u8;
    for &byte in data {
        crc ^= byte;
        for _ in 0..8 {
            if (crc & 0x80) != 0 {
                crc = (crc << 1) ^ 0x07;
            } else {
                crc <<= 1;
            }
        }
    }
    crc
}

/// Find SMBus PIIX4 adapter bus paths (AMD chipset).
/// Returns e.g. ["/dev/i2c-18", "/dev/i2c-19", "/dev/i2c-20"].
#[cfg(target_os = "linux")]
fn find_piix4_busses() -> Vec<String> {
    // Allow explicit override.
    if let Ok(bus) = std::env::var("CNTRL_CORSAIR_RAM_I2C_BUS") {
        let t = bus.trim();
        if !t.is_empty() {
            let path = if t.starts_with("/dev/") {
                t.to_string()
            } else {
                format!("/dev/i2c-{t}")
            };
            eprintln!("[corsair_ram] using override bus: {path}");
            return vec![path];
        }
    }

    let mut out = Vec::new();
    let sys = Path::new("/sys/bus/i2c/devices");
    let Ok(entries) = std::fs::read_dir(sys) else {
        return out;
    };
    for entry in entries.flatten() {
        let dirname = entry.file_name().to_string_lossy().into_owned();
        // We want adapter entries like "i2c-18", not device entries like "18-0051".
        if !dirname.starts_with("i2c-") {
            continue;
        }
        let name_path = entry.path().join("name");
        let Ok(name) = std::fs::read_to_string(&name_path) else {
            continue;
        };
        let name = name.trim();
        // AMD chipset SMBus adapters identify as "SMBus PIIX4 adapter ...".
        if name.contains("SMBus PIIX4") || name.contains("piix4_smbus") {
            let dev_path = format!("/dev/{dirname}");
            if Path::new(&dev_path).exists() {
                eprintln!("[corsair_ram] found PIIX4 bus: {dev_path} ({name})");
                out.push(dev_path);
            }
        }
    }
    out.sort();
    out
}

/// Presence/health of the Corsair RAM target, for the Devices view.
pub enum RamProbe {
    Ready { modules: usize, detail: String },
    Blocked(String),
    Absent(String),
}

/// Read-only probe: scan PIIX4 SMBus buses for Corsair DDR5 modules. No writes.
#[cfg(target_os = "linux")]
pub fn probe() -> RamProbe {
    let buses = find_piix4_busses();
    if buses.is_empty() {
        return RamProbe::Absent(
            "no SMBus PIIX4 adapter — needs acpi_enforce_resources=lax".to_string(),
        );
    }
    let mut perm_denied = false;
    let mut modules: Vec<String> = Vec::new();
    for bus in &buses {
        for addr in DDR5_ADDR_LO..=DDR5_ADDR_HI {
            match open_i2c(bus, addr, true) {
                Ok(mut dev) => {
                    if detect_corsair(&mut dev).is_some() {
                        modules.push(format!("{bus}@0x{addr:02x}"));
                    }
                }
                Err(e) => {
                    if e.contains("Permission denied") {
                        perm_denied = true;
                    }
                }
            }
        }
    }
    if !modules.is_empty() {
        return RamProbe::Ready {
            modules: modules.len(),
            detail: format!("{} module(s): {}", modules.len(), modules.join(", ")),
        };
    }
    if perm_denied {
        return RamProbe::Blocked(
            "/dev/i2c-* permission denied — add user to the 'i2c' group and re-login".to_string(),
        );
    }
    RamProbe::Absent("SMBus present but no Corsair DRAM detected".to_string())
}

#[cfg(not(target_os = "linux"))]
pub fn probe() -> RamProbe {
    RamProbe::Absent("corsair_ram requires Linux".to_string())
}

/// Open an I2C device. Uses I2C_SLAVE_FORCE when `force` is true to
/// bypass kernel driver claims (needed if spd5118 is bound).
#[cfg(target_os = "linux")]
fn open_i2c(bus: &str, addr: u16, force: bool) -> Result<i2cdev::linux::LinuxI2CDevice, String> {
    if force {
        unsafe { i2cdev::linux::LinuxI2CDevice::force_new(bus, addr) }
            .map_err(|e| format!("{bus}@0x{addr:02x}: {e}"))
    } else {
        i2cdev::linux::LinuxI2CDevice::new(bus, addr)
            .map_err(|e| format!("{bus}@0x{addr:02x}: {e}"))
    }
}

/// Check registers 0x43 and 0x44 to confirm a Corsair DRAM RGB controller.
/// Returns the protocol version byte (reg 0x44) on success.
#[cfg(target_os = "linux")]
fn detect_corsair(dev: &mut i2cdev::linux::LinuxI2CDevice) -> Option<u8> {
    use i2cdev::core::I2CDevice;
    let a = dev.smbus_read_byte_data(0x43).ok()?;
    if a != 0x1A && a != 0x1B && a != 0x1C {
        return None;
    }
    let b = dev.smbus_read_byte_data(0x44).ok()?;
    if b == 0x03 || b == 0x04 {
        Some(b)
    } else {
        None
    }
}

/// Wait for the DRAM controller to finish processing (bit 3 of status
/// register 0x30 clears when ready).
#[cfg(target_os = "linux")]
fn wait_ready(dev: &mut i2cdev::linux::LinuxI2CDevice) {
    use i2cdev::core::I2CDevice;
    for _ in 0..10 {
        match dev.smbus_read_byte_data(0x30) {
            Ok(s) if (s & 0x08) == 0 => return,
            _ => {}
        }
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
}

/// Write effect configuration (static mode) via the binary-buffer protocol.
/// This tells the DIMM to stop any running animated effect and accept
/// per-LED colors.
#[cfg(target_os = "linux")]
fn set_static_effect(
    dev: &mut i2cdev::linux::LinuxI2CDevice,
    r: u8,
    g: u8,
    b: u8,
) -> Result<(), String> {
    use i2cdev::core::I2CDevice;

    // Effect payload: mode=static, speed=medium, custom colors, no direction,
    // two color slots filled with the target color, full brightness.
    let mut buf = [0u8; 20];
    buf[0] = 0x10; // CORSAIR_DRAM_MODE_STATIC
    buf[1] = 0x01; // speed: medium
    buf[2] = 0x01; // color_mode: custom (not random)
    // buf[3] = 0x00; direction (already zero)
    buf[4] = r;
    buf[5] = g;
    buf[6] = b;
    buf[7] = 0xFF; // brightness
    buf[8] = r;
    buf[9] = g;
    buf[10] = b;
    buf[11] = 0xFF; // brightness

    // Binary-buffer write: reset → start → data bytes → CRC verify → commit.
    dev.smbus_write_byte_data(0x0B, 0x00)
        .map_err(|e| format!("effect reset: {e}"))?;
    dev.smbus_write_byte_data(0x21, 0x00)
        .map_err(|e| format!("effect start: {e}"))?;
    for &byte in &buf {
        dev.smbus_write_byte_data(0x20, byte)
            .map_err(|e| format!("effect data: {e}"))?;
    }

    let calc_crc = crc8(&buf);
    let dev_crc = dev
        .smbus_read_byte_data(0x42)
        .map_err(|e| format!("effect crc read: {e}"))?;
    if calc_crc != dev_crc {
        return Err(format!(
            "effect CRC mismatch (calc=0x{calc_crc:02x}, dev=0x{dev_crc:02x})"
        ));
    }

    // 0x82 = write configuration; 0x01 = effect config type.
    dev.smbus_write_byte_data(0x82, 0x01)
        .map_err(|e| format!("effect commit: {e}"))?;
    wait_ready(dev);
    Ok(())
}

/// Set per-LED colors using protocol ≥ 4 direct block write (0x31/0x32).
#[cfg(target_os = "linux")]
fn set_colors_block(
    dev: &mut i2cdev::linux::LinuxI2CDevice,
    led_count: u8,
    r: u8,
    g: u8,
    b: u8,
) -> Result<(), String> {
    use i2cdev::core::I2CDevice;

    // Payload: [led_count, R,G,B, R,G,B, …, CRC8]
    let mut payload = Vec::with_capacity((led_count as usize) * 3 + 2);
    payload.push(led_count);
    for _ in 0..led_count {
        payload.push(r);
        payload.push(g);
        payload.push(b);
    }
    payload.push(crc8(&payload[..payload.len()]));

    let first = payload.len().min(32);
    dev.smbus_write_block_data(0x31, &payload[..first])
        .map_err(|e| format!("block write 0x31: {e}"))?;
    if payload.len() > 32 {
        dev.smbus_write_block_data(0x32, &payload[32..])
            .map_err(|e| format!("block write 0x32: {e}"))?;
    }
    Ok(())
}

/// Fallback: set per-LED colors via binary-buffer byte-at-a-time protocol.
/// Used when the SMBus adapter doesn't support block writes.
#[cfg(target_os = "linux")]
fn set_colors_buffer(
    dev: &mut i2cdev::linux::LinuxI2CDevice,
    led_count: u8,
    r: u8,
    g: u8,
    b: u8,
) -> Result<(), String> {
    use i2cdev::core::I2CDevice;

    // Same payload format as block write: [led_count, R,G,B × N, CRC8]
    let mut payload = Vec::with_capacity((led_count as usize) * 3 + 2);
    payload.push(led_count);
    for _ in 0..led_count {
        payload.push(r);
        payload.push(g);
        payload.push(b);
    }
    payload.push(crc8(&payload[..payload.len()]));

    dev.smbus_write_byte_data(0x0B, 0x00)
        .map_err(|e| format!("color reset: {e}"))?;
    dev.smbus_write_byte_data(0x21, 0x00)
        .map_err(|e| format!("color start: {e}"))?;
    for &byte in &payload {
        dev.smbus_write_byte_data(0x20, byte)
            .map_err(|e| format!("color data: {e}"))?;
    }

    let calc_crc = crc8(&payload);
    let dev_crc = dev
        .smbus_read_byte_data(0x42)
        .map_err(|e| format!("color crc read: {e}"))?;
    if calc_crc != dev_crc {
        return Err(format!(
            "color CRC mismatch (calc=0x{calc_crc:02x}, dev=0x{dev_crc:02x})"
        ));
    }

    // 0x82 = write configuration; 0x01 = color data type.
    dev.smbus_write_byte_data(0x82, 0x01)
        .map_err(|e| format!("color commit: {e}"))?;
    wait_ready(dev);
    Ok(())
}

/// Write a solid color to a single DIMM. Tries block write first, then
/// falls back to binary buffer. Effect (static mode) is set first with a
/// generous delay to let the controller finish processing before the
/// color write.
#[cfg(target_os = "linux")]
fn write_dimm_color(
    dev: &mut i2cdev::linux::LinuxI2CDevice,
    led_count: u8,
    r: u8,
    g: u8,
    b: u8,
    bus_addr: &str,
) -> Result<(), String> {
    // 1. Set the DIMM to static mode so it stops any animated effect.
    //    After committing via 0x82 the controller needs time to process;
    //    wait_ready + a fixed delay prevents the color write from being
    //    NACKed.
    if let Err(e) = set_static_effect(dev, r, g, b) {
        eprintln!("[corsair_ram] {bus_addr}: effect write failed ({e}), trying colors anyway");
    } else {
        // Extra settle time — the DRAM controller may NACK I2C for ~50-100 ms
        // after committing effect config.
        std::thread::sleep(std::time::Duration::from_millis(100));
    }

    // 2. Write per-LED colors. Try block write first (fast), fall back to
    //    byte-at-a-time buffer write if the adapter rejects block data.
    match set_colors_block(dev, led_count, r, g, b) {
        Ok(()) => {
            eprintln!("[corsair_ram] {bus_addr}: block write OK ({led_count} LEDs)");
            Ok(())
        }
        Err(block_err) => {
            eprintln!(
                "[corsair_ram] {bus_addr}: block write failed ({block_err}), trying buffer path"
            );
            set_colors_buffer(dev, led_count, r, g, b)
        }
    }
}

/// Scan PIIX4 SMBus adapters, detect Corsair DRAM controllers, and set color.
#[cfg(target_os = "linux")]
fn apply_i2c(r: u8, g: u8, b: u8) -> Result<String, String> {
    let led_count = std::env::var("CNTRL_CORSAIR_RAM_LED_COUNT")
        .ok()
        .and_then(|s| s.trim().parse::<u8>().ok())
        .unwrap_or(DEFAULT_LED_COUNT);

    let busses = find_piix4_busses();
    if busses.is_empty() {
        return Err(
            "no SMBus PIIX4 adapters found; set CNTRL_CORSAIR_RAM_I2C_BUS=/dev/i2c-N".to_string(),
        );
    }

    let mut touched = Vec::new();
    let mut errors = Vec::new();

    for bus in &busses {
        for addr in DDR5_ADDR_LO..=DDR5_ADDR_HI {
            let mut dev = match open_i2c(bus, addr, false) {
                Ok(d) => d,
                Err(_) => {
                    // Try force-open in case a kernel driver (spd5118) claims it.
                    match open_i2c(bus, addr, true) {
                        Ok(d) => d,
                        Err(e) => {
                            errors.push(e);
                            continue;
                        }
                    }
                }
            };

            let proto = match detect_corsair(&mut dev) {
                Some(v) => v,
                None => continue,
            };

            let tag = format!("{bus}@0x{addr:02x}(proto={proto})");
            eprintln!("[corsair_ram] detected: {tag}");

            match write_dimm_color(&mut dev, led_count, r, g, b, &tag) {
                Ok(()) => touched.push(tag),
                Err(e) => errors.push(format!("{tag}: {e}")),
            }
        }
    }

    if touched.is_empty() {
        if errors.is_empty() {
            return Err(
                "no Corsair DRAM controllers found on PIIX4 SMBus; \
                 set CNTRL_CORSAIR_RAM_I2C_BUS if bus number differs"
                    .to_string(),
            );
        }
        return Err(format!(
            "Corsair DRAM detected but write failed: {}",
            errors.join(" | ")
        ));
    }

    Ok(format!(
        "corsair_ram: color #{r:02x}{g:02x}{b:02x} → [{}]",
        touched.join(", ")
    ))
}

// ── public entry point ───────────────────────────────────────────────

pub fn apply_profile(doc: &ProfileDocument) -> Result<String, String> {
    use crate::lighting_modes::{EFFECT_ID_OFF, MODE_CUSTOM, MODE_STATIC};

    let effect = doc.data.light.config.effect;
    if effect != MODE_STATIC && effect != MODE_CUSTOM && effect != EFFECT_ID_OFF {
        return Err(format!(
            "unsupported effect id {effect} for corsair_ram (static/custom/off only)"
        ));
    }

    let (mut r, mut g, mut b) = profile_rgb(doc)?;
    if effect == EFFECT_ID_OFF {
        r = 0;
        g = 0;
        b = 0;
    }

    #[cfg(target_os = "linux")]
    {
        return apply_i2c(r, g, b);
    }

    #[cfg(not(target_os = "linux"))]
    Err("corsair_ram backend requires Linux (SMBus I2C)".to_string())
}

