//! Host-side RGB / lighting for MCHOSE Ace68-II (USB `41e4:2116`, **HID interface 1**).
//!
//! Mirrors `~/Documents/GitHub/CNTRL_Python/scripts/set_rgb_rw.py`: 64-byte `0x55` frames on the
//! vendor `hidraw` node, **raw `read`/`write`** (see Python note: `hidapi::write` can drop packets
//! on Linux). Speed / extra fields: see **`protocol.md`** in CNTRL_Python.
//!
//! Effect templates: `resources/effects_light_v2.json` (keys `"1"`…`"22"`, byte 21 = effect id).

use hex::FromHex;
use hidapi::HidApi;
use nix::poll::{poll, PollFd, PollFlags, PollTimeout};
use serde::Deserialize;
use std::collections::HashMap;
use std::fs::File;
use std::io::{Read, Write};
use std::os::unix::io::AsFd;
use std::path::PathBuf;
use std::sync::LazyLock;
use std::thread;
use std::time::{Duration, Instant};

const VID: u16 = 0x41e4;
const PID: u16 = 0x2116;
const PIPELINE_TIMEOUT_MS: u128 = 8000;

const GET_DIY_LIGHT: [u8; 64] = {
    let mut b = [0u8; 64];
    b[0] = 0x55;
    b[1] = 0x04;
    b[3] = 0x38;
    b[4] = 0x38;
    b
};

const GET_PERFORMANCE: [u8; 64] = {
    let mut b = [0u8; 64];
    b[0] = 0x55;
    b[1] = 0x05;
    b[3] = 0x38;
    b[4] = 0x38;
    b
};

const GET_PERFORMANCE2: [u8; 64] = {
    let mut b = [0u8; 64];
    b[0] = 0x55;
    b[1] = 0x05;
    b[3] = 0x40;
    b[4] = 0x08;
    b[5] = 0x38;
    b
};

fn decode64(s: &str) -> [u8; 64] {
    let v = hex::decode(s).expect("hex template");
    assert_eq!(v.len(), 64);
    let mut a = [0u8; 64];
    a.copy_from_slice(&v);
    a
}

static TPL_SINGLE: LazyLock<[u8; 64]> = LazyLock::new(|| {
    decode64("55f200ed38d802003c6c696768744076323e0f000003ff0000006404ff8000006409b900ff0064080000ff1064070000ff1064060000ff1064000000ff1064ff")
});
static TPL_DUAL: LazyLock<[u8; 64]> = LazyLock::new(|| {
    decode64("55f2007638d802003c6c696768744076323e0f00000300ffff006400ff008a1064080000ff1064060000ff106404ff8000006409b900ff0064070000ff1064ff")
});
static TPL_ORANGE: LazyLock<[u8; 64]> = LazyLock::new(|| {
    decode64("55f200f738d802003c6c696768744076323e0f000003ff8000006400ff008a1064080000ff1064060000ff106404ff8000006409b900ff0064070000ff1064ff")
});
static TPL_3CH: LazyLock<[u8; 64]> = LazyLock::new(|| {
    decode64("55f2003738d802003c6c696768744076323e0f00000300ffff006400ff008a1064080000ff1064060000ff106404ff8000006409b900ff0064070000ff1064ff")
});
static TPL_WHITE: LazyLock<[u8; 64]> = LazyLock::new(|| {
    decode64("55f2007538d802003c6c696768744076323e0f000003ffffff006400ff008a1064080000ff1064060000ff106404ff8000006409b900ff0064070000ff1064ff")
});

static ZONE_SINGLE: LazyLock<[u8; 64]> = LazyLock::new(|| {
    decode64("55f2003d380703001064000000ff1064ff0000ff1064110000ff1064100000ff10640f0000ff10640e0000ff1064160000ff1064150000ff1064140000ff1064")
});
static ZONE_MULTI: LazyLock<[u8; 64]> = LazyLock::new(|| {
    decode64("55f20034380703000064070000ff1064ff0000ff1064110000ff1064100000ff10640f0000ff10640e0000ff1064160000ff1064150000ff1064140000ff1064")
});

#[derive(Deserialize)]
struct EffectsFile {
    templates: HashMap<String, String>,
    #[serde(default)]
    zones: HashMap<String, String>,
}

fn load_effect_templates() -> (HashMap<u8, [u8; 64]>, HashMap<u8, [u8; 64]>) {
    let json = include_str!("../../resources/effects_light_v2.json");
    let ef: EffectsFile = serde_json::from_str(json).expect("effects_light_v2.json");
    let mut templates = HashMap::new();
    for (k, hx) in &ef.templates {
        let id: u8 = k.parse().expect("effect key");
        let raw = Vec::from_hex(hx.trim()).expect("effect hex");
        assert_eq!(raw.len(), 64, "effect {id}");
        let mut arr = [0u8; 64];
        arr.copy_from_slice(&raw);
        templates.insert(id, arr);
    }
    let mut zones = HashMap::new();
    for (k, hx) in &ef.zones {
        let id: u8 = k.parse().expect("zone key");
        let raw = Vec::from_hex(hx.trim()).expect("zone hex");
        assert_eq!(raw.len(), 64, "zone {id}");
        let mut arr = [0u8; 64];
        arr.copy_from_slice(&raw);
        zones.insert(id, arr);
    }
    (templates, zones)
}

pub(crate) static EFFECT_DATA: LazyLock<(HashMap<u8, [u8; 64]>, HashMap<u8, [u8; 64]>)> =
    LazyLock::new(load_effect_templates);

/// Recompute the Mchose packet checksum: byte[3] = sum(bytes[4..64]) mod 256.
pub(crate) fn recompute_checksum(pkt: &mut [u8; 64]) {
    pkt[3] = pkt[4..].iter().copied().fold(0u8, |a, b| a.wrapping_add(b));
}

fn classify(r: i32, g: i32, b: i32) -> &'static str {
    let nz = (r > 0) as u8 + (g > 0) as u8 + (b > 0) as u8;
    match nz {
        0 => "zero",
        1 => "single",
        2 => {
            let both_full = [r, g, b].iter().filter(|&&v| v > 0).all(|&v| v == 255);
            if both_full { "dual" } else { "orange" }
        }
        _ => {
            if r == 255 && g == 255 && b == 255 {
                "white"
            } else {
                "3ch"
            }
        }
    }
}

pub(crate) fn profile_template(r: u8, g: u8, b: u8) -> [u8; 64] {
    let ir = r as i32;
    let ig = g as i32;
    let ib = b as i32;
    let mut t = match classify(ir, ig, ib) {
        "zero" | "single" => *TPL_SINGLE,
        "orange" => *TPL_ORANGE,
        "dual" => *TPL_DUAL,
        "white" => *TPL_WHITE,
        "3ch" => *TPL_3CH,
        _ => *TPL_DUAL,
    };
    t[22] = r;
    t[23] = g;
    t[24] = b;
    recompute_checksum(&mut t);
    t
}

pub(crate) fn zone_template(r: u8, g: u8, b: u8) -> [u8; 64] {
    let nz = (r > 0) as u8 + (g > 0) as u8 + (b > 0) as u8;
    let mut z = if nz >= 2 {
        *ZONE_MULTI
    } else {
        *ZONE_SINGLE
    };
    recompute_checksum(&mut z);
    z
}

pub(crate) fn build_apply_a(aa05: &[u8; 64], rgb: (u8, u8, u8), effect_id: Option<u8>) -> [u8; 64] {
    let mut a = *aa05;
    a[0] = 0x55;
    a[1] = 0x06;
    if let Some(eid) = effect_id {
        a[9] = 0x00;
        a[16] = eid;
    } else {
        // Solid: force the static lightMode (web app uses 0x03) so the keyboard
        // shows a solid colour instead of recolouring the running effect.
        a[16] = 0x03;
    }
    a[22] = rgb.0;
    a[23] = rgb.1;
    a[24] = rgb.2;
    recompute_checksum(&mut a);
    a
}

pub(crate) fn build_apply_b(aa05: &[u8; 64], rgb: (u8, u8, u8), effect_id: Option<u8>) -> [u8; 64] {
    let mut b = [0u8; 64];
    b[0..8].copy_from_slice(&[0x55, 0x06, 0x00, 0x00, 0x38, 0x08, 0x00, 0x00]);
    let tail = &aa05[16..];
    b[8..8 + tail.len()].copy_from_slice(tail);
    if let Some(eid) = effect_id {
        b[8] = eid;
    } else {
        // Solid: static lightMode (matches the web app's solid apply_b).
        b[8] = 0x03;
    }
    b[14] = rgb.0;
    b[15] = rgb.1;
    b[16] = rgb.2;
    recompute_checksum(&mut b);
    b
}

pub(crate) fn effect_profile_packet(
    effect_id: u8,
    r: u8,
    g: u8,
    b: u8,
    brightness: u8,
    templates: &HashMap<u8, [u8; 64]>,
) -> Result<[u8; 64], String> {
    let mut pkt = *templates
        .get(&effect_id)
        .ok_or_else(|| format!("unknown effect_id {effect_id} (need 1–22 in effects_light_v2.json)"))?;
    let br = brightness.min(255);
    let scale = f64::from(br) / 255.0;
    pkt[22] = ((f64::from(r) * scale).round() as u32).min(255) as u8;
    pkt[23] = ((f64::from(g) * scale).round() as u32).min(255) as u8;
    pkt[24] = ((f64::from(b) * scale).round() as u32).min(255) as u8;
    recompute_checksum(&mut pkt);
    Ok(pkt)
}

fn find_aa05(pkts: &[[u8; 64]]) -> Option<[u8; 64]> {
    let candidates: Vec<_> = pkts
        .iter()
        .copied()
        .filter(|p| p[0] == 0xAA && p[1] == 0x05 && p[4] == 0x38)
        .collect();
    for c in candidates.iter().rev() {
        if c[3] != 0 {
            return Some(*c);
        }
    }
    candidates.last().copied()
}

/// Resolve `/dev/hidraw*` for vendor lighting (matches CNTRL_Python `vendor_hidraw_path`).
///
/// Prefer USB **interface 1**. If hidapi reports `interface_number == -1` (common on Linux), use
/// **`/dev/hidraw1`** when present, else the first sorted path (Python’s second-loop fallback).
pub fn vendor_hidraw_path() -> Result<PathBuf, String> {
    if let Ok(env) = std::env::var("MCHOSE_HIDRAW") {
        let t = env.trim();
        if !t.is_empty() {
            return Ok(PathBuf::from(t));
        }
    }
    let api = HidApi::new().map_err(|e| e.to_string())?;
    let mut entries: Vec<(i32, PathBuf)> = Vec::new();
    for d in api.device_list() {
        if d.vendor_id() != VID || d.product_id() != PID {
            continue;
        }
        let p = d
            .path()
            .to_str()
            .map_err(|_| "hidraw path is not UTF-8".to_string())?;
        entries.push((d.interface_number(), PathBuf::from(p)));
    }
    entries.sort_by(|a, b| a.0.cmp(&b.0).then(a.1.cmp(&b.1)));

    if let Some((_, p)) = entries.iter().find(|(iface, _)| *iface == 1) {
        return Ok(p.clone());
    }
    if let Some((_, p)) = entries.iter().find(|(_, path)| {
        path.file_name()
            .and_then(|n| n.to_str())
            .is_some_and(|n| n == "hidraw1")
    }) {
        return Ok(p.clone());
    }
    if let Some((iface, p)) = entries.first() {
        eprintln!(
            "mchose_lighting: fallback hidraw (interface_number={iface}): {}",
            p.display()
        );
        return Ok(p.clone());
    }
    Err("No MCHOSE Ace68-II (41e4:2116) HID device found. Connect keyboard; check udev rules (packaging/udev).".into())
}

/// Map one `read()` from hidraw to a 64-byte logical frame (`0x55` / `0xAA` at [0]).
fn normalize_hidraw_in(buf: &[u8]) -> Option<[u8; 64]> {
    if buf.is_empty() {
        return None;
    }
    if buf.len() >= 65 && buf[0] == 0 && matches!(buf[1], 0xAA | 0xAB | 0x55) {
        let mut o = [0u8; 64];
        o.copy_from_slice(&buf[1..65]);
        return Some(o);
    }
    let mut o = [0u8; 64];
    let take = buf.len().min(64);
    o[..take].copy_from_slice(&buf[..take]);
    // Rare: kernel returns exactly 64 bytes with a leading 0x00 report id (byte 65 not yet read).
    if take == 64 && o[0] == 0 && matches!(o[1], 0xAA | 0xAB | 0x55) {
        let mut s = [0u8; 64];
        s[..63].copy_from_slice(&o[1..64]);
        s[63] = 0;
        return Some(s);
    }
    Some(o)
}

fn drain_one(file: &mut File, timeout_ms: u16) -> std::io::Result<Option<[u8; 64]>> {
    let pfd = PollFd::new(file.as_fd(), PollFlags::POLLIN);
    let n = poll(&mut [pfd], PollTimeout::from(timeout_ms)).map_err(|e| {
        std::io::Error::new(std::io::ErrorKind::Other, format!("poll: {e}"))
    })?;
    if n == 0 {
        return Ok(None);
    }
    // Read up to 65 bytes so we can drop a leading 0x00 report-id byte if present.
    let mut raw = [0u8; 65];
    let nread = loop {
        match file.read(&mut raw) {
            Ok(k) => break k,
            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => return Ok(None),
            Err(e) => return Err(e),
        }
    };
    if nread == 0 {
        return Ok(None);
    }
    Ok(normalize_hidraw_in(&raw[..nread]))
}

fn stage_deadline(stage: &str, started: Instant, path: &std::path::Path) -> Result<(), String> {
    let elapsed = started.elapsed().as_millis();
    if elapsed > PIPELINE_TIMEOUT_MS {
        let msg = format!(
            "HID pipeline timeout during {stage} after {elapsed}ms on {}",
            path.display()
        );
        eprintln!("mchose_lighting: {msg}");
        return Err(msg);
    }
    Ok(())
}

fn drain(file: &mut File, max: usize, timeout_ms: u16) -> std::io::Result<Vec<[u8; 64]>> {
    let mut out = Vec::new();
    for _ in 0..max {
        match drain_one(file, timeout_ms)? {
            Some(p) => out.push(p),
            None => break,
        }
    }
    Ok(out)
}

fn send_recv(
    file: &mut File,
    pkt: &[u8; 64],
    n1: usize,
    t1: u16,
    n2: usize,
    t2: u16,
) -> std::io::Result<Vec<[u8; 64]>> {
    file.write_all(pkt)?;
    let mut a = drain(file, n1, t1)?;
    let b = drain(file, n2, t2)?;
    a.extend(b);
    Ok(a)
}

fn initial_drain(file: &mut File) -> std::io::Result<()> {
    for _ in 0..4 {
        let _ = drain_one(file, 40);
    }
    Ok(())
}

/// The MCHOSE web-app "enter PC/DIY control mode" handshake, captured from real
/// USB traffic (95 × 64-byte interrupt-OUT frames: `55 a9` mode switch + `55 08`/
/// `55 a0` buffer init + reads). Without it, a keyboard left in an onboard effect
/// (e.g. after a reboot) ignores our color writes. Run in its own HID session
/// before the color, mirroring the web app. Skip via `CNTRL_KB_NO_HANDSHAKE=1`.
static DIY_HANDSHAKE: &[u8] = include_bytes!("../../resources/keyboard_diy_handshake.bin");

fn prime_diy_mode(path: &std::path::Path) -> std::io::Result<()> {
    if std::env::var("CNTRL_KB_NO_HANDSHAKE")
        .map(|v| {
            let t = v.trim();
            t == "1" || t.eq_ignore_ascii_case("true")
        })
        .unwrap_or(false)
    {
        return Ok(());
    }
    let mut file = File::options().read(true).write(true).open(path)?;
    for _ in 0..4 {
        let _ = drain_one(&mut file, 40);
    }
    for chunk in DIY_HANDSHAKE.chunks(64) {
        let mut frame = [0u8; 64];
        frame[..chunk.len()].copy_from_slice(chunk);
        file.write_all(&frame)?;
        while drain_one(&mut file, 6)?.is_some() {}
        thread::sleep(Duration::from_millis(2));
    }
    thread::sleep(Duration::from_millis(120));
    while drain_one(&mut file, 20)?.is_some() {}
    Ok(())
}

/// Full pipeline: getDiyLight (unless `direct`) → getPerformance ×2 → WRITE_LIGHT_V2 → ZONE → APPLY_A/B.
pub fn run_pipeline(
    rgb: (u8, u8, u8),
    effect_id: Option<u8>,
    brightness: u8,
    speed: Option<u8>,
    direct: bool,
) -> Result<(), String> {
    let (r, g, b) = rgb;
    if let Some(sid) = effect_id {
        if !(1..=22).contains(&sid) {
            return Err("effect_id must be 1..=22".into());
        }
    }
    if speed.is_some() {
        eprintln!("mchose_lighting: speed is not wired to packets yet (see protocol.md).");
    }

    let path = vendor_hidraw_path()?;
    let started = Instant::now();
    eprintln!(
        "mchose_lighting: run_pipeline start path={} direct={} effect_id={:?}",
        path.display(),
        direct,
        effect_id
    );
    // Enter DIY/PC-control mode in its own HID session first (the keyboard reverts
    // to onboard effects on reboot and otherwise ignores our writes).
    eprintln!("mchose_lighting: stage=prime_diy_mode path={}", path.display());
    prime_diy_mode(&path).map_err(|e| format!("prime_diy_mode {}: {e}", path.display()))?;
    stage_deadline("prime_diy_mode", started, &path)?;
    thread::sleep(Duration::from_millis(150));

    let mut file = File::options()
        .read(true)
        .write(true)
        .open(&path)
        .map_err(|e| format!("open {}: {e}", path.display()))?;

    eprintln!("mchose_lighting: stage=initial_drain path={}", path.display());
    initial_drain(&mut file).map_err(|e| e.to_string())?;
    stage_deadline("initial_drain", started, &path)?;

    if !direct {
        eprintln!("mchose_lighting: stage=get_diy_light path={}", path.display());
        send_recv(&mut file, &GET_DIY_LIGHT, 8, 120, 4, 60).map_err(|e| {
            let msg = format!("get_diy_light {}: {e}", path.display());
            eprintln!("mchose_lighting: stage=get_diy_light failed: {msg}");
            msg
        })?;
        stage_deadline("GET_DIY_LIGHT", started, &path)?;
    }

    eprintln!("mchose_lighting: stage=get_performance path={}", path.display());
    let mut perf_pkts = send_recv(&mut file, &GET_PERFORMANCE, 16, 150, 4, 80).map_err(|e| {
        let msg = format!("get_performance {}: {e}", path.display());
        eprintln!("mchose_lighting: stage=get_performance failed: {msg}");
        msg
    })?;
    stage_deadline("GET_PERFORMANCE", started, &path)?;
    eprintln!("mchose_lighting: stage=get_performance2 path={}", path.display());
    let perf2 = send_recv(&mut file, &GET_PERFORMANCE2, 4, 120, 2, 80).map_err(|e| {
        let msg = format!("get_performance2 {}: {e}", path.display());
        eprintln!("mchose_lighting: stage=get_performance2 failed: {msg}");
        msg
    })?;
    stage_deadline("GET_PERFORMANCE2", started, &path)?;
    perf_pkts.extend(perf2);

    let aa05 = find_aa05(&perf_pkts).ok_or_else(|| {
        "no 0xaa 0x05 performance reply — is the Ace68-II connected on interface 1 (MCHOSE_HIDRAW)?".to_string()
    })?;

    let (templates, zone_templates) = &*EFFECT_DATA;
    let profile_pkt = if let Some(eid) = effect_id {
        effect_profile_packet(eid, r, g, b, brightness, templates)?
    } else {
        profile_template(r, g, b)
    };

    eprintln!("mchose_lighting: stage=write_light_v2 path={}", path.display());
    send_recv(&mut file, &profile_pkt, 4, 150, 2, 80).map_err(|e| {
        let msg = format!("write_light_v2 {}: {e}", path.display());
        eprintln!("mchose_lighting: stage=write_light_v2 failed: {msg}");
        msg
    })?;
    stage_deadline("WRITE_LIGHT_V2", started, &path)?;

    let zone_pkt = if let Some(eid) = effect_id {
        if let Some(z) = zone_templates.get(&eid) {
            let mut zp = *z;
            recompute_checksum(&mut zp);
            zp
        } else {
            zone_template(r, g, b)
        }
    } else {
        zone_template(r, g, b)
    };
    eprintln!("mchose_lighting: stage=zone path={}", path.display());
    send_recv(&mut file, &zone_pkt, 4, 150, 2, 80).map_err(|e| {
        let msg = format!("zone {}: {e}", path.display());
        eprintln!("mchose_lighting: stage=zone failed: {msg}");
        msg
    })?;
    stage_deadline("ZONE", started, &path)?;

    let apply_a = build_apply_a(&aa05, (r, g, b), effect_id);
    eprintln!("mchose_lighting: stage=apply_a path={}", path.display());
    send_recv(&mut file, &apply_a, 4, 150, 2, 80).map_err(|e| {
        let msg = format!("apply_a {}: {e}", path.display());
        eprintln!("mchose_lighting: stage=apply_a failed: {msg}");
        msg
    })?;
    stage_deadline("APPLY_A", started, &path)?;

    let apply_b = build_apply_b(&aa05, (r, g, b), effect_id);
    eprintln!("mchose_lighting: stage=apply_b path={}", path.display());
    send_recv(&mut file, &apply_b, 4, 150, 2, 80).map_err(|e| {
        let msg = format!("apply_b {}: {e}", path.display());
        eprintln!("mchose_lighting: stage=apply_b failed: {msg}");
        msg
    })?;
    stage_deadline("APPLY_B", started, &path)?;

    thread::sleep(Duration::from_millis(50));
    stage_deadline("finalize", started, &path)?;
    eprintln!(
        "mchose_lighting: run_pipeline success path={} elapsed_ms={}",
        path.display(),
        started.elapsed().as_millis()
    );
    Ok(())
}

/// `hex_color` = `0x00RRGGBB` (top byte ignored).
pub fn set_effect(
    effect_id: u8,
    hex_color: u32,
    brightness: u8,
    speed: Option<u8>,
    direct: bool,
) -> Result<(), String> {
    let r = ((hex_color >> 16) & 0xff) as u8;
    let g = ((hex_color >> 8) & 0xff) as u8;
    let b = (hex_color & 0xff) as u8;
    run_pipeline(
        (r, g, b),
        Some(effect_id),
        brightness,
        speed,
        direct,
    )
}

/// Solid DIY: same as Python `run_pipeline` without `effect_id` — `_profile_template` uses raw
/// R,G,B; **brightness is not applied** on the wire (Python `run_pipeline` ignores it for solid).
/// Parameter kept for API parity / future use.
pub fn set_solid_rgb(
    r: u8,
    g: u8,
    b: u8,
    _brightness: u8,
    speed: Option<u8>,
    direct: bool,
) -> Result<(), String> {
    run_pipeline((r, g, b), None, 255, speed, direct)
}
