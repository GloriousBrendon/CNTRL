//! Demo canvas effects — procedural, zero assets, pure functions of time.
//!
//! Every effect is `f(pixel, t, params) → color`, so the UI preview and the
//! hardware stream render identical frames without sharing state.

use crate::canvas::Frame;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum EffectKind {
    RainbowWave,
    Plasma,
    Ripple,
    Comet,
    Aurora,
}

impl EffectKind {
    pub fn all() -> [EffectKind; 5] {
        [
            EffectKind::RainbowWave,
            EffectKind::Plasma,
            EffectKind::Ripple,
            EffectKind::Comet,
            EffectKind::Aurora,
        ]
    }

    pub fn label(self) -> &'static str {
        match self {
            EffectKind::RainbowWave => "Rainbow Wave",
            EffectKind::Plasma => "Plasma",
            EffectKind::Ripple => "Ripple",
            EffectKind::Comet => "Comet",
            EffectKind::Aurora => "Aurora",
        }
    }

    /// CLI-friendly id (`cntrl canvas --effect <slug>`).
    pub fn slug(self) -> &'static str {
        match self {
            EffectKind::RainbowWave => "wave",
            EffectKind::Plasma => "plasma",
            EffectKind::Ripple => "ripple",
            EffectKind::Comet => "comet",
            EffectKind::Aurora => "aurora",
        }
    }

    /// Which controls this effect actually reads — the UI shows only these.
    pub fn controls(self) -> EffectControls {
        let colors = match self {
            EffectKind::Ripple | EffectKind::Comet => 1,
            EffectKind::RainbowWave | EffectKind::Plasma | EffectKind::Aurora => 0,
        };
        EffectControls {
            speed: true,
            scale: true,
            colors,
        }
    }

    pub fn from_slug(s: &str) -> Option<Self> {
        let t = s.trim().to_ascii_lowercase();
        Self::all().into_iter().find(|e| e.slug() == t)
    }
}

/// The controls an effect declares (see [`EffectKind::controls`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EffectControls {
    pub speed: bool,
    pub scale: bool,
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
            palette: vec![DEFAULT_PALETTE[0]],
        }
    }
}

/// Render one frame of `kind` at time `t` (seconds since stream start).
pub fn render(kind: EffectKind, frame: &mut Frame, t: f32, params: &EffectParams) {
    match kind {
        EffectKind::RainbowWave => rainbow_wave(frame, t, params),
        EffectKind::Plasma => plasma(frame, t, params),
        EffectKind::Ripple => ripple(frame, t, params),
        EffectKind::Comet => comet(frame, t, params),
        EffectKind::Aurora => aurora(frame, t, params),
    }
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

/// Diagonal rainbow sweeping across the canvas.
fn rainbow_wave(frame: &mut Frame, t: f32, p: &EffectParams) {
    let (w, h) = (frame.width, frame.height);
    for y in 0..h {
        for x in 0..w {
            let (nx, ny) = norm_xy(frame, x, y);
            let hue = (nx + ny * 0.35) * 360.0 * p.scale - t * p.speed * 90.0;
            frame.set(x, y, hsv_to_rgb(hue, 1.0, 1.0));
        }
    }
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
        for kind in EffectKind::all() {
            let mut a = Frame::new(32, 18);
            let mut b = Frame::new(32, 18);
            render(kind, &mut a, 1.25, &params);
            render(kind, &mut b, 1.25, &params);
            assert_eq!(a.px, b.px, "{kind:?} not deterministic");
        }
    }

    #[test]
    fn effects_animate_over_time() {
        let params = EffectParams::default();
        for kind in EffectKind::all() {
            let mut a = Frame::new(32, 18);
            let mut b = Frame::new(32, 18);
            render(kind, &mut a, 0.0, &params);
            render(kind, &mut b, 1.0, &params);
            assert_ne!(a.px, b.px, "{kind:?} static over time");
        }
    }

    #[test]
    fn slug_roundtrip() {
        for kind in EffectKind::all() {
            assert_eq!(EffectKind::from_slug(kind.slug()), Some(kind));
        }
        assert_eq!(EffectKind::from_slug("nope"), None);
    }
}
