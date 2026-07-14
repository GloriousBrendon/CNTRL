//! LED canvas: SignalRGB-style effect→LED mapping, fully native.
//!
//! One virtual canvas (`CANVAS_W`×`CANVAS_H` units) is shared by every device.
//! Each device declares its LED geometry (`LedSpec`: local bounding box + one
//! `[x,y]` per LED); a `Placement` (position/scale/rotation) puts that box on
//! the canvas. Effects render into a `Frame` (a plain RGB framebuffer) and each
//! LED samples the pixel under it — the effect and the hardware never need to
//! know about each other, so one animation flows across the keyboard, ARGB
//! strips, GPU and RAM in physical-desk order.

use crate::keyboard_layout;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// Virtual canvas size in layout units. Also the framebuffer resolution
/// effects render at (14.4k pixels — trivial to fill on CPU per frame).
pub const CANVAS_W: f32 = 160.0;
pub const CANVAS_H: f32 = 90.0;

// ── framebuffer ──────────────────────────────────────────────────────

/// RGB framebuffer effects render into; LEDs sample out of it.
pub struct Frame {
    pub width: usize,
    pub height: usize,
    /// Row-major RGB.
    pub px: Vec<[u8; 3]>,
}

impl Frame {
    pub fn new(width: usize, height: usize) -> Self {
        Self {
            width,
            height,
            px: vec![[0, 0, 0]; width * height],
        }
    }

    /// Canvas-sized frame (the default for both preview and hardware).
    pub fn canvas_sized() -> Self {
        Self::new(CANVAS_W as usize, CANVAS_H as usize)
    }

    #[inline]
    pub fn set(&mut self, x: usize, y: usize, rgb: [u8; 3]) {
        if x < self.width && y < self.height {
            self.px[y * self.width + x] = rgb;
        }
    }

    #[inline]
    pub fn get(&self, x: usize, y: usize) -> [u8; 3] {
        let x = x.min(self.width.saturating_sub(1));
        let y = y.min(self.height.saturating_sub(1));
        self.px[y * self.width + x]
    }

    /// Bilinear sample at canvas coordinates (clamped to edges) — the native
    /// equivalent of SignalRGB's `device.color(x, y)` canvas read-back.
    pub fn sample(&self, x: f32, y: f32) -> [u8; 3] {
        if self.width == 0 || self.height == 0 {
            return [0, 0, 0];
        }
        // Map canvas units → pixel space (frame may differ from canvas size).
        let px = (x / CANVAS_W) * self.width as f32 - 0.5;
        let py = (y / CANVAS_H) * self.height as f32 - 0.5;
        let x0 = px.floor();
        let y0 = py.floor();
        let fx = px - x0;
        let fy = py - y0;
        let xi = |v: f32| (v.max(0.0) as usize).min(self.width - 1);
        let yi = |v: f32| (v.max(0.0) as usize).min(self.height - 1);
        let (x0i, x1i) = (xi(x0), xi(x0 + 1.0));
        let (y0i, y1i) = (yi(y0), yi(y0 + 1.0));
        let mut out = [0u8; 3];
        for (c, o) in out.iter_mut().enumerate() {
            let p00 = self.px[y0i * self.width + x0i][c] as f32;
            let p10 = self.px[y0i * self.width + x1i][c] as f32;
            let p01 = self.px[y1i * self.width + x0i][c] as f32;
            let p11 = self.px[y1i * self.width + x1i][c] as f32;
            let top = p00 + (p10 - p00) * fx;
            let bot = p01 + (p11 - p01) * fx;
            *o = (top + (bot - top) * fy).round().clamp(0.0, 255.0) as u8;
        }
        out
    }

    /// RGBA bytes (alpha=255) for handing to an image widget.
    pub fn to_rgba(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(self.px.len() * 4);
        for p in &self.px {
            out.extend_from_slice(&[p[0], p[1], p[2], 0xff]);
        }
        out
    }
}

// ── device LED geometry ──────────────────────────────────────────────

/// Stable ids for canvas devices (what the layout file keys on). Chain
/// segments use `dled1.N` / `dled2.N` (segment N along the header's chain).
pub const DEV_KEYBOARD: &str = "keyboard";
pub const CHAIN_DLED1: &str = "dled1";
pub const CHAIN_DLED2: &str = "dled2";
pub const DEV_GPU: &str = "gpu";
pub const DEV_RAM0: &str = "ram0";
pub const DEV_RAM1: &str = "ram1";

/// LED geometry of one device in its own local units, SignalRGB-style:
/// `size` is the bounding box, `led_positions` one `[x,y]` per LED in order.
#[derive(Debug, Clone)]
pub struct LedSpec {
    pub id: String,
    pub name: String,
    pub size: (f32, f32),
    pub led_positions: Vec<(f32, f32)>,
    /// Drawn as a ring (fan) rather than a box.
    pub is_fan: bool,
    /// Whether the hardware takes a live per-LED stream.
    pub streamable: bool,
}

/// What is physically plugged along one ARGB header's daisy chain, in chain
/// order — SignalRGB's "components on a channel". Each segment becomes its
/// own placeable canvas device, so individual fans can be positioned (and
/// reversed) to match the case even when the chain wiring is out of order.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SegmentKind {
    Fan,
    Strip,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChainSegment {
    pub kind: SegmentKind,
    pub leds: u8,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Chains {
    pub dled1: Vec<ChainSegment>,
    pub dled2: Vec<ChainSegment>,
}

impl Default for Chains {
    fn default() -> Self {
        Self {
            dled1: vec![ChainSegment {
                kind: SegmentKind::Strip,
                leds: dled1_count(),
            }],
            dled2: vec![ChainSegment {
                kind: SegmentKind::Strip,
                leds: dled2_count(),
            }],
        }
    }
}

impl Chains {
    pub fn header(&self, header: &str) -> &[ChainSegment] {
        match header {
            CHAIN_DLED1 => &self.dled1,
            _ => &self.dled2,
        }
    }

    /// Set every fan on `header` to `leds` LEDs (e.g. 8 for Corsair AR120,
    /// 12 or 16 for most other ARGB rings).
    pub fn set_fan_leds(&mut self, header: &str, leds: u8) {
        let segments = match header {
            CHAIN_DLED1 => &mut self.dled1,
            _ => &mut self.dled2,
        };
        for seg in segments.iter_mut() {
            if seg.kind == SegmentKind::Fan {
                seg.leds = leds.max(1);
            }
        }
    }

    /// Split a header's total LEDs into `n` equal fans (remainder goes to
    /// the last fan), or back to a single strip with `n == 0`.
    pub fn set_fans(&mut self, header: &str, n: u8) {
        let total = match header {
            CHAIN_DLED1 => dled1_count(),
            _ => dled2_count(),
        };
        let segments = if n == 0 {
            vec![ChainSegment {
                kind: SegmentKind::Strip,
                leds: total,
            }]
        } else {
            let per = total / n;
            let mut v: Vec<ChainSegment> = (0..n)
                .map(|_| ChainSegment {
                    kind: SegmentKind::Fan,
                    leds: per,
                })
                .collect();
            v[n as usize - 1].leds += total - per * n;
            v
        };
        match header {
            CHAIN_DLED1 => self.dled1 = segments,
            _ => self.dled2 = segments,
        }
    }
}

/// Every device CNTRL can place on the canvas, in a stable order.
pub fn all_specs(chains: &Chains) -> Vec<LedSpec> {
    let mut out = vec![keyboard_spec()];
    for (header, label) in [(CHAIN_DLED1, "Header 1"), (CHAIN_DLED2, "Header 2")] {
        let segments = chains.header(header);
        let single = segments.len() == 1;
        for (i, seg) in segments.iter().enumerate() {
            let id = format!("{header}.{i}");
            let name = match (seg.kind, single) {
                (SegmentKind::Strip, true) => format!("ARGB {label}"),
                (SegmentKind::Strip, false) => format!("{label} Strip {}", i + 1),
                (SegmentKind::Fan, _) => format!("{label} Fan {}", i + 1),
            };
            out.push(match seg.kind {
                SegmentKind::Fan => fan_spec(id, name, seg.leds),
                SegmentKind::Strip => strip_spec(id, name, seg.leds),
            });
        }
    }
    out.push(gpu_spec());
    out.push(ram_spec(DEV_RAM0, "RAM 1"));
    out.push(ram_spec(DEV_RAM1, "RAM 2"));
    out
}

pub fn spec_by_id(chains: &Chains, id: &str) -> Option<LedSpec> {
    all_specs(chains).into_iter().find(|s| s.id == id)
}

/// D_LED1/D_LED2 totals mirror `ite_rgb`'s defaults (32/36) and the same
/// override env (`CNTRL_ITE_STRIP_LED_COUNTS=dled1,dled2`).
fn strip_counts() -> (u8, u8) {
    std::env::var("CNTRL_ITE_STRIP_LED_COUNTS")
        .ok()
        .and_then(|raw| {
            let mut it = raw.split(',').map(str::trim);
            let a = it.next()?.parse::<u8>().ok()?;
            let b = it.next()?.parse::<u8>().ok()?;
            (a > 0 && b > 0).then_some((a, b))
        })
        .unwrap_or((32, 36))
}

pub fn dled1_count() -> u8 {
    strip_counts().0
}

pub fn dled2_count() -> u8 {
    strip_counts().1
}

fn keyboard_spec() -> LedSpec {
    let keys = keyboard_layout::get_keyboard_layout();
    let led_positions = keys
        .iter()
        .map(|k| (k.x as f32 + k.width as f32 / 2.0, k.y as f32 + 0.5))
        .collect();
    LedSpec {
        id: DEV_KEYBOARD.to_string(),
        name: "Keyboard".to_string(),
        size: (15.0, 5.0),
        led_positions,
        is_fan: false,
        // Per-key live frames via the 0xDD protocol (mchose_direct).
        streamable: true,
    }
}

fn strip_spec(id: String, name: String, count: u8) -> LedSpec {
    let led_positions = (0..count).map(|i| (i as f32 + 0.5, 0.5)).collect();
    LedSpec {
        id,
        name,
        size: (f32::from(count), 1.0),
        led_positions,
        is_fan: false,
        streamable: true,
    }
}

/// ARGB fan: `count` LEDs on a ring, LED 0 at 12 o'clock going clockwise.
fn fan_spec(id: String, name: String, count: u8) -> LedSpec {
    let n = count.max(1) as usize;
    let (cx, cy, r) = (2.0f32, 2.0f32, 1.5f32);
    let led_positions = (0..n)
        .map(|i| {
            let th = (i as f32 / n as f32) * std::f32::consts::TAU;
            (cx + r * th.sin(), cy - r * th.cos())
        })
        .collect();
    LedSpec {
        id,
        name,
        size: (4.0, 4.0),
        led_positions,
        is_fan: true,
        streamable: true,
    }
}

fn gpu_spec() -> LedSpec {
    // One logical zone — sampled at the shroud center.
    LedSpec {
        id: DEV_GPU.to_string(),
        name: "GPU".to_string(),
        size: (6.0, 3.0),
        led_positions: vec![(3.0, 1.5)],
        is_fan: false,
        streamable: true,
    }
}

fn ram_spec(id: &str, name: &str) -> LedSpec {
    // Corsair DDR5: 10 LEDs along the top edge, stick standing vertically.
    let n = 10usize;
    let led_positions = (0..n).map(|i| (0.5, i as f32 + 0.5)).collect();
    LedSpec {
        id: id.to_string(),
        name: name.to_string(),
        size: (1.0, n as f32),
        led_positions,
        is_fan: false,
        streamable: true,
    }
}

// ── placement / layout ───────────────────────────────────────────────

/// Where a device's local box sits on the canvas.
///
/// `x`/`y` is the top-left of the unrotated scaled box; `rotation_deg`
/// rotates the device around its own center.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
pub struct Placement {
    pub x: f32,
    pub y: f32,
    pub scale: f32,
    #[serde(default)]
    pub rotation_deg: f32,
}

impl Placement {
    /// Transform a device-local point into canvas coordinates.
    pub fn to_canvas(&self, spec_size: (f32, f32), local: (f32, f32)) -> (f32, f32) {
        let cx = spec_size.0 * self.scale / 2.0;
        let cy = spec_size.1 * self.scale / 2.0;
        let dx = local.0 * self.scale - cx;
        let dy = local.1 * self.scale - cy;
        let th = self.rotation_deg.to_radians();
        let (s, c) = th.sin_cos();
        (
            self.x + cx + dx * c - dy * s,
            self.y + cy + dx * s + dy * c,
        )
    }

    /// Inverse of `to_canvas`: canvas point → device-local units.
    pub fn to_local(&self, spec_size: (f32, f32), canvas: (f32, f32)) -> (f32, f32) {
        let cx = spec_size.0 * self.scale / 2.0;
        let cy = spec_size.1 * self.scale / 2.0;
        let dx = canvas.0 - self.x - cx;
        let dy = canvas.1 - self.y - cy;
        let th = (-self.rotation_deg).to_radians();
        let (s, c) = th.sin_cos();
        (
            (dx * c - dy * s + cx) / self.scale,
            (dx * s + dy * c + cy) / self.scale,
        )
    }

    /// Does a canvas point fall inside the device's (possibly rotated) box?
    pub fn contains(&self, spec_size: (f32, f32), canvas: (f32, f32)) -> bool {
        let (lx, ly) = self.to_local(spec_size, canvas);
        lx >= 0.0 && lx <= spec_size.0 && ly >= 0.0 && ly <= spec_size.1
    }
}

/// A device placed on the canvas, with per-element tuning (SignalRGB-style
/// device properties: invert for backwards-mounted fans, plus color trim).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlacedDevice {
    pub id: String,
    pub placement: Placement,
    /// Included when streaming / sampling.
    #[serde(default = "default_true")]
    pub enabled: bool,
    /// Per-device brightness percent (multiplies the global brightness).
    #[serde(default = "default_100")]
    pub brightness: f32,
    /// Hue rotation in degrees applied to sampled colors.
    #[serde(default)]
    pub hue_shift: f32,
    /// Saturation percent (100 = unchanged, 0 = white-washed, 200 = vivid).
    #[serde(default = "default_100")]
    pub saturation: f32,
    /// Reverse this segment's LED order on the wire — fixes fans/strips
    /// whose daisy-chain enters from the far end.
    #[serde(default)]
    pub reversed: bool,
}

impl PlacedDevice {
    pub fn new(id: impl Into<String>, placement: Placement) -> Self {
        Self {
            id: id.into(),
            placement,
            enabled: true,
            brightness: 100.0,
            hue_shift: 0.0,
            saturation: 100.0,
            reversed: false,
        }
    }
}

fn default_true() -> bool {
    true
}

fn default_100() -> f32 {
    100.0
}

/// Canvas-wide effect settings that persist with the layout.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct EffectSettings {
    pub effect: crate::canvas_effects::EffectKind,
    pub speed: f32,
    pub scale: f32,
    /// 0–100.
    pub brightness: f32,
    pub color: [u8; 3],
    pub fps: u32,
}

impl Default for EffectSettings {
    fn default() -> Self {
        Self {
            effect: crate::canvas_effects::EffectKind::RainbowWave,
            speed: 1.0,
            scale: 1.0,
            brightness: 100.0,
            color: [216, 143, 146],
            fps: 20,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CanvasLayout {
    #[serde(default)]
    pub version: u32,
    pub devices: Vec<PlacedDevice>,
    #[serde(default)]
    pub settings: EffectSettings,
    /// What is plugged along each ARGB header (SignalRGB "components").
    #[serde(default)]
    pub chains: Chains,
}

impl Default for CanvasLayout {
    fn default() -> Self {
        let mut layout = Self {
            version: 1,
            devices: Vec::new(),
            settings: EffectSettings::default(),
            chains: Chains::default(),
        };
        layout.reconcile();
        layout
    }
}

impl CanvasLayout {
    /// Default placement for a device id (desk-ish arrangement; chain
    /// segments fan out in a row from their header's corner).
    fn default_placement(&self, id: &str) -> Placement {
        let place = |x: f32, y: f32, scale: f32| Placement {
            x,
            y,
            scale,
            rotation_deg: 0.0,
        };
        if let Some(rest) = id.strip_prefix("dled1.") {
            let i: f32 = rest.parse().unwrap_or(0.0);
            return place(10.0 + i * 26.0, 4.0, if self.chains.dled1.len() == 1 { 4.0 } else { 5.0 });
        }
        if let Some(rest) = id.strip_prefix("dled2.") {
            let i: f32 = rest.parse().unwrap_or(0.0);
            return place(10.0 + i * 26.0, 66.0, if self.chains.dled2.len() == 1 { 4.0 } else { 5.0 });
        }
        match id {
            DEV_KEYBOARD => place(50.0, 35.0, 4.0),
            DEV_GPU => place(128.0, 42.0, 4.0),
            DEV_RAM0 => place(126.0, 12.0, 2.2),
            DEV_RAM1 => place(140.0, 12.0, 2.2),
            _ => place(70.0, 40.0, 3.0),
        }
    }

    /// Drop placements for unknown ids and add defaults for missing devices,
    /// so layouts survive catalog and chain-config changes.
    pub fn reconcile(&mut self) {
        let specs = all_specs(&self.chains);
        self.devices
            .retain(|d| specs.iter().any(|s| s.id == d.id));
        for spec in &specs {
            if !self.devices.iter().any(|d| d.id == spec.id) {
                let placement = self.default_placement(&spec.id);
                self.devices.push(PlacedDevice::new(spec.id.clone(), placement));
            }
        }
    }

    pub fn spec(&self, id: &str) -> Option<LedSpec> {
        spec_by_id(&self.chains, id)
    }

    pub fn device_mut(&mut self, id: &str) -> Option<&mut PlacedDevice> {
        self.devices.iter_mut().find(|d| d.id == id)
    }

    pub fn device(&self, id: &str) -> Option<&PlacedDevice> {
        self.devices.iter().find(|d| d.id == id)
    }
}

// ── sampling ─────────────────────────────────────────────────────────

/// Per-LED colors for one placed device: transform each LED's local position
/// through the placement, sample the frame, then apply the device's own
/// trim (brightness × global, hue shift, saturation).
pub fn sample_device(
    frame: &Frame,
    spec: &LedSpec,
    placed: &PlacedDevice,
    global_brightness_pct: f32,
) -> Vec<[u8; 3]> {
    let k = (global_brightness_pct / 100.0).clamp(0.0, 1.0)
        * (placed.brightness / 100.0).clamp(0.0, 1.0);
    let hue = placed.hue_shift;
    let sat = (placed.saturation / 100.0).clamp(0.0, 2.0);
    let trim_color = hue.abs() > 0.01 || (sat - 1.0).abs() > 0.01;
    spec.led_positions
        .iter()
        .map(|&p| {
            let (cx, cy) = placed.placement.to_canvas(spec.size, p);
            let mut c = frame.sample(cx, cy);
            if trim_color {
                let (h, s, v) = crate::canvas_effects::rgb_to_hsv(c);
                c = crate::canvas_effects::hsv_to_rgb(h + hue, (s * sat).clamp(0.0, 1.0), v);
            }
            [
                (c[0] as f32 * k) as u8,
                (c[1] as f32 * k) as u8,
                (c[2] as f32 * k) as u8,
            ]
        })
        .collect()
}

/// Average color over a device's box (for single-color devices like the
/// keyboard push or the GPU zone) — samples the LED positions and averages.
pub fn average_device_color(
    frame: &Frame,
    spec: &LedSpec,
    placed: &PlacedDevice,
    global_brightness_pct: f32,
) -> [u8; 3] {
    let colors = sample_device(frame, spec, placed, global_brightness_pct);
    if colors.is_empty() {
        return [0, 0, 0];
    }
    let n = colors.len() as u32;
    let sum = colors.iter().fold([0u32; 3], |acc, c| {
        [
            acc[0] + c[0] as u32,
            acc[1] + c[1] as u32,
            acc[2] + c[2] as u32,
        ]
    });
    [(sum[0] / n) as u8, (sum[1] / n) as u8, (sum[2] / n) as u8]
}

// ── persistence ──────────────────────────────────────────────────────

pub fn layout_path() -> Result<PathBuf, String> {
    if let Ok(p) = std::env::var("CNTRL_CANVAS_LAYOUT_PATH") {
        let t = p.trim();
        if !t.is_empty() {
            return Ok(PathBuf::from(t));
        }
    }
    dirs::config_dir()
        .map(|p| p.join("cntrl").join("canvas_layout.json"))
        .ok_or_else(|| "Could not resolve config directory".to_string())
}

pub fn load_layout() -> CanvasLayout {
    let mut layout = layout_path()
        .ok()
        .and_then(|p| std::fs::read_to_string(p).ok())
        .and_then(|s| serde_json::from_str::<CanvasLayout>(&s).ok())
        .unwrap_or_default();
    layout.reconcile();
    layout
}

pub fn save_layout(layout: &CanvasLayout) -> Result<(), String> {
    let path = layout_path()?;
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    }
    let s = serde_json::to_string_pretty(layout).map_err(|e| e.to_string())?;
    // Atomic write, same pattern as profile slots.
    let tmp = path.with_extension(format!("json.tmp.{}", std::process::id()));
    std::fs::write(&tmp, s).map_err(|e| e.to_string())?;
    std::fs::rename(&tmp, &path).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frame_sample_corners_and_bilinear() {
        let mut f = Frame::new(2, 2);
        f.set(0, 0, [255, 0, 0]);
        f.set(1, 0, [0, 255, 0]);
        f.set(0, 1, [0, 0, 255]);
        f.set(1, 1, [255, 255, 255]);
        // Center of top-left pixel in canvas units.
        let c = f.sample(CANVAS_W * 0.25, CANVAS_H * 0.25);
        assert_eq!(c, [255, 0, 0]);
        // Canvas center = equal blend of all four.
        let c = f.sample(CANVAS_W * 0.5, CANVAS_H * 0.5);
        assert_eq!(c, [128, 128, 128]);
        // Way off-canvas clamps.
        let c = f.sample(-100.0, -100.0);
        assert_eq!(c, [255, 0, 0]);
    }

    #[test]
    fn placement_roundtrip_with_rotation() {
        let p = Placement {
            x: 10.0,
            y: 20.0,
            scale: 3.0,
            rotation_deg: 37.0,
        };
        let size = (15.0, 5.0);
        let local = (4.25, 1.5);
        let canvas = p.to_canvas(size, local);
        let back = p.to_local(size, canvas);
        assert!((back.0 - local.0).abs() < 1e-4);
        assert!((back.1 - local.1).abs() < 1e-4);
    }

    #[test]
    fn placement_rotation_90_maps_axes() {
        let p = Placement {
            x: 0.0,
            y: 0.0,
            scale: 1.0,
            rotation_deg: 90.0,
        };
        let size = (4.0, 2.0);
        // Center is invariant under rotation about the center.
        let c = p.to_canvas(size, (2.0, 1.0));
        assert!((c.0 - 2.0).abs() < 1e-4 && (c.1 - 1.0).abs() < 1e-4);
        // Local +x becomes canvas +y after 90°.
        let c = p.to_canvas(size, (3.0, 1.0));
        assert!((c.0 - 2.0).abs() < 1e-4 && (c.1 - 2.0).abs() < 1e-4);
    }

    #[test]
    fn contains_respects_rotation() {
        let p = Placement {
            x: 0.0,
            y: 0.0,
            scale: 1.0,
            rotation_deg: 90.0,
        };
        let size = (10.0, 2.0);
        // Rotated 90° about center (5,1): the box now spans x∈[4,6], y∈[-4,6].
        assert!(p.contains(size, (5.0, 5.0)));
        assert!(!p.contains(size, (9.0, 1.0)));
    }

    #[test]
    fn layout_reconcile_fills_missing_and_drops_unknown() {
        let mut layout = CanvasLayout {
            version: 1,
            devices: vec![PlacedDevice::new(
                "bogus",
                Placement {
                    x: 0.0,
                    y: 0.0,
                    scale: 1.0,
                    rotation_deg: 0.0,
                },
            )],
            settings: EffectSettings::default(),
            chains: Chains::default(),
        };
        layout.reconcile();
        assert!(layout.device("bogus").is_none());
        assert_eq!(layout.devices.len(), all_specs(&layout.chains).len());
    }

    #[test]
    fn chain_split_reconciles_segment_devices() {
        let mut layout = CanvasLayout::default();
        assert!(layout.device("dled1.0").is_some());
        assert!(layout.device("dled1.1").is_none());

        // Split header 1 into 4 fans: 4 placeable devices, LEDs conserved.
        layout.chains.set_fans(CHAIN_DLED1, 4);
        layout.reconcile();
        for i in 0..4 {
            let id = format!("dled1.{i}");
            let spec = layout.spec(&id).expect("fan spec");
            assert!(spec.is_fan);
            assert!(layout.device(&id).is_some(), "{id} missing");
        }
        let total: usize = layout
            .chains
            .dled1
            .iter()
            .map(|s| s.leds as usize)
            .sum();
        assert_eq!(total, dled1_count() as usize);

        // Back to a single strip: fan devices dropped.
        layout.chains.set_fans(CHAIN_DLED1, 0);
        layout.reconcile();
        assert!(layout.device("dled1.1").is_none());
        assert!(!layout.spec("dled1.0").unwrap().is_fan);
    }

    #[test]
    fn fan_spec_ring_geometry() {
        let spec = fan_spec("f".into(), "Fan".into(), 12);
        assert_eq!(spec.led_positions.len(), 12);
        // LED 0 at 12 o'clock.
        let (x0, y0) = spec.led_positions[0];
        assert!((x0 - 2.0).abs() < 1e-4 && (y0 - 0.5).abs() < 1e-4);
        // All on the ring.
        for &(x, y) in &spec.led_positions {
            let d = ((x - 2.0).powi(2) + (y - 2.0).powi(2)).sqrt();
            assert!((d - 1.5).abs() < 1e-4);
        }
    }

    #[test]
    fn layout_persistence_roundtrip() {
        let dir = std::env::temp_dir().join(format!("cntrl-canvas-test-{}", std::process::id()));
        let path = dir.join("layout.json");
        // Env-based override; serialize/deserialize through the real fns.
        unsafe { std::env::set_var("CNTRL_CANVAS_LAYOUT_PATH", &path) };
        let mut layout = CanvasLayout::default();
        layout.device_mut(DEV_GPU).unwrap().placement.x = 42.5;
        save_layout(&layout).unwrap();
        let loaded = load_layout();
        assert_eq!(loaded.device(DEV_GPU).unwrap().placement.x, 42.5);
        unsafe { std::env::remove_var("CNTRL_CANVAS_LAYOUT_PATH") };
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn keyboard_spec_matches_layout() {
        let spec = keyboard_spec();
        assert_eq!(
            spec.led_positions.len(),
            keyboard_layout::get_keyboard_layout().len()
        );
        // All key centers inside the declared box.
        for &(x, y) in &spec.led_positions {
            assert!(x > 0.0 && x < spec.size.0);
            assert!(y > 0.0 && y < spec.size.1);
        }
    }

    #[test]
    fn sample_device_applies_brightness() {
        let mut f = Frame::canvas_sized();
        for y in 0..f.height {
            for x in 0..f.width {
                f.set(x, y, [200, 100, 50]);
            }
        }
        let spec = gpu_spec();
        let pl = Placement {
            x: 40.0,
            y: 40.0,
            scale: 2.0,
            rotation_deg: 0.0,
        };
        let placed = PlacedDevice::new(DEV_GPU, pl);
        let full = sample_device(&f, &spec, &placed, 100.0);
        let half = sample_device(&f, &spec, &placed, 50.0);
        assert_eq!(full[0], [200, 100, 50]);
        assert_eq!(half[0], [100, 50, 25]);

        // Per-device brightness stacks with global.
        let mut dim = PlacedDevice::new(DEV_GPU, pl);
        dim.brightness = 50.0;
        let quarter = sample_device(&f, &spec, &dim, 50.0);
        assert_eq!(quarter[0], [50, 25, 12]);

        // Hue shift by 120° turns pure red into pure green.
        for y in 0..f.height {
            for x in 0..f.width {
                f.set(x, y, [255, 0, 0]);
            }
        }
        let mut shifted = PlacedDevice::new(DEV_GPU, pl);
        shifted.hue_shift = 120.0;
        let c = sample_device(&f, &spec, &shifted, 100.0);
        assert_eq!(c[0], [0, 255, 0]);

        // Saturation 0 washes to white.
        let mut washed = PlacedDevice::new(DEV_GPU, pl);
        washed.saturation = 0.0;
        let c = sample_device(&f, &spec, &washed, 100.0);
        assert_eq!(c[0], [255, 255, 255]);
    }
}
