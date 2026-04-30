//! Linux: HID access for MCHOSE Ace68-II (VID 0x41e4, PID 0x2116).
//!
//! ## What is implemented
//! **Lighting** is pushed to the keyboard using the same 64-byte `0x55` / `hidraw` pipeline as
//! CNTRL_Python (`scripts/set_rgb_rw.py`, `protocol.md`): `mchose_lighting` runs on **HID interface 1**.
//!
//! ## Not implemented (no Python reference in CNTRL_Python)
//! Full **KeyboardProfile** upload (keymap, macros, performance blocks as 519-byte `wiredCommand` /
//! multi-packet `0x07` sessions) is **not** specified in the repo; `protocol.md` marks macro/keymap
//! flows as unverified. Those remain local-only in JSON.

use crate::mchose_lighting;
use crate::profile::ProfileDocument;
use crate::hid::HidInterfaceDiagnostic;
use crate::lighting_modes::{self, EFFECT_ID_OFF, MODE_CUSTOM, MODE_STATIC};

const VENDOR_ID: u16 = 0x41e4;
const PRODUCT_ID: u16 = 0x2116;

fn bytes_to_hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{:02x}", b)).collect::<Vec<_>>().join("")
}

/// Resolved hidraw path and/or resolution error (for status bar and Lighting diagnostics).
pub fn vendor_hidraw_path_status() -> (Option<String>, Option<String>) {
    match mchose_lighting::vendor_hidraw_path() {
        Ok(p) => (Some(p.to_string_lossy().into_owned()), None),
        Err(e) => (None, Some(e)),
    }
}

pub fn keyboard_connected() -> bool {
    hidapi::HidApi::new()
        .map(|api| {
            api.device_list()
                .any(|d| d.vendor_id() == VENDOR_ID && d.product_id() == PRODUCT_ID)
        })
        .unwrap_or(false)
}

pub fn read_profile_from_device() -> Result<ProfileDocument, String> {
    // TODO: READ_LIGHT_V2 / F1 fragments — until then same default as mock.
    Ok(crate::profile::default_profile_document())
}

/// Push **lighting** from the in-memory profile (effect, color, brightness, direct).
///
/// Always attempts USB (same as CNTRL_Python `set_rgb_rw.py`). Do **not** gate on
/// `keyboard_connected()` — hidapi enumeration can be empty while `/dev/hidraw*` still works
/// after udev rules, so skipping here made “Apply effect” a silent no-op.
///
/// See `push_profile_lighting` for effect-id rules.
pub fn write_profile_to_device(profile: &ProfileDocument) -> Result<(), String> {
    push_profile_lighting(profile)
}

fn parse_hex_rgb(s: &str) -> Result<(u8, u8, u8), String> {
    let t = s.trim().strip_prefix('#').unwrap_or(s.trim());
    if t.len() != 6 || !t.chars().all(|c| c.is_ascii_hexdigit()) {
        return Err(format!(
            "light.config.hexColor must be #rrggbb (got {s:?})"
        ));
    }
    let v = u32::from_str_radix(t, 16).map_err(|_| format!("invalid hex color {s:?}"))?;
    Ok(((v >> 16) as u8, ((v >> 8) & 0xff) as u8, (v & 0xff) as u8))
}

fn rgb_u32(r: u8, g: u8, b: u8) -> u32 {
    (u32::from(r) << 16) | (u32::from(g) << 8) | u32::from(b)
}

/// Maps `ProfileDocument.data.light.config` to CNTRL_Python `run_pipeline` / `set_effect` semantics.
///
/// - `MODE_STATIC`/`MODE_CUSTOM` → solid DIY (`set_solid_rgb`).
/// - Legacy **0** → treated as custom for backward compatibility.
/// - `EFFECT_ID_OFF` → solid black.
/// - Preset ids from `lighting_modes::PRESET_MODES` → built-in `<light@v2>` templates.
///
/// UI brightness 0–100 is scaled to 0–255 for preset effects only. Speed is not on the wire yet.
fn push_profile_lighting(profile: &ProfileDocument) -> Result<(), String> {
    let cfg = &profile.data.light.config;
    let eff = cfg.effect;
    let (r, g, b) = parse_hex_rgb(&cfg.hex_color)?;
    let brightness_u8 = ((cfg.brightness.clamp(0, 100) as u32 * 255) / 100) as u8;
    let direct = cfg.direct != 0;
    let speed = None;

    if eff == MODE_STATIC {
        return mchose_lighting::set_solid_rgb(r, g, b, 255, speed, direct);
    }
    if eff == EFFECT_ID_OFF {
        return mchose_lighting::set_solid_rgb(0, 0, 0, 255, speed, direct);
    }
    if eff == MODE_CUSTOM || eff == 0 {
        return mchose_lighting::set_solid_rgb(r, g, b, 255, speed, direct);
    }
    if lighting_modes::is_preset_effect_id(eff) {
        return mchose_lighting::set_effect(
            eff as u8,
            rgb_u32(r, g, b),
            brightness_u8,
            speed,
            direct,
        );
    }

    Err(format!(
        "Lighting effect id {} cannot be sent over HID (use mapped presets, MODE_STATIC={}, MODE_CUSTOM={}, OFF={}). \
         Keymap, macros, and performance are not transferred.",
        eff,
        MODE_STATIC,
        MODE_CUSTOM,
        EFFECT_ID_OFF
    ))
}

/// `true`: lighting is pushed over USB on save (Linux). Other profile sections remain local.
pub fn profile_updates_reach_keyboard() -> bool {
    true
}

/// Enumerate HID interfaces for this VID/PID and read each **report descriptor** (opens each path).
pub fn hid_diagnostics() -> Result<Vec<HidInterfaceDiagnostic>, String> {
    let api = hidapi::HidApi::new().map_err(|e| e.to_string())?;
    let mut out = Vec::new();
    for d in api.device_list() {
        if d.vendor_id() != VENDOR_ID || d.product_id() != PRODUCT_ID {
            continue;
        }
        let path = d.path().to_str().unwrap_or("").to_string();
        let product_string = d.product_string().map(str::to_string);
        let interface_number = d.interface_number();
        let mut open_or_read_error = None;
        let mut report_descriptor_hex = None;
        match d.open_device(&api) {
            Ok(dev) => {
                let mut buf = vec![0u8; hidapi::MAX_REPORT_DESCRIPTOR_SIZE];
                match dev.get_report_descriptor(&mut buf) {
                    Ok(n) => {
                        report_descriptor_hex = Some(bytes_to_hex(&buf[..n]));
                    }
                    Err(e) => {
                        open_or_read_error = Some(format!("get_report_descriptor: {e}"));
                    }
                }
            }
            Err(e) => {
                open_or_read_error = Some(format!("open: {e}"));
            }
        }
        out.push(HidInterfaceDiagnostic {
            path,
            interface_number,
            product_string,
            report_descriptor_hex,
            open_or_read_error,
        });
    }
    Ok(out)
}
