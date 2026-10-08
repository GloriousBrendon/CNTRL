//! LED canvas: SignalRGB-style effect→LED mapping, fully native.
//!
//! One virtual canvas (`CANVAS_W`×`CANVAS_H` units) is shared by every device.
//! Each device declares its LED geometry (`LedSpec`: local bounding box + one
//! `[x,y]` per LED); a `Placement` (position/scale/rotation) puts that box on
//! the canvas. Effects render into a `Frame` (a plain RGB framebuffer) and each
//! LED samples the pixel under it — the effect and the hardware never need to
//! know about each other, so one animation flows across the keyboard, ARGB
//! strips, GPU and RAM in physical-desk order.

use crate::canvas_effects::EffectKind;
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
/// 12V non-addressable RGB header (ITE zone 2) — one color for the whole
/// device, sampled as the average under its box.
pub const DEV_RGB12V: &str = "rgb12v";

/// The motherboard's own lit zones (ITE zones that are neither an ARGB header
/// nor the 12V header), as `(canvas id, zone index)`. Which of them a board
/// really wires up varies; they start disabled, and Identify shows what each is.
pub fn board_zones() -> Vec<(String, u8)> {
    let rgb12v = crate::ite_rgb::rgb12v_zone();
    (0..8u8)
        // Zones 3 and 4 are the D_LED1 / D_LED2 strip headers.
        .filter(|z| ![3, 4, rgb12v].contains(z))
        .map(|z| (format!("mbzone{z}"), z))
        .collect()
}

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

/// LEDs per fan when a header first gets fans (Corsair AR120 and many slim
/// rings; most others are 12 or 16).
const DEFAULT_FAN_LEDS: u8 = 8;

impl Chains {
    pub fn header(&self, header: &str) -> &[ChainSegment] {
        match header {
            CHAIN_DLED1 => &self.dled1,
            _ => &self.dled2,
        }
    }

    fn header_mut(&mut self, header: &str) -> &mut Vec<ChainSegment> {
        match header {
            CHAIN_DLED1 => &mut self.dled1,
            _ => &mut self.dled2,
        }
    }

    /// Set every fan on `header` to `leds` LEDs.
    pub fn set_fan_leds(&mut self, header: &str, leds: u8) {
        for seg in self.header_mut(header).iter_mut() {
            if seg.kind == SegmentKind::Fan {
                seg.leds = leds.max(1);
            }
        }
    }

    /// Put `n` fans first on `header`'s chain. Each keeps the header's current
    /// LEDs-per-fan, and a trailing strip stays. The chain's length is whatever
    /// its segments add up to — a fan's LED count is a property of the fan, not
    /// a share of some header total.
    pub fn set_fans(&mut self, header: &str, n: u8) {
        let segments = self.header_mut(header);
        let per_fan = segments
            .iter()
            .find(|s| s.kind == SegmentKind::Fan)
            .map_or(DEFAULT_FAN_LEDS, |s| s.leds);
        let strips = segments.iter().filter(|s| s.kind == SegmentKind::Strip).copied();
        *segments = (0..n)
            .map(|_| ChainSegment {
                kind: SegmentKind::Fan,
                leds: per_fan,
            })
            .chain(strips)
            .collect();
    }

    /// Set the strip at the end of `header`'s chain to `leds` LEDs; 0 removes
    /// it. A header with no fans and no strip is empty: nothing is placed on
    /// the canvas for it and the stream never writes to it.
    pub fn set_strip_leds(&mut self, header: &str, leds: u8) {
        let segments = self.header_mut(header);
        segments.retain(|s| s.kind != SegmentKind::Strip);
        if leds > 0 {
            segments.push(ChainSegment {
                kind: SegmentKind::Strip,
                leds,
            });
        }
    }

    pub fn strip_leds(&self, header: &str) -> u8 {
        self.header(header)
            .iter()
            .find(|s| s.kind == SegmentKind::Strip)
            .map_or(0, |s| s.leds)
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
    out.push(rgb12v_spec());
    for (id, zone) in board_zones() {
        // One color per zone — a few sample points, averaged like the 12V header.
        out.push(LedSpec {
            id,
            name: format!("Board zone {zone}"),
            size: (4.0, 1.5),
            led_positions: vec![(0.75, 0.75), (2.0, 0.75), (3.25, 0.75)],
            is_fan: false,
            streamable: true,
        });
    }
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

/// 12V RGB header: whole strip is one color — a handful of sample points
/// along the box, averaged.
fn rgb12v_spec() -> LedSpec {
    let n = 5usize;
    let led_positions = (0..n).map(|i| (i as f32 * 2.0 + 1.0, 0.75)).collect();
    LedSpec {
        id: DEV_RGB12V.to_string(),
        name: "RGB 12V".to_string(),
        size: (10.0, 1.5),
        led_positions,
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
    /// The hardware behind this device was detected. Refreshed by
    /// [`CanvasLayout::mark_present`]; absent devices keep their placement
    /// but are neither shown nor written.
    #[serde(default = "default_true")]
    pub present: bool,
}

impl PlacedDevice {
    /// Should the stream sample and write this device?
    pub fn active(&self) -> bool {
        self.enabled && self.present
    }
}

/// The backend (`devices::BACKEND_*`) that drives a canvas device id.
fn backend_of(id: &str) -> &'static str {
    use crate::devices::*;
    match id {
        DEV_KEYBOARD => BACKEND_KEYBOARD,
        DEV_GPU => BACKEND_GPU,
        DEV_RAM0 | DEV_RAM1 => BACKEND_CORSAIR,
        // ARGB headers, the 12V header and the board zones all hang off the ITE chip.
        _ => BACKEND_ITE,
    }
}

/// Backends whose hardware is not on this machine right now (probes devices).
pub fn absent_backends() -> Vec<&'static str> {
    crate::devices::enumerate()
        .into_iter()
        .filter(|d| d.status == crate::devices::DeviceStatus::Absent)
        .map(|d| d.backend_id)
        .collect()
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
            present: true,
        }
    }
}

fn default_true() -> bool {
    true
}

fn default_100() -> f32 {
    100.0
}

/// The per-effect part of the settings, remembered for each effect so
/// switching back to one restores how it was tuned.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EffectValues {
    pub speed: f32,
    pub scale: f32,
    #[serde(default)]
    pub angle: f32,
    pub palette: Vec<[u8; 3]>,
}

/// How an overlay effect is mixed onto the base effect.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Blend {
    /// Overlay replaces the base (at its opacity).
    Normal,
    /// Light adds up — sparkles over a color.
    Add,
    /// Like Add but never clips; softer.
    Screen,
    /// Overlay darkens — a moving mask over the base.
    Multiply,
}

impl Blend {
    pub const ALL: [Blend; 4] = [Blend::Add, Blend::Screen, Blend::Multiply, Blend::Normal];

    pub fn label(self) -> &'static str {
        match self {
            Blend::Normal => "Normal",
            Blend::Add => "Add",
            Blend::Screen => "Screen",
            Blend::Multiply => "Multiply",
        }
    }

    /// Mix one channel of `top` onto `base` at `opacity` (0..=1).
    fn mix(self, base: u8, top: u8, opacity: f32) -> u8 {
        let (a, b) = (base as f32, top as f32);
        let blended = match self {
            Blend::Normal => b,
            Blend::Add => (a + b).min(255.0),
            Blend::Screen => 255.0 - (255.0 - a) * (255.0 - b) / 255.0,
            Blend::Multiply => a * b / 255.0,
        };
        (a + (blended - a) * opacity.clamp(0.0, 1.0)).round() as u8
    }
}

/// A second effect drawn over the base one. It uses that effect's own saved
/// tuning (speed, colors…) — tune it by selecting it as the main effect once.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Overlay {
    pub effect: EffectKind,
    pub blend: Blend,
    /// 0–100.
    pub opacity: f32,
}

/// Canvas-wide effect settings that persist with the layout. `speed`, `scale`
/// and `palette` belong to the active effect; other effects' values wait in
/// `saved`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct EffectSettings {
    pub effect: EffectKind,
    pub speed: f32,
    pub scale: f32,
    /// Direction in degrees, for effects that travel.
    pub angle: f32,
    /// 0–100.
    pub brightness: f32,
    pub palette: Vec<[u8; 3]>,
    /// Single color of pre-palette layout files; folded into `palette` on load.
    #[serde(skip_serializing)]
    color: Option<[u8; 3]>,
    /// Use the desktop theme's accent as the first palette color.
    pub accent_from_theme: bool,
    pub fps: u32,
    pub saved: std::collections::BTreeMap<EffectKind, EffectValues>,
    pub overlay: Option<Overlay>,
    /// Scale the whole canvas brightness with the music's loudness.
    pub audio_brightness: bool,
}

impl Default for EffectSettings {
    fn default() -> Self {
        Self {
            effect: EffectKind::RainbowWave,
            speed: 1.0,
            scale: 1.0,
            angle: 0.0,
            brightness: 100.0,
            palette: Vec::new(),
            color: None,
            accent_from_theme: false,
            fps: 20,
            saved: Default::default(),
            overlay: None,
            audio_brightness: false,
        }
    }
}

impl EffectSettings {
    /// Switch effect: stash the current effect's values, bring back the new
    /// one's (or defaults the first time).
    pub fn set_effect(&mut self, effect: EffectKind) {
        if effect == self.effect {
            return;
        }
        let stashed = EffectValues {
            speed: self.speed,
            scale: self.scale,
            angle: self.angle,
            palette: std::mem::take(&mut self.palette),
        };
        self.saved.insert(self.effect, stashed);
        let v = self.saved.remove(&effect).unwrap_or(EffectValues {
            speed: 1.0,
            scale: 1.0,
            angle: 0.0,
            palette: Vec::new(),
        });
        self.effect = effect;
        (self.speed, self.scale, self.angle, self.palette) = (v.speed, v.scale, v.angle, v.palette);
        self.normalize();
    }

    /// Fold in the legacy color and make the palette exactly as long as the
    /// active effect needs (kept colors first, defaults for the rest).
    fn normalize(&mut self) {
        if let Some(c) = self.color.take() {
            if self.palette.is_empty() {
                self.palette.push(c);
            }
        }
        let want = self.effect.controls().colors;
        for i in self.palette.len()..want {
            self.palette.push(self.effect.default_color(i));
        }
        // Extra colors are kept (an effect with fewer simply ignores them).
    }

    /// Does anything in these settings react to sound?
    pub fn needs_audio(&self) -> bool {
        self.audio_brightness
            || self.effect.uses_audio()
            || self.overlay.is_some_and(|o| o.effect.uses_audio())
    }

    /// Parameters for an effect that isn't the active one: its saved tuning,
    /// or its defaults.
    fn params_for(&self, effect: EffectKind) -> crate::canvas_effects::EffectParams {
        if effect == self.effect {
            return self.params();
        }
        let wanted = effect.controls().colors;
        let saved = self.saved.get(&effect);
        crate::canvas_effects::EffectParams {
            speed: saved.map_or(1.0, |v| v.speed),
            scale: saved.map_or(1.0, |v| v.scale),
            angle: saved.map_or(0.0, |v| v.angle),
            palette: (0..wanted)
                .map(|i| {
                    saved
                        .and_then(|v| v.palette.get(i).copied())
                        .unwrap_or(effect.default_color(i))
                })
                .collect(),
        }
    }

    /// Draw the whole look — base effect, overlay, audio brightness — into
    /// `frame`. The one place preview and hardware both render through.
    pub fn render_into(&self, frame: &mut Frame, t: f32, input: &crate::canvas_effects::EffectInput) {
        use crate::canvas_effects::render;
        render(self.effect, frame, t, &self.params(), input);
        if let Some(overlay) = self.overlay {
            let mut top = Frame::new(frame.width, frame.height);
            render(overlay.effect, &mut top, t, &self.params_for(overlay.effect), input);
            let opacity = overlay.opacity / 100.0;
            for (base, top) in frame.px.iter_mut().zip(&top.px) {
                for c in 0..3 {
                    base[c] = overlay.blend.mix(base[c], top[c], opacity);
                }
            }
        }
        if self.audio_brightness {
            // Never fully dark: silence dims the scene instead of blanking it.
            let k = 0.2 + 0.8 * input.audio.level.clamp(0.0, 1.0);
            for px in frame.px.iter_mut() {
                *px = px.map(|c| (c as f32 * k) as u8);
            }
        }
    }

    /// What the effect functions take. Resolves the theme accent if enabled.
    pub fn params(&self) -> crate::canvas_effects::EffectParams {
        let mut palette = self.palette.clone();
        if self.accent_from_theme {
            if let (Some(first), Some(accent)) = (palette.first_mut(), theme_accent()) {
                *first = accent;
            }
        }
        crate::canvas_effects::EffectParams {
            speed: self.speed,
            scale: self.scale,
            angle: self.angle,
            palette,
        }
    }
}

/// The theme accent, re-read from disk at most once a second (this is called
/// per rendered frame).
fn theme_accent() -> Option<[u8; 3]> {
    use std::sync::Mutex;
    use std::time::{Duration, Instant};
    static CACHE: Mutex<Option<(Instant, Option<[u8; 3]>)>> = Mutex::new(None);
    let mut cache = CACHE.lock().ok()?;
    if let Some((at, accent)) = *cache {
        if at.elapsed() < Duration::from_secs(1) {
            return accent;
        }
    }
    let accent = crate::theme_sync::current_theme_color()
        .ok()
        .map(|(r, g, b)| [r, g, b]);
    *cache = Some((Instant::now(), accent));
    accent
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
        if let Some(zone) = id.strip_prefix("mbzone") {
            let i: f32 = zone.parse().unwrap_or(0.0);
            return place(140.0, 34.0 + i * 6.0, 3.0);
        }
        match id {
            DEV_KEYBOARD => place(50.0, 35.0, 4.0),
            DEV_GPU => place(128.0, 42.0, 4.0),
            DEV_RAM0 => place(126.0, 12.0, 2.2),
            DEV_RAM1 => place(140.0, 12.0, 2.2),
            DEV_RGB12V => place(118.0, 60.0, 3.0),
            _ => place(70.0, 40.0, 3.0),
        }
    }

    /// Drop placements for unknown ids and add defaults for missing devices,
    /// so layouts survive catalog and chain-config changes.
    pub fn reconcile(&mut self) {
        self.settings.normalize();
        let specs = all_specs(&self.chains);
        self.devices
            .retain(|d| specs.iter().any(|s| s.id == d.id));
        for spec in &specs {
            if !self.devices.iter().any(|d| d.id == spec.id) {
                let placement = self.default_placement(&spec.id);
                let mut placed = PlacedDevice::new(spec.id.clone(), placement);
                // Opt-in: most boards wire up only some zones, and writing a
                // zone takes it away from the firmware's own lighting.
                placed.enabled = !spec.id.starts_with("mbzone");
                self.devices.push(placed);
            }
        }
    }

    /// Flag which devices have hardware behind them (see [`absent_backends`]).
    /// A backend can report several devices; one of them present is enough.
    pub fn mark_present(&mut self, absent: &[&str]) {
        for d in &mut self.devices {
            d.present = !absent.contains(&backend_of(&d.id));
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
    fn chain_segments_keep_their_own_led_counts() {
        let mut layout = CanvasLayout::default();
        assert!(layout.device("dled1.0").is_some());

        // Three 8-LED fans replace nothing: the default strip stays behind them.
        layout.chains.set_fans(CHAIN_DLED1, 3);
        layout.chains.set_strip_leds(CHAIN_DLED1, 0);
        layout.reconcile();
        let leds: Vec<u8> = layout.chains.dled1.iter().map(|s| s.leds).collect();
        assert_eq!(leds, [8, 8, 8]);
        assert!(layout.spec("dled1.2").unwrap().is_fan);

        // Per-fan LEDs survive a change of fan count.
        layout.chains.set_fan_leds(CHAIN_DLED1, 12);
        layout.chains.set_fans(CHAIN_DLED1, 2);
        layout.chains.set_strip_leds(CHAIN_DLED1, 30);
        layout.reconcile();
        let segs: Vec<(SegmentKind, u8)> =
            layout.chains.dled1.iter().map(|s| (s.kind, s.leds)).collect();
        assert_eq!(
            segs,
            [(SegmentKind::Fan, 12), (SegmentKind::Fan, 12), (SegmentKind::Strip, 30)]
        );
        assert!(layout.device("dled1.2").is_some());
        assert_eq!(layout.spec("dled1.2").unwrap().led_positions.len(), 30);

        // Empty header: no canvas devices at all.
        layout.chains.set_fans(CHAIN_DLED1, 0);
        layout.chains.set_strip_leds(CHAIN_DLED1, 0);
        layout.reconcile();
        assert!(layout.chains.dled1.is_empty());
        assert!(layout.device("dled1.0").is_none());
    }

    #[test]
    fn effect_settings_migrate_and_remember_per_effect() {
        // A pre-palette layout file: single `color`, no palette/saved keys.
        let old = r#"{"effect":"Comet","speed":1.1,"scale":3.0,"brightness":100.0,
                      "color":[1,2,3],"fps":40}"#;
        let mut s: EffectSettings = serde_json::from_str(old).unwrap();
        s.normalize();
        assert_eq!(s.palette, vec![[1, 2, 3]]);
        assert_eq!(s.params().color(0), [1, 2, 3]);

        // Switching away and back restores Comet's own tuning.
        s.set_effect(EffectKind::Plasma);
        assert_eq!((s.speed, s.scale), (1.0, 1.0));
        s.speed = 2.5;
        s.set_effect(EffectKind::Comet);
        assert_eq!((s.speed, s.scale), (1.1, 3.0));
        assert_eq!(s.palette, vec![[1, 2, 3]]);
        s.set_effect(EffectKind::Plasma);
        assert_eq!(s.speed, 2.5);

        // The legacy key is not written back; the new shape round-trips.
        let json = serde_json::to_string(&s).unwrap();
        assert!(!json.contains("\"color\""));
        let back: EffectSettings = serde_json::from_str(&json).unwrap();
        assert_eq!(back.saved[&EffectKind::Comet].palette, vec![[1, 2, 3]]);
    }

    #[test]
    fn absent_hardware_is_hidden_but_keeps_its_placement() {
        let mut layout = CanvasLayout::default();
        layout.device_mut(DEV_GPU).unwrap().placement.x = 42.5;
        layout.mark_present(&[crate::devices::BACKEND_GPU, crate::devices::BACKEND_ITE]);
        assert!(!layout.device(DEV_GPU).unwrap().active());
        assert!(!layout.device("dled1.0").unwrap().active());
        assert!(!layout.device(DEV_RGB12V).unwrap().active());
        assert!(layout.device(DEV_KEYBOARD).unwrap().active());
        assert!(layout.device(DEV_RAM0).unwrap().active());

        // Plugged back in: same spot as before.
        layout.mark_present(&[]);
        let gpu = layout.device(DEV_GPU).unwrap();
        assert!(gpu.active());
        assert_eq!(gpu.placement.x, 42.5);
    }

    #[test]
    fn blend_modes_on_fixed_pixels() {
        assert_eq!(Blend::Normal.mix(100, 200, 1.0), 200);
        assert_eq!(Blend::Normal.mix(100, 200, 0.5), 150);
        assert_eq!(Blend::Add.mix(100, 200, 1.0), 255);
        assert_eq!(Blend::Add.mix(100, 50, 1.0), 150);
        assert_eq!(Blend::Screen.mix(0, 200, 1.0), 200);
        assert_eq!(Blend::Screen.mix(255, 200, 1.0), 255);
        assert_eq!(Blend::Multiply.mix(255, 100, 1.0), 100);
        assert_eq!(Blend::Multiply.mix(100, 0, 1.0), 0);
        // Zero opacity leaves the base alone in every mode.
        for blend in Blend::ALL {
            assert_eq!(blend.mix(77, 200, 0.0), 77);
        }
    }

    #[test]
    fn overlay_and_audio_brightness_change_the_frame() {
        let input = crate::canvas_effects::EffectInput::default();
        let mut s = EffectSettings::default();
        s.set_effect(EffectKind::Solid);
        let mut base = Frame::new(32, 18);
        s.render_into(&mut base, 1.0, &input);
        assert_eq!(base.get(5, 5), s.palette[0]);

        // Twinkle added on top: no pixel gets darker, some get brighter.
        s.overlay = Some(Overlay {
            effect: EffectKind::Twinkle,
            blend: Blend::Add,
            opacity: 100.0,
        });
        let mut layered = Frame::new(32, 18);
        s.render_into(&mut layered, 1.0, &input);
        assert!(layered.px.iter().zip(&base.px).all(|(l, b)| (0..3).all(|c| l[c] >= b[c])));
        assert_ne!(layered.px, base.px);
        assert!(!s.needs_audio());

        // In silence, audio brightness dims the scene to its floor.
        s.overlay = None;
        s.audio_brightness = true;
        assert!(s.needs_audio());
        let mut dim = Frame::new(32, 18);
        s.render_into(&mut dim, 1.0, &input);
        assert_eq!(dim.get(5, 5), s.palette[0].map(|c| (c as f32 * 0.2) as u8));
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
