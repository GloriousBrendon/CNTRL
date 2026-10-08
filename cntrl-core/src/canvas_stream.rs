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
//! One thread renders and samples; each device family writes on its own
//! thread at its own pace, reopening a failed device after a short pause.
//!
//! The UI edits `StreamShared` under the mutex; the worker picks changes up on
//! the next frame. Effects are pure functions of time, so the on-screen
//! preview and the hardware always agree.

use crate::canvas::{self, CanvasLayout, Frame};
use crate::canvas_effects;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// Everything the worker needs, editable live from the UI.
#[derive(Debug, Clone)]
pub struct StreamShared {
    pub layout: CanvasLayout,
    /// Device id to single out on the hardware (solid white, first wire LED
    /// red, everything else dark) — for finding which physical fan is which.
    pub identify: Option<String>,
}

/// Worker → UI status.
#[derive(Debug, Clone, Default)]
pub struct StreamStats {
    pub frames: u64,
    pub actual_fps: f32,
    /// Human-readable per-backend session state ("dled ok via /dev/hidraw3 · ram 2 dimms").
    pub backends: String,
    /// Writes per second and average / worst write time per backend over the
    /// last second ("keyboard 60/s 7.0/9.1 ms · strips 45/s 19.4/23.0 ms").
    pub timing: String,
    pub last_error: Option<String>,
    /// The live data the last frame was rendered with (previews mirror it).
    pub input: canvas_effects::EffectInput,
}

pub struct StreamHandle {
    stop: Arc<AtomicBool>,
    pub shared: Arc<Mutex<StreamShared>>,
    pub stats: Arc<Mutex<StreamStats>>,
    /// Effect time zero — previews use it so screen and hardware stay in phase.
    pub started: Instant,
    join: Option<std::thread::JoinHandle<()>>,
}

/// Streams alive in this process. With [`active`], lets static applies
/// (theme sync, boot restore) stay out of a running stream's way.
static LOCAL_STREAMS: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

/// Is a canvas stream driving the devices — here or in the daemon?
pub fn active() -> bool {
    LOCAL_STREAMS.load(Ordering::SeqCst) > 0
        || crate::ipc::canvas_request(&crate::ipc::Request::CanvasStatus)
            .ok()
            .flatten()
            .is_some_and(|s| s.running)
}

impl StreamHandle {
    /// Snapshot for IPC / status lines.
    pub fn status(&self) -> crate::ipc::CanvasStatus {
        let st = self.stats.lock().expect("stream stats").clone();
        crate::ipc::CanvasStatus {
            running: self.is_running(),
            elapsed_secs: self.started.elapsed().as_secs_f32(),
            frames: st.frames,
            fps: st.actual_fps,
            backends: st.backends,
            timing: st.timing,
            last_error: st.last_error,
            bands: st.input.audio.bands.to_vec(),
            level: st.input.audio.level,
        }
    }

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
    let shared = Arc::new(Mutex::new(StreamShared {
        layout,
        identify: None,
    }));
    let started = Instant::now();
    let stats = Arc::new(Mutex::new(StreamStats::default()));

    let t_stop = Arc::clone(&stop);
    let t_shared = Arc::clone(&shared);
    let t_stats = Arc::clone(&stats);
    let join = std::thread::Builder::new()
        .name("cntrl-canvas-stream".to_string())
        .spawn(move || {
            LOCAL_STREAMS.fetch_add(1, Ordering::SeqCst);
            worker(t_stop, t_shared, t_stats, started);
            LOCAL_STREAMS.fetch_sub(1, Ordering::SeqCst);
        })
        .expect("spawn canvas stream thread");

    StreamHandle {
        stop,
        shared,
        stats,
        started,
        join: Some(join),
    }
}

/// How long a backend waits before reopening a device that failed.
const RETRY_AFTER: Duration = Duration::from_secs(2);

#[derive(Default)]
struct BackendStatus {
    /// "ok (/dev/hidraw3)" or the last error; empty until first used.
    state: String,
    error: Option<String>,
    /// Write time over the current one-second window.
    total: Duration,
    worst: Duration,
    writes: u32,
}

/// One device family on its own thread, so a slow bus (ITE ≈ 20 ms a frame)
/// can't hold back a fast one (keyboard ≈ 7 ms). The renderer offers every
/// frame; a backend still busy with the previous one just misses it.
struct Backend<P> {
    name: &'static str,
    tx: Option<std::sync::mpsc::SyncSender<P>>,
    status: Arc<Mutex<BackendStatus>>,
    join: Option<std::thread::JoinHandle<()>>,
}

type ApplySaved = fn(&crate::profile::ProfileDocument) -> Result<String, String>;

impl<P: Send + 'static> Backend<P> {
    /// `restore`: (device_state key, apply fn) to put saved lighting back when
    /// the stream ends — PC backends stay frozen on the last frame otherwise.
    fn spawn<S: 'static>(
        name: &'static str,
        min_interval: Duration,
        open: fn() -> Result<S, String>,
        describe: fn(&S) -> String,
        write: fn(&mut S, &P) -> Result<(), String>,
        restore: Option<(&'static str, ApplySaved)>,
    ) -> Self {
        let (tx, rx) = std::sync::mpsc::sync_channel::<P>(1);
        let status = Arc::new(Mutex::new(BackendStatus::default()));
        let t_status = Arc::clone(&status);
        let join = std::thread::Builder::new()
            .name(format!("cntrl-canvas-{name}"))
            .spawn(move || {
                let mut session: Option<S> = None;
                let mut opened = false;
                let mut retry_at = Instant::now();
                let mut last_write: Option<Instant> = None;
                // Ends when the renderer drops its sender.
                for payload in rx {
                    if last_write.is_some_and(|t| t.elapsed() < min_interval) {
                        continue;
                    }
                    if session.is_none() {
                        if Instant::now() < retry_at {
                            continue;
                        }
                        match open() {
                            Ok(s) => {
                                t_status.lock().expect("backend status").state =
                                    format!("ok ({})", describe(&s));
                                session = Some(s);
                                opened = true;
                            }
                            Err(e) => {
                                retry_at = Instant::now() + RETRY_AFTER;
                                let mut st = t_status.lock().expect("backend status");
                                st.state = e.clone();
                                st.error = Some(e);
                                continue;
                            }
                        }
                    }
                    let mark = Instant::now();
                    let result = write(session.as_mut().expect("session opened above"), &payload);
                    let spent = mark.elapsed();
                    last_write = Some(Instant::now());
                    let mut st = t_status.lock().expect("backend status");
                    st.total += spent;
                    st.worst = st.worst.max(spent);
                    st.writes += 1;
                    if let Err(e) = result {
                        // Drop the session; reopen after a pause, not every frame.
                        session = None;
                        retry_at = Instant::now() + RETRY_AFTER;
                        st.state = e.clone();
                        st.error = Some(e);
                    }
                }
                drop(session);
                if let (true, Some((key, apply))) = (opened, restore) {
                    if let Some(state) = crate::device_state::load().get(key) {
                        if let Err(e) = apply(&crate::device_state::doc_for(state)) {
                            eprintln!("canvas: {name} restore after stream failed: {e}");
                        }
                    }
                }
            })
            .expect("spawn canvas backend thread");
        Self {
            name,
            tx: Some(tx),
            status,
            join: Some(join),
        }
    }

    fn offer(&self, payload: P) {
        if let Some(tx) = &self.tx {
            let _ = tx.try_send(payload);
        }
    }

    /// Fold this backend into the stats strings and reset its timing window.
    fn report(&self, backends: &mut Vec<String>, timing: &mut Vec<String>, error: &mut Option<String>) {
        let mut st = self.status.lock().expect("backend status");
        if !st.state.is_empty() {
            backends.push(format!("{} {}", self.name, st.state));
        }
        if st.writes > 0 {
            let ms = |d: Duration| d.as_secs_f32() * 1000.0;
            timing.push(format!(
                "{} {}/s {:.1}/{:.1} ms",
                self.name,
                st.writes,
                ms(st.total) / st.writes as f32,
                ms(st.worst)
            ));
        }
        if let Some(e) = st.error.take() {
            *error = Some(format!("{}: {e}", self.name));
        }
        (st.total, st.worst, st.writes) = (Duration::ZERO, Duration::ZERO, 0);
    }

    /// Stop the thread and wait for it (including its restore).
    fn finish(&mut self) {
        self.tx.take();
        if let Some(j) = self.join.take() {
            let _ = j.join();
        }
    }
}

type KeyboardFrame = [[u8; 3]; crate::mchose_direct::LIVE_FRAME_SLOTS];
/// (D_LED1 wire colors, D_LED2 wire colors, single-color zones as (zone, color)).
type IteFrame = (Vec<[u8; 3]>, Vec<[u8; 3]>, Vec<(u8, [u8; 3])>);

fn worker(
    stop: Arc<AtomicBool>,
    shared: Arc<Mutex<StreamShared>>,
    stats: Arc<Mutex<StreamStats>>,
    t0: Instant,
) {
    let mut frame = Frame::canvas_sized();
    let mut kb = Backend::<KeyboardFrame>::spawn(
        "keyboard",
        Duration::ZERO,
        crate::mchose_direct::Ace68Direct::open,
        |s| s.path().to_string(),
        |s, wire| s.write_live_frame(wire),
        // The keyboard reverts to its saved effect by itself.
        None,
    );
    let mut ite = Backend::<IteFrame>::spawn(
        "strips",
        Duration::ZERO,
        crate::ite_rgb::IteDirect::open,
        |s| s.path().to_string(),
        |s, (dled1, dled2, zones)| s.write_strips(dled1, dled2, zones),
        Some(("ite_rgb", crate::ite_rgb::apply_profile)),
    );
    let mut gpu = Backend::<[u8; 3]>::spawn(
        "gpu",
        Duration::ZERO,
        crate::gpu_rgb::GpuDirect::open,
        |s| s.bus().to_string(),
        |s, c| s.write_color(c[0], c[1], c[2]),
        Some(("gpu_rgb", crate::gpu_rgb::apply_profile)),
    );
    let mut ram = Backend::<Vec<Vec<[u8; 3]>>>::spawn(
        "ram",
        RAM_MIN_INTERVAL,
        crate::corsair_ram::RamDirect::open,
        |s| format!("{} dimm", s.dimm_count()),
        |s, per_dimm| s.write_colors(per_dimm),
        Some(("corsair_ram", crate::corsair_ram::apply_profile)),
    );
    let mut audio: Option<crate::audio::AudioCapture> = None;
    let mut audio_retry_at = Instant::now();
    let board_zones = canvas::board_zones();
    // LED slot per layout key (constant; computed once).
    let kb_slots = crate::mchose_direct::layout_led_indices();
    let mut frames: u64 = 0;
    let mut fps_window = Instant::now();
    let mut fps_frames: u64 = 0;

    while !stop.load(Ordering::Relaxed) {
        let frame_start = Instant::now();
        let (layout, identify) = {
            let shared = shared.lock().expect("stream shared");
            (shared.layout.clone(), shared.identify.clone())
        };
        let identify = identify.as_deref();
        let s = &layout.settings;
        // Capture sound only while something reacts to it.
        if !s.needs_audio() {
            audio = None;
        } else if audio.is_none() && Instant::now() >= audio_retry_at {
            match crate::audio::AudioCapture::start() {
                Ok(capture) => audio = Some(capture),
                Err(e) => {
                    audio_retry_at = Instant::now() + RETRY_AFTER;
                    stats.lock().expect("stream stats").last_error = Some(e);
                }
            }
        }
        let input = canvas_effects::EffectInput {
            audio: audio.as_ref().map(|a| a.levels()).unwrap_or_default(),
        };
        let t = t0.elapsed().as_secs_f32();
        s.render_into(&mut frame, t, &input);
        stats.lock().expect("stream stats").input = input;

        // ── Keyboard: per-key live frame (0xDD) ──
        if let Some(colors) =
            sample_if_enabled(&frame, &layout, canvas::DEV_KEYBOARD, s.brightness, identify)
        {
            let mut wire = [[0u8; 3]; crate::mchose_direct::LIVE_FRAME_SLOTS];
            for (key, color) in colors.iter().enumerate() {
                if let Some(Some(slot)) = kb_slots.get(key) {
                    wire[*slot] = *color;
                }
            }
            kb.offer(wire);
        }

        // ── ITE ARGB headers: each daisy chain assembled from its placed
        // segments in chain order; the 12V RGB header and enabled board zones
        // ride along as one averaged color each ──
        let dled1 = assemble_chain(&frame, &layout, canvas::CHAIN_DLED1, s.brightness, identify);
        let dled2 = assemble_chain(&frame, &layout, canvas::CHAIN_DLED2, s.brightness, identify);
        let zones: Vec<(u8, [u8; 3])> = [(canvas::DEV_RGB12V.to_string(), crate::ite_rgb::rgb12v_zone())]
            .into_iter()
            .chain(board_zones.iter().cloned())
            .filter_map(|(id, zone)| {
                Some((zone, average_if_enabled(&frame, &layout, &id, s.brightness, identify)?))
            })
            .collect();
        if dled1.is_some() || dled2.is_some() || !zones.is_empty() {
            ite.offer((dled1.unwrap_or_default(), dled2.unwrap_or_default(), zones));
        }

        // ── GPU: averaged zone color (change-detected in the session) ──
        if let Some(avg) = average_if_enabled(&frame, &layout, canvas::DEV_GPU, s.brightness, identify) {
            gpu.offer(avg);
        }

        // ── RAM: per-LED block writes (rate-limited in its backend) ──
        let per_dimm: Vec<Vec<[u8; 3]>> = [canvas::DEV_RAM0, canvas::DEV_RAM1]
            .into_iter()
            .filter_map(|id| sample_if_enabled(&frame, &layout, id, s.brightness, identify))
            .collect();
        if !per_dimm.is_empty() {
            ram.offer(per_dimm);
        }

        frames += 1;
        fps_frames += 1;
        stats.lock().expect("stream stats").frames = frames;
        let win = fps_window.elapsed();
        if win >= Duration::from_secs(1) {
            let (mut backends, mut timing, mut error) = (Vec::new(), Vec::new(), None);
            kb.report(&mut backends, &mut timing, &mut error);
            ite.report(&mut backends, &mut timing, &mut error);
            gpu.report(&mut backends, &mut timing, &mut error);
            ram.report(&mut backends, &mut timing, &mut error);
            let mut st = stats.lock().expect("stream stats");
            st.actual_fps = fps_frames as f32 / win.as_secs_f32();
            st.backends = backends.join(" · ");
            st.timing = timing.join(" · ");
            if error.is_some() {
                st.last_error = error;
            }
            fps_window = Instant::now();
            fps_frames = 0;
        }

        // Pace to the requested FPS.
        let target = Duration::from_secs_f32(1.0 / (s.fps.clamp(1, 60) as f32));
        let spent = frame_start.elapsed();
        if spent < target {
            std::thread::sleep(target - spent);
        }
    }

    // Each backend re-applies its saved state as its thread winds down.
    kb.finish();
    ite.finish();
    gpu.finish();
    ram.finish();
}

fn sample_if_enabled(
    frame: &Frame,
    layout: &CanvasLayout,
    id: &str,
    brightness: f32,
    identify: Option<&str>,
) -> Option<Vec<[u8; 3]>> {
    let placed = layout.device(id).filter(|d| d.enabled)?;
    let spec = layout.spec(id)?;
    if !spec.streamable {
        return None;
    }
    let Some(target) = identify else {
        return Some(canvas::sample_device(frame, &spec, placed, brightness));
    };
    let n = spec.led_positions.len();
    if target != id {
        return Some(vec![[0, 0, 0]; n]);
    }
    let mut colors = vec![[255, 255, 255]; n];
    if n > 1 {
        // `reversed` flips the order on the wire later; mark the LED that
        // ends up first there.
        colors[if placed.reversed { n - 1 } else { 0 }] = [255, 0, 0];
    }
    Some(colors)
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
    identify: Option<&str>,
) -> Option<Vec<[u8; 3]>> {
    let segments = layout.chains.header(header).to_vec();
    let mut wire = Vec::new();
    let mut any = false;
    for (i, seg) in segments.iter().enumerate() {
        let id = format!("{header}.{i}");
        match sample_if_enabled(frame, layout, &id, brightness, identify) {
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

/// One color for a whole device: the mean of its sample points.
fn average_if_enabled(
    frame: &Frame,
    layout: &CanvasLayout,
    id: &str,
    brightness: f32,
    identify: Option<&str>,
) -> Option<[u8; 3]> {
    let colors = sample_if_enabled(frame, layout, id, brightness, identify)?;
    let n = colors.len().max(1) as u32;
    let sum = colors.iter().fold([0u32; 3], |acc, c| {
        [acc[0] + c[0] as u32, acc[1] + c[1] as u32, acc[2] + c[2] as u32]
    });
    Some([(sum[0] / n) as u8, (sum[1] / n) as u8, (sum[2] / n) as u8])
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
    let s = &layout.settings;
    let mut frame = Frame::canvas_sized();
    s.render_into(&mut frame, t, &canvas_effects::EffectInput::default());
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::canvas::{Placement, CHAIN_DLED1};

    /// Header 1 = two 4-LED strips laid end to end on a frame whose red
    /// channel is the pixel's x, so every LED's color tells where it sampled.
    fn two_strips() -> (Frame, CanvasLayout) {
        let mut frame = Frame::canvas_sized();
        for y in 0..frame.height {
            for x in 0..frame.width {
                frame.set(x, y, [x as u8, 0, 0]);
            }
        }
        let mut layout = CanvasLayout::default();
        layout.chains.dled1 = vec![
            canvas::ChainSegment {
                kind: canvas::SegmentKind::Strip,
                leds: 4,
            };
            2
        ];
        layout.reconcile();
        for (id, x) in [("dled1.0", 10.0), ("dled1.1", 50.0)] {
            layout.device_mut(id).unwrap().placement = Placement {
                x,
                y: 10.0,
                scale: 10.0,
                rotation_deg: 0.0,
            };
        }
        (frame, layout)
    }

    fn reds(wire: &[[u8; 3]]) -> Vec<u8> {
        wire.iter().map(|c| c[0]).collect()
    }

    #[test]
    fn chain_is_assembled_in_wire_order() {
        let (frame, mut layout) = two_strips();
        let wire = assemble_chain(&frame, &layout, CHAIN_DLED1, 100.0, None).unwrap();
        assert_eq!(reds(&wire), [15, 25, 35, 45, 55, 65, 75, 85]);

        // A reversed segment flips only its own LEDs.
        layout.device_mut("dled1.1").unwrap().reversed = true;
        let wire = assemble_chain(&frame, &layout, CHAIN_DLED1, 100.0, None).unwrap();
        assert_eq!(reds(&wire), [15, 25, 35, 45, 85, 75, 65, 55]);

        // A disabled segment still occupies its LEDs, so later ones don't shift.
        layout.device_mut("dled1.0").unwrap().enabled = false;
        let wire = assemble_chain(&frame, &layout, CHAIN_DLED1, 100.0, None).unwrap();
        assert_eq!(reds(&wire), [0, 0, 0, 0, 85, 75, 65, 55]);

        // Nothing enabled, or nothing plugged in: leave the header alone.
        layout.device_mut("dled1.1").unwrap().enabled = false;
        assert!(assemble_chain(&frame, &layout, CHAIN_DLED1, 100.0, None).is_none());
        layout.chains.dled1.clear();
        layout.reconcile();
        assert!(assemble_chain(&frame, &layout, CHAIN_DLED1, 100.0, None).is_none());
    }

    #[test]
    fn identify_lights_one_device_and_marks_its_first_wire_led() {
        let (frame, mut layout) = two_strips();
        layout.device_mut("dled1.1").unwrap().reversed = true;
        let wire = assemble_chain(&frame, &layout, CHAIN_DLED1, 100.0, Some("dled1.1")).unwrap();
        assert_eq!(&wire[..4], &[[0, 0, 0]; 4]);
        assert_eq!(wire[4], [255, 0, 0]);
        assert_eq!(&wire[5..], &[[255, 255, 255]; 3]);
    }
}
