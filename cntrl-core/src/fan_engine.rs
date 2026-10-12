//! Fan control engine loop + crash/termination safety.
//!
//! Shared by `cntrl fans run` (foreground) and the daemon's `fan_watcher` thread.
//! On any orderly exit — Ctrl-C (SIGINT), SIGTERM, panic, or a stop request — the
//! engine restores every driven fan to the enable/pwm it had before the engine
//! engaged, mirroring the canvas stream's "stop → restore saved lighting" rule.
//!
//! `SIGKILL` (`kill -9`) and hard crashes cannot be trapped by the engine
//! itself, so a second process stands by ([`watchdog`], started as
//! `cntrl fans watchdog`): it waits on the engine's lock, which the kernel
//! frees when the engine dies in any way, and then restores the fans.

use crate::fan_curves::{self, FanTickSummary, TickState};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Once;
use std::time::Duration;

/// Set by the signal handler. The loop polls it.
static STOP: AtomicBool = AtomicBool::new(false);
static PANIC_HOOK: Once = Once::new();
static SIGNALS: Once = Once::new();

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

/// An OS lock preventing two engines (e.g. the daemon and a manual
/// `cntrl fans run`) from fighting over the same `pwm*` files. Held for the
/// lifetime of the returned guard. The kernel drops it when the process dies,
/// however it dies, so a crash or power cut can't leave it stuck.
pub struct EngineLock {
    _file: std::fs::File,
}

fn lock_path() -> Result<std::path::PathBuf, String> {
    dirs::config_dir()
        .map(|p| p.join("cntrl").join("fan_engine.lock"))
        .ok_or_else(|| "Could not resolve config directory".to_string())
}

/// Acquire the engine lock, or return an error naming the holding pid.
pub fn acquire_lock() -> Result<EngineLock, String> {
    acquire_lock_at(&lock_path()?)
}

/// Block until no engine holds the lock at `path` (it stopped, crashed or was
/// killed: the kernel releases the lock with the process).
pub(crate) fn wait_for_engine_exit(path: &std::path::Path) -> Result<(), String> {
    let file = std::fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(path)
        .map_err(|e| format!("{}: {e}", path.display()))?;
    file.lock().map_err(|e| format!("{}: {e}", path.display()))
}

/// Stand by until the running engine is gone, then hand back any fan it left
/// in manual mode. After a clean stop there is nothing left to restore.
pub fn watchdog() -> Result<String, String> {
    wait_for_engine_exit(&lock_path()?)?;
    Ok(fan_curves::restore_all())
}

pub(crate) fn acquire_lock_at(path: &std::path::Path) -> Result<EngineLock, String> {
    use std::io::Write;
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    }
    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(path)
        .map_err(|e| format!("{}: {e}", path.display()))?;
    if file.try_lock().is_err() {
        // The pid in the file is informational only (written by the holder).
        let pid = std::fs::read_to_string(path).unwrap_or_default();
        return Err(format!(
            "fan engine already running (pid {}); stop it first",
            pid.trim()
        ));
    }
    let _ = file.set_len(0);
    let _ = write!(file, "{}", std::process::id());
    Ok(EngineLock { _file: file })
}

/// Start the stand-by process that restores the fans if this one dies without
/// doing so (see [`watchdog`]). Call it once the engine lock is held. `exe` is
/// the `cntrl` binary.
pub fn spawn_watchdog(exe: &std::path::Path) {
    use std::process::{Command, Stdio};
    let spawned = Command::new(exe)
        .args(["fans", "watchdog"])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .spawn();
    if let Err(e) = spawned {
        eprintln!("cntrl fans: no watchdog ({e}); a hard kill would leave fans at a fixed speed");
    }
}

/// Run the control loop until a stop is requested (SIGINT/SIGTERM),
/// then restore all driven fans. `log` is called with each tick's summary.
///
/// Config is reloaded every tick so GUI/CLI edits apply live, but the long-lived
/// [`TickState`] (EMA + last-written duty) persists across iterations.
pub fn run_loop(mut log: impl FnMut(&FanTickSummary)) {
    STOP.store(false, Ordering::SeqCst);
    let mut state = TickState::default();
    // Starts true so a state file left by a hard-killed engine is restored
    // even when the engine comes up disabled.
    let mut engaged = true;

    while !should_stop() {
        let cfg = fan_curves::load();

        if cfg.enabled {
            engaged = true;
            if let Some(msg) = fan_curves::release_unassigned(&cfg) {
                eprintln!("cntrl fans: curve removed — {msg}");
            }
            let discovery = crate::fan_control::discover();
            match fan_curves::capture_originals(&cfg, &discovery) {
                Ok(()) => {
                    let summary = fan_curves::tick(&cfg, &discovery, &mut state, false);
                    log(&summary);
                }
                // Without the saved original we could never hand the fan back.
                Err(e) => eprintln!("cntrl fans: not driving fans, could not save their state: {e}"),
            }
        } else if engaged {
            // Disabled in the UI: hand every fan back rather than leave it in
            // manual mode at its last duty.
            engaged = false;
            state = TickState::default();
            eprintln!("cntrl fans: engine disabled — {}", fan_curves::restore_all());
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lock_is_exclusive_and_ignores_leftover_files() {
        let path = std::env::temp_dir().join(format!("cntrl-lock-test-{}", std::process::id()));
        // A file left by a dead engine (power cut) must not block a new one.
        std::fs::write(&path, "1").unwrap();
        let held = acquire_lock_at(&path).expect("stale file must not block");
        assert!(acquire_lock_at(&path).is_err(), "second engine must be refused");
        drop(held);
        assert!(acquire_lock_at(&path).is_ok(), "lock must be free after drop");
        let _ = std::fs::remove_file(path);
    }
}
