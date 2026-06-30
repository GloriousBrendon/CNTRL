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
    std::thread::spawn(theme_watcher);
    for stream in listener.incoming() {
        match stream {
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
    Ok(())
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
    };
    let out = serde_json::to_string(&resp).map_err(|e| e.to_string())?;
    stream
        .write_all(out.as_bytes())
        .and_then(|_| stream.write_all(b"\n"))
        .map_err(|e| e.to_string())?;
    Ok(())
}
