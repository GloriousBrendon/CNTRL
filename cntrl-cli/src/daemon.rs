//! Foreground Unix socket server (`cntrl daemon`).

use cntrl_core::ipc::{self, Request, Response};
use std::io::{BufRead, BufReader, Write};
#[cfg(unix)]
use std::os::unix::net::{UnixListener, UnixStream};

pub fn run() -> Result<(), String> {
    #[cfg(not(unix))]
    {
        return Err("cntrl daemon is only supported on Unix".into());
    }
    #[cfg(unix)]
    {
        run_unix()
    }
}

#[cfg(unix)]
fn run_unix() -> Result<(), String> {
    let path = ipc::socket_path();
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    }
    if path.exists() {
        // Stale socket from crashed daemon: remove if nothing is listening.
        if UnixStream::connect(&path).is_err() {
            let _ = std::fs::remove_file(&path);
        } else {
            return Err(
                "Socket already in use — is cntrl daemon already running?".into(),
            );
        }
    }
    let listener = UnixListener::bind(&path).map_err(|e| e.to_string())?;
    eprintln!("cntrl daemon: listening on {}", path.display());

    // Fan-control safety: restore fans on SIGINT/SIGTERM/panic before exit.
    cntrl_core::fan_engine::install_panic_hook();
    cntrl_core::fan_engine::install_signal_handlers();

    std::thread::spawn(theme_watcher);
    let fan = std::thread::spawn(fan_watcher);

    if cntrl_core::settings::load().canvas_autostart {
        eprintln!("cntrl daemon: starting canvas stream (autostart)");
        // No presence check here: at login the devices may not have appeared
        // yet. The stream retries a missing device every couple of seconds.
        canvas(&Request::CanvasStart {
            layout: cntrl_core::canvas::load_layout(),
        });
    }

    // Accept on its own thread (blocking, so clients are served at once) while
    // this one waits for a stop signal. The accept thread dies with the process.
    std::thread::spawn(move || {
        for conn in listener.incoming() {
            match conn {
                Ok(s) => {
                    std::thread::spawn(move || {
                        if let Err(e) = handle_client(s) {
                            eprintln!("cntrl daemon: client error: {e}");
                        }
                    });
                }
                Err(e) => eprintln!("cntrl daemon: accept error: {e}"),
            }
        }
    });
    while !cntrl_core::fan_engine::should_stop() {
        std::thread::sleep(std::time::Duration::from_millis(100));
    }

    // Stopping the stream puts each device's saved lighting back.
    canvas(&Request::CanvasStop);
    let _ = std::fs::remove_file(&path);
    // The fan thread restores the fans on its way out; exiting before it
    // finishes would strand them in manual mode.
    let _ = fan.join();
    eprintln!("cntrl daemon: stopped.");
    Ok(())
}

/// The daemon-owned canvas stream: it outlives the panel, and is the one
/// writer to the devices while it runs.
#[cfg(unix)]
static CANVAS: std::sync::Mutex<Option<cntrl_core::canvas_stream::StreamHandle>> =
    std::sync::Mutex::new(None);

/// Carry out a `Canvas*` request and report the stream's state afterwards.
#[cfg(unix)]
fn canvas(req: &Request) -> ipc::CanvasStatus {
    use cntrl_core::canvas_stream;
    let mut slot = CANVAS.lock().expect("canvas stream slot");
    // A stream whose thread died is as good as none.
    if slot.as_ref().is_some_and(|s| !s.is_running()) {
        *slot = None;
    }
    match req {
        Request::CanvasStart { layout } => {
            if let Some(old) = slot.take() {
                old.stop();
            }
            *slot = Some(canvas_stream::start(layout.clone()));
        }
        Request::CanvasUpdate { layout, identify } => {
            if let Some(stream) = slot.as_ref() {
                let mut shared = stream.shared.lock().expect("stream shared");
                shared.layout = layout.clone();
                shared.identify = identify.clone();
            }
        }
        Request::CanvasStop => {
            if let Some(stream) = slot.take() {
                stream.stop();
            }
        }
        Request::CanvasRestart => {
            if let Some(old) = slot.take() {
                let layout = old.shared.lock().expect("stream shared").layout.clone();
                old.stop();
                *slot = Some(canvas_stream::start(layout));
            }
        }
        _ => {}
    }
    slot.as_ref().map(|s| s.status()).unwrap_or_default()
}

/// Background fan control loop, mirroring `theme_watcher`'s shape. Reloads the
/// curve config every tick (so GUI edits apply live) while keeping long-lived
/// smoothing/last-written state inside the engine loop.
fn fan_watcher() {
    // If a foreground `cntrl fans run` already holds the lock, don't fight it.
    let _lock = match cntrl_core::fan_engine::acquire_lock() {
        Ok(l) => l,
        Err(e) => {
            eprintln!("cntrl daemon: fan engine not started: {e}");
            return;
        }
    };
    cntrl_core::fan_engine::run_loop(|summary| {
        let active = summary
            .reports
            .iter()
            .any(|r| {
                use cntrl_core::fan_curves::ApplyResult::*;
                matches!(r.result, Written | Error | HandedBack)
            });
        if active {
            eprintln!("cntrl daemon: {}", summary.short_message());
        }
    });
}

/// Background watcher: when theme sync is enabled in settings, re-apply the theme
/// accent to the lighting whenever `~/.config/themes/colors.conf` changes.
/// Settings are reloaded each tick so the UI toggle takes effect without a restart.
fn theme_watcher() {
    use cntrl_core::{ipc::ApplyOptions, settings, theme, theme_sync};
    use std::time::{Duration, SystemTime};

    let mut last_mtime: Option<SystemTime> = std::fs::metadata(theme::theme_file_path())
        .ok()
        .and_then(|m| m.modified().ok());

    loop {
        std::thread::sleep(Duration::from_millis(1000));
        let cfg = settings::load();
        if !cfg.theme_sync_enabled {
            continue;
        }
        let current = std::fs::metadata(theme::theme_file_path())
            .ok()
            .and_then(|m| m.modified().ok());
        if current == last_mtime {
            continue;
        }
        last_mtime = current;
        let slot = cfg.theme_sync_slot.min(3);
        let options = ApplyOptions {
            keyboard: cfg.theme_sync_keyboard,
            pc_rgb: cfg.theme_sync_pc,
        };
        match theme_sync::apply_theme(slot, options) {
            Ok((r, g, b)) => eprintln!(
                "cntrl daemon: theme sync applied #{r:02x}{g:02x}{b:02x} to slot {slot}"
            ),
            Err(e) => eprintln!("cntrl daemon: theme sync failed: {e}"),
        }
    }
}

#[cfg(unix)]
fn handle_client(mut stream: UnixStream) -> Result<(), String> {
    let mut reader = BufReader::new(&stream);
    let mut line = String::new();
    reader.read_line(&mut line).map_err(|e| e.to_string())?;
    let trimmed = line.trim();
    if trimmed.is_empty() {
        return Ok(());
    }
    let req: Request = serde_json::from_str(trimmed).map_err(|e| e.to_string())?;
    let resp = match req {
        Request::Ping => Response::ok_msg("pong"),
        Request::ApplyProfile { document, slot } => {
            // An explicit apply wins over a running effect.
            canvas(&Request::CanvasStop);
            match ipc::apply_local(&document, slot) {
                Ok(()) => Response::ok_msg("lighting applied (keyboard + pc-rgb fanout)"),
                Err(e) => {
                    if e.to_lowercase().contains("hid") {
                        Response::err_msg(format!("HID: {e}"))
                    } else {
                        Response::err_msg(e)
                    }
                }
            }
        }
        canvas_req => {
            let mut resp = Response::ok_msg("");
            resp.canvas = Some(canvas(&canvas_req));
            resp
        }
    };
    let out = serde_json::to_string(&resp).map_err(|e| e.to_string())?;
    stream
        .write_all(out.as_bytes())
        .and_then(|_| stream.write_all(b"\n"))
        .map_err(|e| e.to_string())?;
    Ok(())
}
