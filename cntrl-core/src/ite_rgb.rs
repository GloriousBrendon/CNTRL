//! ITE motherboard/case RGB backend (M5.4 starter, used by M5.3 validation flow).
//!
//! Safety model mirrors other backends:
//! - strict VID/PID targeting
//! - optional interface pinning
//! - no hardcoded guessed protocol payloads
//! - packet templates are user-provided via env vars

use crate::profile::ProfileDocument;

const ITE_VID: u16 = 0x048d;
const ITE_PID: u16 = 0x5702;

#[derive(Debug, Clone)]
pub struct IteProbe {
    pub path: String,
    pub interface_number: i32,
    pub manufacturer: Option<String>,
    pub product: Option<String>,
    pub serial: Option<String>,
    pub score: i32,
}

fn score_probe(interface_number: i32, product: Option<&str>, preferred_interface: Option<i32>) -> i32 {
    let mut score = 0;
    if let Some(pref) = preferred_interface {
        if interface_number == pref {
            score += 100;
        } else {
            score -= 15;
        }
    }
    if let Some(p) = product {
        let l = p.to_ascii_lowercase();
        if l.contains("ite") {
            score += 25;
        }
    }
    score
}

pub fn discover_candidates() -> Result<Vec<IteProbe>, String> {
    let api = hidapi::HidApi::new().map_err(|e| e.to_string())?;
    let preferred_interface = std::env::var("CNTRL_ITE_INTERFACE")
        .ok()
        .and_then(|s| s.trim().parse::<i32>().ok());
    let mut out = Vec::new();
    for d in api.device_list() {
        if d.vendor_id() != ITE_VID || d.product_id() != ITE_PID {
            continue;
        }
        out.push(IteProbe {
            path: d.path().to_string_lossy().into_owned(),
            interface_number: d.interface_number(),
            manufacturer: d.manufacturer_string().map(str::to_string),
            product: d.product_string().map(str::to_string),
            serial: d.serial_number().map(str::to_string),
            score: score_probe(
                d.interface_number(),
                d.product_string(),
                preferred_interface,
            ),
        });
    }
    out.sort_by(|a, b| b.score.cmp(&a.score).then(a.path.cmp(&b.path)));
    Ok(out)
}

fn hex_nibble(c: char) -> Option<u8> {
    match c {
        '0'..='9' => Some((c as u8) - b'0'),
        'a'..='f' => Some((c as u8) - b'a' + 10),
        'A'..='F' => Some((c as u8) - b'A' + 10),
        _ => None,
    }
}

fn parse_hex_byte(tok: &str) -> Result<u8, String> {
    if tok.len() != 2 {
        return Err(format!("token {tok:?} must be exactly two hex chars"));
    }
    let mut ch = tok.chars();
    let hi = hex_nibble(ch.next().unwrap()).ok_or_else(|| format!("invalid hex token {tok:?}"))?;
    let lo = hex_nibble(ch.next().unwrap()).ok_or_else(|| format!("invalid hex token {tok:?}"))?;
    Ok((hi << 4) | lo)
}

fn expand_packet_template(template: &str, r: u8, g: u8, b: u8) -> Result<Vec<u8>, String> {
    let mut out = Vec::new();
    for tok in template.split_whitespace() {
        let byte = match tok {
            "rr" | "RR" => r,
            "gg" | "GG" => g,
            "bb" | "BB" => b,
            _ => parse_hex_byte(tok)?,
        };
        out.push(byte);
    }
    if out.is_empty() {
        return Err("packet template produced no bytes".to_string());
    }
    Ok(out)
}

fn normalize_ite_packet(packet: &[u8]) -> Result<[u8; 64], String> {
    // Accept either a native 64-byte feature payload or a 65-byte
    // report-id-prefixed payload commonly produced by manual captures.
    let bytes: &[u8] = match packet.len() {
        64 => packet,
        65 => {
            if packet[0] == 0x00 || packet[0] == 0xCC {
                &packet[1..]
            } else {
                return Err(format!(
                    "ITE 65-byte packet has unexpected leading byte {:02X} (expected 00/CC)",
                    packet[0]
                ));
            }
        }
        n => {
            return Err(format!(
                "ITE packet must be 64 bytes (or 65 with report-id prefix), got {n}"
            ));
        }
    };

    let mut out = [0u8; 64];
    out.copy_from_slice(bytes);
    Ok(out)
}

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

fn write_ite_packet(dev: &hidapi::HidDevice, packet: &[u8]) -> Result<(), String> {
    let packet = normalize_ite_packet(packet)?;

    // OpenRGB traces for this controller show HIDIOCSFEATURE(64) writes.
    // Try exact 64-byte payload first (matching ioctl size), then a
    // report-id-prefixed variant, then plain interrupt write fallback.
    if dev.send_feature_report(&packet).is_ok() {
        return Ok(());
    }
    let mut prefixed = Vec::with_capacity(packet.len() + 1);
    prefixed.push(0x00);
    prefixed.extend_from_slice(&packet);
    if dev.send_feature_report(&prefixed).is_ok() {
        return Ok(());
    }
    dev.write(&packet).map(|_| ()).map_err(|e| e.to_string())
}

fn cc_report(cmd: u8, b: u8, c: u8) -> [u8; 64] {
    let mut p = [0u8; 64];
    p[0] = 0xCC;
    p[1] = cmd;
    p[2] = b;
    p[3] = c;
    p
}

fn run_openrgb_prelude(dev: &hidapi::HidDevice) -> Result<(), String> {
    // Mirror OpenRGB GigabyteRGBFusion2USB bring-up:
    // - CC 60 + get_feature_report
    // - optional CC 48 (lamp array off)
    // - reset CC20..CC27 + apply CC28 + beat off CC31
    write_ite_packet(dev, &cc_report(0x60, 0x00, 0x00))?;

    let mut report = [0u8; 64];
    report[0] = 0xCC;
    let _ = dev.get_feature_report(&mut report).map_err(|e| e.to_string())?;

    let support_cmd_flag = report[8];
    if support_cmd_flag >= 0x02 {
        write_ite_packet(dev, &cc_report(0x48, 0x00, 0x00))?;
    }

    for reg in 0x20u8..=0x27u8 {
        write_ite_packet(dev, &cc_report(reg, 0x00, 0x00))?;
    }
    // fast apply for 0x5702 path in OpenRGB
    write_ite_packet(dev, &cc_report(0x28, 0xFF, 0x00))?;
    write_ite_packet(dev, &cc_report(0x31, 0x00, 0x00))?;
    // Strip calibration/LED-count config, present in every OpenRGB bring-up
    // (captures/openrgb/*_ite_1_3_0_payloads.txt): cc 32 1b + cc 34 10.
    write_ite_packet(dev, &cc_report(0x32, 0x1B, 0x00))?;
    write_ite_packet(dev, &cc_report(0x34, 0x10, 0x00))?;
    Ok(())
}

fn send_openrgb_static_effect(dev: &hidapi::HidDevice, r: u8, g: u8, b: u8) -> Result<(), String> {
    // The firmware keeps a per-zone effect config: writing register 0x20
    // with an all-zones mask only configures zone 0, so zones like the 12V
    // RGB header never picked up solid applies. Mirror OpenRGB's all-zones
    // loop instead: one effect packet per zone register, then one apply.
    for zone in 0u8..=5 {
        let mut effect = [0u8; 64];
        effect[0] = 0xCC; // report id
        effect[1] = 0x20 + zone; // per-zone effect register
        effect[2] = 1 << zone; // this zone's bitmask (low byte, LE u32)
        effect[11] = 0x01; // static effect
        effect[12] = 0xFF; // max brightness
        effect[14] = b; // color0 blue (BGR32 little-endian)
        effect[15] = g; // color0 green
        effect[16] = r; // color0 red
        write_ite_packet(dev, &effect)?;
    }

    // OpenRGB ApplyEffect(false): command 0x28 with zone mask.
    let mut apply = [0u8; 64];
    apply[0] = 0xCC;
    apply[1] = 0x28;
    apply[2] = 0xFF;
    write_ite_packet(dev, &apply)?;
    Ok(())
}

/// Max LEDs per 64-byte strip packet (5-byte header + 19×3 color bytes = 62).
const STRIP_LEDS_PER_PACKET: usize = 19;

/// Per-LED strip data packet. `colors` are already in wire order.
fn build_openrgb_strip_packet_multi(header: u8, offset_bytes: u16, colors: &[[u8; 3]]) -> [u8; 64] {
    let count = colors.len().min(STRIP_LEDS_PER_PACKET);
    let mut p = [0u8; 64];
    p[0] = 0xCC;
    p[1] = header;
    p[2] = (offset_bytes & 0x00FF) as u8;
    p[3] = (offset_bytes >> 8) as u8;
    p[4] = (count * 3) as u8;
    let mut idx = 5usize;
    for c in colors.iter().take(count) {
        p[idx] = c[0];
        p[idx + 1] = c[1];
        p[idx + 2] = c[2];
        idx += 3;
    }
    p
}

fn build_openrgb_strip_packet(header: u8, offset_bytes: u16, led_count: u8, r: u8, g: u8, b: u8) -> [u8; 64] {
    let colors = vec![[r, g, b]; led_count as usize];
    build_openrgb_strip_packet_multi(header, offset_bytes, &colors)
}

fn parse_strip_color_order() -> [usize; 3] {
    // Fan strips are often GRB. Keep this overrideable for board/fan variance.
    // Supported values: RGB, RBG, GRB, GBR, BRG, BGR.
    let raw = std::env::var("CNTRL_ITE_STRIP_COLOR_ORDER")
        .unwrap_or_else(|_| "GRB".to_string())
        .trim()
        .to_ascii_uppercase();
    match raw.as_str() {
        "RGB" => [0, 1, 2],
        "RBG" => [0, 2, 1],
        "GRB" => [1, 0, 2],
        "GBR" => [1, 2, 0],
        "BRG" => [2, 0, 1],
        "BGR" => [2, 1, 0],
        _ => [1, 0, 2], // safe default for most ARGB fan strips
    }
}

fn parse_led_counts_env() -> Option<(u8, u8)> {
    let raw = std::env::var("CNTRL_ITE_STRIP_LED_COUNTS").ok()?;
    let mut parts = raw.split(',').map(str::trim);
    let a = parts.next()?.parse::<u8>().ok()?;
    let b = parts.next()?.parse::<u8>().ok()?;
    if parts.next().is_some() || a == 0 || b == 0 {
        return None;
    }
    Some((a, b))
}

fn send_openrgb_strip_direct_colors(dev: &hidapi::HidDevice, r: u8, g: u8, b: u8) -> Result<(), String> {
    // OpenRGB direct writes for IT5702 commonly use headers 0x58/0x59
    // (D_LED1/D_LED2 ARGB paths), typically with totals 32 and 36 LEDs.
    // Override via `CNTRL_ITE_STRIP_LED_COUNTS=dled1_total,dled2_total`.
    let (dled1_total, dled2_total) = parse_led_counts_env().unwrap_or((32, 36));
    let order = parse_strip_color_order();
    let rgb = [r, g, b];
    let (sr, sg, sb) = (rgb[order[0]], rgb[order[1]], rgb[order[2]]);
    for (header, total) in [(0x58u8, dled1_total), (0x59u8, dled2_total)] {
        let first = total.min(19);
        let second = total.saturating_sub(first);
        let p0 = build_openrgb_strip_packet(header, 0, first, sr, sg, sb);
        write_ite_packet(dev, &p0)?;
        if second > 0 {
            let p1 = build_openrgb_strip_packet(header, u16::from(first) * 3, second, sr, sg, sb);
            write_ite_packet(dev, &p1)?;
        }
    }
    Ok(())
}

pub fn apply_profile(doc: &ProfileDocument) -> Result<String, String> {
    use crate::lighting_modes::{EFFECT_ID_OFF, MODE_CUSTOM, MODE_STATIC};

    let effect = doc.data.light.config.effect;
    if effect != MODE_STATIC && effect != MODE_CUSTOM && effect != EFFECT_ID_OFF {
        return Err(format!(
            "unsupported effect id {effect} for ite_rgb backend (supports static/custom/off only)"
        ));
    }
    let (mut r, mut g, mut b) = profile_rgb(doc)?;
    if effect == EFFECT_ID_OFF {
        r = 0;
        g = 0;
        b = 0;
    }

    let apply_template = std::env::var("CNTRL_ITE_APPLY_TEMPLATE").unwrap_or_default();
    let init_templates = std::env::var("CNTRL_ITE_INIT_TEMPLATES").unwrap_or_default();
    let preferred_path = std::env::var("CNTRL_ITE_PATH")
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty());

    let candidates = discover_candidates()?;
    let selected = if let Some(p) = preferred_path.as_deref() {
        candidates
            .iter()
            .find(|c| c.path == p)
            .ok_or_else(|| {
                let all = candidates
                    .iter()
                    .map(|c| format!("{}(iface={})", c.path, c.interface_number))
                    .collect::<Vec<_>>()
                    .join(", ");
                format!("CNTRL_ITE_PATH={p} not found among candidates: [{all}]")
            })?
    } else {
        candidates
            .first()
            .ok_or_else(|| "no ITE HID candidates found (VID 048d PID 5702)".to_string())?
    };

    let api = hidapi::HidApi::new().map_err(|e| e.to_string())?;
    let info = api
        .device_list()
        .find(|d| d.path().to_string_lossy() == selected.path)
        .ok_or_else(|| format!("selected ITE path disappeared: {}", selected.path))?;
    let dev = info.open_device(&api).map_err(|e| e.to_string())?;

    let prelude_enabled = std::env::var("CNTRL_ITE_OPENRGB_PRELUDE")
        .map(|s| {
            let t = s.trim().to_ascii_lowercase();
            !(t == "0" || t == "false" || t == "off")
        })
        .unwrap_or(true);
    if prelude_enabled {
        run_openrgb_prelude(&dev)?;
    }

    let native_static_enabled = std::env::var("CNTRL_ITE_NATIVE_STATIC")
        .map(|s| {
            let t = s.trim().to_ascii_lowercase();
            !(t == "0" || t == "false" || t == "off")
        })
        .unwrap_or(true);

    if native_static_enabled {
        send_openrgb_static_effect(&dev, r, g, b)?;
        let strip_direct_enabled = std::env::var("CNTRL_ITE_STRIP_DIRECT")
            .map(|s| {
                let t = s.trim().to_ascii_lowercase();
                !(t == "0" || t == "false" || t == "off")
            })
            .unwrap_or(true);
        if strip_direct_enabled {
            send_openrgb_strip_direct_colors(&dev, r, g, b)?;
        }
    } else {
        if apply_template.trim().is_empty() {
            return Err("CNTRL_ITE_APPLY_TEMPLATE is not set".to_string());
        }
        for raw in init_templates.split(';').map(str::trim).filter(|s| !s.is_empty()) {
            let packet = expand_packet_template(raw, r, g, b)?;
            write_ite_packet(&dev, &packet)?;
        }
        let apply_packet = expand_packet_template(&apply_template, r, g, b)?;
        write_ite_packet(&dev, &apply_packet)?;
    }

    Ok(format!(
        "ite_rgb applied via {} (iface={})",
        selected.path, selected.interface_number
    ))
}

// ── canvas streaming (per-LED direct mode) ───────────────────────────

/// Persistent direct-mode session for canvas streaming: device stays open,
/// bring-up runs once, then `write_strips` pushes true per-LED colors on the
/// D_LED1/D_LED2 ARGB headers every frame.
pub struct IteDirect {
    dev: hidapi::HidDevice,
    order: [usize; 3],
    path: String,
    /// Last color sent to each single-color zone (skip the bus when unchanged).
    zone_last: [Option<[u8; 3]>; 8],
}

/// Zone index of the 12V non-addressable RGB header (identified by lighting
/// each zone a different color on this board). Override with
/// `CNTRL_ITE_RGB12V_ZONE` for other boards.
pub fn rgb12v_zone() -> u8 {
    std::env::var("CNTRL_ITE_RGB12V_ZONE")
        .ok()
        .and_then(|s| s.trim().parse::<u8>().ok())
        .filter(|z| *z <= 7)
        .unwrap_or(2)
}

impl IteDirect {
    pub fn open() -> Result<Self, String> {
        let preferred_path = std::env::var("CNTRL_ITE_PATH")
            .ok()
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty());
        let candidates = discover_candidates()?;
        let selected = if let Some(p) = preferred_path.as_deref() {
            candidates
                .iter()
                .find(|c| c.path == p)
                .ok_or_else(|| format!("CNTRL_ITE_PATH={p} not found"))?
        } else {
            candidates
                .first()
                .ok_or_else(|| "no ITE HID candidates found (VID 048d PID 5702)".to_string())?
        };

        let api = hidapi::HidApi::new().map_err(|e| e.to_string())?;
        let info = api
            .device_list()
            .find(|d| d.path().to_string_lossy() == selected.path)
            .ok_or_else(|| format!("selected ITE path disappeared: {}", selected.path))?;
        let dev = info.open_device(&api).map_err(|e| e.to_string())?;
        run_openrgb_prelude(&dev)?;
        Ok(Self {
            dev,
            order: parse_strip_color_order(),
            path: selected.path.clone(),
            zone_last: [None; 8],
        })
    }

    pub fn path(&self) -> &str {
        &self.path
    }

    /// Push one frame of per-LED colors (RGB, canvas order) to both ARGB
    /// strip headers, then latch **only the two D_LED zones** — OpenRGB's
    /// direct-mode sequence (strips → zone effect 0x23/0x24 → apply 0x18).
    /// Applying with mask 0xFF instead re-renders the motherboard's own
    /// zones every frame and the whole board strobes.
    pub fn write_strips(
        &mut self,
        dled1: &[[u8; 3]],
        dled2: &[[u8; 3]],
        zones: &[(u8, [u8; 3])],
    ) -> Result<(), String> {
        for (header, colors) in [(0x58u8, dled1), (0x59u8, dled2)] {
            if colors.is_empty() {
                continue;
            }
            let wire: Vec<[u8; 3]> = colors
                .iter()
                .map(|c| [c[self.order[0]], c[self.order[1]], c[self.order[2]]])
                .collect();
            for (i, chunk) in wire.chunks(STRIP_LEDS_PER_PACKET).enumerate() {
                let offset = (i * STRIP_LEDS_PER_PACKET * 3) as u16;
                let p = build_openrgb_strip_packet_multi(header, offset, chunk);
                write_ite_packet(&self.dev, &p)?;
            }
        }
        // Zone effect (static, full brightness) for D_LED1 (zone 3, register
        // 0x23, mask 0x08) and D_LED2 (zone 4, register 0x24, mask 0x10).
        for (reg, mask) in [(0x23u8, 0x08u8), (0x24u8, 0x10u8)] {
            let mut z = [0u8; 64];
            z[0] = 0xCC;
            z[1] = reg;
            z[2] = mask;
            z[11] = 0x01; // static effect type
            z[12] = 0xFF; // max brightness
            write_ite_packet(&self.dev, &z)?;
        }
        // Single-color zones (12V RGB header, the board's own lit zones): one
        // static color each, only touched when it changes. Re-latching a zone
        // every frame restarts its effect and the board strobes.
        let mut apply_mask = 0x18u8;
        for &(zone, c) in zones {
            let Some(last) = self.zone_last.get_mut(zone as usize) else {
                continue;
            };
            if *last == Some(c) {
                continue;
            }
            let mut z = [0u8; 64];
            z[0] = 0xCC;
            z[1] = 0x20 + zone;
            z[2] = 1 << zone;
            z[11] = 0x01; // static
            z[12] = 0xFF; // brightness
            z[14] = c[2]; // BGR color0
            z[15] = c[1];
            z[16] = c[0];
            write_ite_packet(&self.dev, &z)?;
            *last = Some(c);
            apply_mask |= 1 << zone;
        }
        // Latch only the zones we touched (0xFF would re-render the
        // motherboard's other zones every frame and strobe the board).
        let mut apply = [0u8; 64];
        apply[0] = 0xCC;
        apply[1] = 0x28;
        apply[2] = apply_mask;
        write_ite_packet(&self.dev, &apply)
    }
}

