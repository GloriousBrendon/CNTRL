use gtk::gdk;
use gtk::prelude::*;

#[derive(Clone, Debug)]
pub struct Key {
    pub label: &'static str,
    pub x: f64,      // Position in units (1u = 1 unit)
    pub y: f64,      // Row
    pub width: f64,  // Size in units
    pub idx: usize,  // Index in 68-key layout
}

// Ace68 keyboard layout with proper key sizes
pub fn get_keyboard_layout() -> Vec<Key> {
    vec![
        // Row 0: Function row + Escape
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

        // Row 1: Tab + Top Row
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

        // Row 2: Caps + Middle Row
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

        // Row 3: Shift + Bottom Letter Row + Arrows
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

        // Row 4: Ctrl/Win/Alt + Space + Arrows
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

pub fn draw_keyboard(
    area: &gtk::DrawingArea,
    cr: &gtk::cairo::Context,
    width: i32,
    height: i32,
    key_colors: &[(u8, u8, u8)],
    selected: Option<usize>,
) {
    let layout = get_keyboard_layout();

    // Background
    cr.set_source_rgb(0.08, 0.08, 0.10);
    let _ = cr.paint();

    let width_f = width as f64;
    let height_f = height as f64;

    // Calculate base unit size (15 units width needed for full keyboard)
    let base_unit = width_f / 15.0;
    let row_height = height_f / 5.0;

    // Draw keys
    for key in &layout {
        let px = key.x * base_unit;
        let py = key.y * row_height;
        let pw = key.width * base_unit;
        let ph = row_height;

        let (r, g, b) = key_colors
            .get(key.idx)
            .copied()
            .unwrap_or((20, 20, 24));

        // Key background
        cr.set_source_rgb(r as f64 / 255.0, g as f64 / 255.0, b as f64 / 255.0);
        cr.rectangle(px + 2.0, py + 2.0, pw - 4.0, ph - 4.0);
        let _ = cr.fill();

        // Selection highlight
        if Some(key.idx) == selected {
            cr.set_source_rgb(1.0, 1.0, 1.0);
            cr.set_line_width(2.5);
            cr.rectangle(px + 2.0, py + 2.0, pw - 4.0, ph - 4.0);
            let _ = cr.stroke();
        }

        // Key label
        cr.set_source_rgb(1.0, 1.0, 1.0);
        cr.set_font_size(10.0);
        if let Ok(ext) = cr.text_extents(key.label) {
            let label_x = px + (pw - ext.width()) / 2.0;
            let label_y = py + (ph + ext.height()) / 2.0;
            cr.move_to(label_x, label_y);
            let _ = cr.show_text(key.label);
        }
    }

    area.set_cursor_from_name(Some("pointer"));
}

pub fn key_index_at(width: i32, height: i32, x: f64, y: f64) -> Option<usize> {
    if width <= 0 || height <= 0 {
        return None;
    }

    let layout = get_keyboard_layout();
    let width_f = width as f64;
    let height_f = height as f64;
    let base_unit = width_f / 15.0;
    let row_height = height_f / 5.0;

    for key in &layout {
        let px = key.x * base_unit;
        let py = key.y * row_height;
        let pw = key.width * base_unit;
        let ph = row_height;

        if x >= px && x < px + pw && y >= py && y < py + ph {
            return Some(key.idx);
        }
    }

    None
}

pub fn rgba_from_rgb(rgb: (u8, u8, u8)) -> gdk::RGBA {
    gdk::RGBA::new(
        rgb.0 as f32 / 255.0,
        rgb.1 as f32 / 255.0,
        rgb.2 as f32 / 255.0,
        1.0,
    )
}
