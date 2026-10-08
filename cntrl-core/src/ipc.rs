//! Line-delimited JSON over a Unix stream socket (`CNTRL_SOCKET` or `XDG_RUNTIME_DIR/cntrl/daemon.sock`).

use crate::profile::ProfileDocument;
use serde::{Deserialize, Serialize};
use std::io::{BufRead, BufReader, Write};
use std::path::PathBuf;

#[cfg(unix)]
use std::os::unix::net::UnixStream;

/// Override socket path for tests / custom layouts.
pub fn socket_path() -> PathBuf {
    if let Ok(p) = std::env::var("CNTRL_SOCKET") {
        let t = p.trim();
        if !t.is_empty() {
            return PathBuf::from(t);
        }
    }
    let runtime = std::env::var("XDG_RUNTIME_DIR").unwrap_or_else(|_| "/tmp".into());
    PathBuf::from(runtime).join("cntrl").join("daemon.sock")
}

/// One request per connection (newline-terminated JSON).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "cmd", rename_all = "snake_case")]
pub enum Request {
    Ping,
    /// Persist `document` to `slot` and push lighting over HID.
    ApplyProfile {
        document: ProfileDocument,
        slot: u8,
    },
    /// Start (or replace) the daemon-owned canvas stream with this layout.
    CanvasStart { layout: crate::canvas::CanvasLayout },
    /// Live-edit the running stream. Ignored when none is running.
    CanvasUpdate {
        layout: crate::canvas::CanvasLayout,
        #[serde(default)]
        identify: Option<String>,
    },
    CanvasStop,
    /// Stop and start again with the same layout (devices lost power on resume).
    CanvasRestart,
    CanvasStatus,
}

/// State of the daemon's canvas stream; every `Canvas*` request returns it.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct CanvasStatus {
    pub running: bool,
    /// Seconds since the stream's effect clock started (previews sync to it).
    pub elapsed_secs: f32,
    pub frames: u64,
    pub fps: f32,
    pub backends: String,
    pub timing: String,
    pub last_error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Response {
    pub ok: bool,
    #[serde(default)]
    pub message: String,
    #[serde(default)]
    pub error: Option<String>,
    #[serde(default)]
    pub canvas: Option<CanvasStatus>,
}

impl Response {
    pub fn ok_msg(msg: impl Into<String>) -> Self {
        Self {
            ok: true,
            message: msg.into(),
            error: None,
            canvas: None,
        }
    }

    pub fn err_msg(msg: impl Into<String>) -> Self {
        Self {
            ok: false,
            message: String::new(),
            error: Some(msg.into()),
            canvas: None,
        }
    }
}

/// Try to send `ApplyProfile` to a running daemon. Returns `Ok(None)` if no daemon is reachable.
#[cfg(unix)]
pub fn try_apply_via_daemon(doc: &ProfileDocument, slot: u8) -> Result<Option<Response>, String> {
    let path = socket_path();
    let mut stream = match UnixStream::connect(&path) {
        Ok(s) => s,
        Err(_) => return Ok(None),
    };
    let req = Request::ApplyProfile {
        document: doc.clone(),
        slot,
    };
    let line = serde_json::to_string(&req).map_err(|e| e.to_string())?;
    stream
        .write_all(line.as_bytes())
        .and_then(|_| stream.write_all(b"\n"))
        .map_err(|e| e.to_string())?;
    let mut reader = BufReader::new(stream);
    let mut resp_line = String::new();
    reader
        .read_line(&mut resp_line)
        .map_err(|e| e.to_string())?;
    let resp: Response = serde_json::from_str(resp_line.trim()).map_err(|e| e.to_string())?;
    Ok(Some(resp))
}

#[cfg(not(unix))]
pub fn try_apply_via_daemon(
    _doc: &ProfileDocument,
    _slot: u8,
) -> Result<Option<Response>, String> {
    Ok(None)
}

#[derive(Debug, Clone, Copy)]
pub struct ApplyOptions {
    pub keyboard: bool,
    pub pc_rgb: bool,
}

impl ApplyOptions {
    pub const ALL: Self = Self {
        keyboard: true,
        pc_rgb: true,
    };
}

/// Persist + HID locally, or via daemon when reachable.
pub fn apply_local_or_daemon(doc: &ProfileDocument, slot: u8) -> Result<(), String> {
    #[cfg(unix)]
    match try_apply_via_daemon(doc, slot) {
        Ok(Some(resp)) => {
            if resp.ok {
                return Ok(());
            }
            return Err(resp.error.unwrap_or_else(|| resp.message));
        }
        Ok(None) => {}
        Err(e) => {
            eprintln!("cntrl: daemon IPC error ({e}), using local HID");
        }
    }

    #[cfg(not(unix))]
    let _ = (doc, slot);

    apply_local(doc, slot)
}

/// Persist + keyboard HID + optional PC RGB backends locally (no daemon IPC).
pub fn apply_local(doc: &ProfileDocument, slot: u8) -> Result<(), String> {
    apply_local_with_options(doc, slot, ApplyOptions::ALL)
}

/// Persist locally, then apply only the selected device classes.
pub fn apply_local_with_options(
    doc: &ProfileDocument,
    slot: u8,
    options: ApplyOptions,
) -> Result<(), String> {
    use crate::persistence::persist_profile_slot;
    persist_profile_slot(slot, doc)?;
    push_to_devices(doc, options)
}

/// Push `doc` to the selected devices **without** persisting a profile slot.
/// Used by the boot-restore path so re-applying saved per-device state doesn't
/// clobber the active profile.
pub fn push_to_devices(doc: &ProfileDocument, options: ApplyOptions) -> Result<(), String> {
    use crate::hid::backend as hid;
    use crate::rgb_backends::{ApplyResult, BackendRegistry};

    if options.pc_rgb {
        if let Err(e) = crate::rgb_probe::ensure_runtime_probe(false) {
            eprintln!("cntrl: rgb probe init failed, continuing with existing env: {e}");
        }
    }
    let hid_result = if options.keyboard {
        let doc_for_hid = doc.clone();
        Some(match std::thread::spawn(move || hid::write_profile_to_device(&doc_for_hid)).join() {
            Ok(r) => r,
            Err(_) => Err("keyboard apply thread panicked".to_string()),
        })
    } else {
        None
    };
    let summary = if options.pc_rgb {
        let doc_for_rgb = doc.clone();
        Some(match std::thread::spawn(move || {
            let registry = BackendRegistry::from_env();
            registry.run(&doc_for_rgb)
        })
        .join()
        {
            Ok(s) => s,
            Err(_) => {
                return Err("pc-rgb apply thread panicked".to_string());
            }
        })
    } else {
        None
    };

    let verbose = std::env::var("CNTRL_PC_RGB_VERBOSE")
        .map(|s| {
            let t = s.trim().to_ascii_lowercase();
            t == "1" || t == "true" || t == "on"
        })
        .unwrap_or(false);
    let mut hard_errors: Vec<String> = Vec::new();
    if let Some(summary) = summary {
        if verbose {
            eprintln!("cntrl: {}", summary.short_message());
            for report in &summary.reports {
                eprintln!(
                    "cntrl: backend={} result={:?} reason={}",
                    report.backend_id, report.result, report.reason
                );
            }
        } else if summary.has_partial_failure() {
            eprintln!("cntrl: {}", summary.short_message());
            for report in &summary.reports {
                if report.result != ApplyResult::Ok {
                    eprintln!(
                        "cntrl: backend={} result={:?} reason={}",
                        report.backend_id, report.result, report.reason
                    );
                    if report.result == ApplyResult::Error {
                        hard_errors.push(format!("{}: {}", report.backend_id, report.reason));
                    }
                }
            }
        }
        for report in &summary.reports {
            if report.result == ApplyResult::Error {
                hard_errors.push(format!("{}: {}", report.backend_id, report.reason));
            }
        }
    }
    if !hard_errors.is_empty() {
        return Err(format!("pc-rgb backend error(s): {}", hard_errors.join(" | ")));
    }
    if let Some(hid_result) = hid_result {
        if let Err(e) = hid_result {
            return Err(format!("HID: {e}"));
        }
    }
    Ok(())
}

/// Send a `Canvas*` request. `Ok(None)` = no daemon is running (stream
/// locally instead); `Err` = the daemon answered with an error or hung.
pub fn canvas_request(req: &Request) -> Result<Option<CanvasStatus>, String> {
    #[cfg(unix)]
    {
        if UnixStream::connect(socket_path()).is_err() {
            return Ok(None);
        }
        let resp = send_request(req).map_err(|e| format!("daemon: {e}"))?;
        if !resp.ok {
            return Err(resp.error.unwrap_or(resp.message));
        }
        Ok(Some(resp.canvas.unwrap_or_default()))
    }
    #[cfg(not(unix))]
    {
        let _ = req;
        Ok(None)
    }
}

#[cfg(unix)]
pub fn send_request(req: &Request) -> std::io::Result<Response> {
    let path = socket_path();
    let mut stream = UnixStream::connect(&path)?;
    // Canvas requests are made from UI threads; a hung daemon must not freeze
    // them. Applies talk to slow hardware and get no limit.
    if !matches!(req, Request::ApplyProfile { .. }) {
        stream.set_read_timeout(Some(std::time::Duration::from_secs(3)))?;
    }
    let line = serde_json::to_string(req).map_err(|e| {
        std::io::Error::new(std::io::ErrorKind::InvalidData, e)
    })?;
    stream
        .write_all(line.as_bytes())
        .and_then(|_| stream.write_all(b"\n"))?;
    let mut reader = BufReader::new(stream);
    let mut resp_line = String::new();
    reader.read_line(&mut resp_line)?;
    serde_json::from_str(resp_line.trim()).map_err(|e| {
        std::io::Error::new(std::io::ErrorKind::InvalidData, e)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn canvas_messages_round_trip_and_old_responses_still_parse() {
        let req = Request::CanvasUpdate {
            layout: crate::canvas::CanvasLayout::default(),
            identify: Some("dled2.1".into()),
        };
        let json = serde_json::to_string(&req).unwrap();
        assert!(json.contains(r#""cmd":"canvas_update""#));
        match serde_json::from_str(&json).unwrap() {
            Request::CanvasUpdate { identify, layout } => {
                assert_eq!(identify.as_deref(), Some("dled2.1"));
                assert!(layout.device(crate::canvas::DEV_KEYBOARD).is_some());
            }
            other => panic!("wrong request: {other:?}"),
        }
        // Unit variants carry only the tag.
        assert_eq!(serde_json::to_string(&Request::CanvasStop).unwrap(), r#"{"cmd":"canvas_stop"}"#);

        // A response from a daemon that predates the canvas field.
        let old: Response = serde_json::from_str(r#"{"ok":true,"message":"pong"}"#).unwrap();
        assert!(old.canvas.is_none());
        let mut resp = Response::ok_msg("");
        resp.canvas = Some(CanvasStatus {
            running: true,
            fps: 60.0,
            ..Default::default()
        });
        let back: Response = serde_json::from_str(&serde_json::to_string(&resp).unwrap()).unwrap();
        assert!(back.canvas.unwrap().running);
    }
}
