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
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Response {
    pub ok: bool,
    #[serde(default)]
    pub message: String,
    #[serde(default)]
    pub error: Option<String>,
}

impl Response {
    pub fn ok_msg(msg: impl Into<String>) -> Self {
        Self {
            ok: true,
            message: msg.into(),
            error: None,
        }
    }

    pub fn err_msg(msg: impl Into<String>) -> Self {
        Self {
            ok: false,
            message: String::new(),
            error: Some(msg.into()),
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

/// Persist + HID locally, or via daemon when reachable.
pub fn apply_local_or_daemon(doc: &ProfileDocument, slot: u8) -> Result<(), String> {
    use crate::hid::backend as hid;
    use crate::persistence::persist_profile_slot;

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

    persist_profile_slot(slot, doc)?;
    hid::write_profile_to_device(doc)
}

#[cfg(unix)]
pub fn send_request(req: &Request) -> std::io::Result<Response> {
    let path = socket_path();
    let mut stream = UnixStream::connect(&path)?;
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
