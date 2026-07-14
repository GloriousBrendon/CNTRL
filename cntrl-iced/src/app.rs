//! ICED shell: full desktop panel with sidebar + Lighting / Canvas / Devices / Sync pages.

use crate::canvas_tab;
use cntrl_core::canvas::{self as core_canvas, CanvasLayout};
use cntrl_core::canvas_effects::EffectKind;
use cntrl_core::canvas_stream::StreamHandle;
use cntrl_core::color::{hex_to_rgb, rgb_to_hex};
use cntrl_core::theme;
use cntrl_core::lighting_modes::{
    self, EFFECT_ID_OFF, MODE_CUSTOM, MODE_STATIC, PRESET_MODES,
};
use cntrl_core::persistence::{
    load_profile_slot, read_startup_slot,
};
use cntrl_core::profile::{self, ProfileDocument};
use iced::widget::{
    button, checkbox, column, container, row, scrollable, slider, text, text_input, Space,
};
use iced::window;
use iced::theme::Palette;
use iced::{
    Alignment, Color, Element, Font, Length, Task, Theme,
};
use std::time::{Duration, SystemTime};

/// Bundled Nerd Font for device-type icons (Material Design Icons glyph range).
const NERD_FONT_BYTES: &[u8] =
    include_bytes!("/usr/share/fonts/TTF/MesloLGSNerdFontMono-Regular.ttf");
const ICON_FONT: Font = Font::with_name("MesloLGS Nerd Font Mono");

fn icon<'a>(glyph: &'a str, size: u16, color: Color) -> Element<'a, Message> {
    text(glyph).font(ICON_FONT).size(f32::from(size)).color(color).into()
}

pub fn run() -> iced::Result {
    iced::application(boot, update, view)
        .title("CNTRL")
        .font(NERD_FONT_BYTES)
        .theme(|app: &App| app.ui_theme.clone())
        .subscription(|app: &App| {
            let theme_poll =
                iced::time::every(Duration::from_secs(1)).map(|_| Message::ThemePoll);
            if app.tab == ShellTab::Canvas {
                iced::Subscription::batch([
                    theme_poll,
                    iced::time::every(Duration::from_millis(100)).map(|_| Message::CanvasTick),
                ])
            } else {
                theme_poll
            }
        })
        .window(window::Settings {
            size: iced::Size::new(1100.0, 720.0),
            ..window::Settings::default()
        })
        .run()
}

fn boot() -> (App, Task<Message>) {
    if let Err(e) = cntrl_core::rgb_probe::ensure_runtime_probe(false) {
        eprintln!("cntrl: rgb probe init failed on app boot: {e}");
    }
    let slot = read_startup_slot();
    let profile = load_profile_slot(slot).unwrap_or_else(|_| profile::default_profile_document());
    let mut app = App::from_profile(profile, slot);
    app.theme_mtime = std::fs::metadata(theme::theme_file_path())
        .ok()
        .and_then(|m| m.modified().ok());
    app.colors = theme::load_theme_from_disk();
    app.ui_theme = iced_theme_from_colors(&app.colors);
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

pub(crate) fn hexc(s: &str, fallback: Color) -> Color {
    color_from_hex(s).unwrap_or(fallback)
}

/// Pick black/white text for good contrast against `c`.
fn contrast_on(c: Color) -> Color {
    let lum = 0.299 * c.r + 0.587 * c.g + 0.114 * c.b;
    if lum > 0.6 {
        Color::from_rgb(0.06, 0.06, 0.06)
    } else {
        Color::WHITE
    }
}

// --- rice-faithful style helpers (capture pywal colors into iced style closures) ---

fn app_bg(colors: &theme::ThemeColors) -> impl Fn(&Theme) -> container::Style {
    let bg = hexc(&colors.bg1, Color::from_rgb8(0x0f, 0x18, 0x22));
    let text = hexc(&colors.text, Color::WHITE);
    move |_| container::Style {
        background: Some(bg.into()),
        text_color: Some(text),
        ..Default::default()
    }
}

fn sidebar_bg(colors: &theme::ThemeColors) -> impl Fn(&Theme) -> container::Style {
    let bg = hexc(&colors.bg2, Color::from_rgb8(0x08, 0x0c, 0x10));
    move |_| container::Style {
        background: Some(bg.into()),
        ..Default::default()
    }
}

/// Secondary/caption text colour: the theme text at reduced opacity.
pub(crate) fn muted(colors: &theme::ThemeColors) -> Color {
    let mut c = hexc(&colors.text, Color::WHITE);
    c.a = 0.55;
    c
}

/// Hairline border colour derived from the text colour (for definition on dark bg).
fn hairline(colors: &theme::ThemeColors) -> Color {
    let mut c = hexc(&colors.text, Color::WHITE);
    c.a = 0.10;
    c
}

fn card(colors: &theme::ThemeColors) -> impl Fn(&Theme) -> container::Style {
    let surface = hexc(&colors.surface, Color::from_rgb8(0x1f, 0x25, 0x2e));
    let line = hairline(colors);
    move |_| container::Style {
        background: Some(surface.into()),
        border: iced::Border {
            radius: 14.0.into(),
            width: 1.0,
            color: line,
        },
        ..Default::default()
    }
}

fn swatch_box(rgb: (u8, u8, u8)) -> impl Fn(&Theme) -> container::Style {
    let c = Color::from_rgb8(rgb.0, rgb.1, rgb.2);
    move |_| container::Style {
        background: Some(c.into()),
        border: iced::Border {
            radius: 14.0.into(),
            width: 1.0,
            color: Color::from_rgba(1.0, 1.0, 1.0, 0.18),
        },
        ..Default::default()
    }
}

/// Accent-coloured slider (filled track + accent handle).
fn accent_slider(colors: &theme::ThemeColors) -> impl Fn(&Theme, slider::Status) -> slider::Style {
    let accent = hexc(&colors.accent, Color::from_rgb8(0xd8, 0x8f, 0x92));
    let track = hexc(&colors.bg2, Color::from_rgb8(0x08, 0x0c, 0x10));
    move |theme, status| {
        let mut s = slider::default(theme, status);
        s.rail.backgrounds = (
            iced::Background::Color(accent),
            iced::Background::Color(track),
        );
        s.rail.width = 5.0;
        s.rail.border.radius = 3.0.into();
        s.handle.background = iced::Background::Color(accent);
        s.handle.border_width = 2.0;
        s.handle.border_color = Color::from_rgba(1.0, 1.0, 1.0, 0.9);
        if let slider::HandleShape::Circle { radius } = &mut s.handle.shape {
            *radius = 9.0;
        }
        s
    }
}

/// A colour swatch button: filled with `rgb`, ringed in accent when selected.
fn swatch_button_style(
    rgb: (u8, u8, u8),
    selected: bool,
    accent: Color,
) -> impl Fn(&Theme, button::Status) -> button::Style {
    let fill = Color::from_rgb8(rgb.0, rgb.1, rgb.2);
    move |theme, status| {
        let mut s = button::secondary(theme, status);
        s.background = Some(fill.into());
        s.border = iced::Border {
            radius: 9.0.into(),
            width: if selected { 2.5 } else { 1.0 },
            color: if selected {
                accent
            } else {
                Color::from_rgba(1.0, 1.0, 1.0, 0.18)
            },
        };
        s
    }
}

fn nav_button_style(
    colors: &theme::ThemeColors,
    active: bool,
) -> impl Fn(&Theme, button::Status) -> button::Style {
    let accent = hexc(&colors.accent, Color::from_rgb8(0xd8, 0x8f, 0x92));
    let text = hexc(&colors.text, Color::WHITE);
    let surface = hexc(&colors.surface, Color::from_rgb8(0x1f, 0x25, 0x2e));
    move |theme, status| {
        let hovered = matches!(status, button::Status::Hovered);
        let bg = if active {
            accent
        } else if hovered {
            surface
        } else {
            Color::TRANSPARENT
        };
        let fg = if active { contrast_on(accent) } else { text };
        let mut s = button::secondary(theme, status);
        s.background = Some(bg.into());
        s.text_color = fg;
        s.border = iced::Border {
            radius: 8.0.into(),
            ..s.border
        };
        s
    }
}

fn accent_button_style(
    colors: &theme::ThemeColors,
) -> impl Fn(&Theme, button::Status) -> button::Style {
    let accent = hexc(&colors.accent, Color::from_rgb8(0xd8, 0x8f, 0x92));
    move |theme, status| {
        let dim = matches!(status, button::Status::Hovered);
        let bg = if dim {
            lerp_color(accent, Color::WHITE, 0.12)
        } else {
            accent
        };
        let mut s = button::primary(theme, status);
        s.background = Some(bg.into());
        s.text_color = contrast_on(accent);
        s.border = iced::Border {
            radius: 10.0.into(),
            ..s.border
        };
        s
    }
}

fn segment_style(
    colors: &theme::ThemeColors,
    active: bool,
) -> impl Fn(&Theme, button::Status) -> button::Style {
    nav_button_style(colors, active)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) enum ShellTab {
    #[default]
    Lighting,
    Canvas,
    Devices,
    Sync,
}

impl ShellTab {
    fn label(self) -> &'static str {
        match self {
            ShellTab::Lighting => "Lighting",
            ShellTab::Canvas => "Canvas",
            ShellTab::Devices => "Devices",
            ShellTab::Sync => "Sync",
        }
    }

    fn all() -> [ShellTab; 4] {
        [
            ShellTab::Lighting,
            ShellTab::Canvas,
            ShellTab::Devices,
            ShellTab::Sync,
        ]
    }

    /// Startup tab override (`CNTRL_START_TAB=lighting|canvas|devices|sync`).
    fn from_env() -> Self {
        match std::env::var("CNTRL_START_TAB")
            .unwrap_or_default()
            .trim()
            .to_ascii_lowercase()
            .as_str()
        {
            "canvas" => ShellTab::Canvas,
            "devices" => ShellTab::Devices,
            "sync" => ShellTab::Sync,
            _ => ShellTab::Lighting,
        }
    }
}

/// One selectable lighting target (SignalRGB-style per-device selection).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TargetKind {
    Keyboard,
    Ite,
    Gpu,
    Corsair,
}

impl TargetKind {
    /// Map a detected device to its target (None for devices with no backend).
    fn from_device_kind(k: cntrl_core::devices::DeviceKind) -> Self {
        use cntrl_core::devices::DeviceKind;
        match k {
            DeviceKind::Keyboard => TargetKind::Keyboard,
            DeviceKind::Motherboard => TargetKind::Ite,
            DeviceKind::GraphicsCard => TargetKind::Gpu,
            DeviceKind::Memory => TargetKind::Corsair,
        }
    }
}

/// Which devices the next Apply writes to. Each can be toggled independently,
/// so you can color e.g. just the RAM, or the keyboard + GPU.
#[derive(Debug, Clone, Copy)]
struct Targets {
    keyboard: bool,
    ite: bool,
    gpu: bool,
    corsair: bool,
}

impl Default for Targets {
    fn default() -> Self {
        Self {
            keyboard: true,
            ite: true,
            gpu: true,
            corsair: true,
        }
    }
}

impl Targets {
    fn get(&self, k: TargetKind) -> bool {
        match k {
            TargetKind::Keyboard => self.keyboard,
            TargetKind::Ite => self.ite,
            TargetKind::Gpu => self.gpu,
            TargetKind::Corsair => self.corsair,
        }
    }

    fn toggle(&mut self, k: TargetKind) {
        match k {
            TargetKind::Keyboard => self.keyboard = !self.keyboard,
            TargetKind::Ite => self.ite = !self.ite,
            TargetKind::Gpu => self.gpu = !self.gpu,
            TargetKind::Corsair => self.corsair = !self.corsair,
        }
    }

    fn any_pc(&self) -> bool {
        self.ite || self.gpu || self.corsair
    }

    /// `CNTRL_PC_RGB_BACKENDS` value for the selected PC backends (empty if none).
    fn pc_env_value(&self) -> String {
        let mut v = Vec::new();
        if self.ite {
            v.push("ite_rgb");
        }
        if self.gpu {
            v.push("gpu_rgb");
        }
        if self.corsair {
            v.push("corsair_ram");
        }
        v.join(",")
    }
}

struct App {
    /// ICED theme (pywal-driven when `colors.conf` is present).
    ui_theme: Theme,
    /// Raw pywal colors (bg1/bg2/surface/accent/text) for rice-faithful styling.
    colors: theme::ThemeColors,
    /// Last observed mtime of `theme_file_path()` for hot reload.
    theme_mtime: Option<SystemTime>,
    tab: ShellTab,
    effect_id: i32,
    brightness: u8,
    speed: u8,
    /// HSL for color controls (H degrees, S/L 0–1).
    hsl: (f32, f32, f32),
    current_color: (u8, u8, u8),
    /// Editable hex buffer for the custom-color field (kept in sync with current_color).
    hex_input: String,
    profile: ProfileDocument,
    active_slot: u8,
    status: String,
    /// Per-device apply selection.
    targets: Targets,
    /// Native device inventory (refreshed on boot and via the Devices tab).
    devices: Vec<cntrl_core::devices::RgbDevice>,
    /// Persisted theme-sync settings (mirrors `settings.json`).
    settings: cntrl_core::settings::Settings,
    /// Canvas tab: device arrangement + effect settings (persisted).
    canvas_layout: CanvasLayout,
    /// Currently selected canvas device id (for rotate/scale controls).
    canvas_selected: Option<String>,
    /// Rendered effect backdrop for the arrangement view.
    canvas_preview: iced::widget::image::Handle,
    /// Effect clock epoch (preview time base).
    canvas_t0: std::time::Instant,
    /// Live hardware stream, when running.
    canvas_stream: Option<StreamHandle>,
    /// Canvas tab status line (stream stats / errors / push results).
    canvas_status: String,
}

/// Derive the editable lighting fields from a stored profile document.
fn derive_profile_state(doc: &ProfileDocument) -> (i32, u8, u8, (u8, u8, u8), (f32, f32, f32)) {
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
    (effect_id, brightness, speed, current_color, hsl)
}

impl App {
    fn from_profile(doc: ProfileDocument, active_slot: u8) -> Self {
        let (effect_id, brightness, speed, current_color, hsl) = derive_profile_state(&doc);
        let canvas_layout = core_canvas::load_layout();
        let canvas_preview = canvas_tab::render_preview(&canvas_layout, 0.0);

        Self {
            ui_theme: Theme::Dark,
            colors: theme::ThemeColors::default(),
            theme_mtime: None,
            tab: ShellTab::from_env(),
            effect_id,
            brightness,
            speed,
            hsl,
            current_color,
            hex_input: rgb_to_hex(current_color),
            profile: doc,
            active_slot,
            status: String::new(),
            targets: Targets::default(),
            devices: cntrl_core::devices::enumerate(),
            settings: cntrl_core::settings::load(),
            canvas_layout,
            canvas_selected: None,
            canvas_preview,
            canvas_t0: std::time::Instant::now(),
            canvas_stream: None,
            canvas_status: String::new(),
        }
    }

    /// Push the current layout/settings into the running stream (if any) so
    /// edits take effect on the next hardware frame.
    fn sync_stream(&self) {
        if let Some(stream) = &self.canvas_stream {
            if let Ok(mut shared) = stream.shared.lock() {
                shared.layout = self.canvas_layout.clone();
            }
        }
    }

    /// Persist the canvas layout, reporting failures in the status line.
    fn save_canvas_layout(&mut self) {
        if let Err(e) = core_canvas::save_layout(&self.canvas_layout) {
            self.canvas_status = format!("Layout save failed: {e}");
        }
    }

    /// Reload just the profile-derived fields for `slot`, preserving UI/device/settings state.
    fn load_slot_in_place(&mut self, doc: ProfileDocument, slot: u8) {
        let (effect_id, brightness, speed, current_color, hsl) = derive_profile_state(&doc);
        self.effect_id = effect_id;
        self.brightness = brightness;
        self.speed = speed;
        self.current_color = current_color;
        self.hsl = hsl;
        self.hex_input = rgb_to_hex(current_color);
        self.profile = doc;
        self.active_slot = slot;
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
pub(crate) enum Message {
    ThemePoll,
    Tab(ShellTab),
    Effect(i32),
    Hue(f32),
    Sat(f32),
    Light(f32),
    Brightness(f32),
    Speed(f32),
    Swatch((u8, u8, u8)),
    HexChanged(String),
    HexSubmit,
    Apply,
    ApplyDone(Result<(), String>),
    Slot(u8),
    ToggleTarget(TargetKind),
    RefreshDevices,
    ToggleThemeSync(bool),
    ToggleThemeSyncKeyboard(bool),
    ToggleThemeSyncPc(bool),
    ApplyThemeNow,
    ThemeApplied(Result<(u8, u8, u8), String>),
    // ── Canvas tab ──
    CanvasTick,
    CanvasEffect(EffectKind),
    CanvasSpeed(f32),
    CanvasScale(f32),
    CanvasBrightness(f32),
    CanvasFps(f32),
    CanvasSelect(Option<String>),
    CanvasMove(String, f32, f32),
    CanvasDragEnd,
    CanvasToggleEnabled(String),
    CanvasRotateSelected,
    CanvasScaleSelected(f32),
    CanvasResetLayout,
    CanvasStreamToggle,
    CanvasPushKeyboard,
    CanvasPushKeyboardDone(Result<(u8, u8, u8), String>),
    /// Configure an ARGB header's daisy chain: 0 = single strip, n = n fans.
    CanvasChainFans(&'static str, u8),
    /// Set LEDs per fan for every fan on a header (8 = AR120, 12/16 common).
    CanvasChainFanLeds(&'static str, u8),
    // Per-element trim on the selected device.
    CanvasDevBrightness(f32),
    CanvasDevHue(f32),
    CanvasDevSaturation(f32),
    CanvasDevReverse,
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
            app.colors = theme::load_theme_from_disk();
            app.ui_theme = iced_theme_from_colors(&app.colors);
            Task::none()
        }
        Message::Tab(t) => {
            app.tab = t;
            if t == ShellTab::Devices {
                app.devices = cntrl_core::devices::enumerate();
            }
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
            app.hex_input = rgb_to_hex(rgb);
            Task::none()
        }
        Message::HexChanged(s) => {
            app.hex_input = s;
            // Live-apply when it's a complete valid hex.
            if let Ok(rgb) = hex_to_rgb(&app.hex_input) {
                app.current_color = rgb;
                app.hsl = rgb_to_hsl_f32(rgb.0, rgb.1, rgb.2);
            }
            Task::none()
        }
        Message::HexSubmit => {
            match hex_to_rgb(&app.hex_input) {
                Ok(rgb) => {
                    app.current_color = rgb;
                    app.hsl = rgb_to_hsl_f32(rgb.0, rgb.1, rgb.2);
                    app.hex_input = rgb_to_hex(rgb);
                }
                Err(_) => app.status = format!("Invalid hex: {}", app.hex_input),
            }
            Task::none()
        }
        Message::Slot(slot) if slot <= 3 => {
            match load_profile_slot(slot) {
                Ok(doc) => {
                    app.load_slot_in_place(doc, slot);
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
            let targets = app.targets;
            if !targets.keyboard && !targets.any_pc() {
                app.status = "No devices selected".into();
                return Task::none();
            }
            Task::perform(
                async move {
                    tokio::task::spawn_blocking(move || {
                        use cntrl_core::ipc::{self, ApplyOptions};

                        // Restrict the PC fan-out to exactly the selected backends.
                        unsafe {
                            std::env::set_var("CNTRL_PC_RGB_BACKENDS", targets.pc_env_value());
                        }

                        let cfg = &doc.data.light.config;
                        // Remember per-device state so `cntrl restore` brings it back on boot.
                        let mut backends: Vec<String> = Vec::new();
                        if targets.keyboard {
                            backends.push("keyboard".into());
                        }
                        if targets.ite {
                            backends.push("ite_rgb".into());
                        }
                        if targets.gpu {
                            backends.push("gpu_rgb".into());
                        }
                        if targets.corsair {
                            backends.push("corsair_ram".into());
                        }
                        cntrl_core::device_state::remember(
                            &backends,
                            &cfg.hex_color,
                            cfg.effect,
                            cfg.brightness,
                            cfg.speed,
                        );

                        ipc::apply_local_with_options(
                            &doc,
                            slot,
                            ApplyOptions {
                                keyboard: targets.keyboard,
                                pc_rgb: targets.any_pc(),
                            },
                        )
                        .map_err(|e| e.to_string())
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
        Message::ToggleTarget(kind) => {
            app.targets.toggle(kind);
            Task::none()
        }
        Message::RefreshDevices => {
            app.devices = cntrl_core::devices::enumerate();
            app.status = "Devices refreshed".into();
            Task::none()
        }
        Message::ToggleThemeSync(v) => {
            app.settings.theme_sync_enabled = v;
            app.settings.theme_sync_slot = app.active_slot;
            persist_settings(app);
            Task::none()
        }
        Message::ToggleThemeSyncKeyboard(v) => {
            app.settings.theme_sync_keyboard = v;
            persist_settings(app);
            Task::none()
        }
        Message::ToggleThemeSyncPc(v) => {
            app.settings.theme_sync_pc = v;
            persist_settings(app);
            Task::none()
        }
        Message::ApplyThemeNow => {
            let slot = app.active_slot;
            let options = cntrl_core::ipc::ApplyOptions {
                keyboard: app.settings.theme_sync_keyboard,
                pc_rgb: app.settings.theme_sync_pc,
            };
            Task::perform(
                async move {
                    tokio::task::spawn_blocking(move || {
                        cntrl_core::theme_sync::apply_theme(slot, options)
                    })
                    .await
                    .unwrap_or_else(|e| Err(format!("Join: {e}")))
                },
                Message::ThemeApplied,
            )
        }
        Message::ThemeApplied(result) => {
            app.status = match result {
                Ok((r, g, b)) => format!("Theme applied: #{r:02x}{g:02x}{b:02x}"),
                Err(e) => format!("Theme sync: {e}"),
            };
            Task::none()
        }
        Message::CanvasTick => {
            let t = app.canvas_t0.elapsed().as_secs_f32();
            app.canvas_preview = canvas_tab::render_preview(&app.canvas_layout, t);
            // Surface live stream stats while running.
            if let Some(stream) = &app.canvas_stream {
                if let Ok(stats) = stream.stats.lock() {
                    let mut line = format!(
                        "Streaming · {:.0} fps · frame {}",
                        stats.actual_fps, stats.frames
                    );
                    if !stats.backends.is_empty() {
                        line.push_str(&format!(" · {}", stats.backends));
                    }
                    if let Some(e) = &stats.last_error {
                        line.push_str(&format!(" · last error: {e}"));
                    }
                    app.canvas_status = line;
                }
            }
            Task::none()
        }
        Message::CanvasEffect(kind) => {
            app.canvas_layout.settings.effect = kind;
            app.sync_stream();
            app.save_canvas_layout();
            Task::none()
        }
        Message::CanvasSpeed(v) => {
            app.canvas_layout.settings.speed = v;
            app.sync_stream();
            app.save_canvas_layout();
            Task::none()
        }
        Message::CanvasScale(v) => {
            app.canvas_layout.settings.scale = v;
            app.sync_stream();
            app.save_canvas_layout();
            Task::none()
        }
        Message::CanvasBrightness(v) => {
            app.canvas_layout.settings.brightness = v.clamp(0.0, 100.0);
            app.sync_stream();
            app.save_canvas_layout();
            Task::none()
        }
        Message::CanvasFps(v) => {
            app.canvas_layout.settings.fps = (v.round() as u32).clamp(5, 40);
            app.sync_stream();
            app.save_canvas_layout();
            Task::none()
        }
        Message::CanvasSelect(id) => {
            app.canvas_selected = id;
            Task::none()
        }
        Message::CanvasMove(id, x, y) => {
            if let Some(d) = app.canvas_layout.device_mut(&id) {
                d.placement.x = x;
                d.placement.y = y;
            }
            app.sync_stream();
            Task::none()
        }
        Message::CanvasDragEnd => {
            app.save_canvas_layout();
            Task::none()
        }
        Message::CanvasToggleEnabled(id) => {
            if let Some(d) = app.canvas_layout.device_mut(&id) {
                d.enabled = !d.enabled;
            }
            app.sync_stream();
            app.save_canvas_layout();
            Task::none()
        }
        Message::CanvasRotateSelected => {
            if let Some(id) = app.canvas_selected.clone() {
                if let Some(d) = app.canvas_layout.device_mut(&id) {
                    d.placement.rotation_deg = (d.placement.rotation_deg + 90.0) % 360.0;
                }
                app.sync_stream();
                app.save_canvas_layout();
            }
            Task::none()
        }
        Message::CanvasScaleSelected(factor) => {
            if let Some(id) = app.canvas_selected.clone() {
                if let Some(d) = app.canvas_layout.device_mut(&id) {
                    d.placement.scale = (d.placement.scale * factor).clamp(0.5, 20.0);
                }
                app.sync_stream();
                app.save_canvas_layout();
            }
            Task::none()
        }
        Message::CanvasResetLayout => {
            let settings = app.canvas_layout.settings;
            app.canvas_layout = CanvasLayout {
                settings,
                ..CanvasLayout::default()
            };
            app.canvas_selected = None;
            app.sync_stream();
            app.save_canvas_layout();
            app.canvas_status = "Layout reset".into();
            Task::none()
        }
        Message::CanvasStreamToggle => {
            match app.canvas_stream.take() {
                Some(stream) => {
                    stream.stop();
                    app.canvas_status = "Stream stopped".into();
                }
                None => {
                    app.canvas_stream =
                        Some(cntrl_core::canvas_stream::start(app.canvas_layout.clone()));
                    app.canvas_status = "Stream starting…".into();
                }
            }
            Task::none()
        }
        Message::CanvasPushKeyboard => {
            let layout = app.canvas_layout.clone();
            let t = app.canvas_t0.elapsed().as_secs_f32();
            app.canvas_status = "Pushing snapshot to keyboard…".into();
            Task::perform(
                async move {
                    tokio::task::spawn_blocking(move || {
                        cntrl_core::canvas_stream::push_keyboard_snapshot(&layout, t)
                            .map(|c| (c[0], c[1], c[2]))
                    })
                    .await
                    .unwrap_or_else(|e| Err(format!("Join: {e}")))
                },
                Message::CanvasPushKeyboardDone,
            )
        }
        Message::CanvasPushKeyboardDone(result) => {
            app.canvas_status = match result {
                Ok((r, g, b)) => format!("Keyboard set to #{r:02x}{g:02x}{b:02x}"),
                Err(e) => format!("Keyboard push failed: {e}"),
            };
            Task::none()
        }
        Message::CanvasChainFanLeds(header, leds) => {
            app.canvas_layout.chains.set_fan_leds(header, leds);
            app.canvas_layout.reconcile();
            app.sync_stream();
            app.save_canvas_layout();
            Task::none()
        }
        Message::CanvasChainFans(header, n) => {
            app.canvas_layout.chains.set_fans(header, n);
            app.canvas_layout.reconcile();
            // Selection may have pointed at a dropped segment.
            if let Some(id) = &app.canvas_selected {
                if app.canvas_layout.device(id).is_none() {
                    app.canvas_selected = None;
                }
            }
            app.sync_stream();
            app.save_canvas_layout();
            Task::none()
        }
        Message::CanvasDevBrightness(v) => {
            with_selected(app, |d| d.brightness = v.clamp(0.0, 100.0));
            Task::none()
        }
        Message::CanvasDevHue(v) => {
            with_selected(app, |d| d.hue_shift = v.clamp(0.0, 360.0));
            Task::none()
        }
        Message::CanvasDevSaturation(v) => {
            with_selected(app, |d| d.saturation = v.clamp(0.0, 200.0));
            Task::none()
        }
        Message::CanvasDevReverse => {
            with_selected(app, |d| d.reversed = !d.reversed);
            Task::none()
        }
    }
}

/// Mutate the selected canvas device, then sync + persist.
fn with_selected(app: &mut App, f: impl FnOnce(&mut cntrl_core::canvas::PlacedDevice)) {
    if let Some(id) = app.canvas_selected.clone() {
        if let Some(d) = app.canvas_layout.device_mut(&id) {
            f(d);
        }
        app.sync_stream();
        app.save_canvas_layout();
    }
}

fn persist_settings(app: &App) {
    if let Err(e) = cntrl_core::settings::save(&app.settings) {
        eprintln!("cntrl-iced: failed to save settings: {e}");
    }
}

fn sync_rgb_from_hsl(app: &mut App) {
    let (r, g, b) = hsl_to_rgb(app.hsl.0, app.hsl.1, app.hsl.2);
    app.current_color = (r, g, b);
    app.hex_input = rgb_to_hex(app.current_color);
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
    panel_view(app)
}

fn panel_view(app: &App) -> Element<'_, Message> {
    let colors = &app.colors;
    let accent = hexc(&colors.accent, Color::from_rgb8(0xd8, 0x8f, 0x92));

    let mut nav = column![
        text("CNTRL").size(24).color(accent),
        text("rgb lighting").size(11).color(muted(colors)),
        Space::new().height(8),
        hairline_divider(colors),
        Space::new().height(12),
    ]
    .spacing(2)
    .width(Length::Fill);

    for t in ShellTab::all() {
        nav = nav.push(nav_btn(t, app.tab, colors));
    }

    nav = nav.push(Space::new().height(22));
    nav = nav.push(text("PROFILE").size(10).color(muted(colors)));
    nav = nav.push(Space::new().height(2));
    nav = nav.push(
        row![
            slot_button(0, app.active_slot, colors),
            slot_button(1, app.active_slot, colors),
            slot_button(2, app.active_slot, colors),
            slot_button(3, app.active_slot, colors),
        ]
        .spacing(8),
    );
    nav = nav.push(Space::new().height(Length::Fill));
    nav = nav.push(device_summary(app));

    let sidebar = container(nav.height(Length::Fill))
        .padding(20)
        .width(216)
        .height(Length::Fill)
        .style(sidebar_bg(colors));

    let main: Element<'_, Message> = match app.tab {
        ShellTab::Lighting => lighting_view(app),
        ShellTab::Canvas => canvas_view(app),
        ShellTab::Devices => devices_view(app),
        ShellTab::Sync => sync_view(app),
    };
    let main = container(main)
        .padding(28)
        .width(Length::Fill)
        .height(Length::Fill);

    container(row![sidebar, main])
        .width(Length::Fill)
        .height(Length::Fill)
        .style(app_bg(&app.colors))
        .into()
}

fn nav_btn(tab: ShellTab, current: ShellTab, colors: &theme::ThemeColors) -> Element<'_, Message> {
    button(text(tab.label()).size(15))
        .on_press(Message::Tab(tab))
        .width(Length::Fill)
        .padding([10, 12])
        .style(nav_button_style(colors, tab == current))
        .into()
}

/// Compact device-health line for the sidebar footer.
fn device_summary(app: &App) -> Element<'_, Message> {
    use cntrl_core::devices::DeviceStatus;
    let ready = app
        .devices
        .iter()
        .filter(|d| d.status == DeviceStatus::Ready)
        .count();
    let blocked = app
        .devices
        .iter()
        .filter(|d| d.status == DeviceStatus::Blocked)
        .count();
    let (dot, color) = if blocked > 0 {
        ("●", Color::from_rgb8(0xe0, 0x6c, 0x4c))
    } else if ready > 0 {
        ("●", Color::from_rgb8(0x4c, 0xc2, 0x6a))
    } else {
        ("○", Color::from_rgb8(0x80, 0x80, 0x80))
    };
    let summary = if blocked > 0 {
        format!("{blocked} blocked · {ready} ready")
    } else {
        format!("{ready} device(s) ready")
    };
    button(
        row![text(dot).color(color), text(summary).size(12)]
            .spacing(8)
            .align_y(Alignment::Center),
    )
    .on_press(Message::Tab(ShellTab::Devices))
    .width(Length::Fill)
    .padding([8, 10])
    .style(nav_button_style(&app.colors, false))
    .into()
}

fn slot_button(slot: u8, active: u8, colors: &theme::ThemeColors) -> Element<'static, Message> {
    let label = format!("{}", slot + 1);
    button(text(label))
        .on_press(Message::Slot(slot))
        .padding([8, 12])
        .style(segment_style(colors, active == slot))
        .into()
}

fn effect_name(id: i32) -> String {
    if id == MODE_STATIC {
        "Static".to_string()
    } else if id == MODE_CUSTOM {
        "Custom".to_string()
    } else if id == EFFECT_ID_OFF {
        "Off".to_string()
    } else {
        PRESET_MODES
            .iter()
            .find(|m| i32::from(m.effect_id) == id)
            .map(|m| m.label.to_string())
            .unwrap_or_else(|| format!("Effect {id}"))
    }
}

fn labeled_slider<'a>(
    label: &'a str,
    value_label: String,
    range: std::ops::RangeInclusive<f32>,
    value: f32,
    colors: &theme::ThemeColors,
    on_change: impl Fn(f32) -> Message + 'a,
) -> Element<'a, Message> {
    let head = row![
        text(label).size(12).color(muted(colors)),
        Space::new().width(Length::Fill),
        text(value_label).size(12).color(muted(colors)),
    ]
    .align_y(Alignment::Center);
    column![
        head,
        slider(range, value, on_change).style(accent_slider(colors))
    ]
    .spacing(5)
    .into()
}

/// Page title + muted caption.
fn section_header<'a>(
    title: &'a str,
    caption: impl Into<String>,
    colors: &theme::ThemeColors,
) -> Element<'a, Message> {
    column![
        text(title).size(26),
        text(caption.into()).size(13).color(muted(colors)),
    ]
    .spacing(3)
    .into()
}

/// Small card section label (uppercased, muted).
fn card_label<'a>(label: &'a str, colors: &theme::ThemeColors) -> Element<'a, Message> {
    text(label).size(13).color(muted(colors)).into()
}

/// 1px horizontal rule in the hairline colour.
fn hairline_divider(colors: &theme::ThemeColors) -> Element<'static, Message> {
    let line = hairline(colors);
    container(Space::new().height(1).width(Length::Fill))
        .style(move |_| container::Style {
            background: Some(line.into()),
            ..Default::default()
        })
        .into()
}

fn lighting_view(app: &App) -> Element<'_, Message> {
    let colors = &app.colors;
    let hex = rgb_to_hex(app.current_color);

    let preview = container(Space::new())
        .width(60)
        .height(60)
        .style(swatch_box(app.current_color));
    let header = row![
        section_header(
            "Lighting",
            format!(
                "Slot {} · {}",
                app.active_slot + 1,
                effect_name(app.effect_id)
            ),
            colors,
        ),
        Space::new().width(Length::Fill),
        text(hex)
            .font(iced::Font::MONOSPACE)
            .size(15)
            .color(muted(colors)),
        preview,
    ]
    .spacing(16)
    .align_y(Alignment::Center);

    // --- Effects card ---
    let special = row![
        effect_btn("Static", MODE_STATIC, app.effect_id, colors),
        effect_btn("Custom", MODE_CUSTOM, app.effect_id, colors),
        effect_btn("Off", EFFECT_ID_OFF, app.effect_id, colors),
    ]
    .spacing(8);
    let presets: Element<'_, Message> = {
        let mut col = column![].spacing(5);
        for m in PRESET_MODES {
            col = col.push(effect_btn(m.label, i32::from(m.effect_id), app.effect_id, colors));
        }
        scrollable(col).height(Length::Fill).into()
    };
    let effects = container(
        column![
            card_label("EFFECT", colors),
            special,
            Space::new().height(2),
            presets,
        ]
        .spacing(12),
    )
    .padding(18)
    .height(Length::Fill)
    .width(Length::FillPortion(1))
    .style(card(colors));

    // --- Color + Controls cards ---
    let color_card = container(
        column![
            card_label("COLOR", colors),
            labeled_slider(
                "Hue",
                format!("{:.0}°", app.hsl.0),
                0.0..=360.0,
                app.hsl.0,
                colors,
                Message::Hue
            ),
            labeled_slider(
                "Saturation",
                format!("{:.0}%", app.hsl.1 * 100.0),
                0.0..=100.0,
                app.hsl.1 * 100.0,
                colors,
                Message::Sat
            ),
            labeled_slider(
                "Lightness",
                format!("{:.0}%", app.hsl.2 * 100.0),
                0.0..=100.0,
                app.hsl.2 * 100.0,
                colors,
                Message::Light
            ),
            Space::new().height(2),
            swatches(app.current_color, colors),
            hex_input_row(&app.hex_input, app.current_color, colors),
        ]
        .spacing(12),
    )
    .padding(18)
    .width(Length::Fill)
    .style(card(colors));

    let controls_card = container(
        column![
            card_label("CONTROLS", colors),
            labeled_slider(
                "Brightness",
                format!("{}%", app.brightness),
                0.0..=100.0,
                app.brightness as f32,
                colors,
                Message::Brightness
            ),
            labeled_slider(
                "Speed",
                format!("{}", app.speed),
                0.0..=5.0,
                app.speed as f32,
                colors,
                Message::Speed
            ),
        ]
        .spacing(12),
    )
    .padding(18)
    .width(Length::Fill)
    .style(card(colors));

    let right = column![color_card, controls_card]
        .spacing(16)
        .width(Length::FillPortion(1));

    // --- Apply bar: per-device target chips + accent Apply ---
    let apply_bar = column![
        row![
            text("APPLY TO").size(11).color(muted(colors)),
            Space::new().width(Length::Fill),
            status_text(&app.status, colors),
        ]
        .align_y(Alignment::Center),
        row![
            target_chips(app),
            Space::new().width(Length::Fill),
            button(text("Apply").size(15))
                .on_press(Message::Apply)
                .padding([12, 34])
                .style(accent_button_style(colors)),
        ]
        .spacing(12)
        .align_y(Alignment::Center),
    ]
    .spacing(10);

    column![
        header,
        row![effects, right].spacing(16).height(Length::Fill),
        apply_bar,
    ]
    .spacing(18)
    .into()
}

/// Row of per-device selection chips (icon + label), built from detected devices.
fn target_chips(app: &App) -> Element<'_, Message> {
    let colors = &app.colors;
    let mut r = row![].spacing(8);
    for d in &app.devices {
        let kind = TargetKind::from_device_kind(d.kind);
        let label = match kind {
            TargetKind::Keyboard => "Keyboard",
            TargetKind::Ite => "Motherboard",
            TargetKind::Gpu => "GPU",
            TargetKind::Corsair => "RAM",
        };
        r = r.push(target_chip(
            label,
            d.kind.icon(),
            kind,
            app.targets.get(kind),
            colors,
        ));
    }
    r.into()
}

fn target_chip<'a>(
    label: &'a str,
    glyph: &'a str,
    kind: TargetKind,
    selected: bool,
    colors: &theme::ThemeColors,
) -> Element<'a, Message> {
    let accent = hexc(&colors.accent, Color::from_rgb8(0xd8, 0x8f, 0x92));
    let fg = if selected {
        contrast_on(accent)
    } else {
        hexc(&colors.text, Color::WHITE)
    };
    button(
        row![
            text(glyph).font(ICON_FONT).size(16).color(fg),
            text(label).size(13).color(fg),
        ]
        .spacing(7)
        .align_y(Alignment::Center),
    )
    .on_press(Message::ToggleTarget(kind))
    .padding([8, 12])
    .style(segment_style(colors, selected))
    .into()
}

fn canvas_view(app: &App) -> Element<'_, Message> {
    let colors = &app.colors;
    let s = app.canvas_layout.settings;
    let streaming = app.canvas_stream.is_some();

    let header = row![
        section_header(
            "Canvas",
            "Arrange devices on one effect surface — drag to move; LEDs sample the pixels under them",
            colors,
        ),
        Space::new().width(Length::Fill),
        button(text(if streaming { "Stop stream" } else { "Start stream" }).size(15))
            .on_press(Message::CanvasStreamToggle)
            .padding([11, 26])
            .style(accent_button_style(colors)),
    ]
    .spacing(16)
    .align_y(Alignment::Center);

    // ── the arrangement scene ──
    let scene = container(canvas_tab::scene_element(
        &app.canvas_layout,
        &app.canvas_preview,
        app.canvas_selected.as_deref(),
        colors,
    ))
    .padding(6)
    .width(Length::Fill)
    .height(Length::Fill)
    .style(card(colors));

    // ── effect picker ──
    let mut effect_row = row![].spacing(8);
    for kind in EffectKind::all() {
        effect_row = effect_row.push(
            button(text(kind.label()).size(13))
                .on_press(Message::CanvasEffect(kind))
                .padding([8, 12])
                .style(segment_style(colors, s.effect == kind)),
        );
    }

    // ── sliders ──
    let sliders = row![
        labeled_slider(
            "Speed",
            format!("{:.1}×", s.speed),
            0.1..=3.0,
            s.speed,
            colors,
            Message::CanvasSpeed
        ),
        labeled_slider(
            "Scale",
            format!("{:.1}×", s.scale),
            0.3..=3.0,
            s.scale,
            colors,
            Message::CanvasScale
        ),
        labeled_slider(
            "Brightness",
            format!("{:.0}%", s.brightness),
            0.0..=100.0,
            s.brightness,
            colors,
            Message::CanvasBrightness
        ),
        labeled_slider(
            "FPS",
            format!("{}", s.fps),
            5.0..=60.0,
            s.fps as f32,
            colors,
            Message::CanvasFps
        ),
    ]
    .spacing(18);

    // ── device chips: toggle each device's canvas participation. Each chip
    // carries the same identifying color as the device's border on the canvas ──
    let mut chips = row![].spacing(8);
    for (i, (id, spec, enabled)) in canvas_tab::placed_specs(&app.canvas_layout)
        .into_iter()
        .enumerate()
    {
        // Compact names so a fully split chain still fits on one row.
        let label = spec
            .name
            .replace("ARGB Header ", "Header ")
            .replace("Header 1 ", "H1 ")
            .replace("Header 2 ", "H2 ");
        let ident = canvas_tab::device_color(i);
        chips = chips.push(
            button(
                row![
                    container(Space::new().width(8).height(8)).style(move |_| {
                        container::Style {
                            background: Some(ident.into()),
                            border: iced::Border {
                                radius: 4.0.into(),
                                ..Default::default()
                            },
                            ..Default::default()
                        }
                    }),
                    text(label).size(12),
                ]
                .spacing(6)
                .align_y(Alignment::Center),
            )
            .on_press(Message::CanvasToggleEnabled(id))
            .padding([7, 10])
            .style(segment_style(colors, enabled)),
        );
    }

    // ── ARGB header chains: what's daisy-chained on each header, and how
    // many LEDs each fan has (AR120 = 8; most rings are 12 or 16) ──
    let chain_row = |header: &'static str, label: &'static str| {
        let segs = app.canvas_layout.chains.header(header);
        let fans = segs.iter().filter(|s| s.kind == cntrl_core::canvas::SegmentKind::Fan).count() as u8;
        let is_strip = fans == 0;
        let fan_leds = segs
            .iter()
            .find(|s| s.kind == cntrl_core::canvas::SegmentKind::Fan)
            .map(|s| s.leds);
        let mut r = row![text(label).size(12).color(muted(&app.colors))].spacing(6);
        r = r.push(
            button(text("Strip").size(11))
                .on_press(Message::CanvasChainFans(header, 0))
                .padding([5, 9])
                .style(segment_style(&app.colors, is_strip)),
        );
        for n in 2..=4u8 {
            r = r.push(
                button(text(format!("{n} fans")).size(11))
                    .on_press(Message::CanvasChainFans(header, n))
                    .padding([5, 9])
                    .style(segment_style(&app.colors, fans == n)),
            );
        }
        if let Some(cur) = fan_leds {
            r = r.push(text("·").size(11).color(muted(&app.colors)));
            for leds in [8u8, 12, 16] {
                r = r.push(
                    button(text(format!("{leds}/fan")).size(11))
                        .on_press(Message::CanvasChainFanLeds(header, leds))
                        .padding([5, 9])
                        .style(segment_style(&app.colors, cur == leds)),
                );
            }
        }
        r.align_y(Alignment::Center)
    };

    // ── selection controls + per-element trim (SignalRGB-style) ──
    let selected = app
        .canvas_selected
        .as_deref()
        .and_then(|id| app.canvas_layout.device(id));
    let selected_name = app
        .canvas_selected
        .as_deref()
        .and_then(|id| app.canvas_layout.spec(id))
        .map(|s| s.name)
        .unwrap_or_else(|| "none".to_string());
    let mut sel_controls = row![
        text(format!("Selected: {selected_name}"))
            .size(12)
            .color(muted(colors)),
        button(text("⟳ 90°").size(12))
            .on_press(Message::CanvasRotateSelected)
            .padding([7, 10])
            .style(segment_style(colors, false)),
        button(text("−").size(12))
            .on_press(Message::CanvasScaleSelected(1.0 / 1.15))
            .padding([7, 12])
            .style(segment_style(colors, false)),
        button(text("+").size(12))
            .on_press(Message::CanvasScaleSelected(1.15))
            .padding([7, 12])
            .style(segment_style(colors, false)),
    ]
    .spacing(8)
    .align_y(Alignment::Center);
    if let Some(d) = selected {
        sel_controls = sel_controls.push(
            button(text(if d.reversed { "⇄ Reversed" } else { "⇄ Reverse" }).size(12))
                .on_press(Message::CanvasDevReverse)
                .padding([7, 10])
                .style(segment_style(colors, d.reversed)),
        );
    }
    sel_controls = sel_controls.push(Space::new().width(Length::Fill));
    sel_controls = sel_controls.push(
        button(text("Push to keyboard").size(12))
            .on_press(Message::CanvasPushKeyboard)
            .padding([7, 12])
            .style(segment_style(colors, false)),
    );
    sel_controls = sel_controls.push(
        button(text("Reset layout").size(12))
            .on_press(Message::CanvasResetLayout)
            .padding([7, 12])
            .style(segment_style(colors, false)),
    );

    // Per-device trim sliders, visible while something is selected.
    let trim: Element<'_, Message> = match selected {
        Some(d) => row![
            labeled_slider(
                "Device brightness",
                format!("{:.0}%", d.brightness),
                0.0..=100.0,
                d.brightness,
                colors,
                Message::CanvasDevBrightness
            ),
            labeled_slider(
                "Hue shift",
                format!("{:.0}°", d.hue_shift),
                0.0..=360.0,
                d.hue_shift,
                colors,
                Message::CanvasDevHue
            ),
            labeled_slider(
                "Saturation",
                format!("{:.0}%", d.saturation),
                0.0..=200.0,
                d.saturation,
                colors,
                Message::CanvasDevSaturation
            ),
        ]
        .spacing(18)
        .into(),
        None => Space::new().height(0).into(),
    };

    let controls = container(
        column![
            row![
                card_label("EFFECT", colors),
                Space::new().width(Length::Fill),
                status_text(&app.canvas_status, colors),
            ]
            .align_y(Alignment::Center),
            effect_row,
            sliders,
            hairline_divider(colors),
            row![card_label("DEVICES", colors), chips]
                .spacing(14)
                .align_y(Alignment::Center),
            row![
                card_label("ARGB CHAINS", colors),
                chain_row(cntrl_core::canvas::CHAIN_DLED1, "Header 1:"),
                chain_row(cntrl_core::canvas::CHAIN_DLED2, "Header 2:"),
            ]
            .spacing(18)
            .align_y(Alignment::Center),
            sel_controls,
            trim,
        ]
        .spacing(12),
    )
    .padding(18)
    .width(Length::Fill)
    .style(card(colors));

    column![header, scene, controls].spacing(14).into()
}

fn sync_view(app: &App) -> Element<'_, Message> {
    let colors = &app.colors;
    let s = &app.settings;

    let theme_card = container(
        column![
            card_label("THEME SYNC", colors),
            text("Follows ~/.config/themes/active/colors.conf — its accent drives the lighting.")
                .size(13)
                .color(muted(colors)),
            checkbox(s.theme_sync_enabled)
                .label("Auto-apply when the theme changes (requires `cntrl daemon` running)")
                .on_toggle(Message::ToggleThemeSync),
            Space::new().height(2),
            text("Apply to").size(12).color(muted(colors)),
            row![
                checkbox(s.theme_sync_keyboard)
                    .label("Keyboard")
                    .on_toggle(Message::ToggleThemeSyncKeyboard),
                checkbox(s.theme_sync_pc)
                    .label("PC RGB")
                    .on_toggle(Message::ToggleThemeSyncPc),
            ]
            .spacing(24),
            Space::new().height(4),
            row![
                button(text("Apply theme now"))
                    .on_press(Message::ApplyThemeNow)
                    .padding([10, 22])
                    .style(accent_button_style(colors)),
                status_text(&app.status, colors),
            ]
            .spacing(14)
            .align_y(Alignment::Center),
        ]
        .spacing(12),
    )
    .padding(20)
    .width(Length::Fill)
    .style(card(colors));

    let hint_card = container(
        column![
            card_label("PER-DEVICE COLOR", colors),
            text("Pick which components each Apply writes to on the Lighting tab — \
                  e.g. select only RAM to recolor just the memory, then keyboard for a different shade.")
                .size(13)
                .color(muted(colors)),
        ]
        .spacing(8),
    )
    .padding(20)
    .width(Length::Fill)
    .style(card(colors));

    scrollable(
        column![
            section_header("Sync", "Theme integration and targeting", colors),
            theme_card,
            hint_card,
        ]
        .spacing(20),
    )
    .height(Length::Fill)
    .into()
}

fn devices_view(app: &App) -> Element<'_, Message> {
    use cntrl_core::devices::DeviceStatus;
    let colors = &app.colors;

    let header = row![
        section_header("Devices", "Detected natively — no external tools", colors),
        Space::new().width(Length::Fill),
        button(text("Refresh"))
            .on_press(Message::RefreshDevices)
            .padding([9, 18])
            .style(segment_style(colors, false)),
    ]
    .align_y(Alignment::Center);

    let mut col = column![header].spacing(14);

    let accent = hexc(&colors.accent, Color::from_rgb8(0xd8, 0x8f, 0x92));
    for d in &app.devices {
        // Subtitle: "kind · backend · usb-id" (usb-id only for HID devices).
        let mut subtitle = format!("{} · {}", d.kind.label(), d.backend_id);
        if d.vid != 0 {
            subtitle.push_str(&format!(" · {}", d.usb_id()));
        }
        let card = container(
            row![
                // Device-type icon in the theme accent (neutral — not status).
                icon(d.kind.icon(), 30, accent),
                column![
                    row![
                        text(d.name.clone()).size(16),
                        Space::new().width(Length::Fill),
                        status_pill(d.status),
                    ]
                    .align_y(Alignment::Center),
                    text(subtitle).size(12).color(muted(colors)),
                    text(d.detail.clone()).size(11).color(muted(colors)),
                ]
                .spacing(4),
            ]
            .spacing(16)
            .align_y(Alignment::Center),
        )
        .padding(16)
        .width(Length::Fill)
        .style(card(colors));
        col = col.push(card);
    }

    if app
        .devices
        .iter()
        .any(|d| d.status == DeviceStatus::Blocked)
    {
        col = col.push(
            text(
                "Some devices are permission-blocked. Install the udev rule \
                 (packaging/udev) and replug — see README.",
            )
            .size(12)
            .color(muted(colors)),
        );
    }

    scrollable(col).height(Length::Fill).into()
}

fn effect_btn(
    lbl: &'static str,
    id: i32,
    active_id: i32,
    colors: &theme::ThemeColors,
) -> Element<'static, Message> {
    button(text(lbl))
        .on_press(Message::Effect(id))
        .width(Length::Fill)
        .padding([8, 10])
        .style(segment_style(colors, id == active_id))
        .into()
}

fn status_text<'a>(status: &'a str, colors: &theme::ThemeColors) -> Element<'a, Message> {
    text(status).size(13).color(muted(colors)).into()
}

/// Status dot + label, shown consistently on every device card.
fn status_pill(status: cntrl_core::devices::DeviceStatus) -> Element<'static, Message> {
    use cntrl_core::devices::DeviceStatus;
    let color = match status {
        DeviceStatus::Ready => Color::from_rgb8(0x4c, 0xc2, 0x6a),
        DeviceStatus::Blocked => Color::from_rgb8(0xe0, 0x6c, 0x4c),
        DeviceStatus::Absent => Color::from_rgb8(0x80, 0x80, 0x80),
    };
    let dot = container(Space::new().width(8).height(8)).style(move |_| container::Style {
        background: Some(color.into()),
        border: iced::Border {
            radius: 4.0.into(),
            ..Default::default()
        },
        ..Default::default()
    });
    row![dot, text(status.label()).size(12).color(color)]
        .spacing(6)
        .align_y(Alignment::Center)
        .into()
}

const SWATCH_COLORS: [(u8, u8, u8); 16] = [
    (255, 0, 0),     // red
    (255, 96, 0),    // orange
    (255, 200, 0),   // amber
    (255, 255, 0),   // yellow
    (128, 255, 0),   // lime
    (0, 255, 0),     // green
    (0, 255, 160),   // spring
    (0, 220, 255),   // cyan
    (0, 128, 255),   // azure
    (0, 0, 255),     // blue
    (128, 0, 255),   // violet
    (200, 0, 255),   // purple
    (255, 0, 200),   // magenta
    (255, 0, 110),   // pink
    (255, 255, 255), // white
    (255, 90, 60),   // warm
];

fn swatches(current: (u8, u8, u8), colors: &theme::ThemeColors) -> Element<'static, Message> {
    let accent = hexc(&colors.accent, Color::from_rgb8(0xd8, 0x8f, 0x92));
    // Two rows of 8 so the whole palette is always visible in the narrow card.
    let mut grid = column![].spacing(8);
    for chunk in SWATCH_COLORS.chunks(8) {
        let mut r = row![].spacing(8);
        for &c in chunk {
            let selected = c == current;
            r = r.push(
                button(Space::new().width(26).height(26))
                    .on_press(Message::Swatch(c))
                    .style(swatch_button_style(c, selected, accent)),
            );
        }
        grid = grid.push(r);
    }
    grid.into()
}

/// Custom-color row: a hex text field (type/paste `#rrggbb`) + a live preview chip.
fn hex_input_row<'a>(
    value: &'a str,
    current: (u8, u8, u8),
    colors: &theme::ThemeColors,
) -> Element<'a, Message> {
    let valid = hex_to_rgb(value).is_ok();
    let line = hairline(colors);
    let field = text_input("#rrggbb", value)
        .on_input(Message::HexChanged)
        .on_submit(Message::HexSubmit)
        .font(iced::Font::MONOSPACE)
        .size(14)
        .padding([8, 10])
        .width(Length::Fixed(120.0))
        .style(move |theme, status| {
            let mut s = text_input::default(theme, status);
            s.border = iced::Border {
                radius: 8.0.into(),
                width: 1.0,
                color: if valid {
                    line
                } else {
                    Color::from_rgb8(0xe0, 0x6c, 0x4c)
                },
            };
            s
        });
    let preview = container(Space::new())
        .width(34)
        .height(34)
        .style(swatch_box(current));
    column![
        text("Custom").size(12).color(muted(colors)),
        row![field, preview].spacing(10).align_y(Alignment::Center),
    ]
    .spacing(6)
    .into()
}
