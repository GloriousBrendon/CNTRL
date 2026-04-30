//! ICED shell: panel with sidebar + Lighting page, or compact popup widgets.

use cntrl_core::color::{hex_to_rgb, rgb_to_hex};
use cntrl_core::ipc;
use cntrl_core::theme;
use cntrl_core::lighting_modes::{
    self, EFFECT_ID_OFF, MODE_CUSTOM, MODE_STATIC, PRESET_MODES,
};
use cntrl_core::persistence::{
    load_profile_slot, read_startup_slot,
};
use cntrl_core::profile::{self, ProfileDocument};
use iced::widget::{
    button, column, container, row, scrollable, slider, text, Space,
};
use iced::window;
use iced::theme::Palette;
use iced::{
    Alignment, Color, Element, Length, Task, Theme,
};
use std::process::Stdio;
use std::time::{Duration, SystemTime};

pub fn run(mode: crate::UiMode) -> iced::Result {
    let (w, h) = match mode {
        crate::UiMode::Panel => (1100.0, 720.0),
        crate::UiMode::PopupKeebColor
        | crate::UiMode::PopupKeebEffects
        | crate::UiMode::PopupKeebControls
        | crate::UiMode::PopupKeebProfiles => (480.0, 720.0),
    };
    iced::application(move || boot(mode), update, view)
        .title("CNTRL")
        .theme(|app: &App| app.ui_theme.clone())
        .subscription(|_| iced::time::every(Duration::from_secs(1)).map(|_| Message::ThemePoll))
        .window(window::Settings {
            size: iced::Size::new(w, h),
            ..window::Settings::default()
        })
        .run()
}

fn boot(mode: crate::UiMode) -> (App, Task<Message>) {
    let slot = read_startup_slot();
    let profile = load_profile_slot(slot).unwrap_or_else(|_| profile::default_profile_document());
    let mut app = App::from_profile(profile, slot, mode);
    app.theme_mtime = std::fs::metadata(theme::theme_file_path())
        .ok()
        .and_then(|m| m.modified().ok());
    app.ui_theme = iced_theme_from_colors(&theme::load_theme_from_disk());
    (app, Task::none())
}

fn iced_theme_from_colors(tc: &theme::ThemeColors) -> Theme {
    Theme::custom("cntrl-pywal", palette_from_theme_colors(tc))
}

fn palette_from_theme_colors(tc: &theme::ThemeColors) -> Palette {
    let base = Palette::DARK;
    let background = color_from_hex(&tc.bg1).unwrap_or(base.background);
    let text = color_from_hex(&tc.text).unwrap_or(base.text);
    let primary = color_from_hex(&tc.accent).unwrap_or(base.primary);
    let t = 0.25;
    Palette {
        background,
        text,
        primary,
        success: lerp_color(base.success, primary, t),
        warning: lerp_color(base.warning, primary, t),
        danger: lerp_color(base.danger, primary, t),
    }
}

fn color_from_hex(hex: &str) -> Option<Color> {
    let (r, g, b) = hex_to_rgb(hex).ok()?;
    Some(Color::from_rgb8(r, g, b))
}

fn lerp_color(a: Color, b: Color, t: f32) -> Color {
    Color::from_rgba(
        a.r + (b.r - a.r) * t,
        a.g + (b.g - a.g) * t,
        a.b + (b.b - a.b) * t,
        a.a + (b.a - a.a) * t,
    )
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
enum ShellTab {
    #[default]
    Lighting,
    Profiles,
    Performance,
}

struct App {
    mode: crate::UiMode,
    /// ICED theme (pywal-driven when `colors.conf` is present).
    ui_theme: Theme,
    /// Last observed mtime of `theme_file_path()` for hot reload.
    theme_mtime: Option<SystemTime>,
    tab: ShellTab,
    effect_id: i32,
    brightness: u8,
    speed: u8,
    /// HSL for color controls (H degrees, S/L 0–1).
    hsl: (f32, f32, f32),
    current_color: (u8, u8, u8),
    profile: ProfileDocument,
    active_slot: u8,
    status: String,
}

impl App {
    fn from_profile(doc: ProfileDocument, active_slot: u8, mode: crate::UiMode) -> Self {
        let loaded_effect = doc.data.light.config.effect;
        let effect_id = if loaded_effect == 0 {
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

        let brightness = doc.data.light.config.brightness.clamp(0, 100) as u8;
        let speed = doc.data.light.config.speed.clamp(0, 5) as u8;

        let mut current_color = (216, 143, 146);
        if let Some(first) = doc.data.light.colors.first() {
            if let Ok(rgb) = hex_to_rgb(first) {
                current_color = rgb;
            }
        }
        let hsl = rgb_to_hsl_f32(current_color.0, current_color.1, current_color.2);

        Self {
            mode,
            ui_theme: Theme::Dark,
            theme_mtime: None,
            tab: ShellTab::Lighting,
            effect_id,
            brightness,
            speed,
            hsl,
            current_color,
            profile: doc,
            active_slot,
            status: String::new(),
        }
    }

    fn build_profile_document(&self) -> ProfileDocument {
        let mut doc = self.profile.clone();
        doc.data.light.config.effect = self.effect_id;
        doc.data.light.config.brightness = self.brightness as i32;
        doc.data.light.config.speed = self.speed as i32;
        doc.data.light.config.hex_color = rgb_to_hex(self.current_color);
        doc.data.light.colors = vec![rgb_to_hex(self.current_color)];
        doc
    }
}

#[derive(Debug, Clone)]
enum Message {
    ThemePoll,
    OpenPanel,
    Tab(ShellTab),
    Effect(i32),
    Hue(f32),
    Sat(f32),
    Light(f32),
    Brightness(f32),
    Speed(f32),
    Swatch((u8, u8, u8)),
    Apply,
    ApplyDone(Result<(), String>),
    Slot(u8),
}

fn update(app: &mut App, message: Message) -> Task<Message> {
    match message {
        Message::ThemePoll => {
            let path = theme::theme_file_path();
            let current = std::fs::metadata(&path)
                .ok()
                .and_then(|m| m.modified().ok());
            if current == app.theme_mtime {
                return Task::none();
            }
            app.theme_mtime = current;
            app.ui_theme = iced_theme_from_colors(&theme::load_theme_from_disk());
            Task::none()
        }
        Message::OpenPanel => {
            if let Ok(exe) = std::env::current_exe() {
                let _ = std::process::Command::new(exe)
                    .arg("panel")
                    .stdin(Stdio::null())
                    .stdout(Stdio::null())
                    .stderr(Stdio::null())
                    .spawn();
            }
            Task::none()
        }
        Message::Tab(t) => {
            app.tab = t;
            Task::none()
        }
        Message::Effect(id) => {
            app.effect_id = id;
            Task::none()
        }
        Message::Hue(h) => {
            app.hsl.0 = h;
            sync_rgb_from_hsl(app);
            Task::none()
        }
        Message::Sat(s) => {
            app.hsl.1 = s / 100.0;
            sync_rgb_from_hsl(app);
            Task::none()
        }
        Message::Light(l) => {
            app.hsl.2 = l / 100.0;
            sync_rgb_from_hsl(app);
            Task::none()
        }
        Message::Brightness(b) => {
            app.brightness = b.round().clamp(0.0, 100.0) as u8;
            Task::none()
        }
        Message::Speed(s) => {
            app.speed = s.round().clamp(0.0, 5.0) as u8;
            Task::none()
        }
        Message::Swatch(rgb) => {
            app.current_color = rgb;
            app.hsl = rgb_to_hsl_f32(rgb.0, rgb.1, rgb.2);
            Task::none()
        }
        Message::Slot(slot) if slot <= 3 => {
            app.active_slot = slot;
            let m = app.mode;
            match load_profile_slot(slot) {
                Ok(doc) => {
                    *app = App::from_profile(doc, slot, m);
                    app.status = format!("Loaded profile slot {slot}");
                }
                Err(e) => app.status = format!("Load failed: {e}"),
            }
            Task::none()
        }
        Message::Slot(_) => Task::none(),
        Message::Apply => {
            let doc = app.build_profile_document();
            app.profile = doc.clone();
            let slot = app.active_slot;
            Task::perform(
                async move {
                    tokio::task::spawn_blocking(move || {
                        let eff = doc.data.light.config.effect;
                        eprintln!("cntrl-iced: apply profile.data.light.config.effect={eff}");
                        if lighting_modes::is_preset_effect_id(eff) {
                            eprintln!("cntrl-iced: HID uses firmware effect_id={eff} (set_effect)");
                        } else if eff == MODE_STATIC || eff == MODE_CUSTOM {
                            eprintln!(
                                "cntrl-iced: HID solid DIY (set_solid_rgb); static/custom are not firmware id 3"
                            );
                        } else if eff == EFFECT_ID_OFF {
                            eprintln!("cntrl-iced: HID off (solid black)");
                        }
                        ipc::apply_local_or_daemon(&doc, slot).map_err(|e| e.to_string())
                    })
                    .await
                    .unwrap_or_else(|e| Err(format!("Join: {e}")))
                },
                Message::ApplyDone,
            )
        }
        Message::ApplyDone(result) => {
            app.status = match result {
                Ok(()) => "Lighting applied".into(),
                Err(e) => e,
            };
            Task::none()
        }
    }
}

fn sync_rgb_from_hsl(app: &mut App) {
    let (r, g, b) = hsl_to_rgb(app.hsl.0, app.hsl.1, app.hsl.2);
    app.current_color = (r, g, b);
}

fn rgb_to_hsl_f32(r: u8, g: u8, b: u8) -> (f32, f32, f32) {
    let (h, s, l) = rgb_to_hsl_f64(r as f64, g as f64, b as f64);
    (h as f32, s as f32, l as f32)
}

fn rgb_to_hsl_f64(r: f64, g: f64, b: f64) -> (f64, f64, f64) {
    let r = r / 255.0;
    let g = g / 255.0;
    let b = b / 255.0;
    let max = r.max(g).max(b);
    let min = r.min(g).min(b);
    let l = (max + min) / 2.0;
    if (max - min).abs() < f64::EPSILON {
        return (0.0, 0.0, l);
    }
    let h = if (max - r).abs() < f64::EPSILON {
        (60.0 * ((g - b) / (max - min)) + 360.0) % 360.0
    } else if (max - g).abs() < f64::EPSILON {
        (60.0 * ((b - r) / (max - min)) + 120.0) % 360.0
    } else {
        (60.0 * ((r - g) / (max - min)) + 240.0) % 360.0
    };
    let s = (max - min) / (1.0 - (2.0 * l - 1.0).abs());
    (h, s.clamp(0.0, 1.0), l.clamp(0.0, 1.0))
}

fn hsl_to_rgb(h: f32, s: f32, l: f32) -> (u8, u8, u8) {
    let h = h as f64;
    let s = s as f64;
    let l = l as f64;
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
    let r = ((r1 + m) * 255.0).round() as u8;
    let g = ((g1 + m) * 255.0).round() as u8;
    let b = ((b1 + m) * 255.0).round() as u8;
    (r, g, b)
}

fn view(app: &App) -> Element<'_, Message> {
    match app.mode {
        crate::UiMode::Panel => panel_view(app),
        crate::UiMode::PopupKeebColor => popup_color_view(app),
        crate::UiMode::PopupKeebEffects => popup_effects_view(app),
        crate::UiMode::PopupKeebControls => popup_controls_view(app),
        crate::UiMode::PopupKeebProfiles => popup_profiles_view(app),
    }
}

fn panel_view(app: &App) -> Element<'_, Message> {
    let sidebar = column![
        tab_btn("Lighting", ShellTab::Lighting, app.tab),
        tab_btn("Profiles", ShellTab::Profiles, app.tab),
        tab_btn("Performance", ShellTab::Performance, app.tab),
        Space::new().height(16),
        text("Profile slot").size(12),
        row![
            slot_button(0, app.active_slot),
            slot_button(1, app.active_slot),
            slot_button(2, app.active_slot),
            slot_button(3, app.active_slot),
        ]
        .spacing(6),
    ]
    .spacing(8)
    .width(160);

    let main: Element<'_, Message> = match app.tab {
        ShellTab::Lighting => lighting_view(app),
        ShellTab::Profiles => placeholder("Profiles — use GTK or a later milestone."),
        ShellTab::Performance => placeholder("Performance — use GTK or a later milestone."),
    };

    let content = row![sidebar, Space::new().width(16), main].spacing(0);

    container(content)
        .padding(16)
        .width(Length::Fill)
        .height(Length::Fill)
        .into()
}

fn open_panel_button() -> Element<'static, Message> {
    button(text("Open full app →"))
        .on_press(Message::OpenPanel)
        .style(button::secondary)
        .width(Length::Fill)
        .into()
}

fn popup_color_view(app: &App) -> Element<'_, Message> {
    let hex = rgb_to_hex(app.current_color);
    column![
        text("Color").size(16),
        text("Hue").size(12),
        slider(0.0..=360.0, app.hsl.0, Message::Hue),
        text("Saturation").size(12),
        slider(0.0..=100.0, app.hsl.1 * 100.0, Message::Sat),
        text("Lightness").size(12),
        slider(0.0..=100.0, app.hsl.2 * 100.0, Message::Light),
        swatches(),
        text(format!("Current {hex}")).font(iced::Font::MONOSPACE),
        row![
            button(text("Apply")).on_press(Message::Apply).padding([10, 16]),
            text(&app.status).size(14),
        ]
        .spacing(12)
        .align_y(Alignment::Center),
        open_panel_button(),
    ]
    .spacing(10)
    .padding(16)
    .width(Length::Fill)
    .into()
}

fn popup_effects_view(app: &App) -> Element<'_, Message> {
    let special = row![
        effect_btn("Static", MODE_STATIC),
        effect_btn("Custom", MODE_CUSTOM),
        effect_btn("Off", EFFECT_ID_OFF),
    ]
    .spacing(6);

    let presets: Element<'_, Message> = {
        let mut col = column![].spacing(4);
        for m in PRESET_MODES {
            col = col.push(effect_btn(m.label, i32::from(m.effect_id)));
        }
        scrollable(col).height(Length::Fixed(360.0)).into()
    };

    column![
        text("Effects").size(16),
        special,
        presets,
        row![
            button(text("Apply")).on_press(Message::Apply).padding([10, 16]),
            text(&app.status).size(14),
        ]
        .spacing(12)
        .align_y(Alignment::Center),
        open_panel_button(),
    ]
    .spacing(10)
    .padding(16)
    .width(Length::Fill)
    .into()
}

fn popup_controls_view(app: &App) -> Element<'_, Message> {
    column![
        text("Controls").size(16),
        text("Brightness").size(12),
        slider(0.0..=100.0, app.brightness as f32, Message::Brightness),
        text("Speed").size(12),
        slider(0.0..=5.0, app.speed as f32, Message::Speed),
        row![
            button(text("Apply")).on_press(Message::Apply).padding([10, 16]),
            text(&app.status).size(14),
        ]
        .spacing(12)
        .align_y(Alignment::Center),
        open_panel_button(),
    ]
    .spacing(10)
    .padding(16)
    .width(Length::Fill)
    .into()
}

fn popup_profiles_view(app: &App) -> Element<'_, Message> {
    column![
        text("Profile").size(16),
        row![
            slot_button(0, app.active_slot),
            slot_button(1, app.active_slot),
            slot_button(2, app.active_slot),
            slot_button(3, app.active_slot),
        ]
        .spacing(6),
        text(&app.status).size(14),
        open_panel_button(),
    ]
    .spacing(10)
    .padding(16)
    .width(Length::Fill)
    .into()
}

fn slot_button(slot: u8, active: u8) -> Element<'static, Message> {
    let label = format!("{}", slot + 1);
    let on = active == slot;
    button(text(label))
        .on_press(Message::Slot(slot))
        .style(if on {
            button::primary
        } else {
            button::secondary
        })
        .into()
}

fn tab_btn(
    label: &'static str,
    tab: ShellTab,
    current: ShellTab,
) -> Element<'static, Message> {
    let active = tab == current;
    button(text(label))
        .on_press(Message::Tab(tab))
        .width(Length::Fill)
        .style(if active {
            button::primary
        } else {
            button::secondary
        })
        .into()
}

fn placeholder(msg: &'static str) -> Element<'static, Message> {
    container(text(msg).size(16))
        .center_x(Length::Fill)
        .center_y(Length::Fill)
        .into()
}

fn lighting_view(app: &App) -> Element<'_, Message> {
    let special = row![
        effect_btn("Static", MODE_STATIC),
        effect_btn("Custom", MODE_CUSTOM),
        effect_btn("Off", EFFECT_ID_OFF),
    ]
    .spacing(6);

    let presets: Element<'_, Message> = {
        let mut col = column![].spacing(4);
        for m in PRESET_MODES {
            col = col.push(effect_btn(m.label, i32::from(m.effect_id)));
        }
        scrollable(col).height(Length::Fixed(320.0)).into()
    };

    let effects = column![text("Effects").size(14), special, presets].spacing(8);

    let hex = rgb_to_hex(app.current_color);
    let controls = column![
        text("Controls").size(14),
        text(format!("Current color {hex}")).font(iced::Font::MONOSPACE),
        text("Brightness").size(12),
        slider(0.0..=100.0, app.brightness as f32, Message::Brightness),
        text("Speed").size(12),
        slider(0.0..=5.0, app.speed as f32, Message::Speed),
    ]
    .spacing(6);

    let color_col = column![
        text("Color (HSL)").size(14),
        text("Hue").size(12),
        slider(0.0..=360.0, app.hsl.0, Message::Hue),
        text("Saturation").size(12),
        slider(0.0..=100.0, app.hsl.1 * 100.0, Message::Sat),
        text("Lightness").size(12),
        slider(0.0..=100.0, app.hsl.2 * 100.0, Message::Light),
        swatches(),
    ]
    .spacing(6);

    let apply_row = row![
        button(text("Apply To Keyboard"))
            .on_press(Message::Apply)
            .padding([12, 24]),
        text(&app.status).size(14),
    ]
    .spacing(16)
    .align_y(Alignment::Center);

    column![
        row![
            container(effects).padding(12).width(Length::FillPortion(2)),
            container(controls).padding(12).width(Length::FillPortion(1)),
            container(color_col).padding(12).width(Length::FillPortion(1)),
        ]
        .spacing(12),
        apply_row,
    ]
    .spacing(8)
    .into()
}

fn effect_btn(lbl: &'static str, id: i32) -> Element<'static, Message> {
    button(text(lbl))
        .on_press(Message::Effect(id))
        .width(Length::Fill)
        .into()
}

fn swatches() -> Element<'static, Message> {
    let colors: [(u8, u8, u8); 12] = [
        (255, 0, 0),
        (255, 127, 0),
        (255, 255, 0),
        (0, 255, 0),
        (0, 127, 255),
        (0, 0, 255),
        (127, 0, 255),
        (255, 0, 127),
        (255, 255, 255),
        (128, 128, 128),
        (192, 0, 0),
        (0, 0, 0),
    ];
    let mut r = row![].spacing(6);
    for c in colors {
        r = r.push(
            button(Space::new().width(28).height(28))
                .on_press(Message::Swatch(c))
                .style(move |t, st| {
                    let mut s = button::secondary(t, st);
                    s.background = Some(iced::Background::Color(Color::from_rgb8(c.0, c.1, c.2)));
                    s
                }),
        );
    }
    r.into()
}
