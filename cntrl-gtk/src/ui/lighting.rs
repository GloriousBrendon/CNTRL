use cntrl_core::ipc;
use cntrl_core::lighting_modes::{self, EFFECT_ID_OFF, MODE_CUSTOM, MODE_STATIC, PRESET_MODES};
use cntrl_core::profile;
use crate::ui::keyboard;
use crate::AppShared;
use adw::prelude::*;
use std::cell::RefCell;
use std::rc::Rc;

#[derive(Clone, Copy)]
struct HSL {
    h: f64, // 0-360
    s: f64, // 0-1
    l: f64, // 0-1
}

#[derive(Clone)]
struct LightingState {
    effect_id: i32,
    brightness: u8,
    speed: u8,
    current_color: (u8, u8, u8),
    current_hsl: HSL,
    selected_key: Option<usize>,
    key_colors: Vec<(u8, u8, u8)>,
}

fn hsv_to_rgb(h: f64, s: f64, v: f64) -> (u8, u8, u8) {
    let c = v * s;
    let x = c * (1.0 - (((h / 60.0) % 2.0) - 1.0).abs());
    let m = v - c;
    let (r1, g1, b1) = if h < 60.0 {
        (c, x, 0.0)
    } else if h < 120.0 {
        (x, c, 0.0)
    } else if h < 180.0 {
        (0.0, c, x)
    } else if h < 240.0 {
        (0.0, x, c)
    } else if h < 300.0 {
        (x, 0.0, c)
    } else {
        (c, 0.0, x)
    };
    (
        ((r1 + m) * 255.0).round() as u8,
        ((g1 + m) * 255.0).round() as u8,
        ((b1 + m) * 255.0).round() as u8,
    )
}

fn hsl_to_rgb(h: f64, s: f64, l: f64) -> (u8, u8, u8) {
    let c = (1.0 - (2.0 * l - 1.0).abs()) * s;
    let x = c * (1.0 - (((h / 60.0) % 2.0) - 1.0).abs());
    let m = l - c / 2.0;
    let (r1, g1, b1) = if h < 60.0 {
        (c, x, 0.0)
    } else if h < 120.0 {
        (x, c, 0.0)
    } else if h < 180.0 {
        (0.0, c, x)
    } else if h < 240.0 {
        (0.0, x, c)
    } else if h < 300.0 {
        (x, 0.0, c)
    } else {
        (c, 0.0, x)
    };
    (
        ((r1 + m) * 255.0).round() as u8,
        ((g1 + m) * 255.0).round() as u8,
        ((b1 + m) * 255.0).round() as u8,
    )
}

fn rgb_to_hsl(r: u8, g: u8, b: u8) -> HSL {
    let r = r as f64 / 255.0;
    let g = g as f64 / 255.0;
    let b = b as f64 / 255.0;

    let max = r.max(g).max(b);
    let min = r.min(g).min(b);
    let l = (max + min) / 2.0;

    let h = if (max - min).abs() < 0.0001 {
        0.0
    } else if (max - r).abs() < 0.0001 {
        (60.0 * ((g - b) / (max - min)) + 360.0) % 360.0
    } else if (max - g).abs() < 0.0001 {
        (60.0 * ((b - r) / (max - min)) + 120.0) % 360.0
    } else {
        (60.0 * ((r - g) / (max - min)) + 240.0) % 360.0
    };

    let s = if (max - min).abs() < 0.0001 {
        0.0
    } else {
        (max - min) / (1.0 - (2.0 * l - 1.0).abs())
    };

    HSL {
        h: h.clamp(0.0, 360.0),
        s: s.clamp(0.0, 1.0),
        l: l.clamp(0.0, 1.0),
    }
}

fn rgb_hex(rgb: (u8, u8, u8)) -> String {
    format!("#{:02x}{:02x}{:02x}", rgb.0, rgb.1, rgb.2)
}

const PICKER_WIDTH: i32 = 320;
const PICKER_HEIGHT: i32 = 200;
const HUE_BAR_WIDTH: i32 = 20;

fn draw_hsl_picker(cr: &gtk::cairo::Context, current_hue: f64) {
    let width = PICKER_WIDTH as f64;
    let height = PICKER_HEIGHT as f64;
    let hue_bar_w = HUE_BAR_WIDTH as f64;

    // Main HSL square: Saturation (X-axis) vs Lightness (Y-axis), with current hue
    for y in 0..PICKER_HEIGHT {
        for x in 0..(PICKER_WIDTH - HUE_BAR_WIDTH) {
            let s = x as f64 / (width - hue_bar_w);
            let l = 1.0 - (y as f64 / height);
            let color = hsl_to_rgb(current_hue, s, l);
            cr.set_source_rgb(
                color.0 as f64 / 255.0,
                color.1 as f64 / 255.0,
                color.2 as f64 / 255.0,
            );
            cr.rectangle(x as f64, y as f64, 1.0, 1.0);
            let _ = cr.fill();
        }
    }

    // Hue bar on the right
    for y in 0..PICKER_HEIGHT {
        let h = (y as f64 / height) * 360.0;
        let color = hsl_to_rgb(h, 1.0, 0.5);
        cr.set_source_rgb(
            color.0 as f64 / 255.0,
            color.1 as f64 / 255.0,
            color.2 as f64 / 255.0,
        );
        cr.rectangle(width - hue_bar_w, y as f64, hue_bar_w, 1.0);
        let _ = cr.fill();
    }

    // Border
    cr.set_source_rgb(0.3, 0.3, 0.3);
    cr.set_line_width(1.0);
    cr.rectangle(0.0, 0.0, width - hue_bar_w - 1.0, height - 1.0);
    let _ = cr.stroke();
    cr.rectangle(width - hue_bar_w - 1.0, 0.0, hue_bar_w - 1.0, height - 1.0);
    let _ = cr.stroke();
}

fn draw_selection_indicator(
    cr: &gtk::cairo::Context,
    s: f64,
    l: f64,
    h: f64,
) {
    let width = PICKER_WIDTH as f64;
    let height = PICKER_HEIGHT as f64;
    let hue_bar_w = HUE_BAR_WIDTH as f64;

    // Circle indicator on the square
    let circle_x = s * (width - hue_bar_w);
    let circle_y = (1.0 - l) * height;
    cr.set_source_rgb(1.0, 1.0, 1.0);
    cr.set_line_width(2.0);
    cr.arc(circle_x, circle_y, 6.0, 0.0, std::f64::consts::PI * 2.0);
    let _ = cr.stroke();

    // Marker on hue bar
    let hue_y = (h / 360.0) * height;
    cr.rectangle(width - hue_bar_w - 2.0, hue_y - 2.0, hue_bar_w + 4.0, 4.0);
    let _ = cr.stroke();
}

pub fn page(shared: AppShared, toasts: adw::ToastOverlay) -> gtk::Box {
    let root = gtk::Box::new(gtk::Orientation::Vertical, 16);
    root.set_margin_top(16);
    root.set_margin_bottom(16);
    root.set_margin_start(16);
    root.set_margin_end(16);
    root.add_css_class("lighting-page");

    let initial_hsl = rgb_to_hsl(216, 143, 146);

    let state = Rc::new(RefCell::new(LightingState {
        effect_id: MODE_CUSTOM,
        brightness: 100,
        speed: 2,
        current_color: (216, 143, 146),
        current_hsl: initial_hsl,
        selected_key: None,
        key_colors: vec![(20, 20, 24); 68],
    }));

    // ===== KEYBOARD VISUALIZER (TOP, FULL WIDTH) =====
    let keyboard_group = gtk::Box::new(gtk::Orientation::Vertical, 8);
    keyboard_group.add_css_class("card");
    
    let keyboard_label = gtk::Label::new(Some("Keyboard"));
    keyboard_label.add_css_class("card-title");
    keyboard_label.set_halign(gtk::Align::Start);
    keyboard_group.append(&keyboard_label);

    let keyboard_area = gtk::DrawingArea::new();
    keyboard_area.set_content_width(860);
    keyboard_area.set_content_height(180);
    {
        let state = state.clone();
        keyboard_area.set_draw_func(move |area, cr, width, height| {
            let st = state.borrow();
            keyboard::draw_keyboard(area, cr, width, height, &st.key_colors, st.selected_key);
        });
    }

    let key_click = gtk::GestureClick::new();
    {
        let state = state.clone();
        let area = keyboard_area.clone();
        key_click.connect_pressed(move |_, _, x, y| {
            let width = area.width();
            let height = area.height();
            if let Some(idx) = keyboard::key_index_at(width, height, x, y) {
                let mut st = state.borrow_mut();
                st.selected_key = Some(idx);
                let c = st.current_color;
                if let Some(slot) = st.key_colors.get_mut(idx) {
                    *slot = c;
                }
            }
            area.queue_draw();
        });
    }
    keyboard_area.add_controller(key_click);
    keyboard_group.append(&keyboard_area);

    root.append(&keyboard_group);

    // ===== THREE-COLUMN GRID =====
    let grid = gtk::Box::new(gtk::Orientation::Horizontal, 16);
    grid.set_homogeneous(true);

    // ------ LEFT COLUMN: EFFECTS (PILL GRID) ------
    let effects_card = gtk::Box::new(gtk::Orientation::Vertical, 12);
    effects_card.add_css_class("card");
    
    let effects_title = gtk::Label::new(Some("Effects"));
    effects_title.add_css_class("card-title");
    effects_title.set_halign(gtk::Align::Start);
    effects_card.append(&effects_title);

    let effects_scroll = gtk::ScrolledWindow::new();
    effects_scroll.set_vexpand(true);
    effects_scroll.set_hexpand(true);

    let effects_grid = gtk::FlowBox::new();
    effects_grid.set_max_children_per_line(2);
    effects_grid.set_selection_mode(gtk::SelectionMode::Single);
    effects_grid.set_homogeneous(true);
    effects_grid.add_css_class("effects-grid");

    // Single source of truth: hub mapping in lighting_modes::PRESET_MODES.
    let state_for_static = state.clone();
    let keyboard_for_static = keyboard_area.clone();
    create_effect_pill(
        &effects_grid,
        "Static",
        MODE_STATIC,
        &state_for_static,
        &keyboard_for_static,
    );

    let state_for_custom = state.clone();
    let keyboard_for_custom = keyboard_area.clone();
    create_effect_pill(
        &effects_grid,
        "Custom",
        MODE_CUSTOM,
        &state_for_custom,
        &keyboard_for_custom,
    );

    let state_for_off = state.clone();
    let keyboard_for_off = keyboard_area.clone();
    create_effect_pill(
        &effects_grid,
        "Off",
        EFFECT_ID_OFF,
        &state_for_off,
        &keyboard_for_off,
    );

    for mode in PRESET_MODES {
        let state_for_effect = state.clone();
        let keyboard_for_effect = keyboard_area.clone();
        create_effect_pill(
            &effects_grid,
            mode.label,
            i32::from(mode.effect_id),
            &state_for_effect,
            &keyboard_for_effect,
        );
    }

    effects_scroll.set_child(Some(&effects_grid));
    effects_card.append(&effects_scroll);
    grid.append(&effects_card);

    // ------ CENTER COLUMN: CONTROLS ------
    let controls_card = gtk::Box::new(gtk::Orientation::Vertical, 16);
    controls_card.add_css_class("card");

    let controls_title = gtk::Label::new(Some("Controls"));
    controls_title.add_css_class("card-title");
    controls_title.set_halign(gtk::Align::Start);
    controls_card.append(&controls_title);

    // Brightness
    let brightness_box = gtk::Box::new(gtk::Orientation::Vertical, 8);
    let brightness_label = gtk::Label::new(Some("Brightness"));
    brightness_label.add_css_class("control-label");
    brightness_label.set_halign(gtk::Align::Start);
    let brightness = gtk::Scale::with_range(gtk::Orientation::Horizontal, 0.0, 100.0, 1.0);
    brightness.set_value(100.0);
    brightness.set_draw_value(false);
    brightness.add_css_class("slider");
    brightness_box.append(&brightness_label);
    brightness_box.append(&brightness);

    // Speed
    let speed_box = gtk::Box::new(gtk::Orientation::Vertical, 8);
    let speed_label = gtk::Label::new(Some("Speed"));
    speed_label.add_css_class("control-label");
    speed_label.set_halign(gtk::Align::Start);
    let speed = gtk::Scale::with_range(gtk::Orientation::Horizontal, 0.0, 5.0, 1.0);
    speed.set_value(2.0);
    speed.set_draw_value(false);
    speed.add_css_class("slider");
    speed_box.append(&speed_label);
    speed_box.append(&speed);

    // Color display
    let color_display_box = gtk::Box::new(gtk::Orientation::Vertical, 8);
    let color_display_label = gtk::Label::new(Some("Current Color"));
    color_display_label.add_css_class("control-label");
    color_display_label.set_halign(gtk::Align::Start);
    let color_label = gtk::Label::new(Some("#d88f92"));
    color_label.add_css_class("color-hex");
    color_label.set_selectable(true);
    color_display_box.append(&color_display_label);
    color_display_box.append(&color_label);

    controls_card.append(&brightness_box);
    controls_card.append(&speed_box);
    controls_card.append(&color_display_box);
    grid.append(&controls_card);

    // ------ RIGHT COLUMN: COLOR PICKER + SWATCHES ------
    let picker_card = gtk::Box::new(gtk::Orientation::Vertical, 12);
    picker_card.add_css_class("card");

    let picker_title = gtk::Label::new(Some("Color Picker"));
    picker_title.add_css_class("card-title");
    picker_title.set_halign(gtk::Align::Start);
    picker_card.append(&picker_title);

    let picker = gtk::DrawingArea::new();
    picker.set_content_width(PICKER_WIDTH);
    picker.set_content_height(PICKER_HEIGHT);
    picker.add_css_class("picker");
    {
        let state = state.clone();
        picker.set_draw_func(move |_, cr, _, _| {
            let st = state.borrow();
            draw_hsl_picker(cr, st.current_hsl.h);
            draw_selection_indicator(cr, st.current_hsl.s, st.current_hsl.l, st.current_hsl.h);
        });
    }

    let picker_click = gtk::GestureClick::new();
    {
        let state = state.clone();
        let picker_clone = picker.clone();
        let keyboard_area_clone = keyboard_area.clone();
        let color_label = color_label.clone();
        picker_click.connect_pressed(move |_, _n_press, x, y| {
            handle_picker_interaction(&state, &picker_clone, &color_label, x, y);
            picker_clone.queue_draw();
            keyboard_area_clone.queue_draw();
        });
    }
    picker.add_controller(picker_click);

    let picker_drag = gtk::GestureDrag::new();
    let drag_origin = Rc::new(RefCell::new((0.0, 0.0)));
    {
        let drag_origin = drag_origin.clone();
        picker_drag.connect_drag_begin(move |_, x, y| {
            *drag_origin.borrow_mut() = (x, y);
        });
    }
    {
        let state = state.clone();
        let picker_clone = picker.clone();
        let keyboard_area_clone = keyboard_area.clone();
        let color_label = color_label.clone();
        let drag_origin = drag_origin.clone();
        picker_drag.connect_drag_update(move |_, dx, dy| {
            let (start_x, start_y) = *drag_origin.borrow();
            let x = start_x + dx;
            let y = start_y + dy;
            handle_picker_interaction(&state, &picker_clone, &color_label, x, y);
            picker_clone.queue_draw();
            keyboard_area_clone.queue_draw();
        });
    }
    picker.add_controller(picker_drag);

    picker_card.append(&picker);

    // Preset color swatches
    let swatches_label = gtk::Label::new(Some("Presets"));
    swatches_label.add_css_class("swatch-label");
    swatches_label.set_halign(gtk::Align::Start);
    picker_card.append(&swatches_label);

    let swatches = gtk::FlowBox::new();
    swatches.set_max_children_per_line(6);
    swatches.set_selection_mode(gtk::SelectionMode::None);
    swatches.add_css_class("swatches");

    // Standard web colors: Red, Orange, Yellow, Green, Cyan, Blue, Purple, Magenta, White, Gray, Dark Red, Black
    let preset_colors = vec![
        (255, 0, 0), (255, 127, 0), (255, 255, 0), (0, 255, 0),
        (0, 127, 255), (0, 0, 255), (127, 0, 255), (255, 0, 127),
        (255, 255, 255), (128, 128, 128), (192, 0, 0), (0, 0, 0),
    ];

    for (r, g, b) in preset_colors {
        let swatch = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        swatch.set_size_request(32, 32);
        swatch.add_css_class("swatch");
        swatch.set_margin_top(4);
        swatch.set_margin_bottom(4);
        swatch.set_margin_start(4);
        swatch.set_margin_end(4);

        let css_provider = gtk::CssProvider::new();
        css_provider.load_from_string(&format!(
            ".swatch {{ background-color: rgb({},{},{}); border-radius: 6px; border: 1px solid rgba(0,0,0,0.2); transition: transform 150ms ease-out; }}", 
            r, g, b
        ));
        swatch.style_context().add_provider(&css_provider, gtk::STYLE_PROVIDER_PRIORITY_APPLICATION);

        let swatch_click = gtk::GestureClick::new();
        {
            let state = state.clone();
            let color_label = color_label.clone();
            let picker_clone = picker.clone();
            let keyboard_area_clone = keyboard_area.clone();
            swatch_click.connect_pressed(move |_, _, _, _| {
                let mut st = state.borrow_mut();
                st.current_color = (r, g, b);
                st.current_hsl = rgb_to_hsl(r, g, b);
                color_label.set_text(&format!("#{:02x}{:02x}{:02x}", r, g, b));
                drop(st);
                picker_clone.queue_draw();
                keyboard_area_clone.queue_draw();
            });
        }
        swatch.add_controller(swatch_click);
        swatches.append(&swatch);
    }

    picker_card.append(&swatches);
    grid.append(&picker_card);

    root.append(&grid);

    // ===== APPLY BUTTON (BOTTOM) =====
    let apply = gtk::Button::with_label("Apply To Keyboard");
    apply.add_css_class("suggested-action");
    apply.add_css_class("apply-button");
    apply.set_size_request(-1, 48);
    {
        let state = state.clone();
        let shared = shared.clone();
        let toasts = toasts.clone();
        apply.connect_clicked(move |_| {
            let snapshot = state.borrow().clone();
            let mut maybe_profile = shared.profile.lock().ok().map(|p| p.clone());
            if let Some(mut doc) = maybe_profile.take() {
                doc.data.light.config.effect = snapshot.effect_id;
                doc.data.light.config.brightness = snapshot.brightness as i32;
                doc.data.light.config.speed = snapshot.speed as i32;
                doc.data.light.config.hex_color = rgb_hex(snapshot.current_color);
                doc.data.light.colors = snapshot.key_colors.iter().map(|c| rgb_hex(*c)).collect();

                if let Ok(mut p) = shared.profile.lock() {
                    *p = doc.clone();
                }

                let slot = shared.active_slot.lock().map(|g| *g).unwrap_or(0);
                let msg = match ipc::apply_local_or_daemon(&doc, slot) {
                    Ok(()) => "Lighting applied".to_string(),
                    Err(e) => {
                        eprintln!("apply error: {e}");
                        format!("Apply failed: {e}")
                    }
                };
                toasts.add_toast(adw::Toast::new(&msg));
            }
        });
    }
    root.append(&apply);

    // Wire up slider changes
    {
        let state = state.clone();
        brightness.connect_value_changed(move |s| {
            state.borrow_mut().brightness = s.value().round() as u8;
        });
    }

    {
        let state = state.clone();
        speed.connect_value_changed(move |s| {
            state.borrow_mut().speed = s.value().round() as u8;
        });
    }

    // Load initial profile state
    if let Ok(doc) = shared.profile.lock().map(|p| p.clone()) {
        let mut st = state.borrow_mut();
        let loaded_effect = doc.data.light.config.effect;
        st.effect_id = if loaded_effect == 0 {
            // Backward compatibility: old files used 0 for custom/DIY.
            MODE_CUSTOM
        } else if loaded_effect == EFFECT_ID_OFF
            || loaded_effect == MODE_STATIC
            || loaded_effect == MODE_CUSTOM
            || lighting_modes::is_preset_effect_id(loaded_effect)
        {
            loaded_effect
        } else {
            MODE_CUSTOM
        };
        st.brightness = doc.data.light.config.brightness.clamp(0, 100) as u8;
        st.speed = doc.data.light.config.speed.clamp(0, 5) as u8;
        if let Some(first) = doc.data.light.colors.first() {
            if let Ok((r, g, b)) = crate::theme::hex_to_rgb(first) {
                st.current_color = (r, g, b);
                st.current_hsl = rgb_to_hsl(r, g, b);
                color_label.set_text(&format!("#{:02x}{:02x}{:02x}", r, g, b));
            }
        }
        st.key_colors = doc
            .data
            .light
            .colors
            .iter()
            .take(68)
            .map(|hex| crate::theme::hex_to_rgb(hex).unwrap_or((20, 20, 24)))
            .collect();
        if st.key_colors.len() < 68 {
            st.key_colors.resize(68, (20, 20, 24));
        }
    }

    let _ = profile::code_to_color_index;

    root
}

fn create_effect_pill(
    container: &gtk::FlowBox,
    label: &str,
    effect_id: i32,
    state: &Rc<RefCell<LightingState>>,
    keyboard: &gtk::DrawingArea,
) {
    let pill = gtk::Button::with_label(label);
    pill.add_css_class("effect-pill");
    
    let state = state.clone();
    let keyboard = keyboard.clone();
    pill.connect_clicked(move |_| {
        state.borrow_mut().effect_id = effect_id;
        keyboard.queue_draw();
    });

    container.append(&pill);
}

fn handle_picker_interaction(
    state: &Rc<RefCell<LightingState>>,
    picker: &gtk::DrawingArea,
    color_label: &gtk::Label,
    x: f64,
    y: f64,
) {
    // Get actual allocated size of the picker at event time
    let width = picker.width().max(1) as f64;
    let height = picker.height().max(1) as f64;
    let hue_bar_w = (HUE_BAR_WIDTH as f64).min(width * 0.1);

    // Clamp coordinates to picker bounds
    let x_clamped = x.clamp(0.0, width);
    let y_clamped = y.clamp(0.0, height);

    let mut st = state.borrow_mut();

    // Check if click is on hue bar (right side)
    if x_clamped >= width - hue_bar_w {
        let h = (y_clamped / height) * 360.0;
        st.current_hsl.h = h;
    } else {
        // Click on main square: update saturation and lightness
        let s = x_clamped / (width - hue_bar_w);
        let l = 1.0 - (y_clamped / height);
        st.current_hsl.s = s;
        st.current_hsl.l = l;
    }

    // Convert HSL to RGB
    let (r, g, b) = hsl_to_rgb(st.current_hsl.h, st.current_hsl.s, st.current_hsl.l);
    st.current_color = (r, g, b);

    // Update display
    color_label.set_text(&format!("#{:02x}{:02x}{:02x}", r, g, b));

    // If a key is selected, update its color too
    if let Some(idx) = st.selected_key {
        if let Some(slot) = st.key_colors.get_mut(idx) {
            *slot = (r, g, b);
        }
    }
}
