//! GTK integration: hot reload via inotify. Parsing lives in `cntrl_core::theme`.

pub use cntrl_core::color::hex_to_rgb;
pub use cntrl_core::theme::{load_theme_from_disk, theme_file_path, ThemeColors};

use std::sync::{Arc, Mutex};

#[cfg(target_os = "linux")]
use inotify::{Inotify, WatchMask};

#[cfg(target_os = "linux")]
pub fn start_theme_watcher<F>(theme_state: Arc<Mutex<ThemeColors>>, on_change: F)
where
    F: Fn(ThemeColors) + Send + 'static,
{
    std::thread::spawn(move || {
        let path = theme_file_path();
        let Some(parent) = path.parent() else {
            eprintln!("theme watcher: invalid theme file path");
            return;
        };

        let mut inotify = match Inotify::init() {
            Ok(v) => v,
            Err(e) => {
                eprintln!("theme watcher: inotify init failed: {e}");
                return;
            }
        };

        if let Err(e) = inotify.watches().add(
            parent,
            WatchMask::MODIFY
                | WatchMask::CREATE
                | WatchMask::MOVED_TO
                | WatchMask::MOVED_FROM
                | WatchMask::DELETE
                | WatchMask::DELETE_SELF,
        ) {
            eprintln!("theme watcher: failed to add watch for {}: {e}", parent.display());
            return;
        }

        let mut last_accent = {
            let g = theme_state.lock().ok();
            g.as_ref()
                .map(|x| x.accent.clone())
                .unwrap_or_else(|| ThemeColors::default().accent)
        };

        let mut buffer = [0u8; 4096];
        loop {
            match inotify.read_events_blocking(&mut buffer) {
                Ok(events) => {
                    for event in events {
                        let name_matches = event
                            .name
                            .map(|n| n.to_string_lossy() == "colors.conf")
                            .unwrap_or(false);
                        let self_changed = event.mask.contains(inotify::EventMask::DELETE_SELF);
                        if !name_matches && !self_changed {
                            continue;
                        }

                        let fresh = load_theme_from_disk();
                        let old = {
                            let mut guard = match theme_state.lock() {
                                Ok(g) => g,
                                Err(e) => {
                                    eprintln!("theme watcher: state lock failed: {e}");
                                    continue;
                                }
                            };
                            let old = guard.clone();
                            *guard = fresh.clone();
                            old
                        };

                        if old == fresh {
                            continue;
                        }

                        on_change(fresh.clone());

                        if last_accent != fresh.accent {
                            sync_keyboard_to_accent(&fresh.accent);
                            last_accent = fresh.accent.clone();
                        }
                    }
                }
                Err(e) => {
                    eprintln!("theme watcher: read error: {e}");
                    std::thread::sleep(std::time::Duration::from_millis(250));
                }
            }
        }
    });
}

#[cfg(target_os = "linux")]
fn sync_keyboard_to_accent(hex: &str) {
    let Ok((r, g, b)) = hex_to_rgb(hex) else {
        eprintln!("theme watcher: invalid accent format {hex}");
        return;
    };
    if let Err(e) = cntrl_core::mchose_lighting::set_solid_rgb(r, g, b, 100, Some(0), true) {
        eprintln!("theme watcher: failed to sync keyboard accent: {e}");
    }
}

#[cfg(not(target_os = "linux"))]
pub fn start_theme_watcher<F>(_theme_state: Arc<Mutex<ThemeColors>>, _on_change: F)
where
    F: Fn(ThemeColors) + Send + 'static,
{
}
