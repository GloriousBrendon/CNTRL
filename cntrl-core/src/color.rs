//! Shared `#rrggbb` parsing (no UI).

/// Parse `#rrggbb` or `rrggbb` into RGB.
pub fn hex_to_rgb(hex: &str) -> Result<(u8, u8, u8), String> {
    let raw = hex.trim().strip_prefix('#').unwrap_or(hex.trim());
    if raw.len() != 6 || !raw.chars().all(|c| c.is_ascii_hexdigit()) {
        return Err("Expected #rrggbb".to_string());
    }
    let r = u8::from_str_radix(&raw[0..2], 16).map_err(|e| e.to_string())?;
    let g = u8::from_str_radix(&raw[2..4], 16).map_err(|e| e.to_string())?;
    let b = u8::from_str_radix(&raw[4..6], 16).map_err(|e| e.to_string())?;
    Ok((r, g, b))
}

pub fn rgb_to_hex(rgb: (u8, u8, u8)) -> String {
    format!("#{:02x}{:02x}{:02x}", rgb.0, rgb.1, rgb.2)
}
