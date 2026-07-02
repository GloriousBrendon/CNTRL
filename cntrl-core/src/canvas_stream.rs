//! Canvas → hardware streaming engine.
//!
//! A worker thread renders the active effect into a `Frame` at the configured
//! FPS, samples every enabled placed device, and pushes colors over the
//! device-native fast paths (persistent sessions — devices are opened once):
//!
//! - Keyboard (Ace68-II): true per-key live frames (0xDD, ~7 ms/frame)
//! - ITE ARGB strips: true per-LED frames (D_LED1/D_LED2 headers)
//! - GPU (Fusion2 i2c): sampled zone color, change-detected, never saved to flash
//! - Corsair RAM: per-LED SMBus block writes, rate-limited (PIIX4 is slow)
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
    kb: Option<crate::mchose_direct::Ace68Direct>,
    kb_err: Option<String>,
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
        match (&self.kb, &self.kb_err) {
            (Some(s), _) => parts.push(format!("keyboard ok ({})", s.path())),
            (None, Some(e)) => parts.push(format!("keyboard: {e}")),
            _ => {}
        }
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
        kb: None,
        kb_err: None,
        ite: None,
        ite_err: None,
        gpu: None,
        gpu_err: None,
        ram: None,
        ram_err: None,
        ram_last_write: Instant::now() - RAM_MIN_INTERVAL,
    };
    // LED slot per layout key (constant; computed once).
    let kb_slots = crate::mchose_direct::layout_led_indices();
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

        // ── Keyboard: per-key live frame (0xDD) every tick ──
        if let Some(colors) = sample_if_enabled(&frame, &layout, canvas::DEV_KEYBOARD, s.brightness)
        {
            if sessions.kb.is_none() && sessions.kb_err.is_none() {
                match crate::mchose_direct::Ace68Direct::open() {
                    Ok(s) => sessions.kb = Some(s),
                    Err(e) => sessions.kb_err = Some(e),
                }
            }
            if let Some(kb) = &mut sessions.kb {
                let mut wire = [[0u8; 3]; crate::mchose_direct::LIVE_FRAME_SLOTS];
                for (key, color) in colors.iter().enumerate() {
                    if let Some(Some(slot)) = kb_slots.get(key) {
                        wire[*slot] = *color;
                    }
                }
                if let Err(e) = kb.write_live_frame(&wire) {
                    frame_err = Some(format!("keyboard: {e}"));
                    sessions.kb = None;
                }
            }
        }

        // ── ITE ARGB headers: assemble each daisy chain from its placed
        // segments (fans/strips) in chain order, honoring per-device reverse ──
        let dled1 = assemble_chain(&frame, &layout, canvas::CHAIN_DLED1, s.brightness);
        let dled2 = assemble_chain(&frame, &layout, canvas::CHAIN_DLED2, s.brightness);
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

    // Stream ended: the keyboard reverts to its saved effect by itself; the
    // PC backends don't — they'd stay frozen on the last streamed frame (and
    // the ITE bring-up reset the motherboard zones). Re-apply each streamed
    // backend's saved state.
    let states = crate::device_state::load();
    if sessions.ite.is_some() {
        drop(sessions.ite.take());
        if let Some(state) = states.get("ite_rgb") {
            let doc = crate::device_state::doc_for(state);
            if let Err(e) = crate::ite_rgb::apply_profile(&doc) {
                eprintln!("canvas: ite restore after stream failed: {e}");
            }
        }
    }
    if sessions.gpu.is_some() {
        drop(sessions.gpu.take());
        if let Some(state) = states.get("gpu_rgb") {
            let doc = crate::device_state::doc_for(state);
            if let Err(e) = crate::gpu_rgb::apply_profile(&doc) {
                eprintln!("canvas: gpu restore after stream failed: {e}");
            }
        }
    }
    if sessions.ram.is_some() {
        drop(sessions.ram.take());
        if let Some(state) = states.get("corsair_ram") {
            let doc = crate::device_state::doc_for(state);
            if let Err(e) = crate::corsair_ram::apply_profile(&doc) {
                eprintln!("canvas: ram restore after stream failed: {e}");
            }
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
    let spec = layout.spec(id)?;
    if !spec.streamable {
        return None;
    }
    Some(canvas::sample_device(frame, &spec, placed, brightness))
}

/// Concatenate a header's placed segments into one wire buffer, in chain
/// order. Disabled segments contribute black (they still occupy their LEDs
/// in the chain); `reversed` segments are flipped for backwards wiring.
/// Returns `None` when every segment is disabled (header untouched).
fn assemble_chain(
    frame: &Frame,
    layout: &CanvasLayout,
    header: &str,
    brightness: f32,
) -> Option<Vec<[u8; 3]>> {
    let segments = layout.chains.header(header).to_vec();
    let mut wire = Vec::new();
    let mut any = false;
    for (i, seg) in segments.iter().enumerate() {
        let id = format!("{header}.{i}");
        match sample_if_enabled(frame, layout, &id, brightness) {
            Some(mut colors) => {
                any = true;
                if layout.device(&id).is_some_and(|d| d.reversed) {
                    colors.reverse();
                }
                wire.extend(colors);
            }
            None => wire.extend(std::iter::repeat([0u8; 3]).take(seg.leds as usize)),
        }
    }
    any.then_some(wire)
}

fn average_if_enabled(
    frame: &Frame,
    layout: &CanvasLayout,
    id: &str,
    brightness: f32,
) -> Option<[u8; 3]> {
    let placed = layout.device(id).filter(|d| d.enabled)?;
    let spec = layout.spec(id)?;
    Some(canvas::average_device_color(frame, &spec, placed, brightness))
}

/// One-shot: sample the keyboard's placement at time `t` and persist the
/// average as a solid color (unlike the live stream, this survives after
/// streaming stops — the keyboard reverts to it as its saved effect).
pub fn push_keyboard_snapshot(layout: &CanvasLayout, t: f32) -> Result<[u8; 3], String> {
    let placed = layout
        .device(canvas::DEV_KEYBOARD)
        .filter(|d| d.enabled)
        .ok_or_else(|| "keyboard is disabled on the canvas".to_string())?;
    let spec = layout.spec(canvas::DEV_KEYBOARD).expect("keyboard spec");
    let s = layout.settings;
    let mut frame = Frame::canvas_sized();
    let params = EffectParams {
        speed: s.speed,
        scale: s.scale,
        color: s.color,
    };
    canvas_effects::render(s.effect, &mut frame, t, &params);
    let avg = canvas::average_device_color(&frame, &spec, placed, s.brightness);
    // Fast protocol client first; legacy replay pipeline as fallback.
    match crate::mchose_direct::Ace68Direct::open()
        .and_then(|mut d| d.apply_solid(avg[0], avg[1], avg[2]))
    {
        Ok(()) => {}
        Err(e) => {
            eprintln!("canvas: fast keyboard push failed ({e}); using legacy pipeline");
            crate::mchose_lighting::set_solid_rgb(avg[0], avg[1], avg[2], 255, None, false)?;
        }
    }
    Ok(avg)
}
