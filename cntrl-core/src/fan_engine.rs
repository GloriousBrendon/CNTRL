//! Fan control engine loop + crash/termination safety.
//!
//! Shared by `cntrl fans run` (foreground) and the daemon's `fan_watcher` thread.
//! On any orderly exit — Ctrl-C (SIGINT), SIGTERM, panic, or a stop request — the
//! engine restores every driven fan to the enable/pwm it had before the engine
//! engaged, mirroring the canvas stream's "stop → restore saved lighting" rule.
//!
//! Residual risk (documented, not solved): `SIGKILL` (`kill -9`) cannot be
//! trapped, so a hard-killed engine leaves fans at their last commanded duty in
//! manual mode. `cntrl fans restore` recovers from that on next run.

use crate::fan_curves::{self, FanTickSummary, TickState};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Once;
use std::time::Duration;

/// Set by the signal handler and by `request_stop`. The loop polls it.
static STOP: AtomicBool = AtomicBool::new(false);
static PANIC_HOOK: Once = Once::new();
static SIGNALS: Once = Once::new();

pub fn request_stop() {
    STOP.store(true, Ordering::SeqCst);
}

pub fn should_stop() -> bool {
    STOP.load(Ordering::SeqCst)
}

/// Async-signal-safe: only stores to an atomic. Restore happens in the loop.
extern "C" fn on_signal(_sig: i32) {
    STOP.store(true, Ordering::SeqCst);
}

/// Install SIGINT + SIGTERM handlers that ask the engine loop to stop (and thus
/// restore). Safe to call more than once.
#[cfg(target_os = "linux")]
pub fn install_signal_handlers() {
    use nix::sys::signal::{sigaction, SaFlags, SigAction, SigHandler, SigSet, Signal};
    SIGNALS.call_once(|| {
        let action = SigAction::new(
            SigHandler::Handler(on_signal),
            SaFlags::empty(),
            SigSet::empty(),
        );
        // SAFETY: the handler only performs an atomic store — async-signal-safe.
        unsafe {
            let _ = sigaction(Signal::SIGINT, &action);
            let _ = sigaction(Signal::SIGTERM, &action);
        }
    });
}

#[cfg(not(target_os = "linux"))]
pub fn install_signal_handlers() {}

/// Chain a panic hook that restores fans before the default panic output runs,
/// so a crash in any thread doesn't strand fans in manual mode.
pub fn install_panic_hook() {
    PANIC_HOOK.call_once(|| {
        let prev = std::panic::take_hook();
        std::panic::set_hook(Box::new(move |info| {
            let msg = fan_curves::restore_all();
            eprintln!("cntrl fans: panic — {msg}");
            prev(info);
        }));
    });
}

/// A pid lockfile preventing two engines (e.g. the daemon and a manual
/// `cntrl fans run`) from fighting over the same `pwm*` files. Held for the
/// lifetime of the returned guard; released (file removed) on drop.
pub struct EngineLock {
    path: std::path::PathBuf,
}

impl Drop for EngineLock {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
    }
}

fn lock_path() -> Result<std::path::PathBuf, String> {
    dirs::config_dir()
        .map(|p| p.join("cntrl").join("fan_engine.lock"))
        .ok_or_else(|| "Could not resolve config directory".to_string())
}

/// Acquire the engine lock, or return an error naming the holding pid. A lock
/// whose pid is no longer alive is treated as stale and taken over.
pub fn acquire_lock() -> Result<EngineLock, String> {
    let path = lock_path()?;
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    }
    if let Ok(raw) = std::fs::read_to_string(&path) {
        if let Ok(pid) = raw.trim().parse::<i32>() {
            if pid_alive(pid) {
                return Err(format!(
                    "fan engine already running (pid {pid}); stop it first or run `cntrl fans restore`"
                ));
            }
        }
        // Stale lock — fall through and overwrite.
    }
    std::fs::write(&path, std::process::id().to_string()).map_err(|e| e.to_string())?;
    Ok(EngineLock { path })
}

fn pid_alive(pid: i32) -> bool {
    std::path::Path::new(&format!("/proc/{pid}")).exists()
}

/// Run the control loop until a stop is requested (signal or `request_stop`),
/// then restore all driven fans. `log` is called with each tick's summary.
///
/// Config is reloaded every tick so GUI/CLI edits apply live, but the long-lived
/// [`TickState`] (EMA + last-written duty) persists across iterations.
pub fn run_loop(mut log: impl FnMut(&FanTickSummary)) {
    STOP.store(false, Ordering::SeqCst);
    let mut state = TickState::default();

    while !should_stop() {
        let cfg = fan_curves::load();

        if cfg.enabled {
            let discovery = crate::fan_control::discover();
            if let Err(e) = fan_curves::capture_and_engage(&cfg, &discovery) {
                eprintln!("cntrl fans: capture/engage failed: {e}");
            }
            let summary = fan_curves::tick(&cfg, &discovery, &mut state, false);
            log(&summary);
        }

        // Sleep the tick interval in short slices so stop is responsive.
        let interval = cfg.tick_interval_ms.max(250);
        let mut waited = 0u64;
        while waited < interval && !should_stop() {
            let step = 200u64.min(interval - waited);
            std::thread::sleep(Duration::from_millis(step));
            waited += step;
        }
    }

    let msg = fan_curves::restore_all();
    eprintln!("cntrl fans: engine stopped — {msg}");
}
