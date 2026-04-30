//! Waybar-friendly toggle: `cntrl popup <widget> --toggle` kills that widget's popup or spawns it.

use cntrl_iced::UiMode;
use std::path::PathBuf;

pub fn pid_file_path(mode: UiMode) -> PathBuf {
    let runtime = std::env::var("XDG_RUNTIME_DIR").unwrap_or_else(|_| "/tmp".into());
    PathBuf::from(runtime)
        .join("cntrl")
        .join(format!("popup-{}.pid", mode.slug()))
}

/// If a popup is running, terminate it and return `true`. Otherwise return `false`.
pub fn try_close_existing_popup(mode: UiMode) -> Result<bool, String> {
    #[cfg(unix)]
    {
        use std::io::Read;
        let path = pid_file_path(mode);
        if !path.exists() {
            return Ok(false);
        }
        let mut s = String::new();
        std::fs::File::open(&path)
            .and_then(|mut f| f.read_to_string(&mut s))
            .map_err(|e| e.to_string())?;
        let pid: libc::pid_t =
            s.trim().parse().map_err(|_| format!("invalid {}", path.display()))?;
        unsafe {
            libc::kill(pid, libc::SIGTERM);
        }
        let _ = std::fs::remove_file(&path);
        Ok(true)
    }
    #[cfg(not(unix))]
    {
        Ok(false)
    }
}

pub fn write_pid_file(mode: UiMode) -> Result<(), String> {
    if let Some(dir) = pid_file_path(mode).parent() {
        std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    }
    let pid = std::process::id() as i32;
    std::fs::write(pid_file_path(mode), format!("{pid}\n")).map_err(|e| e.to_string())
}

pub fn remove_pid_file(mode: UiMode) {
    let _ = std::fs::remove_file(pid_file_path(mode));
}
