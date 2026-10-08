//! Drive the lighting from the user's theme switcher.
//!
//! The switcher writes a pywal-style `~/.config/themes/colors.conf`; its `accent`
//! key becomes the lighting color. Two entry points share this logic:
//!   * `cntrl apply-theme` (the switcher calls it directly), and
//!   * the daemon's file watcher (auto-apply when colors.conf changes).

use crate::color::{hex_to_rgb, rgb_to_hex};
use crate::ipc::{apply_local_with_options, ApplyOptions};
use crate::lighting_modes::{EFFECT_ID_OFF, MODE_STATIC};
use crate::persistence::load_profile_slot;
use crate::theme;

/// Read the accent color from the theme file. Errors if the file is absent so
/// callers don't silently apply the built-in default.
pub fn current_theme_color() -> Result<(u8, u8, u8), String> {
    let path = theme::theme_file_path();
    if !path.exists() {
        return Err(format!("theme file not found: {}", path.display()));
    }
    let tc = theme::load_theme_from_disk();
    hex_to_rgb(&tc.accent).map_err(|e| format!("accent {:?}: {e}", tc.accent))
}

/// Apply the current theme accent to `slot`, keeping the slot's existing effect
/// (so presets keep animating, just recolored). Returns the RGB that was applied.
pub fn apply_theme(slot: u8, options: ApplyOptions) -> Result<(u8, u8, u8), String> {
    let (r, g, b) = current_theme_color()?;
    let mut doc = load_profile_slot(slot)?;
    let hex = rgb_to_hex((r, g, b));
    doc.data.light.config.hex_color = hex.clone();
    doc.data.light.colors = vec![hex];
    // If the slot was Off, switch to Static so the theme color is actually visible.
    if doc.data.light.config.effect == EFFECT_ID_OFF {
        doc.data.light.config.effect = MODE_STATIC;
    }
    apply_local_with_options(&doc, slot, options)?;
    // Persist the *promoted* doc, so a slot that was Off doesn't get re-saved as
    // Off and re-applied dark by `cntrl restore` on the next boot.
    crate::device_state::remember_applied(&doc, options);
    Ok((r, g, b))
}

/// Convenience: resolve scope from settings flags into [`ApplyOptions`].
pub fn options_from_settings(keyboard: bool, pc: bool) -> ApplyOptions {
    ApplyOptions {
        keyboard,
        pc_rgb: pc,
    }
}
