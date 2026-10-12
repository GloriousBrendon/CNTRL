//! Screen → canvas-sized frame for the Screen Ambient effect.
//!
//! A capture thread runs `grim` (wlroots screencopy: Hyprland, Sway, …) about
//! [`CAPTURE_FPS`] times a second, shrinks each grab to the canvas and blends
//! it into the previous one. Nothing runs unless the effect is active.
//!
//! ponytail: one `grim` process per grab (~15 ms wall, ~6 ms CPU per 1440p monitor)
//! instead of holding a screencopy or portal session — no new build
//! dependencies, but several percent of a core per monitor while the effect runs and no
//! GNOME/KDE support. Upgrade to `wayland-client` screencopy or the
//! ScreenCast portal if either matters.

use crate::canvas::Frame;
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

const CAPTURE_FPS: u64 = 10;
/// Share of the new grab in each update: enough to follow a film, not enough
/// to strobe on a hard cut.
const BLEND: f32 = 0.6;
/// Samples averaged per canvas cell along each axis.
const TAPS: usize = 4;

/// Shrink a binary PPM (`P6`, 8-bit) to `w`×`h` by averaging a grid of
/// samples per cell. `None` if the data is not such an image.
pub fn downscale_ppm(ppm: &[u8], w: usize, h: usize) -> Option<Frame> {
    // Header: "P6 <width> <height> <maxval>" then one whitespace byte.
    let mut pos = 0;
    let mut token = || {
        while ppm.get(pos)?.is_ascii_whitespace() {
            pos += 1;
        }
        let start = pos;
        while !ppm.get(pos)?.is_ascii_whitespace() {
            pos += 1;
        }
        std::str::from_utf8(&ppm[start..pos]).ok()
    };
    if token()? != "P6" {
        return None;
    }
    let src_w: usize = token()?.parse().ok()?;
    let src_h: usize = token()?.parse().ok()?;
    if token()? != "255" || src_w == 0 || src_h == 0 {
        return None;
    }
    let data = ppm.get(pos + 1..)?;
    if data.len() < src_w * src_h * 3 {
        return None;
    }

    let mut out = Frame::new(w, h);
    for y in 0..h {
        for x in 0..w {
            let mut sum = [0u32; 3];
            for ty in 0..TAPS {
                for tx in 0..TAPS {
                    let sx = (x * TAPS + tx) * src_w / (w * TAPS);
                    let sy = (y * TAPS + ty) * src_h / (h * TAPS);
                    let i = (sy * src_w + sx) * 3;
                    for c in 0..3 {
                        sum[c] += u32::from(data[i + c]);
                    }
                }
            }
            out.px[y * w + x] = sum.map(|s| (s / (TAPS * TAPS) as u32) as u8);
        }
    }
    Some(out)
}

/// A running capture. Dropping it stops the thread.
pub struct ScreenCapture {
    frame: Arc<Mutex<Arc<Frame>>>,
    stop: Arc<AtomicBool>,
}

impl ScreenCapture {
    /// `output` names one monitor (`DP-1`); empty captures the whole desktop.
    pub fn start(output: &str) -> Result<Self, String> {
        let grab = {
            let output = output.to_string();
            move || -> Result<Frame, String> {
                let mut cmd = Command::new("grim");
                cmd.args(["-t", "ppm"]);
                if !output.is_empty() {
                    cmd.args(["-o", &output]);
                }
                let out = cmd
                    .arg("-")
                    .stdin(Stdio::null())
                    .stderr(Stdio::piped())
                    .output()
                    .map_err(|e| format!("screen capture needs `grim`: {e}"))?;
                let (w, h) = (crate::canvas::CANVAS_W as usize, crate::canvas::CANVAS_H as usize);
                downscale_ppm(&out.stdout, w, h).ok_or_else(|| {
                    format!("screen capture failed: {}", String::from_utf8_lossy(&out.stderr).trim())
                })
            }
        };
        // Grab once up front so a missing tool or monitor is reported now.
        let first = grab()?;
        let frame = Arc::new(Mutex::new(Arc::new(first)));
        let stop = Arc::new(AtomicBool::new(false));
        let (shared, stopping) = (Arc::clone(&frame), Arc::clone(&stop));
        std::thread::Builder::new()
            .name("cntrl-screen".to_string())
            .spawn(move || {
                let interval = Duration::from_millis(1000 / CAPTURE_FPS);
                while !stopping.load(Ordering::Relaxed) {
                    let started = Instant::now();
                    // A failed grab (monitor asleep, session locked) keeps the last picture.
                    if let Ok(new) = grab() {
                        let mut slot = shared.lock().expect("screen frame");
                        let mut mixed = (**slot).clone();
                        for (old, new) in mixed.px.iter_mut().zip(&new.px) {
                            for c in 0..3 {
                                old[c] = (old[c] as f32 + (new[c] as f32 - old[c] as f32) * BLEND).round() as u8;
                            }
                        }
                        *slot = Arc::new(mixed);
                    }
                    std::thread::sleep(interval.saturating_sub(started.elapsed()));
                }
            })
            .map_err(|e| e.to_string())?;
        Ok(Self { frame, stop })
    }

    pub fn frame(&self) -> Arc<Frame> {
        Arc::clone(&self.frame.lock().expect("screen frame"))
    }
}

impl Drop for ScreenCapture {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ppm_is_averaged_down_to_the_canvas() {
        // 4×2 image: left half red, right half blue.
        let mut ppm = b"P6\n4 2\n255\n".to_vec();
        for _ in 0..2 {
            ppm.extend([255, 0, 0, 255, 0, 0, 0, 0, 255, 0, 0, 255]);
        }
        let f = downscale_ppm(&ppm, 2, 1).unwrap();
        assert_eq!(f.px, vec![[255, 0, 0], [0, 0, 255]]);
        // One cell covering everything is the average of both halves.
        assert_eq!(downscale_ppm(&ppm, 1, 1).unwrap().px, vec![[127, 0, 127]]);

        assert!(downscale_ppm(b"P6\n4 2\n255\nshort", 2, 1).is_none());
        assert!(downscale_ppm(b"not an image", 2, 1).is_none());
    }

    /// Needs a wlroots session: `cargo test -p cntrl-core live_grab -- --ignored`.
    /// Writes what the effect sees to /tmp/cntrl-screen.ppm.
    #[test]
    #[ignore]
    fn live_grab() {
        let capture = ScreenCapture::start(&std::env::var("CNTRL_SCREEN_OUTPUT").unwrap_or_default()).unwrap();
        std::thread::sleep(Duration::from_millis(500));
        let f = capture.frame();
        assert!(f.px.iter().any(|p| *p != [0, 0, 0]), "screen grab is all black");
        let mut ppm = format!("P6\n{} {}\n255\n", f.width, f.height).into_bytes();
        ppm.extend(f.px.iter().flatten());
        std::fs::write("/tmp/cntrl-screen.ppm", ppm).unwrap();
    }
}
