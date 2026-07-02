//! Ace68 68-key layout geometry (shared by GTK canvas and ICED canvas). Units: 15 wide × 5 rows.

#[derive(Clone, Debug)]
pub struct Key {
    pub label: &'static str,
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub idx: usize,
}

pub fn get_keyboard_layout() -> Vec<Key> {
    vec![
        Key { label: "Esc", x: 0.0, y: 0.0, width: 1.0, idx: 0 },
        Key { label: "1", x: 1.0, y: 0.0, width: 1.0, idx: 1 },
        Key { label: "2", x: 2.0, y: 0.0, width: 1.0, idx: 2 },
        Key { label: "3", x: 3.0, y: 0.0, width: 1.0, idx: 3 },
        Key { label: "4", x: 4.0, y: 0.0, width: 1.0, idx: 4 },
        Key { label: "5", x: 5.0, y: 0.0, width: 1.0, idx: 5 },
        Key { label: "6", x: 6.0, y: 0.0, width: 1.0, idx: 6 },
        Key { label: "7", x: 7.0, y: 0.0, width: 1.0, idx: 7 },
        Key { label: "8", x: 8.0, y: 0.0, width: 1.0, idx: 8 },
        Key { label: "9", x: 9.0, y: 0.0, width: 1.0, idx: 9 },
        Key { label: "0", x: 10.0, y: 0.0, width: 1.0, idx: 10 },
        Key { label: "-", x: 11.0, y: 0.0, width: 1.0, idx: 11 },
        Key { label: "=", x: 12.0, y: 0.0, width: 1.0, idx: 12 },
        Key { label: "⌫", x: 13.0, y: 0.0, width: 2.0, idx: 13 },
        Key { label: "Tab", x: 0.0, y: 1.0, width: 1.5, idx: 14 },
        Key { label: "Q", x: 1.5, y: 1.0, width: 1.0, idx: 15 },
        Key { label: "W", x: 2.5, y: 1.0, width: 1.0, idx: 16 },
        Key { label: "E", x: 3.5, y: 1.0, width: 1.0, idx: 17 },
        Key { label: "R", x: 4.5, y: 1.0, width: 1.0, idx: 18 },
        Key { label: "T", x: 5.5, y: 1.0, width: 1.0, idx: 19 },
        Key { label: "Y", x: 6.5, y: 1.0, width: 1.0, idx: 20 },
        Key { label: "U", x: 7.5, y: 1.0, width: 1.0, idx: 21 },
        Key { label: "I", x: 8.5, y: 1.0, width: 1.0, idx: 22 },
        Key { label: "O", x: 9.5, y: 1.0, width: 1.0, idx: 23 },
        Key { label: "P", x: 10.5, y: 1.0, width: 1.0, idx: 24 },
        Key { label: "[", x: 11.5, y: 1.0, width: 1.0, idx: 25 },
        Key { label: "]", x: 12.5, y: 1.0, width: 1.0, idx: 26 },
        Key { label: "\\", x: 13.5, y: 1.0, width: 1.5, idx: 27 },
        Key { label: "Caps", x: 0.0, y: 2.0, width: 1.75, idx: 28 },
        Key { label: "A", x: 1.75, y: 2.0, width: 1.0, idx: 29 },
        Key { label: "S", x: 2.75, y: 2.0, width: 1.0, idx: 30 },
        Key { label: "D", x: 3.75, y: 2.0, width: 1.0, idx: 31 },
        Key { label: "F", x: 4.75, y: 2.0, width: 1.0, idx: 32 },
        Key { label: "G", x: 5.75, y: 2.0, width: 1.0, idx: 33 },
        Key { label: "H", x: 6.75, y: 2.0, width: 1.0, idx: 34 },
        Key { label: "J", x: 7.75, y: 2.0, width: 1.0, idx: 35 },
        Key { label: "K", x: 8.75, y: 2.0, width: 1.0, idx: 36 },
        Key { label: "L", x: 9.75, y: 2.0, width: 1.0, idx: 37 },
        Key { label: ";", x: 10.75, y: 2.0, width: 1.0, idx: 38 },
        Key { label: "'", x: 11.75, y: 2.0, width: 1.0, idx: 39 },
        Key { label: "↩", x: 12.75, y: 2.0, width: 2.25, idx: 40 },
        Key { label: "⇧", x: 0.0, y: 3.0, width: 2.25, idx: 41 },
        Key { label: "Z", x: 2.25, y: 3.0, width: 1.0, idx: 42 },
        Key { label: "X", x: 3.25, y: 3.0, width: 1.0, idx: 43 },
        Key { label: "C", x: 4.25, y: 3.0, width: 1.0, idx: 44 },
        Key { label: "V", x: 5.25, y: 3.0, width: 1.0, idx: 45 },
        Key { label: "B", x: 6.25, y: 3.0, width: 1.0, idx: 46 },
        Key { label: "N", x: 7.25, y: 3.0, width: 1.0, idx: 47 },
        Key { label: "M", x: 8.25, y: 3.0, width: 1.0, idx: 48 },
        Key { label: ",", x: 9.25, y: 3.0, width: 1.0, idx: 49 },
        Key { label: ".", x: 10.25, y: 3.0, width: 1.0, idx: 50 },
        Key { label: "/", x: 11.25, y: 3.0, width: 1.0, idx: 51 },
        Key { label: "⇧", x: 12.25, y: 3.0, width: 1.75, idx: 52 },
        Key { label: "↑", x: 14.0, y: 3.0, width: 1.0, idx: 53 },
        Key { label: "Ctrl", x: 0.0, y: 4.0, width: 1.25, idx: 54 },
        Key { label: "⊞", x: 1.25, y: 4.0, width: 1.0, idx: 55 },
        Key { label: "Alt", x: 2.25, y: 4.0, width: 1.25, idx: 56 },
        Key { label: "Space", x: 3.5, y: 4.0, width: 6.25, idx: 57 },
        Key { label: "Alt", x: 9.75, y: 4.0, width: 1.25, idx: 58 },
        Key { label: "Ctrl", x: 11.0, y: 4.0, width: 1.0, idx: 59 },
        Key { label: "←", x: 12.0, y: 4.0, width: 1.0, idx: 60 },
        Key { label: "↓", x: 13.0, y: 4.0, width: 1.0, idx: 61 },
        Key { label: "→", x: 14.0, y: 4.0, width: 1.0, idx: 62 },
    ]
}

/// HID usage code for each key, aligned with `get_keyboard_layout()` order.
/// Left/right modifiers are distinguished by position (225/229 = L/R Shift,
/// 224/228 = L/R Ctrl, 226/230 = L/R Alt, 227 = Win).
pub fn hid_usage_codes() -> [u16; 63] {
    [
        // Row 0: Esc 1..0 - = Bksp
        41, 30, 31, 32, 33, 34, 35, 36, 37, 38, 39, 45, 46, 42,
        // Row 1: Tab Q W E R T Y U I O P [ ] \
        43, 20, 26, 8, 21, 23, 28, 24, 12, 18, 19, 47, 48, 49,
        // Row 2: Caps A S D F G H J K L ; ' Enter
        57, 4, 22, 7, 9, 10, 11, 13, 14, 15, 51, 52, 40,
        // Row 3: LShift Z X C V B N M , . / RShift Up
        225, 29, 27, 6, 25, 5, 17, 16, 54, 55, 56, 229, 82,
        // Row 4: LCtrl Win LAlt Space RAlt RCtrl Left Down Right
        224, 227, 226, 44, 230, 228, 80, 81, 79,
    ]
}

/// Hit-test in widget pixel space (`width` × `height`), same math as GTK `key_index_at`.
pub fn key_index_at(width: f32, height: f32, x: f32, y: f32) -> Option<usize> {
    if width <= 0.0 || height <= 0.0 {
        return None;
    }
    let layout = get_keyboard_layout();
    let width_f = width;
    let height_f = height;
    let base_unit = width_f / 15.0;
    let row_height = height_f / 5.0;

    for key in &layout {
        let px = key.x as f32 * base_unit;
        let py = key.y as f32 * row_height;
        let pw = key.width as f32 * base_unit;
        let ph = row_height;

        if x >= px && x < px + pw && y >= py && y < py + ph {
            return Some(key.idx);
        }
    }

    None
}
