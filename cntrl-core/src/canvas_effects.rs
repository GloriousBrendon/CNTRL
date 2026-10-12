//! Demo canvas effects — procedural, zero assets, pure functions of time.
//!
//! Every effect is `f(pixel, t, params) → color`, so the UI preview and the
//! hardware stream render identical frames without sharing state.

use crate::canvas::Frame;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum EffectKind {
    // Everyday
    Solid,
    Gradient,
    RadialGradient,
    Breathing,
    SpectrumCycle,
    RainbowWave,
    RainbowRadial,
    RainbowSpiral,
    ColorWipe,
    // Ambient
    Plasma,
    Aurora,
    Lava,
    Fire,
    Waves,
    Twinkle,
    Rain,
    Matrix,
    // Motion
    Ripple,
    NeonPulse,
    Comet,
    Scanner,
    RadarSweep,
    BouncingBalls,
    // Audio-reactive
    SpectrumBars,
    AudioWave,
    BeatPulse,
    // Screen-reactive
    ScreenAmbient,
}

const WHITE: [u8; 3] = [255, 255, 255];
const RAIN_BLUE: [u8; 3] = [60, 140, 255];
const MATRIX_GREEN: [u8; 3] = [0, 255, 70];

/// Everything the UI and CLI need to know about one effect.
struct Meta {
    slug: &'static str,
    label: &'static str,
    speed: bool,
    scale: bool,
    angle: bool,
    /// Palette colors the effect reads; `own` are its defaults for them
    /// (the shared default palette fills any it doesn't name).
    colors: usize,
    own: &'static [[u8; 3]],
}

const fn meta(slug: &'static str, label: &'static str, colors: usize) -> Meta {
    Meta {
        slug,
        label,
        speed: true,
        scale: true,
        angle: false,
        colors,
        own: &[],
    }
}

impl EffectKind {
    pub const ALL: [EffectKind; 27] = [
        EffectKind::Solid,
        EffectKind::Gradient,
        EffectKind::RadialGradient,
        EffectKind::Breathing,
        EffectKind::SpectrumCycle,
        EffectKind::RainbowWave,
        EffectKind::RainbowRadial,
        EffectKind::RainbowSpiral,
        EffectKind::ColorWipe,
        EffectKind::Plasma,
        EffectKind::Aurora,
        EffectKind::Lava,
        EffectKind::Fire,
        EffectKind::Waves,
        EffectKind::Twinkle,
        EffectKind::Rain,
        EffectKind::Matrix,
        EffectKind::Ripple,
        EffectKind::NeonPulse,
        EffectKind::Comet,
        EffectKind::Scanner,
        EffectKind::RadarSweep,
        EffectKind::BouncingBalls,
        EffectKind::SpectrumBars,
        EffectKind::AudioWave,
        EffectKind::BeatPulse,
        EffectKind::ScreenAmbient,
    ];

    /// Reacts to system audio (the stream captures it only while one is active).
    pub fn uses_audio(self) -> bool {
        use EffectKind::*;
        matches!(self, SpectrumBars | AudioWave | BeatPulse)
    }

    /// Shows the screen (the stream captures it only while one is active).
    pub fn uses_screen(self) -> bool {
        self == EffectKind::ScreenAmbient
    }

    fn meta(self) -> Meta {
        use EffectKind::*;
        match self {
            Solid => Meta { speed: false, scale: false, ..meta("solid", "Solid", 1) },
            Gradient => Meta { angle: true, ..meta("gradient", "Gradient", 3) },
            RadialGradient => meta("radial", "Radial Gradient", 3),
            Breathing => Meta { scale: false, ..meta("breathing", "Breathing", 1) },
            SpectrumCycle => Meta { scale: false, ..meta("spectrum", "Spectrum Cycle", 0) },
            RainbowWave => Meta { angle: true, ..meta("wave", "Rainbow Wave", 0) },
            RainbowRadial => meta("rainbow-radial", "Rainbow Radial", 0),
            RainbowSpiral => meta("spiral", "Rainbow Spiral", 0),
            ColorWipe => Meta { scale: false, angle: true, ..meta("wipe", "Color Wipe", 3) },
            Plasma => meta("plasma", "Plasma", 0),
            Aurora => meta("aurora", "Aurora", 0),
            Lava => meta("lava", "Lava", 3),
            Fire => meta("fire", "Fire", 0),
            Waves => meta("waves", "Waves", 2),
            Twinkle => Meta { own: &[WHITE], ..meta("twinkle", "Twinkle", 1) },
            Rain => Meta { own: &[RAIN_BLUE], ..meta("rain", "Rain", 1) },
            Matrix => Meta { own: &[MATRIX_GREEN], ..meta("matrix", "Matrix", 1) },
            Ripple => meta("ripple", "Ripple", 1),
            NeonPulse => meta("neon", "Neon Pulse", 2),
            Comet => meta("comet", "Comet", 1),
            Scanner => Meta { angle: true, ..meta("scanner", "Scanner", 1) },
            RadarSweep => meta("radar", "Radar Sweep", 1),
            BouncingBalls => meta("balls", "Bouncing Balls", 3),
            SpectrumBars => Meta { speed: false, scale: false, ..meta("bars", "Spectrum Bars", 2) },
            AudioWave => Meta { speed: false, scale: false, ..meta("audio-wave", "Audio Wave", 2) },
            BeatPulse => Meta { speed: false, ..meta("beat", "Beat Pulse", 1) },
            ScreenAmbient => Meta { speed: false, scale: false, ..meta("screen", "Screen Ambient", 0) },
        }
    }

    pub fn label(self) -> &'static str {
        self.meta().label
    }

    /// CLI-friendly id (`cntrl canvas --effect <slug>`).
    pub fn slug(self) -> &'static str {
        self.meta().slug
    }

    /// Which controls this effect actually reads — the UI shows only these.
    pub fn controls(self) -> EffectControls {
        let m = self.meta();
        EffectControls {
            speed: m.speed,
            scale: m.scale,
            angle: m.angle,
            colors: m.colors,
        }
    }

    /// Default for palette color `i` of this effect.
    pub fn default_color(self, i: usize) -> [u8; 3] {
        self.meta()
            .own
            .get(i)
            .copied()
            .unwrap_or(DEFAULT_PALETTE[i % DEFAULT_PALETTE.len()])
    }

    pub fn from_slug(s: &str) -> Option<Self> {
        let t = s.trim().to_ascii_lowercase();
        Self::ALL.into_iter().find(|e| e.slug() == t)
    }
}

impl std::fmt::Display for EffectKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.label())
    }
}

/// The controls an effect declares (see [`EffectKind::controls`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EffectControls {
    pub speed: bool,
    pub scale: bool,
    /// Direction of travel, in degrees.
    pub angle: bool,
    /// How many palette colors the effect uses (0 = it makes its own).
    pub colors: usize,
}

/// Starting palette; also pads a palette when an effect wants more colors.
pub const DEFAULT_PALETTE: [[u8; 3]; 6] = [
    [216, 143, 146],
    [0, 179, 255],
    [140, 82, 255],
    [0, 220, 130],
    [255, 170, 0],
    [255, 60, 90],
];

/// Effect inputs. `speed`/`scale` are multipliers around 1.0; `palette` feeds
/// the color-based effects, in the order the user arranged it.
#[derive(Debug, Clone)]
pub struct EffectParams {
    pub speed: f32,
    pub scale: f32,
    /// Degrees; 0 = left to right, 90 = top to bottom.
    pub angle: f32,
    pub palette: Vec<[u8; 3]>,
}

impl EffectParams {
    /// Palette color `i`, falling back to the default palette when the user's
    /// is shorter than the effect needs.
    pub fn color(&self, i: usize) -> [u8; 3] {
        self.palette
            .get(i)
            .copied()
            .unwrap_or(DEFAULT_PALETTE[i % DEFAULT_PALETTE.len()])
    }
}

impl Default for EffectParams {
    fn default() -> Self {
        Self {
            speed: 1.0,
            scale: 1.0,
            angle: 0.0,
            palette: DEFAULT_PALETTE.to_vec(),
        }
    }
}

/// Live data an effect may react to. Everything defaults to "nothing there"
/// (silence), so effects that ignore it stay pure functions of time.
#[derive(Debug, Clone, Default)]
pub struct EffectInput {
    pub audio: crate::audio::AudioLevels,
    /// The screen, already shrunk to the canvas; `None` when not captured.
    pub screen: Option<std::sync::Arc<Frame>>,
}

/// Render one frame of `kind` at time `t` (seconds since stream start).
pub fn render(kind: EffectKind, frame: &mut Frame, t: f32, params: &EffectParams, input: &EffectInput) {
    use EffectKind::*;
    match kind {
        SpectrumBars => spectrum_bars(frame, params, input),
        AudioWave => audio_wave(frame, params, input),
        BeatPulse => beat_pulse(frame, params, input),
        ScreenAmbient => match &input.screen {
            Some(screen) => fill(frame, |nx, ny| {
                let x = ((nx * screen.width as f32) as usize).min(screen.width - 1);
                let y = ((ny * screen.height as f32) as usize).min(screen.height - 1);
                screen.px[y * screen.width + x]
            }),
            // No picture (capture failed or not running): dark, not stale.
            None => fill(frame, |_, _| [0, 0, 0]),
        },
        RainbowWave => rainbow_wave(frame, t, params),
        Plasma => plasma(frame, t, params),
        Ripple => ripple(frame, t, params),
        Comet => comet(frame, t, params),
        Aurora => aurora(frame, t, params),
        Solid => fill(frame, |_, _| params.color(0)),
        Gradient => gradient(frame, t, params),
        RadialGradient => radial_gradient(frame, t, params),
        Breathing => breathing(frame, t, params),
        SpectrumCycle => {
            let c = hsv_to_rgb(t * params.speed * 40.0, 1.0, 1.0);
            fill(frame, |_, _| c)
        }
        RainbowRadial => rainbow_radial(frame, t, params),
        RainbowSpiral => rainbow_spiral(frame, t, params),
        ColorWipe => color_wipe(frame, t, params),
        Lava => lava(frame, t, params),
        Fire => fire(frame, t, params),
        Waves => waves(frame, t, params),
        Twinkle => twinkle(frame, t, params),
        Rain => rain(frame, t, params, false),
        Matrix => rain(frame, t, params, true),
        NeonPulse => neon_pulse(frame, t, params),
        Scanner => scanner(frame, t, params),
        RadarSweep => radar_sweep(frame, t, params),
        BouncingBalls => bouncing_balls(frame, t, params),
    }
}

// ── shared building blocks ───────────────────────────────────────────

/// Canvas width / height: multiply a normalized x-distance by this (or use
/// [`centered`]) so circles come out round.
const ASPECT: f32 = 16.0 / 9.0;

/// Fill every pixel from `f(nx, ny)`, both in [0,1).
fn fill(frame: &mut Frame, f: impl Fn(f32, f32) -> [u8; 3]) {
    let (w, h) = (frame.width, frame.height);
    for y in 0..h {
        for x in 0..w {
            let (nx, ny) = norm_xy(frame, x, y);
            frame.set(x, y, f(nx, ny));
        }
    }
}

/// (dx, dy) from the canvas center in height units — isotropic.
fn centered(nx: f32, ny: f32) -> (f32, f32) {
    ((nx - 0.5) * ASPECT, ny - 0.5)
}

/// Position of a pixel along the `angle` direction, 0 at one edge of the
/// canvas and 1 at the other.
fn along(nx: f32, ny: f32, angle_deg: f32) -> f32 {
    let (s, c) = angle_deg.to_radians().sin_cos();
    let (dx, dy) = centered(nx, ny);
    // Half-extent of the canvas rectangle projected onto the direction.
    let half = (c.abs() * ASPECT + s.abs()) * 0.5;
    ((dx * c + dy * s) / half.max(1e-6) + 1.0) * 0.5
}

fn lerp_rgb(a: [u8; 3], b: [u8; 3], k: f32) -> [u8; 3] {
    let k = k.clamp(0.0, 1.0);
    let ch = |i: usize| (a[i] as f32 + (b[i] as f32 - a[i] as f32) * k).round() as u8;
    [ch(0), ch(1), ch(2)]
}

fn scale_rgb(c: [u8; 3], k: f32) -> [u8; 3] {
    let k = k.clamp(0.0, 1.0);
    [(c[0] as f32 * k) as u8, (c[1] as f32 * k) as u8, (c[2] as f32 * k) as u8]
}

fn add_rgb(a: [u8; 3], b: [u8; 3]) -> [u8; 3] {
    [a[0].saturating_add(b[0]), a[1].saturating_add(b[1]), a[2].saturating_add(b[2])]
}

/// The first `n` palette colors as a seamless loop: `x` in [0,1) walks through
/// all of them and back to the first.
fn palette_loop(p: &EffectParams, n: usize, x: f32) -> [u8; 3] {
    let pos = x.rem_euclid(1.0) * n as f32;
    let i = pos as usize % n;
    lerp_rgb(p.color(i), p.color((i + 1) % n), pos.fract())
}

/// Repeatable pseudo-random value in [0,1) for a grid cell — effects stay pure
/// functions of time, so preview and hardware draw the same "random" frame.
fn hash(x: i32, y: i32) -> f32 {
    let mut h = (x as u32).wrapping_mul(0x9E37_79B1) ^ (y as u32).wrapping_mul(0x85EB_CA77);
    h ^= h >> 15;
    h = h.wrapping_mul(0x2C1B_3C6D);
    h ^= h >> 12;
    (h & 0x00FF_FFFF) as f32 / 16_777_216.0
}

/// Smooth 2D value noise in [0,1].
fn noise(x: f32, y: f32) -> f32 {
    let (x0, y0) = (x.floor(), y.floor());
    let (fx, fy) = (x - x0, y - y0);
    let (sx, sy) = (fx * fx * (3.0 - 2.0 * fx), fy * fy * (3.0 - 2.0 * fy));
    let (xi, yi) = (x0 as i32, y0 as i32);
    let top = hash(xi, yi) + (hash(xi + 1, yi) - hash(xi, yi)) * sx;
    let bot = hash(xi, yi + 1) + (hash(xi + 1, yi + 1) - hash(xi, yi + 1)) * sx;
    top + (bot - top) * sy
}

/// Two octaves of [`noise`], still in [0,1].
fn fbm(x: f32, y: f32) -> f32 {
    (noise(x, y) * 2.0 + noise(x * 2.1 + 17.0, y * 2.1 - 9.0)) / 3.0
}

// ── everyday ─────────────────────────────────────────────────────────

/// The palette laid out along `angle`, drifting slowly.
fn gradient(frame: &mut Frame, t: f32, p: &EffectParams) {
    let shift = t * p.speed * 0.05;
    fill(frame, |nx, ny| palette_loop(p, 3, along(nx, ny, p.angle) * 0.67 * p.scale - shift))
}

/// The palette in rings from the center, flowing outward.
fn radial_gradient(frame: &mut Frame, t: f32, p: &EffectParams) {
    let shift = t * p.speed * 0.08;
    fill(frame, |nx, ny| {
        let (dx, dy) = centered(nx, ny);
        palette_loop(p, 3, (dx * dx + dy * dy).sqrt() * p.scale - shift)
    })
}

/// One color fading in and out; never fully dark, so devices don't look off.
fn breathing(frame: &mut Frame, t: f32, p: &EffectParams) {
    let k = 0.06 + 0.94 * (0.5 - 0.5 * (t * p.speed * 1.6).cos()).powf(1.6);
    let c = scale_rgb(p.color(0), k);
    fill(frame, |_, _| c)
}

fn rainbow_radial(frame: &mut Frame, t: f32, p: &EffectParams) {
    fill(frame, |nx, ny| {
        let (dx, dy) = centered(nx, ny);
        hsv_to_rgb((dx * dx + dy * dy).sqrt() * 360.0 * p.scale - t * p.speed * 90.0, 1.0, 1.0)
    })
}

fn rainbow_spiral(frame: &mut Frame, t: f32, p: &EffectParams) {
    fill(frame, |nx, ny| {
        let (dx, dy) = centered(nx, ny);
        let turn = dy.atan2(dx) / std::f32::consts::TAU;
        let d = (dx * dx + dy * dy).sqrt();
        hsv_to_rgb((turn + d * p.scale) * 360.0 - t * p.speed * 90.0, 1.0, 1.0)
    })
}

/// Each palette color sweeps across in turn, replacing the one before it.
fn color_wipe(frame: &mut Frame, t: f32, p: &EffectParams) {
    let phase = t * p.speed * 0.35;
    let (step, edge) = (phase.floor() as usize, phase.fract());
    fill(frame, |nx, ny| {
        let d = along(nx, ny, p.angle);
        // A soft edge a few LEDs wide reads better than a hard line on strips.
        let k = ((edge * 1.1 - d) / 0.1).clamp(0.0, 1.0);
        lerp_rgb(p.color(step % 3), p.color((step + 1) % 3), k)
    })
}

// ── ambient ──────────────────────────────────────────────────────────

/// Slow blobs of the palette colors drifting through each other.
fn lava(frame: &mut Frame, t: f32, p: &EffectParams) {
    let ts = t * p.speed * 0.35;
    let k = 3.0 * p.scale;
    fill(frame, |nx, ny| {
        let (dx, dy) = centered(nx, ny);
        // Interfering sines rather than noise: blobs stay round at any scale.
        let v = (dx * k + ts).sin()
            + (dy * k * 1.3 - ts * 0.7).sin()
            + ((dx + dy) * k * 0.8 + ts * 0.5).sin()
            + ((dx * dx + dy * dy).sqrt() * k * 1.5 - ts).sin();
        palette_loop(p, 3, (v + 4.0) / 8.0 * 1.5)
    })
}

/// Flames rising from the bottom edge.
fn fire(frame: &mut Frame, t: f32, p: &EffectParams) {
    let ts = t * p.speed * 2.2;
    let k = 5.0 * p.scale;
    fill(frame, |nx, ny| {
        let turbulence = fbm(nx * k * ASPECT, ny * k + ts);
        // Hot at the bottom, cooling with height; noise tears the flames.
        let heat = ((ny - 0.35) * 1.3 + (turbulence - 0.5) * 0.9).clamp(0.0, 1.0);
        match heat {
            h if h < 0.33 => lerp_rgb([0, 0, 0], [200, 20, 0], h / 0.33),
            h if h < 0.7 => lerp_rgb([200, 20, 0], [255, 150, 0], (h - 0.33) / 0.37),
            h => lerp_rgb([255, 150, 0], [255, 240, 160], (h - 0.7) / 0.3),
        }
    })
}

/// Two colors in rolling horizontal swells.
fn waves(frame: &mut Frame, t: f32, p: &EffectParams) {
    let ts = t * p.speed;
    fill(frame, |nx, ny| {
        let swell = (nx * 5.0 * p.scale + ts).sin() * 0.12 + (nx * 2.3 * p.scale - ts * 0.6).sin() * 0.08;
        let k = 0.5 + 0.5 * ((ny + swell) * 3.0 * p.scale * std::f32::consts::TAU - ts * 1.5).sin();
        lerp_rgb(p.color(0), p.color(1), k)
    })
}

/// Stars that fade in and out on a dark field.
fn twinkle(frame: &mut Frame, t: f32, p: &EffectParams) {
    let ts = t * p.speed;
    let cells = 22.0 * p.scale;
    fill(frame, |nx, ny| {
        let (cx, cy) = ((nx * cells * ASPECT).floor() as i32, (ny * cells).floor() as i32);
        // Only some cells hold a star; each has its own rhythm.
        if hash(cx, cy) > 0.45 {
            return [0, 0, 0];
        }
        let rate = 0.6 + hash(cy, cx) * 1.8;
        let glow = (0.5 + 0.5 * (ts * rate + hash(cx + 7, cy - 3) * 40.0).sin()).powi(6);
        scale_rgb(lerp_rgb(p.color(0), WHITE, glow * 0.5), glow)
    })
}

/// Falling streaks. `glyphs` chops them into flickering cells (Matrix).
fn rain(frame: &mut Frame, t: f32, p: &EffectParams, glyphs: bool) {
    let ts = t * p.speed;
    let columns = (if glyphs { 44.0 } else { 60.0 }) * p.scale;
    fill(frame, |nx, ny| {
        let col = (nx * columns).floor() as i32;
        let fall = 0.25 + hash(col, 1) * 0.6;
        // Head position wraps past the bottom so a streak leaves fully.
        let head = (ts * fall + hash(col, 2)).fract() * 1.6 - 0.3;
        let behind = head - ny;
        if !(0.0..0.5).contains(&behind) {
            return [0, 0, 0];
        }
        let mut glow = (-behind / 0.14).exp();
        if glyphs {
            let row = (ny * 26.0).floor() as i32;
            glow *= 0.45 + 0.55 * hash(col * 31 + row, (ts * 9.0) as i32);
        }
        // The leading edge burns toward white.
        scale_rgb(lerp_rgb(p.color(0), WHITE, (1.0 - behind / 0.03).clamp(0.0, 1.0) * 0.7), glow)
    })
}

// ── audio-reactive ───────────────────────────────────────────────────

/// Band level under a horizontal position, blended between neighbours so
/// bars don't step on a 160-pixel-wide canvas.
fn band_at(input: &EffectInput, nx: f32) -> f32 {
    let bands = &input.audio.bands;
    let pos = (nx * bands.len() as f32 - 0.5).clamp(0.0, bands.len() as f32 - 1.0);
    let i = pos as usize;
    let next = bands[(i + 1).min(bands.len() - 1)];
    bands[i] + (next - bands[i]) * pos.fract()
}

/// Classic analyser: bass on the left, bars rising from the bottom.
fn spectrum_bars(frame: &mut Frame, p: &EffectParams, input: &EffectInput) {
    fill(frame, |nx, ny| {
        let height = 1.0 - ny;
        let level = band_at(input, nx);
        if height > level {
            return [0, 0, 0];
        }
        lerp_rgb(p.color(0), p.color(1), height)
    })
}

/// The spectrum mirrored around the middle line, like a waveform.
fn audio_wave(frame: &mut Frame, p: &EffectParams, input: &EffectInput) {
    fill(frame, |nx, ny| {
        let reach = band_at(input, nx) * 0.5;
        let off = (ny - 0.5).abs();
        if off >= reach {
            return [0, 0, 0];
        }
        lerp_rgb(p.color(0), p.color(1), off / 0.5)
    })
}

/// A glow at the center that swells with the bass.
fn beat_pulse(frame: &mut Frame, p: &EffectParams, input: &EffectInput) {
    let bands = &input.audio.bands;
    let bass = bands[..4].iter().sum::<f32>() / 4.0;
    let radius = (0.12 + bass * 0.75) * p.scale;
    fill(frame, |nx, ny| {
        let (dx, dy) = centered(nx, ny);
        let gap = (dx * dx + dy * dy).sqrt() / radius;
        scale_rgb(p.color(0), (-gap * gap).exp() * (0.15 + 0.85 * bass))
    })
}

// ── motion ───────────────────────────────────────────────────────────

/// Thin rings bursting from the center, alternating two colors.
fn neon_pulse(frame: &mut Frame, t: f32, p: &EffectParams) {
    let phase = t * p.speed * 0.6;
    fill(frame, |nx, ny| {
        let (dx, dy) = centered(nx, ny);
        let d = (dx * dx + dy * dy).sqrt() * p.scale;
        // Ring number `n` is at radius `phase - n`; light the nearest two.
        let mut out = [0, 0, 0];
        for n in [(phase - d).floor(), (phase - d).floor() + 1.0] {
            let gap = (phase - n - d) * 9.0;
            let color = p.color((n as i64).rem_euclid(2) as usize);
            out = add_rgb(out, scale_rgb(color, (-gap * gap).exp()));
        }
        out
    })
}

/// A bar of light sweeping back and forth along `angle` (KITT).
fn scanner(frame: &mut Frame, t: f32, p: &EffectParams) {
    let phase = (t * p.speed * 0.5).rem_euclid(2.0);
    let head = if phase < 1.0 { phase } else { 2.0 - phase };
    let width = 0.09 / p.scale.max(0.2);
    fill(frame, |nx, ny| {
        let gap = (along(nx, ny, p.angle) - head) / width;
        let glow = (-gap * gap).exp();
        scale_rgb(lerp_rgb(p.color(0), WHITE, glow * glow * 0.5), glow)
    })
}

/// A rotating beam with a fading wake.
fn radar_sweep(frame: &mut Frame, t: f32, p: &EffectParams) {
    let beam = t * p.speed * 0.35;
    fill(frame, |nx, ny| {
        let (dx, dy) = centered(nx, ny);
        let turn = dy.atan2(dx) / std::f32::consts::TAU;
        let behind = (beam - turn).rem_euclid(1.0);
        scale_rgb(p.color(0), (-behind * 5.0 / p.scale.max(0.2)).exp())
    })
}

/// Three glowing balls bouncing around the canvas, one per palette color.
fn bouncing_balls(frame: &mut Frame, t: f32, p: &EffectParams) {
    let ts = t * p.speed;
    // Triangle wave: a straight-line bounce between 0 and 1.
    let bounce = |x: f32| 1.0 - (x.rem_euclid(2.0) - 1.0).abs();
    let balls: [(f32, f32); 3] = std::array::from_fn(|i| {
        let k = i as f32;
        (bounce(ts * (0.23 + 0.07 * k) + k * 0.61), bounce(ts * (0.31 - 0.05 * k) + k * 0.37))
    });
    let radius = 0.11 * p.scale;
    fill(frame, |nx, ny| {
        let mut out = [0, 0, 0];
        for (i, (bx, by)) in balls.iter().enumerate() {
            let (dx, dy) = ((nx - bx) * ASPECT, ny - by);
            let gap = (dx * dx + dy * dy).sqrt() / radius;
            out = add_rgb(out, scale_rgb(p.color(i), (-gap * gap).exp()));
        }
        out
    })
}

/// h ∈ [0,360), s/v ∈ [0,1] → RGB.
pub fn hsv_to_rgb(h: f32, s: f32, v: f32) -> [u8; 3] {
    let h = h.rem_euclid(360.0);
    let c = v * s;
    let x = c * (1.0 - ((h / 60.0) % 2.0 - 1.0).abs());
    let m = v - c;
    let (r, g, b) = match (h / 60.0) as u32 {
        0 => (c, x, 0.0),
        1 => (x, c, 0.0),
        2 => (0.0, c, x),
        3 => (0.0, x, c),
        4 => (x, 0.0, c),
        _ => (c, 0.0, x),
    };
    [
        ((r + m) * 255.0).round() as u8,
        ((g + m) * 255.0).round() as u8,
        ((b + m) * 255.0).round() as u8,
    ]
}

fn norm_xy(frame: &Frame, x: usize, y: usize) -> (f32, f32) {
    (
        x as f32 / frame.width.max(1) as f32,
        y as f32 / frame.height.max(1) as f32,
    )
}

/// Rainbow sweeping across the canvas along `angle`.
fn rainbow_wave(frame: &mut Frame, t: f32, p: &EffectParams) {
    fill(frame, |nx, ny| {
        hsv_to_rgb(along(nx, ny, p.angle) * 360.0 * p.scale - t * p.speed * 90.0, 1.0, 1.0)
    })
}

/// Classic interference plasma.
fn plasma(frame: &mut Frame, t: f32, p: &EffectParams) {
    let ts = t * p.speed;
    let k = 6.0 * p.scale;
    let (w, h) = (frame.width, frame.height);
    for y in 0..h {
        for x in 0..w {
            let (nx, ny) = norm_xy(frame, x, y);
            let v = (nx * k + ts).sin()
                + (ny * k * 0.8 - ts * 1.3).sin()
                + ((nx + ny) * k * 0.6 + ts * 0.7).sin()
                + (((nx - 0.5).powi(2) + (ny - 0.5).powi(2)).sqrt() * k * 1.7 - ts).sin();
            // v ∈ [-4,4] → hue.
            let hue = (v + 4.0) / 8.0 * 360.0;
            frame.set(x, y, hsv_to_rgb(hue, 0.95, 1.0));
        }
    }
}

/// Expanding rings in the user color, hue-shifting slightly per ring.
fn ripple(frame: &mut Frame, t: f32, p: &EffectParams) {
    let ts = t * p.speed;
    let k = 14.0 * p.scale;
    let base = rgb_to_hue(p.color(0));
    let (w, h) = (frame.width, frame.height);
    for y in 0..h {
        for x in 0..w {
            let (nx, ny) = norm_xy(frame, x, y);
            let d = ((nx - 0.5).powi(2) + ((ny - 0.5) * 0.5625).powi(2)).sqrt();
            let ring = (d * k - ts * 2.2).sin();
            // Sharpen crests into distinct rings.
            let i = ((ring + 1.0) / 2.0).powi(3);
            let hue = base + d * 60.0;
            frame.set(x, y, hsv_to_rgb(hue, 0.9, i));
        }
    }
}

/// A bright head bouncing horizontally with an exponential trail.
fn comet(frame: &mut Frame, t: f32, p: &EffectParams) {
    let (w, h) = (frame.width, frame.height);
    // Triangle-wave bounce across [0,1], plus which pass we're on for direction.
    let phase = (t * p.speed * 0.45).rem_euclid(2.0);
    let (head, dir) = if phase < 1.0 {
        (phase, 1.0f32)
    } else {
        (2.0 - phase, -1.0f32)
    };
    let base = rgb_to_hue(p.color(0));
    let trail = 0.18 / p.scale.max(0.2);
    for y in 0..h {
        for x in 0..w {
            let (nx, ny) = norm_xy(frame, x, y);
            // Distance behind the head along the travel direction.
            let behind = (head - nx) * dir;
            let along = if behind >= 0.0 {
                (-behind / trail).exp()
            } else {
                // Just ahead of the head: tight leading glow.
                (behind / 0.02).exp()
            };
            let vert = (-((ny - 0.5) / 0.35).powi(2)).exp();
            let i = (along * vert).clamp(0.0, 1.0);
            // Head is near-white, trail settles into the user color.
            let sat = 1.0 - 0.7 * i;
            frame.set(x, y, hsv_to_rgb(base, sat.min(0.9), i));
        }
    }
}

/// Slow layered curtains in aurora greens/teals/violets.
fn aurora(frame: &mut Frame, t: f32, p: &EffectParams) {
    let ts = t * p.speed * 0.4;
    let k = 3.0 * p.scale;
    let (w, h) = (frame.width, frame.height);
    for y in 0..h {
        for x in 0..w {
            let (nx, ny) = norm_xy(frame, x, y);
            // Two drifting curtains; each is a soft vertical band whose
            // center line wanders with x and time.
            let c1 = (nx * k * 2.0 + ts).sin() * 0.25 + 0.35;
            let c2 = (nx * k * 3.1 - ts * 1.4 + 1.7).sin() * 0.25 + 0.65;
            let b1 = (-((ny - c1) / 0.18).powi(2)).exp();
            let b2 = (-((ny - c2) / 0.22).powi(2)).exp();
            let hue1 = 140.0 + (nx * k + ts).sin() * 25.0; // greens
            let hue2 = 270.0 + (nx * k * 0.7 - ts).sin() * 30.0; // violets
            let (r1, g1, bl1) = scale3(hsv_to_rgb(hue1, 0.9, 1.0), b1);
            let (r2, g2, bl2) = scale3(hsv_to_rgb(hue2, 0.8, 1.0), b2 * 0.8);
            frame.set(
                x,
                y,
                [
                    (r1 + r2).min(255.0) as u8,
                    (g1 + g2).min(255.0) as u8,
                    (bl1 + bl2).min(255.0) as u8,
                ],
            );
        }
    }
}

fn scale3(c: [u8; 3], k: f32) -> (f32, f32, f32) {
    (c[0] as f32 * k, c[1] as f32 * k, c[2] as f32 * k)
}

/// RGB → (hue ∈ [0,360), saturation ∈ [0,1], value ∈ [0,1]).
pub fn rgb_to_hsv(c: [u8; 3]) -> (f32, f32, f32) {
    let r = c[0] as f32 / 255.0;
    let g = c[1] as f32 / 255.0;
    let b = c[2] as f32 / 255.0;
    let max = r.max(g).max(b);
    let min = r.min(g).min(b);
    let d = max - min;
    let h = if d < f32::EPSILON {
        0.0
    } else if (max - r).abs() < f32::EPSILON {
        (60.0 * (g - b) / d).rem_euclid(360.0)
    } else if (max - g).abs() < f32::EPSILON {
        60.0 * (b - r) / d + 120.0
    } else {
        60.0 * (r - g) / d + 240.0
    };
    let s = if max < f32::EPSILON { 0.0 } else { d / max };
    (h, s, max)
}

fn rgb_to_hue(c: [u8; 3]) -> f32 {
    let r = c[0] as f32 / 255.0;
    let g = c[1] as f32 / 255.0;
    let b = c[2] as f32 / 255.0;
    let max = r.max(g).max(b);
    let min = r.min(g).min(b);
    if (max - min).abs() < f32::EPSILON {
        return 0.0;
    }
    let d = max - min;
    if (max - r).abs() < f32::EPSILON {
        (60.0 * ((g - b) / d)).rem_euclid(360.0)
    } else if (max - g).abs() < f32::EPSILON {
        60.0 * ((b - r) / d) + 120.0
    } else {
        60.0 * ((r - g) / d) + 240.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::canvas::Frame;

    /// Music playing: a falling spectrum, loud bass.
    fn loud() -> EffectInput {
        let mut input = EffectInput::default();
        input.audio.bands = std::array::from_fn(|i| 1.0 - i as f32 / 20.0);
        input.audio.level = 0.6;
        let mut screen = Frame::new(8, 4);
        screen.px.fill([200, 120, 40]);
        input.screen = Some(std::sync::Arc::new(screen));
        input
    }

    #[test]
    fn audio_effects_are_dark_in_silence_and_follow_the_bands() {
        let params = EffectParams::default();
        for kind in EffectKind::ALL.into_iter().filter(|k| k.uses_audio()) {
            let mut f = Frame::new(64, 36);
            render(kind, &mut f, 1.0, &params, &EffectInput::default());
            assert!(f.px.iter().all(|p| p.iter().all(|&c| c < 40)), "{kind:?} lit in silence");
        }
        // Bars: with only the lowest band up, the left edge is lit and the right is not.
        let mut input = EffectInput::default();
        input.audio.bands[0] = 1.0;
        let mut f = Frame::new(64, 36);
        render(EffectKind::SpectrumBars, &mut f, 0.0, &params, &input);
        assert_ne!(f.get(1, 30), [0, 0, 0]);
        assert_eq!(f.get(60, 30), [0, 0, 0]);
    }

    #[test]
    fn hsv_primaries() {
        assert_eq!(hsv_to_rgb(0.0, 1.0, 1.0), [255, 0, 0]);
        assert_eq!(hsv_to_rgb(120.0, 1.0, 1.0), [0, 255, 0]);
        assert_eq!(hsv_to_rgb(240.0, 1.0, 1.0), [0, 0, 255]);
        assert_eq!(hsv_to_rgb(360.0, 1.0, 1.0), [255, 0, 0]);
    }

    #[test]
    fn effects_are_deterministic() {
        let params = EffectParams::default();
        for kind in EffectKind::ALL {
            let mut a = Frame::new(32, 18);
            let mut b = Frame::new(32, 18);
            render(kind, &mut a, 1.25, &params, &loud());
            render(kind, &mut b, 1.25, &params, &loud());
            assert_eq!(a.px, b.px, "{kind:?} not deterministic");
        }
    }

    #[test]
    fn screen_ambient_shows_the_captured_screen() {
        let params = EffectParams::default();
        let mut f = Frame::new(4, 2);
        render(EffectKind::ScreenAmbient, &mut f, 0.0, &params, &EffectInput::default());
        assert!(f.px.iter().all(|p| *p == [0, 0, 0]));

        // Left half red, right half blue, on a screen frame of another size.
        let mut screen = Frame::new(8, 2);
        for (i, px) in screen.px.iter_mut().enumerate() {
            *px = if i % 8 < 4 { [255, 0, 0] } else { [0, 0, 255] };
        }
        let input = EffectInput { screen: Some(std::sync::Arc::new(screen)), ..Default::default() };
        render(EffectKind::ScreenAmbient, &mut f, 0.0, &params, &input);
        assert_eq!(f.px[0], [255, 0, 0]);
        assert_eq!(f.px[3], [0, 0, 255]);
    }

    #[test]
    fn effects_animate_over_time() {
        let params = EffectParams::default();
        for kind in EffectKind::ALL {
            let mut a = Frame::new(32, 18);
            let mut b = Frame::new(32, 18);
            render(kind, &mut a, 0.0, &params, &loud());
            render(kind, &mut b, 1.0, &params, &loud());
            // Solid has nothing to animate; audio effects follow the sound,
            // which this test holds constant.
            if kind == EffectKind::Solid || kind.uses_audio() || kind.uses_screen() {
                assert_eq!(a.px, b.px, "{kind:?} must not depend on time");
                continue;
            }
            assert_ne!(a.px, b.px, "{kind:?} static over time");
        }
    }

    #[test]
    fn along_spans_the_canvas_in_any_direction() {
        for angle in [0.0, 37.0, 90.0, 180.0, 300.0] {
            let d: Vec<f32> = [(0.0, 0.0), (1.0, 0.0), (0.0, 1.0), (1.0, 1.0)]
                .iter()
                .map(|&(x, y)| along(x, y, angle))
                .collect();
            let (lo, hi) = (d.iter().cloned().fold(1.0, f32::min), d.iter().cloned().fold(0.0, f32::max));
            assert!(lo.abs() < 1e-4 && (hi - 1.0).abs() < 1e-4, "{angle}°: {lo}..{hi}");
        }
        // 0° runs left to right, 90° top to bottom.
        assert!(along(0.9, 0.5, 0.0) > along(0.1, 0.5, 0.0));
        assert!(along(0.5, 0.9, 90.0) > along(0.5, 0.1, 90.0));
    }

    #[test]
    fn every_effect_lights_something_and_has_unique_names() {
        let params = EffectParams::default();
        let mut slugs = std::collections::BTreeSet::new();
        for kind in EffectKind::ALL {
            assert!(slugs.insert(kind.slug()), "duplicate slug {}", kind.slug());
            // Over a few seconds no effect may be black everywhere.
            let lit = (0..40).any(|i| {
                let mut f = Frame::new(64, 36);
                render(kind, &mut f, i as f32 * 0.25, &params, &loud());
                f.px.iter().any(|p| p.iter().any(|&c| c > 60))
            });
            assert!(lit, "{kind:?} never lights up");
        }
    }

    /// The UI thread renders the 320×180 preview 30 times a second.
    #[test]
    fn effects_are_fast_enough_for_the_preview() {
        if cfg!(debug_assertions) {
            return; // only meaningful optimized: `cargo test --release`
        }
        let params = EffectParams::default();
        for kind in EffectKind::ALL {
            let mut f = Frame::new(320, 180);
            let start = std::time::Instant::now();
            for i in 0..10 {
                render(kind, &mut f, i as f32, &params, &loud());
            }
            let per_frame = start.elapsed() / 10;
            eprintln!("{kind:?}: {per_frame:?}");
            assert!(per_frame.as_millis() < 8, "{kind:?} takes {per_frame:?} per preview frame");
        }
    }

    /// `cargo test -p cntrl-core contact_sheet -- --ignored` writes every effect
    /// at t=3s to /tmp/cntrl-effects.ppm, for eyeballing a new effect.
    #[test]
    #[ignore]
    fn contact_sheet() {
        let (w, h, cols) = (160usize, 90usize, 6usize);
        let rows = EffectKind::ALL.len().div_ceil(cols);
        let mut sheet = vec![[20u8; 3]; (w + 4) * cols * (h + 4) * rows];
        for (i, kind) in EffectKind::ALL.into_iter().enumerate() {
            let mut f = Frame::new(w, h);
            let params = EffectParams {
                palette: (0..6).map(|c| kind.default_color(c)).collect(),
                ..EffectParams::default()
            };
            render(kind, &mut f, 3.0, &params, &loud());
            let (ox, oy) = ((i % cols) * (w + 4), (i / cols) * (h + 4));
            for y in 0..h {
                for x in 0..w {
                    sheet[(oy + y) * (w + 4) * cols + ox + x] = f.get(x, y);
                }
            }
        }
        let mut out = format!("P6 {} {} 255\n", (w + 4) * cols, (h + 4) * rows).into_bytes();
        out.extend(sheet.iter().flatten());
        std::fs::write("/tmp/cntrl-effects.ppm", out).unwrap();
    }

    #[test]
    fn slug_roundtrip() {
        for kind in EffectKind::ALL {
            assert_eq!(EffectKind::from_slug(kind.slug()), Some(kind));
        }
        assert_eq!(EffectKind::from_slug("nope"), None);
    }
}
