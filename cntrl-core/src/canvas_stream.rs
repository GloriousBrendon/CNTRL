//! Canvas → hardware streaming engine.
//!
//! A worker thread renders the active effect into a `Frame` at the configured
//! FPS, samples every enabled placed device, and pushes colors over the
//! device-native fast paths (persistent sessions — devices are opened once):
//!
//! - ITE ARGB strips: true per-LED frames (D_LED1/D_LED2 headers)
//! - GPU (Fusion2 i2c): sampled zone color, change-detected, never saved to flash
//! - Corsair RAM: per-LED SMBus block writes, rate-limited (PIIX4 is slow)
//! - Keyboard: excluded — its apply pipeline takes seconds (one-shot push only)
//!
//! The UI edits `StreamShared` under the mutex; the worker picks changes up on
//! the next frame. Effects are pure functions of time, so the on-screen
//! preview and the hardware always agree.

use crate::canvas::{self, CanvasLayout, Frame};
use crate::canvas_effects::{self, EffectParams};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// Everything the worker needs, editable live from the UI.
#[derive(Debug, Clone)]
pub struct StreamShared {
    pub layout: CanvasLayout,
}

/// Worker → UI status.
#[derive(Debug, Clone, Default)]
pub struct StreamStats {
    pub frames: u64,
    pub actual_fps: f32,
    /// Human-readable per-backend session state ("dled ok via /dev/hidraw3 · ram 2 dimms").
    pub backends: String,
    pub last_error: Option<String>,
}

pub struct StreamHandle {
    stop: Arc<AtomicBool>,
    pub shared: Arc<Mutex<StreamShared>>,
    pub stats: Arc<Mutex<StreamStats>>,
    join: Option<std::thread::JoinHandle<()>>,
}

impl StreamHandle {
    pub fn stop(mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(j) = self.join.take() {
            let _ = j.join();
        }
    }

    pub fn is_running(&self) -> bool {
        !self.stop.load(Ordering::Relaxed)
            && self.join.as_ref().is_some_and(|j| !j.is_finished())
    }
}

impl Drop for StreamHandle {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(j) = self.join.take() {
            let _ = j.join();
        }
    }
}

/// Minimum gap between RAM SMBus writes (PIIX4 block writes are slow and the
/// DRAM controller NACKs when pushed too hard).
const RAM_MIN_INTERVAL: Duration = Duration::from_millis(100);

pub fn start(layout: CanvasLayout) -> StreamHandle {
    let stop = Arc::new(AtomicBool::new(false));
    let shared = Arc::new(Mutex::new(StreamShared { layout }));
    let stats = Arc::new(Mutex::new(StreamStats::default()));

    let t_stop = Arc::clone(&stop);
    let t_shared = Arc::clone(&shared);
    let t_stats = Arc::clone(&stats);
    let join = std::thread::Builder::new()
        .name("cntrl-canvas-stream".to_string())
        .spawn(move || worker(t_stop, t_shared, t_stats))
        .expect("spawn canvas stream thread");

    StreamHandle {
        stop,
        shared,
        stats,
        join: Some(join),
    }
}

/// Per-backend sessions, opened lazily so one blocked device doesn't kill the
/// whole stream; a failed backend is reported and skipped.
struct Sessions {
    ite: Option<crate::ite_rgb::IteDirect>,
    ite_err: Option<String>,
    gpu: Option<crate::gpu_rgb::GpuDirect>,
    gpu_err: Option<String>,
    ram: Option<crate::corsair_ram::RamDirect>,
    ram_err: Option<String>,
    ram_last_write: Instant,
}

impl Sessions {
    fn describe(&self) -> String {
        let mut parts = Vec::new();
        match (&self.ite, &self.ite_err) {
            (Some(s), _) => parts.push(format!("strips ok ({})", s.path())),
            (None, Some(e)) => parts.push(format!("strips: {e}")),
            _ => {}
        }
        match (&self.gpu, &self.gpu_err) {
            (Some(s), _) => parts.push(format!("gpu ok ({})", s.bus())),
            (None, Some(e)) => parts.push(format!("gpu: {e}")),
            _ => {}
        }
        match (&self.ram, &self.ram_err) {
            (Some(s), _) => parts.push(format!("ram ok ({} dimm)", s.dimm_count())),
            (None, Some(e)) => parts.push(format!("ram: {e}")),
            _ => {}
        }
        parts.join(" · ")
    }
}

fn worker(
    stop: Arc<AtomicBool>,
    shared: Arc<Mutex<StreamShared>>,
    stats: Arc<Mutex<StreamStats>>,
) {
    let mut frame = Frame::canvas_sized();
    let mut sessions = Sessions {
        ite: None,
        ite_err: None,
        gpu: None,
        gpu_err: None,
        ram: None,
        ram_err: None,
        ram_last_write: Instant::now() - RAM_MIN_INTERVAL,
    };
    let t0 = Instant::now();
    let mut frames: u64 = 0;
    let mut fps_window = Instant::now();
    let mut fps_frames: u64 = 0;

    while !stop.load(Ordering::Relaxed) {
        let frame_start = Instant::now();
        let layout = { shared.lock().expect("stream shared").layout.clone() };
        let s = layout.settings;
        let params = EffectParams {
            speed: s.speed,
            scale: s.scale,
            color: s.color,
        };
        let t = t0.elapsed().as_secs_f32();
        canvas_effects::render(s.effect, &mut frame, t, &params);

        let mut frame_err: Option<String> = None;

        // ── ITE ARGB strips: per-LED every frame ──
        let dled1 = sample_if_enabled(&frame, &layout, canvas::DEV_DLED1, s.brightness);
        let dled2 = sample_if_enabled(&frame, &layout, canvas::DEV_DLED2, s.brightness);
        if dled1.is_some() || dled2.is_some() {
            if sessions.ite.is_none() && sessions.ite_err.is_none() {
                match crate::ite_rgb::IteDirect::open() {
                    Ok(s) => sessions.ite = Some(s),
                    Err(e) => sessions.ite_err = Some(e),
                }
            }
            if let Some(ite) = &sessions.ite {
                let empty: Vec<[u8; 3]> = Vec::new();
                let r = ite.write_strips(
                    dled1.as_deref().unwrap_or(&empty),
                    dled2.as_deref().unwrap_or(&empty),
                );
                if let Err(e) = r {
                    frame_err = Some(format!("strips: {e}"));
                    // Drop the session; retry opening next frame.
                    sessions.ite = None;
                }
            }
        }

        // ── GPU: averaged zone color, change-detected ──
        if let Some(avg) = average_if_enabled(&frame, &layout, canvas::DEV_GPU, s.brightness) {
            if sessions.gpu.is_none() && sessions.gpu_err.is_none() {
                match crate::gpu_rgb::GpuDirect::open() {
                    Ok(s) => sessions.gpu = Some(s),
                    Err(e) => sessions.gpu_err = Some(e),
                }
            }
            if let Some(gpu) = &mut sessions.gpu {
                if let Err(e) = gpu.write_color(avg[0], avg[1], avg[2]) {
                    frame_err = frame_err.or(Some(format!("gpu: {e}")));
                    sessions.gpu = None;
                }
            }
        }

        // ── RAM: per-LED block writes, rate-limited ──
        let ram0 = sample_if_enabled(&frame, &layout, canvas::DEV_RAM0, s.brightness);
        let ram1 = sample_if_enabled(&frame, &layout, canvas::DEV_RAM1, s.brightness);
        if (ram0.is_some() || ram1.is_some())
            && sessions.ram_last_write.elapsed() >= RAM_MIN_INTERVAL
        {
            if sessions.ram.is_none() && sessions.ram_err.is_none() {
                match crate::corsair_ram::RamDirect::open() {
                    Ok(s) => sessions.ram = Some(s),
                    Err(e) => sessions.ram_err = Some(e),
                }
            }
            if let Some(ram) = &mut sessions.ram {
                let per_dimm: Vec<Vec<[u8; 3]>> =
                    [ram0, ram1].into_iter().flatten().collect();
                sessions.ram_last_write = Instant::now();
                if let Err(e) = ram.write_colors(&per_dimm) {
                    frame_err = frame_err.or(Some(format!("ram: {e}")));
                }
            }
        }

        frames += 1;
        fps_frames += 1;
        let win = fps_window.elapsed();
        if win >= Duration::from_secs(1) {
            let fps = fps_frames as f32 / win.as_secs_f32();
            fps_window = Instant::now();
            fps_frames = 0;
            let mut st = stats.lock().expect("stream stats");
            st.actual_fps = fps;
        }
        {
            let mut st = stats.lock().expect("stream stats");
            st.frames = frames;
            st.backends = sessions.describe();
            if frame_err.is_some() {
                st.last_error = frame_err;
            }
        }

        // Pace to the requested FPS.
        let target = Duration::from_secs_f32(1.0 / (s.fps.clamp(1, 60) as f32));
        let spent = frame_start.elapsed();
        if spent < target {
            std::thread::sleep(target - spent);
        }
    }
}

fn sample_if_enabled(
    frame: &Frame,
    layout: &CanvasLayout,
    id: &str,
    brightness: f32,
) -> Option<Vec<[u8; 3]>> {
    let placed = layout.device(id).filter(|d| d.enabled)?;
    let spec = canvas::spec_by_id(id)?;
    if !spec.streamable {
        return None;
    }
    Some(canvas::sample_device(frame, &spec, &placed.placement, brightness))
}

fn average_if_enabled(
    frame: &Frame,
    layout: &CanvasLayout,
    id: &str,
    brightness: f32,
) -> Option<[u8; 3]> {
    let placed = layout.device(id).filter(|d| d.enabled)?;
    let spec = canvas::spec_by_id(id)?;
    Some(canvas::average_device_color(
        frame,
        &spec,
        &placed.placement,
        brightness,
    ))
}

/// One-shot: sample the keyboard's placement at time `t` and run the solid
/// apply pipeline once (the keyboard protocol is far too slow to stream).
pub fn push_keyboard_snapshot(layout: &CanvasLayout, t: f32) -> Result<[u8; 3], String> {
    let placed = layout
        .device(canvas::DEV_KEYBOARD)
        .filter(|d| d.enabled)
        .ok_or_else(|| "keyboard is disabled on the canvas".to_string())?;
    let spec = canvas::spec_by_id(canvas::DEV_KEYBOARD).expect("keyboard spec");
    let s = layout.settings;
    let mut frame = Frame::canvas_sized();
    let params = EffectParams {
        speed: s.speed,
        scale: s.scale,
        color: s.color,
    };
    canvas_effects::render(s.effect, &mut frame, t, &params);
    let avg = canvas::average_device_color(&frame, &spec, &placed.placement, s.brightness);
    crate::mchose_lighting::set_solid_rgb(avg[0], avg[1], avg[2], 255, None, false)?;
    Ok(avg)
}
