//! Foreground Unix socket server (`cntrl daemon`).

use cntrl_core::hid::backend as hid;
use cntrl_core::ipc::{self, Request, Response};
use cntrl_core::persistence::persist_profile_slot;
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
            match persist_profile_slot(slot, &document) {
                Err(e) => Response::err_msg(format!("persist: {e}")),
                Ok(()) => match hid::write_profile_to_device(&document) {
                    Ok(()) => Response::ok_msg("lighting applied"),
                    Err(e) => Response::err_msg(format!("HID: {e}")),
                },
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
