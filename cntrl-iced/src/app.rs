//! ICED shell: full desktop panel with sidebar + Lighting (Effects / Layout) / Fans /
//! Devices / Settings pages. Look and behaviour follow `docs/design/HANDOFF.md`.

use crate::canvas_tab;
use crate::tokens;
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
    button, checkbox, column, container, pick_list, row, scrollable, slider, stack, text,
    text_input, Space,
};
use iced::window;
use iced::theme::Palette;
use iced::{
    Alignment, Color, Element, Length, Task, Theme,
};
use std::time::{Duration, Instant};

pub fn run() -> iced::Result {
    iced::application(boot, update, view)
        .title("CNTRL")
        .font(tokens::UI_FONT_FILES[0])
        .font(tokens::UI_FONT_FILES[1])
        .font(tokens::UI_FONT_FILES[2])
        .font(tokens::UI_FONT_FILES[3])
        .default_font(tokens::UI)
        .theme(|app: &App| app.ui_theme.clone())
        .subscription(|app: &App| {
            let mut subs =
                vec![iced::time::every(Duration::from_secs(1)).map(|_| Message::ThemePoll)];
            if app.tab == ShellTab::Canvas {
                subs.push(iced::time::every(Duration::from_millis(33)).map(|_| Message::CanvasTick));
            } else if matches!(app.tab, ShellTab::Fans | ShellTab::Overview) {
                subs.push(iced::time::every(Duration::from_secs(1)).map(|_| Message::FanPoll));
            }
            // Stream edits made on other tabs still have to reach the daemon.
            if app.tab != ShellTab::Canvas && app.canvas_dirty {
                subs.push(iced::time::every(Duration::from_millis(50)).map(|_| Message::CanvasFlush));
            }
            // Only while a lighting change waits to be written.
            if app.apply_pending.is_some() {
                subs.push(iced::time::every(Duration::from_millis(50)).map(|_| Message::ApplyTick));
            }
            iced::Subscription::batch(subs)
        })
        .window(window::Settings {
            size: iced::Size::new(1280.0, 820.0),
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
    app.select_canvas_color(0);
    // Pick up a stream the daemon is already running (autostart, or started
    // before this panel opened).
    app.on_canvas_reply(cntrl_core::ipc::canvas_request(&cntrl_core::ipc::Request::CanvasStatus));
    // The start tab loads what it shows, as if it had been clicked.
    let start = app.tab;
    let task = update(&mut app, Message::Tab(start));
    (app, task)
}

fn iced_theme_from_colors(tc: &theme::ThemeColors) -> Theme {
    Theme::custom("cntrl-pywal", palette_from_theme_colors(tc))
}

fn palette_from_theme_colors(tc: &theme::ThemeColors) -> Palette {
    let base = Palette::DARK;
    Palette {
        background: color_from_hex(&tc.bg1).unwrap_or(base.background),
        text: color_from_hex(&tc.text).unwrap_or(base.text),
        primary: color_from_hex(&tc.accent).unwrap_or(base.primary),
        success: tokens::SUCCESS,
        warning: tokens::WARNING,
        danger: base.danger,
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

// --- style helpers shared by the screens not yet ported to the redesign; they
// read the same tokens through `App::colors` ---

fn app_bg(_colors: &theme::ThemeColors) -> impl Fn(&Theme) -> container::Style {
    move |_| container::Style {
        background: Some(tokens::BG.into()),
        text_color: Some(tokens::TEXT),
        ..Default::default()
    }
}

/// Secondary/caption text colour.
pub(crate) fn muted(_colors: &theme::ThemeColors) -> Color {
    tokens::TEXT_MUTED
}

/// Accent-coloured slider (filled track + accent handle).
fn accent_slider(colors: &theme::ThemeColors) -> impl Fn(&Theme, slider::Status) -> slider::Style {
    let accent = hexc(&colors.accent, Color::from_rgb8(0xd8, 0x8f, 0x92));
    let track = tokens::BORDER_CONTROL;
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) enum ShellTab {
    #[default]
    Overview,
    Lighting,
    Canvas,
    Fans,
    Devices,
    Sync,
}

impl ShellTab {
    /// Startup tab override (`CNTRL_START_TAB=overview|lighting|canvas|fans|devices|sync`).
    fn from_env() -> Self {
        match std::env::var("CNTRL_START_TAB")
            .unwrap_or_default()
            .trim()
            .to_ascii_lowercase()
            .as_str()
        {
            "lighting" => ShellTab::Lighting,
            "canvas" => ShellTab::Canvas,
            "fans" => ShellTab::Fans,
            "devices" => ShellTab::Devices,
            "sync" => ShellTab::Sync,
            _ => ShellTab::Overview,
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

/// Which devices lighting changes are written to. Each can be toggled independently,
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

    fn set_all(&mut self, on: bool) {
        *self = Self { keyboard: on, ite: on, gpu: on, corsair: on };
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
    ui_theme: Theme,
    /// Design tokens in the shape the older style helpers read; the accent is
    /// the current lighting color (see `retint`).
    colors: theme::ThemeColors,
    /// A lighting change waiting to be written (set when it was made).
    apply_pending: Option<Instant>,
    /// A write is running; the next one waits for it.
    apply_busy: bool,
    /// Sidebar profile list is open.
    profile_menu: bool,
    /// Layout tab: show the running effect behind the devices.
    canvas_live_preview: bool,
    /// Layout tab: dragged devices land on a 2-unit grid.
    canvas_snap: bool,
    /// Hex text for the streamed effect's selected color.
    canvas_hex: String,
    /// Devices screen: the card whose details are open (backend id).
    device_open: Option<&'static str>,
    /// Devices screen: the kind being identified (all its zones light up).
    device_identify: Option<TargetKind>,
    /// Identify started the stream itself, so stopping Identify stops it too.
    identify_owns_stream: bool,
    /// Background service starts at login (`None`: unit not installed).
    service_enabled: Option<bool>,
    /// Accent in the desktop theme file, read when Settings opens.
    desktop_accent: Option<(u8, u8, u8)>,
    /// Hex of the last manual theme apply in this session.
    theme_last_applied: Option<String>,
    /// Gallery stills of the streamed effects (rendered once).
    fx_thumbs: Vec<(EffectKind, iced::widget::image::Handle)>,
    /// The background daemon answers on its socket.
    engine_running: bool,
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
    /// The daemon is streaming (it owns the devices; this panel only edits).
    canvas_remote: bool,
    /// Effect time zero of the daemon's stream, for the preview.
    canvas_remote_t0: std::time::Instant,
    /// Layout/identify changed since the daemon last heard from us.
    canvas_dirty: bool,
    canvas_ticks: u32,
    /// Audio levels the running stream renders with, mirrored in the preview.
    canvas_input: cntrl_core::canvas_effects::EffectInput,
    /// Screen grabs for the preview of a screen effect (Canvas tab only).
    canvas_screen: Option<(String, cntrl_core::screen::ScreenCapture)>,
    /// Canvas tab status line (stream stats / errors / push results).
    canvas_status: String,
    /// Identify mode: the stream lights only the selected device.
    canvas_identify: bool,
    /// Which effect color is being edited, and its HSL (kept so hue survives
    /// a trip through grey/black).
    canvas_color_idx: usize,
    canvas_hsl: (f32, f32, f32),
    // ── Fans tab ──
    /// Discovered temp sensors + fan channels (refreshed on tab entry + poll).
    fan_discovery: cntrl_core::fan_control::FanDiscovery,
    /// Live RPM/duty/temperature, read once per poll (never while drawing).
    fan_readings: cntrl_core::fan_control::FanReadings,
    /// Persisted fan curve config (mirrors `fan_curves.json`; GUI is the editor).
    fan_curve_cfg: cntrl_core::fan_curves::FanCurveConfig,
    /// Selected fan channel id (drives the right-hand editor pane).
    fan_selected: Option<String>,
    /// Fans tab status line (save errors / hints).
    fan_status: String,
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
        let fan_discovery = cntrl_core::fan_control::discover();
        let mut canvas_layout = core_canvas::load_layout();
        canvas_layout.mark_present(&core_canvas::absent_backends());
        let canvas_preview = canvas_tab::render_preview(&canvas_layout, 0.0, &Default::default());

        Self {
            ui_theme: Theme::Dark,
            colors: theme::ThemeColors::default(),
            apply_pending: None,
            apply_busy: false,
            profile_menu: false,
            fx_thumbs: stream_thumbs(),
            canvas_live_preview: true,
            canvas_snap: false,
            canvas_hex: String::new(),
            device_open: None,
            device_identify: None,
            identify_owns_stream: false,
            service_enabled: None,
            desktop_accent: None,
            theme_last_applied: None,
            engine_running: cntrl_core::ipc::socket_path().exists(),
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
            canvas_remote: false,
            canvas_remote_t0: std::time::Instant::now(),
            canvas_dirty: false,
            canvas_ticks: 0,
            canvas_input: Default::default(),
            canvas_screen: None,
            canvas_status: String::new(),
            canvas_identify: false,
            canvas_color_idx: 0,
            canvas_hsl: (0.0, 0.0, 0.0),
            fan_readings: fan_discovery.read_all(),
            fan_discovery,
            fan_curve_cfg: cntrl_core::fan_curves::load(),
            fan_selected: None,
            fan_status: String::new(),
        }
    }

    /// Keep the UI accent on the current lighting color (design token
    /// "accent"). Too dark to read on the dark UI: the default accent.
    fn retint(&mut self) {
        let c = self.current_color;
        let dark = c.0.max(c.1).max(c.2) < 70;
        let accent = if dark || !self.settings.tint_ui { tokens::DEFAULT_ACCENT } else { c };
        let accent = rgb_to_hex(accent);
        if self.colors.accent != accent || self.colors.bg1 != tokens::BG_HEX {
            self.colors = theme::ThemeColors {
                bg1: tokens::BG_HEX.into(),
                bg2: tokens::SIDEBAR_HEX.into(),
                surface: tokens::SURFACE_HEX.into(),
                accent,
                text: tokens::TEXT_HEX.into(),
            };
            self.ui_theme = iced_theme_from_colors(&self.colors);
        }
    }

    fn accent(&self) -> Color {
        hexc(&self.colors.accent, tokens::TEXT)
    }

    /// Write the lighting to the devices shortly (see `Message::ApplyTick`).
    fn queue_apply(&mut self) {
        self.apply_pending = Some(Instant::now());
    }

    /// Persist the fan curve config, surfacing any error in the status line.
    fn save_fan_cfg(&mut self) {
        if let Err(e) = cntrl_core::fan_curves::save(&self.fan_curve_cfg) {
            self.fan_status = format!("Save failed: {e}");
        }
    }

    /// The graph curve id CNTRL manages for a given channel (one curve per channel).
    fn curve_id_for(channel_id: &str) -> String {
        format!("{channel_id}::curve")
    }

    /// The assignment for a channel, if one exists.
    fn assignment_for(&self, channel_id: &str) -> Option<&cntrl_core::fan_curves::FanAssignment> {
        self.fan_curve_cfg
            .assignments
            .iter()
            .find(|a| a.channel_id == channel_id)
    }

    fn streaming(&self) -> bool {
        self.canvas_stream.is_some() || self.canvas_remote
    }

    /// Take in the daemon's answer to a `Canvas*` request.
    fn on_canvas_reply(
        &mut self,
        reply: Result<Option<cntrl_core::ipc::CanvasStatus>, String>,
    ) {
        match reply {
            Ok(Some(st)) if st.running => {
                // The stream may run another effect than the saved layout
                // (`cntrl canvas --effect`): show what is really running.
                if let Some(effect) = st.effect {
                    if effect != self.canvas_layout.settings.effect && !self.canvas_dirty {
                        self.canvas_layout.settings.set_effect(effect);
                        self.select_canvas_color(0);
                    }
                }
                self.canvas_input = st.input();
                self.canvas_remote = true;
                self.canvas_remote_t0 = std::time::Instant::now()
                    - std::time::Duration::from_secs_f32(st.elapsed_secs.max(0.0));
                self.canvas_status = stream_status_line(
                    "Streaming (daemon)",
                    st.fps,
                    st.frames,
                    &st.backends,
                    &st.timing,
                    st.last_error.as_deref(),
                );
            }
            Ok(_) => {
                if self.canvas_remote {
                    self.canvas_status = "Stream stopped".into();
                }
                self.canvas_remote = false;
            }
            Err(e) => self.canvas_status = format!("Daemon: {e}"),
        }
    }

    /// Stop whichever stream is running (an explicit Apply wins over it).
    fn stop_canvas_stream(&mut self) {
        self.canvas_identify = false;
        self.device_identify = None;
        self.identify_owns_stream = false;
        if let Some(stream) = self.canvas_stream.take() {
            stream.stop();
            self.canvas_status = "Stream stopped".into();
        }
        if self.canvas_remote {
            let reply = cntrl_core::ipc::canvas_request(&cntrl_core::ipc::Request::CanvasStop);
            self.on_canvas_reply(reply);
        }
    }

    /// Point the color editor at effect color `idx` (clamped to the palette).
    fn select_canvas_color(&mut self, idx: usize) {
        let palette = &self.canvas_layout.settings.palette;
        self.canvas_color_idx = idx.min(palette.len().saturating_sub(1));
        if let Some(c) = palette.get(self.canvas_color_idx) {
            self.canvas_hsl = rgb_to_hsl_f32(c[0], c[1], c[2]);
            self.canvas_hex = rgb_to_hex((c[0], c[1], c[2]));
        }
    }

    /// What the stream should single out: every zone of the device being
    /// identified on the Devices screen, else the Layout tab's selection.
    fn identify_target(&self) -> Option<String> {
        match self.device_identify {
            Some(kind) => Some(layout_ids(self, kind).join(",")),
            None => self.canvas_identify.then(|| self.canvas_selected.clone()).flatten(),
        }
    }

    /// Send pending edits to the daemon's stream, or ask how it is doing.
    fn exchange_with_daemon(&mut self) {
        use cntrl_core::ipc::{canvas_request, Request};
        let req = if std::mem::take(&mut self.canvas_dirty) {
            Request::CanvasUpdate {
                layout: self.canvas_layout.clone(),
                identify: self.identify_target(),
            }
        } else {
            Request::CanvasStatus
        };
        self.on_canvas_reply(canvas_request(&req));
    }

    /// Re-scan hwmon and snapshot its live values for the Fans tab.
    fn refresh_fans(&mut self) {
        self.fan_discovery = cntrl_core::fan_control::discover();
        self.fan_readings = self.fan_discovery.read_all();
    }

    /// Push the current layout/settings into the running stream (if any) so
    /// edits take effect on the next hardware frame.
    fn sync_stream(&mut self) {
        // The daemon gets edits batched once per tick (see `CanvasTick`).
        self.canvas_dirty = self.canvas_remote;
        if let Some(stream) = &self.canvas_stream {
            if let Ok(mut shared) = stream.shared.lock() {
                shared.identify = self.identify_target();
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
    Brightness(f32),
    Speed(f32),
    Swatch((u8, u8, u8)),
    HexChanged(String),
    HexSubmit,
    Apply,
    ApplyDone(Result<(), String>),
    /// Debounce clock for live apply.
    ApplyTick,
    /// A slider was released: write its value now.
    LightingCommit,
    /// Pick an effect the background service renders and streams.
    LightingStream(EffectKind),
    ToggleAllTargets,
    ProfileMenu,
    /// Overview: switch every device's lights off.
    AllLightsOff,
    CanvasLivePreview,
    CanvasSnap,
    /// Turn the selected layout device to exactly this many degrees.
    CanvasSetRotation(f32),
    /// Push stream edits to the daemon (ticks only while some are pending).
    CanvasFlush,
    CanvasHexChanged(String),
    CanvasHexSubmit,
    /// Light one kind of device alone (toggle).
    DeviceIdentify(TargetKind),
    /// Open / close a device card's details (backend id).
    DeviceDetails(&'static str),
    OpenUrl(&'static str),
    /// Background service: start at login / run now.
    ServiceAutostart(bool),
    ServiceRun(bool),
    ToggleTintUi(bool),
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
    CanvasAngle(f32),
    CanvasBrightness(f32),
    CanvasFps(f32),
    CanvasSelect(Option<String>),
    CanvasMove(String, f32, f32),
    CanvasDragEnd,
    CanvasToggleEnabled(String),
    CanvasScaleSelected(f32),
    CanvasResetLayout,
    CanvasStreamToggle,
    CanvasPushKeyboard,
    CanvasPushKeyboardDone(Result<(u8, u8, u8), String>),
    /// Configure an ARGB header's daisy chain: 0 = single strip, n = n fans.
    CanvasChainFans(&'static str, u8),
    /// Set LEDs per fan for every fan on a header (8 = AR120, 12/16 common).
    CanvasChainFanLeds(&'static str, u8),
    /// (header, LEDs) — strip at the end of the chain; 0 removes it.
    CanvasChainStrip(&'static str, f32),
    /// Toggle: light only the selected device on the hardware.
    CanvasIdentify,
    /// Pick which effect color the HSL sliders edit.
    CanvasColorSelect(usize),
    /// Set the selected effect color; `true` also saves (a click, not a drag).
    CanvasColorSet((u8, u8, u8), bool),
    CanvasThemeAccent(bool),
    /// Daemon starts the stream at login.
    CanvasAutostart(bool),
    /// Second effect drawn over the main one (`None` = off).
    CanvasOverlayEffect(Option<EffectKind>),
    CanvasOverlayBlend(cntrl_core::canvas::Blend),
    CanvasOverlayOpacity(f32),
    CanvasAudioBrightness(bool),
    // Per-element trim on the selected device.
    CanvasDevBrightness(f32),
    CanvasDevHue(f32),
    CanvasDevSaturation(f32),
    CanvasDevReverse,
    // ── Fans tab ──
    /// Periodic refresh of live sensor/fan readings while the tab is open.
    FanPoll,
    /// Master enable for the fan control engine.
    FanToggleEnabled(bool),
    /// Select a channel to edit.
    FanSelectChannel(String),
    /// Create a default curve + assignment for a channel (using `sensor_id`).
    FanCreateAssignment(String, String),
    /// Remove a channel's assignment (and its managed curve).
    FanRemoveAssignment(String),
    /// Point in `curve_id` at `index` dragged to (temp, duty).
    FanCurvePointMoved(String, usize, f32, f32),
    /// Add a point to `curve_id` at (temp, duty).
    FanCurveAddPoint(String, f32, f32),
    /// Delete point `index` from `curve_id`.
    FanCurveDeletePoint(String, usize),
    /// Commit the curve edit to disk (fires on drag release / add / delete).
    FanCurveEditDone,
    /// Change the sensor a channel's curve reads from.
    FanSetSensor(String, String),
    FanSetAlsoSensor(String, Option<String>),
    /// Silent / Balanced / Performance / Custom.
    FanSetMode(cntrl_core::fan_curves::FanMode),
    /// "This is a pump" for a channel.
    FanSetPump(String, bool),
    /// Min duty floor for a channel (percent).
    FanMinDuty(String, f32),
}

fn update(app: &mut App, message: Message) -> Task<Message> {
    let task = update_inner(app, message);
    app.retint();
    task
}

fn update_inner(app: &mut App, message: Message) -> Task<Message> {
    match message {
        Message::ThemePoll => {
            if app.tab != ShellTab::Canvas {
                app.canvas_screen = None; // no preview, no grabbing
            }
            app.engine_running = cntrl_core::ipc::socket_path().exists();
            // Off the Layout tab nothing else polls: keep the gallery's
            // selection in step with what the daemon is streaming.
            if app.tab != ShellTab::Canvas && app.canvas_stream.is_none() && app.engine_running {
                app.exchange_with_daemon();
            }
            Task::none()
        }
        Message::Tab(t) => {
            app.tab = t;
            if t == ShellTab::Devices {
                app.devices = cntrl_core::devices::enumerate();
            }
            if t == ShellTab::Canvas && !app.streaming() {
                // Pick up devices plugged in or removed since the last look.
                // (Not while streaming: probing would share the buses with it.)
                app.canvas_layout.mark_present(&core_canvas::absent_backends());
            }
            if t == ShellTab::Sync {
                app.service_enabled = service_enabled();
                app.desktop_accent = cntrl_core::theme_sync::current_theme_color().ok();
            }
            if matches!(t, ShellTab::Fans | ShellTab::Overview) {
                app.refresh_fans();
                app.fan_curve_cfg = cntrl_core::fan_curves::load();
            }
            Task::none()
        }
        Message::Effect(id) => {
            app.effect_id = id;
            app.queue_apply();
            Task::none()
        }
        Message::Hue(h) => {
            // The hue bar picks a pure color; shades come from swatches or hex.
            app.hsl = (h, 1.0, 0.5);
            sync_rgb_from_hsl(app);
            Task::none()
        }
        Message::LightingCommit => {
            app.queue_apply();
            Task::none()
        }
        Message::ApplyTick => {
            let due = app
                .apply_pending
                .is_some_and(|since| since.elapsed() >= Duration::from_millis(100));
            if due && !app.apply_busy {
                app.apply_pending = None;
                return update_inner(app, Message::Apply);
            }
            Task::none()
        }
        Message::LightingStream(kind) => {
            let _ = update_inner(app, Message::CanvasEffect(kind));
            if app.streaming() {
                Task::none()
            } else {
                Task::done(Message::CanvasStreamToggle)
            }
        }
        Message::AllLightsOff => {
            app.targets.set_all(true);
            app.effect_id = EFFECT_ID_OFF;
            app.queue_apply();
            Task::none()
        }
        Message::CanvasLivePreview => {
            app.canvas_live_preview = !app.canvas_live_preview;
            Task::none()
        }
        Message::CanvasSnap => {
            app.canvas_snap = !app.canvas_snap;
            Task::none()
        }
        Message::CanvasSetRotation(deg) => {
            if let Some(id) = app.canvas_selected.clone() {
                if let Some(d) = app.canvas_layout.device_mut(&id) {
                    d.placement.rotation_deg = deg;
                }
                app.sync_stream();
                app.save_canvas_layout();
            }
            Task::none()
        }
        Message::CanvasFlush => {
            app.exchange_with_daemon();
            Task::none()
        }
        Message::CanvasHexChanged(text) => {
            app.canvas_hex = text;
            match hex_to_rgb(&app.canvas_hex) {
                Ok(rgb) => {
                    // Keep what was typed (case, missing '#') while it is valid.
                    let typed = app.canvas_hex.clone();
                    let task = update_inner(app, Message::CanvasColorSet(rgb, false));
                    app.canvas_hex = typed;
                    task
                }
                Err(_) => Task::none(),
            }
        }
        Message::CanvasHexSubmit => {
            if let Ok(rgb) = hex_to_rgb(&app.canvas_hex) {
                return update_inner(app, Message::CanvasColorSet(rgb, true));
            }
            Task::none()
        }
        Message::DeviceIdentify(kind) => {
            if app.device_identify == Some(kind) {
                app.device_identify = None;
                app.sync_stream();
                if std::mem::take(&mut app.identify_owns_stream) && app.streaming() {
                    return Task::done(Message::CanvasStreamToggle);
                }
            } else if !layout_ids(app, kind).is_empty() {
                app.device_identify = Some(kind);
                if app.streaming() {
                    app.sync_stream();
                } else {
                    // No stream yet: run one for the duration of Identify. Its
                    // first update (next flush) carries the identify request.
                    app.identify_owns_stream = true;
                    app.canvas_dirty = true;
                    return Task::done(Message::CanvasStreamToggle);
                }
            }
            Task::none()
        }
        Message::DeviceDetails(id) => {
            app.device_open = (app.device_open != Some(id)).then_some(id);
            Task::none()
        }
        Message::OpenUrl(url) => {
            if let Err(e) = std::process::Command::new("xdg-open").arg(url).spawn() {
                app.status = format!("Could not open {url}: {e}");
            }
            Task::none()
        }
        Message::ServiceAutostart(on) => {
            systemctl(&[if on { "enable" } else { "disable" }, DAEMON_UNIT]);
            app.service_enabled = service_enabled();
            Task::none()
        }
        Message::ServiceRun(on) => {
            systemctl(&[if on { "start" } else { "stop" }, DAEMON_UNIT]);
            app.engine_running = on;
            Task::none()
        }
        Message::ToggleTintUi(on) => {
            app.settings.tint_ui = on;
            persist_settings(app);
            Task::none()
        }
        Message::ToggleAllTargets => {
            let all = app.all_targets_on();
            app.targets.set_all(!all);
            Task::none()
        }
        Message::ProfileMenu => {
            app.profile_menu = !app.profile_menu;
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
            app.queue_apply();
            Task::none()
        }
        Message::HexChanged(s) => {
            app.hex_input = s;
            // Live-apply when it's a complete valid hex.
            if let Ok(rgb) = hex_to_rgb(&app.hex_input) {
                app.current_color = rgb;
                app.hsl = rgb_to_hsl_f32(rgb.0, rgb.1, rgb.2);
                app.queue_apply();
            }
            Task::none()
        }
        Message::HexSubmit => {
            match hex_to_rgb(&app.hex_input) {
                Ok(rgb) => {
                    app.current_color = rgb;
                    app.hsl = rgb_to_hsl_f32(rgb.0, rgb.1, rgb.2);
                    app.hex_input = rgb_to_hex(rgb);
                    app.queue_apply();
                }
                Err(_) => app.status = format!("Invalid hex: {}", app.hex_input),
            }
            Task::none()
        }
        Message::Slot(slot) if slot <= 3 => {
            match load_profile_slot(slot) {
                Ok(doc) => {
                    app.load_slot_in_place(doc, slot);
                    app.status.clear();
                    // Switching profile shows it on the devices.
                    app.queue_apply();
                }
                Err(e) => app.status = format!("Load failed: {e}"),
            }
            app.profile_menu = false;
            Task::none()
        }
        Message::Slot(_) => Task::none(),
        Message::Apply => {
            app.stop_canvas_stream();
            let doc = app.build_profile_document();
            app.profile = doc.clone();
            let slot = app.active_slot;
            let targets = app.targets;
            // Keyboard-only effects leave the other devices on what they show.
            let pc_rgb = targets.any_pc() && !Fx::Device(app.effect_id).keyboard_only();
            if !targets.keyboard && !pc_rgb {
                // Nothing to write; the Lighting footer says why.
                return Task::none();
            }
            app.apply_busy = true;
            Task::perform(
                async move {
                    tokio::task::spawn_blocking(move || {
                        use cntrl_core::ipc::{self, ApplyOptions};
                        use cntrl_core::rgb_backends::BackendRegistry;

                        let options = ApplyOptions {
                            keyboard: targets.keyboard,
                            pc_rgb,
                        };
                        // Restrict the PC fan-out to exactly the selected backends.
                        BackendRegistry::with_enabled(&targets.pc_env_value(), || {
                            ipc::apply_local_with_options(&doc, slot, options)?;
                            // Only what really reached the devices is restored on boot.
                            cntrl_core::device_state::remember_applied(&doc, options);
                            Ok(())
                        })
                    })
                    .await
                    .unwrap_or_else(|e| Err(format!("Join: {e}")))
                },
                Message::ApplyDone,
            )
        }
        Message::ApplyDone(result) => {
            app.apply_busy = false;
            // Empty = fine: the Lighting footer then says where it is live.
            app.status = result.err().unwrap_or_default();
            Task::none()
        }
        Message::ToggleTarget(kind) => {
            app.targets.toggle(kind);
            Task::none()
        }
        Message::RefreshDevices => {
            app.devices = cntrl_core::devices::enumerate();
            if !app.streaming() {
                app.canvas_layout.mark_present(&core_canvas::absent_backends());
            }
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
            match result {
                Ok(rgb) => {
                    app.status.clear();
                    app.desktop_accent = Some(rgb);
                    app.theme_last_applied = Some(rgb_to_hex(rgb).to_uppercase());
                }
                Err(e) => app.status = format!("Theme sync: {e}"),
            }
            Task::none()
        }
        Message::CanvasTick => {
            // While streaming, follow the stream's clock so the preview shows
            // what the hardware shows.
            let t0 = match &app.canvas_stream {
                Some(stream) => stream.started,
                None if app.canvas_remote => app.canvas_remote_t0,
                None => app.canvas_t0,
            };
            let t = t0.elapsed().as_secs_f32();
            // Surface live stream stats while running.
            if let Some(stream) = &app.canvas_stream {
                let st = stream.status();
                app.canvas_input = st.input();
                app.canvas_status = stream_status_line(
                    "Streaming",
                    st.fps,
                    st.frames,
                    &st.backends,
                    &st.timing,
                    st.last_error.as_deref(),
                );
            }
            // Daemon stream: push edits at most once a tick; poll twice a second,
            // or every tick while the preview has to follow the music.
            app.canvas_ticks = app.canvas_ticks.wrapping_add(1);
            let follow = app.canvas_layout.settings.needs_audio();
            if app.canvas_remote && (app.canvas_dirty || follow || app.canvas_ticks % 15 == 0) {
                app.exchange_with_daemon();
            }
            if !app.streaming() {
                // Nothing is capturing sound: preview audio effects in silence.
                app.canvas_input = Default::default();
            }
            // The screen is not sent over the daemon socket; the preview grabs its own.
            let s = &app.canvas_layout.settings;
            if !s.needs_screen() || app.canvas_screen.as_ref().is_some_and(|(o, _)| *o != s.screen_output) {
                app.canvas_screen = None;
            } else if app.canvas_screen.is_none() && app.canvas_ticks % 30 == 0 {
                // Once a second at most: a failing grab must not spawn 30 processes a second.
                match cntrl_core::screen::ScreenCapture::start(&s.screen_output) {
                    Ok(c) => app.canvas_screen = Some((s.screen_output.clone(), c)),
                    Err(e) => app.canvas_status = e,
                }
            }
            app.canvas_input.screen = app.canvas_screen.as_ref().map(|(_, c)| c.frame());
            app.canvas_preview = canvas_tab::render_preview(&app.canvas_layout, t, &app.canvas_input);
            Task::none()
        }
        Message::CanvasColorSelect(i) => {
            app.select_canvas_color(i);
            Task::none()
        }
        Message::CanvasColorSet((r, g, b), save) => {
            if let Some(c) = app.canvas_layout.settings.palette.get_mut(app.canvas_color_idx) {
                *c = [r, g, b];
            }
            app.canvas_hsl = rgb_to_hsl_f32(r, g, b);
            app.canvas_hex = rgb_to_hex((r, g, b));
            app.sync_stream();
            if save {
                app.save_canvas_layout();
            }
            Task::none()
        }
        Message::CanvasThemeAccent(on) => {
            app.canvas_layout.settings.accent_from_theme = on;
            app.sync_stream();
            app.save_canvas_layout();
            Task::none()
        }
        Message::CanvasEffect(kind) => {
            app.canvas_layout.settings.set_effect(kind);
            app.select_canvas_color(0);
            app.sync_stream();
            app.save_canvas_layout();
            Task::none()
        }
        Message::CanvasSpeed(v) => {
            app.canvas_layout.settings.speed = v;
            app.sync_stream();
            Task::none()
        }
        Message::CanvasScale(v) => {
            app.canvas_layout.settings.scale = v;
            app.sync_stream();
            Task::none()
        }
        Message::CanvasAngle(v) => {
            app.canvas_layout.settings.angle = v;
            app.sync_stream();
            Task::none()
        }
        Message::CanvasBrightness(v) => {
            app.canvas_layout.settings.brightness = v.clamp(0.0, 100.0);
            app.sync_stream();
            Task::none()
        }
        Message::CanvasFps(v) => {
            app.canvas_layout.settings.fps = (v.round() as u32).clamp(5, 60);
            app.sync_stream();
            Task::none()
        }
        Message::CanvasSelect(id) => {
            app.canvas_selected = id;
            app.sync_stream();
            Task::none()
        }
        Message::CanvasMove(id, x, y) => {
            let snap = |v: f32| if app.canvas_snap { (v / 2.0).round() * 2.0 } else { v };
            let (x, y) = (snap(x), snap(y));
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
            let settings = app.canvas_layout.settings.clone();
            app.canvas_layout = CanvasLayout {
                settings,
                ..CanvasLayout::default()
            };
            app.canvas_layout.mark_present(&core_canvas::absent_backends());
            app.canvas_selected = None;
            app.sync_stream();
            app.save_canvas_layout();
            app.canvas_status = "Layout reset".into();
            Task::none()
        }
        Message::CanvasStreamToggle => {
            if app.streaming() {
                app.stop_canvas_stream();
            } else {
                // The daemon owns the devices when it runs, so the effect
                // survives closing this panel; stream in-process without it.
                let reply = cntrl_core::ipc::canvas_request(&cntrl_core::ipc::Request::CanvasStart {
                    layout: app.canvas_layout.clone(),
                });
                if let Ok(None) = reply {
                    app.canvas_stream =
                        Some(cntrl_core::canvas_stream::start(app.canvas_layout.clone()));
                    app.canvas_status = "Stream starting… (no daemon: stops with this window)".into();
                } else {
                    app.canvas_status = "Stream starting…".into();
                    let pending = app.canvas_dirty;
                    app.on_canvas_reply(reply);
                    app.canvas_dirty = pending && app.canvas_remote;
                }
            }
            Task::none()
        }
        Message::CanvasOverlayEffect(effect) => {
            let settings = &mut app.canvas_layout.settings;
            settings.overlay = effect.map(|effect| cntrl_core::canvas::Overlay {
                effect,
                // Keep the blend and opacity already chosen, if any.
                blend: settings.overlay.map_or(cntrl_core::canvas::Blend::Add, |o| o.blend),
                opacity: settings.overlay.map_or(100.0, |o| o.opacity),
            });
            app.sync_stream();
            app.save_canvas_layout();
            Task::none()
        }
        Message::CanvasOverlayBlend(blend) => {
            if let Some(o) = &mut app.canvas_layout.settings.overlay {
                o.blend = blend;
            }
            app.sync_stream();
            app.save_canvas_layout();
            Task::none()
        }
        Message::CanvasOverlayOpacity(v) => {
            if let Some(o) = &mut app.canvas_layout.settings.overlay {
                o.opacity = v.clamp(0.0, 100.0);
            }
            app.sync_stream();
            Task::none()
        }
        Message::CanvasAudioBrightness(on) => {
            app.canvas_layout.settings.audio_brightness = on;
            app.sync_stream();
            app.save_canvas_layout();
            Task::none()
        }
        Message::CanvasAutostart(on) => {
            app.settings.canvas_autostart = on;
            persist_settings(app);
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
        Message::CanvasIdentify => {
            app.canvas_identify = !app.canvas_identify;
            app.sync_stream();
            Task::none()
        }
        Message::CanvasChainStrip(header, leds) => {
            app.canvas_layout.chains.set_strip_leds(header, leds.round() as u8);
            app.canvas_layout.reconcile();
            app.sync_stream();
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
            app.save_canvas_layout();
            Task::none()
        }
        Message::FanPoll => {
            app.refresh_fans();
            Task::none()
        }
        Message::FanToggleEnabled(on) => {
            app.fan_curve_cfg.enabled = on;
            app.save_fan_cfg();
            Task::none()
        }
        Message::FanSelectChannel(id) => {
            app.fan_selected = Some(id);
            Task::none()
        }
        Message::FanCreateAssignment(channel_id, sensor_id) => {
            use cntrl_core::fan_curves::{Curve, CurveKind, CurvePoint, FanAssignment};
            let curve_id = App::curve_id_for(&channel_id);
            // Replace any stale curve/assignment for this channel.
            app.fan_curve_cfg.curves.retain(|c| c.id != curve_id);
            app.fan_curve_cfg
                .assignments
                .retain(|a| a.channel_id != channel_id);
            app.fan_curve_cfg.curves.push(Curve {
                id: curve_id.clone(),
                name: format!("{channel_id} curve"),
                kind: CurveKind::Graph {
                    sensor_id,
                    also_sensor_id: None,
                    points: vec![
                        CurvePoint { temp_c: 30.0, duty_pct: 20 },
                        CurvePoint { temp_c: 50.0, duty_pct: 40 },
                        CurvePoint { temp_c: 70.0, duty_pct: 75 },
                        CurvePoint { temp_c: 85.0, duty_pct: 100 },
                    ],
                },
            });
            app.fan_curve_cfg.assignments.push(FanAssignment {
                channel_id: channel_id.clone(),
                curve_id,
                min_duty_pct: 20,
                max_duty_pct: 100,
                pump: None,
            });
            app.fan_selected = Some(channel_id);
            app.save_fan_cfg();
            Task::none()
        }
        Message::FanSetMode(mode) => {
            app.fan_curve_cfg.mode = mode;
            // First mode ever picked: give every motherboard fan that has no
            // sensor yet a sensible one. CPU cooler and pump follow the CPU;
            // case fans follow the hotter of CPU and graphics card. Fans that
            // already have a sensor are left exactly as they are.
            if !app.fan_curve_cfg.sensors_seeded {
                use cntrl_core::fan_control::FanChannelStatus::Controllable;
                use cntrl_core::fan_curves::CurveKind;
                let gpu = app
                    .fan_discovery
                    .sensors
                    .iter()
                    .find(|s| s.id.starts_with("amdgpu/") && s.label == "edge")
                    .or_else(|| app.fan_discovery.sensors.iter().find(|s| s.chip == "amdgpu"))
                    .map(|s| s.id.clone());
                // (channel, CPU sensor, is a case fan)
                let todo: Vec<(String, String, bool)> = app
                    .fan_discovery
                    .channels
                    .iter()
                    .filter(|c| c.chip != "amdgpu" && c.status == Controllable)
                    .filter(|c| app.assignment_for(&c.id).is_none())
                    .filter_map(|c| {
                        let label = c.label.to_ascii_uppercase();
                        let cooler = c.is_pump() || label == "CPU_FAN" || label == "CPU_OPT";
                        Some((c.id.clone(), default_sensor_for(app, &c.id)?, !cooler))
                    })
                    .collect();
                let keep = app.fan_selected.clone();
                for (channel, cpu, case_fan) in todo {
                    let _ = update_inner(app, Message::FanCreateAssignment(channel.clone(), cpu));
                    let curve_id = App::curve_id_for(&channel);
                    if let Some(c) = app.fan_curve_cfg.curves.iter_mut().find(|c| c.id == curve_id) {
                        if let (true, CurveKind::Graph { also_sensor_id, .. }) = (case_fan, &mut c.kind) {
                            *also_sensor_id = gpu.clone();
                        }
                    }
                }
                app.fan_selected = keep;
                app.fan_curve_cfg.sensors_seeded = true;
            }
            app.save_fan_cfg();
            Task::none()
        }
        Message::FanRemoveAssignment(channel_id) => {
            let curve_id = App::curve_id_for(&channel_id);
            app.fan_curve_cfg.curves.retain(|c| c.id != curve_id);
            app.fan_curve_cfg
                .assignments
                .retain(|a| a.channel_id != channel_id);
            app.save_fan_cfg();
            Task::none()
        }
        Message::FanCurvePointMoved(curve_id, index, temp, duty) => {
            if let Some(points) = graph_points_mut(app, &curve_id) {
                if let Some(p) = points.get_mut(index) {
                    p.temp_c = temp.clamp(0.0, 100.0);
                    p.duty_pct = duty.clamp(0.0, 100.0).round() as u8;
                }
            }
            Task::none()
        }
        Message::FanCurveAddPoint(curve_id, temp, duty) => {
            if let Some(points) = graph_points_mut(app, &curve_id) {
                points.push(cntrl_core::fan_curves::CurvePoint {
                    temp_c: temp.clamp(0.0, 100.0),
                    duty_pct: duty.clamp(0.0, 100.0).round() as u8,
                });
                sort_points(points);
            }
            app.save_fan_cfg();
            Task::none()
        }
        Message::FanCurveDeletePoint(curve_id, index) => {
            if let Some(points) = graph_points_mut(app, &curve_id) {
                // Keep at least two points so the curve stays well-defined.
                if points.len() > 2 && index < points.len() {
                    points.remove(index);
                }
            }
            app.save_fan_cfg();
            Task::none()
        }
        Message::FanCurveEditDone => {
            if let Some(id) = app.fan_selected.clone() {
                let curve_id = App::curve_id_for(&id);
                if let Some(points) = graph_points_mut(app, &curve_id) {
                    sort_points(points);
                }
            }
            app.save_fan_cfg();
            Task::none()
        }
        Message::FanSetSensor(curve_id, sensor_id) => {
            if let Some(c) = app.fan_curve_cfg.curves.iter_mut().find(|c| c.id == curve_id) {
                if let cntrl_core::fan_curves::CurveKind::Graph { sensor_id: s, .. } = &mut c.kind {
                    *s = sensor_id;
                }
            }
            app.save_fan_cfg();
            Task::none()
        }
        Message::FanSetAlsoSensor(curve_id, sensor_id) => {
            if let Some(c) = app.fan_curve_cfg.curves.iter_mut().find(|c| c.id == curve_id) {
                if let cntrl_core::fan_curves::CurveKind::Graph { also_sensor_id: s, .. } = &mut c.kind {
                    *s = sensor_id;
                }
            }
            app.save_fan_cfg();
            Task::none()
        }
        Message::FanSetPump(channel_id, on) => {
            if let Some(a) = app.fan_curve_cfg.assignments.iter_mut().find(|a| a.channel_id == channel_id) {
                a.pump = Some(on);
            }
            app.save_fan_cfg();
            Task::none()
        }
        Message::FanMinDuty(channel_id, v) => {
            if let Some(a) = app
                .fan_curve_cfg
                .assignments
                .iter_mut()
                .find(|a| a.channel_id == channel_id)
            {
                a.min_duty_pct = v.round().clamp(0.0, 100.0) as u8;
                if a.min_duty_pct > a.max_duty_pct {
                    a.max_duty_pct = a.min_duty_pct;
                }
            }
            app.save_fan_cfg();
            Task::none()
        }
    }
}

/// Mutable access to a graph curve's points by curve id.
fn graph_points_mut<'a>(
    app: &'a mut App,
    curve_id: &str,
) -> Option<&'a mut Vec<cntrl_core::fan_curves::CurvePoint>> {
    let c = app.fan_curve_cfg.curves.iter_mut().find(|c| c.id == curve_id)?;
    match &mut c.kind {
        cntrl_core::fan_curves::CurveKind::Graph { points, .. } => Some(points),
        _ => None,
    }
}

fn sort_points(points: &mut [cntrl_core::fan_curves::CurvePoint]) {
    points.sort_by(|a, b| a.temp_c.partial_cmp(&b.temp_c).unwrap_or(std::cmp::Ordering::Equal));
}

/// Mutate the selected canvas device and sync the stream. Trim sliders persist
/// on release (`CanvasDragEnd`); other callers save themselves.
fn with_selected(app: &mut App, f: impl FnOnce(&mut cntrl_core::canvas::PlacedDevice)) {
    if let Some(id) = app.canvas_selected.clone() {
        if let Some(d) = app.canvas_layout.device_mut(&id) {
            f(d);
        }
        app.sync_stream();
    }
}

fn stream_status_line(
    prefix: &str,
    fps: f32,
    frames: u64,
    backends: &str,
    timing: &str,
    last_error: Option<&str>,
) -> String {
    let mut line = format!("{prefix} · {fps:.0} fps · frame {frames}");
    if !backends.is_empty() {
        line.push_str(&format!(" · {backends}"));
    }
    if !timing.is_empty() {
        line.push_str(&format!(" · write {timing}"));
    }
    if let Some(e) = last_error {
        line.push_str(&format!(" · last error: {e}"));
    }
    line
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

// Lucide icons (ISC, see resources/icons/LUCIDE-LICENSE).
const ICON_OVERVIEW: &[u8] = include_bytes!("../../resources/icons/layout-dashboard.svg");
const ICON_POWER: &[u8] = include_bytes!("../../resources/icons/power.svg");
const ICON_LIGHTING: &[u8] = include_bytes!("../../resources/icons/sun.svg");
const ICON_FANS: &[u8] = include_bytes!("../../resources/icons/fan.svg");
const ICON_DEVICES: &[u8] = include_bytes!("../../resources/icons/cpu.svg");
const ICON_SYSTEM: &[u8] = include_bytes!("../../resources/icons/monitor.svg");
const ICON_SETTINGS: &[u8] = include_bytes!("../../resources/icons/settings.svg");
const ICON_CHECK: &[u8] = include_bytes!("../../resources/icons/check.svg");
const ICON_UNFOLD: &[u8] = include_bytes!("../../resources/icons/chevrons-up-down.svg");
const ICON_WARNING: &[u8] = include_bytes!("../../resources/icons/triangle-alert.svg");
const ICON_KEYBOARD: &[u8] = include_bytes!("../../resources/icons/keyboard.svg");
const ICON_BOARD: &[u8] = include_bytes!("../../resources/icons/circuit-board.svg");
const ICON_GPU: &[u8] = include_bytes!("../../resources/icons/gpu.svg");
const ICON_RAM: &[u8] = include_bytes!("../../resources/icons/memory-stick.svg");
const ICON_REFRESH: &[u8] = include_bytes!("../../resources/icons/refresh-cw.svg");
const ICON_CHEVRON_DOWN: &[u8] = include_bytes!("../../resources/icons/chevron-down.svg");
const ICON_CHEVRON_UP: &[u8] = include_bytes!("../../resources/icons/chevron-up.svg");

/// A Lucide icon drawn `size` px square in `color`.
fn lucide(bytes: &'static [u8], size: f32, color: Color) -> Element<'static, Message> {
    iced::widget::svg(iced::widget::svg::Handle::from_memory(bytes))
        .width(size)
        .height(size)
        .style(move |_, _| iced::widget::svg::Style { color: Some(color) })
        .into()
}

/// Filled, optionally outlined box: the base of cards, chips, dots and thumbs.
fn boxed(
    fill: impl Into<iced::Background>,
    border: Color,
    width: f32,
    radius: impl Into<iced::border::Radius>,
) -> impl Fn(&Theme) -> container::Style {
    let (fill, radius) = (fill.into(), radius.into());
    move |_| container::Style {
        background: Some(fill),
        border: iced::Border { radius, width, color: border },
        ..Default::default()
    }
}

/// Small solid shape (status dots, the brand mark, the color chip).
fn shape(color: Color, size: f32, radius: f32) -> Element<'static, Message> {
    container(Space::new().width(size).height(size))
        .style(boxed(color, Color::TRANSPARENT, 0.0, radius))
        .into()
}

/// Flat button look: the redesign's nav items, segments and chips.
fn flat_button(
    fill: Color,
    hover: Color,
    fg: Color,
    border: Color,
    radius: f32,
) -> impl Fn(&Theme, button::Status) -> button::Style {
    move |_, status| button::Style {
        background: Some(
            if matches!(status, button::Status::Hovered) { hover } else { fill }.into(),
        ),
        text_color: fg,
        border: iced::Border {
            radius: radius.into(),
            width: if border == Color::TRANSPARENT { 0.0 } else { 1.0 },
            color: border,
        },
        ..button::Style::default()
    }
}

/// Center a fixed-height button's content vertically.
fn vcenter<'a>(content: impl Into<Element<'a, Message>>) -> Element<'a, Message> {
    container(content).center_y(Length::Fill).into()
}

fn panel_view(app: &App) -> Element<'_, Message> {
    let main: Element<'_, Message> = match app.tab {
        ShellTab::Overview => overview_view(app),
        ShellTab::Lighting => lighting_view(app),
        ShellTab::Canvas => canvas_view(app),
        ShellTab::Fans => fans_view(app),
        ShellTab::Devices => devices_view(app),
        ShellTab::Sync => sync_view(app),
    };
    let main = container(main)
        .padding([32, 40])
        .width(Length::Fill)
        .height(Length::Fill);
    let edge = container(Space::new().width(1).height(Length::Fill))
        .style(boxed(tokens::SIDEBAR_EDGE, Color::TRANSPARENT, 0.0, 0.0));

    container(row![sidebar(app), edge, main])
        .width(Length::Fill)
        .height(Length::Fill)
        .style(app_bg(&app.colors))
        .into()
}

fn sidebar(app: &App) -> Element<'_, Message> {
    let accent = app.accent();

    // Brand mark: a 3×3 grid fading away from the diagonal.
    let mut mark = column![].spacing(3);
    for r in 0..3usize {
        let mut line = row![].spacing(3);
        for c in 0..3usize {
            let a = [1.0, 0.55, 0.25][r.abs_diff(c)];
            line = line.push(shape(Color { a, ..accent }, 6.0, 2.0));
        }
        mark = mark.push(line);
    }
    let brand = container(
        row![mark, text("CNTRL").size(18).font(tokens::UI_BOLD)]
            .spacing(10)
            .align_y(Alignment::Center),
    )
    .padding([4, 8]);

    let mut profile = column![button(vcenter(
        row![
            column![
                text("PROFILE").size(11).color(tokens::TEXT_MUTED),
                text(profile_name(app)).size(14).font(tokens::UI_SEMIBOLD),
            ]
            .spacing(1)
            .width(Length::Fill),
            lucide(ICON_UNFOLD, 16.0, tokens::TEXT_MUTED),
        ]
        .spacing(10)
        .align_y(Alignment::Center),
    ))
    .on_press(Message::ProfileMenu)
    .width(Length::Fill)
    .height(52)
    .padding([0, 12])
    .style(flat_button(
        tokens::SURFACE,
        tokens::HOVER,
        tokens::TEXT,
        tokens::BORDER_PROFILE,
        tokens::RADIUS_CONTROL,
    ))]
    .spacing(2);
    if app.profile_menu {
        // The four keyboard slots; only the loaded one's name is known.
        for slot in 0..4u8 {
            let on = slot == app.active_slot;
            let mut line = row![text(format!("Profile {}", slot + 1)).size(14).width(Length::Fill)]
                .align_y(Alignment::Center);
            if on {
                line = line.push(lucide(ICON_CHECK, 16.0, accent));
            }
            profile = profile.push(
                button(vcenter(line))
                    .on_press(Message::Slot(slot))
                    .width(Length::Fill)
                    .height(40)
                    .padding([0, 12])
                    .style(flat_button(
                        if on { tokens::SELECTED } else { Color::TRANSPARENT },
                        tokens::HOVER,
                        if on { tokens::TEXT } else { tokens::TEXT_NAV },
                        Color::TRANSPARENT,
                        8.0,
                    )),
            );
        }
    }

    let lighting_on = matches!(app.tab, ShellTab::Lighting | ShellTab::Canvas);
    let system = container(
        row![
            lucide(ICON_SYSTEM, 20.0, tokens::TEXT_DISABLED),
            text("System").size(14).color(tokens::TEXT_DISABLED).width(Length::Fill),
            container(text("Soon").size(11).color(tokens::TEXT_MUTED))
                .padding([2, 8])
                .style(boxed(
                    Color::TRANSPARENT,
                    tokens::BORDER_CONTROL,
                    1.0,
                    tokens::RADIUS_PILL,
                )),
        ]
        .spacing(12)
        .align_y(Alignment::Center),
    )
    .center_y(tokens::TARGET)
    .padding([0, 12]);
    let nav = column![
        nav_item(ICON_OVERVIEW, "Overview", app.tab == ShellTab::Overview, accent, ShellTab::Overview),
        nav_item(ICON_LIGHTING, "Lighting", lighting_on, accent, ShellTab::Lighting),
        nav_item(ICON_FANS, "Fans", app.tab == ShellTab::Fans, accent, ShellTab::Fans),
        nav_item(ICON_DEVICES, "Devices", app.tab == ShellTab::Devices, accent, ShellTab::Devices),
        system,
    ]
    .spacing(2);

    use cntrl_core::devices::DeviceStatus;
    let count = |s: DeviceStatus| app.devices.iter().filter(|d| d.status == s).count();
    let (ready, blocked) = (count(DeviceStatus::Ready), count(DeviceStatus::Blocked));
    let engine = if app.engine_running { "engine running" } else { "engine stopped" };
    let (dot, line) = if blocked > 0 {
        (tokens::WARNING, format!("{blocked} blocked · {ready} ready"))
    } else if app.engine_running {
        (tokens::SUCCESS, format!("{ready} devices · {engine}"))
    } else {
        (tokens::TEXT_DISABLED, format!("{ready} devices · {engine}"))
    };
    let status = container(
        row![shape(dot, 8.0, tokens::RADIUS_PILL), text(line).size(12).color(tokens::TEXT_MUTED)]
            .spacing(10)
            .align_y(Alignment::Center),
    )
    .padding([10, 12]);

    container(
        column![
            brand,
            profile,
            nav,
            Space::new().height(Length::Fill),
            column![
                nav_item(ICON_SETTINGS, "Settings", app.tab == ShellTab::Sync, accent, ShellTab::Sync),
                status,
            ]
            .spacing(2),
        ]
        .spacing(20),
    )
    .padding([20, 14])
    .width(232)
    .height(Length::Fill)
    .style(boxed(tokens::SIDEBAR, Color::TRANSPARENT, 0.0, 0.0))
    .into()
}

/// The loaded profile's own name, or its slot number.
fn profile_name(app: &App) -> String {
    let name = app.profile.data.name.trim();
    if name.is_empty() {
        format!("Profile {}", app.active_slot + 1)
    } else {
        name.to_string()
    }
}

fn nav_item<'a>(
    glyph: &'static [u8],
    label: &'a str,
    active: bool,
    accent: Color,
    tab: ShellTab,
) -> Element<'a, Message> {
    let fg = if active { tokens::TEXT } else { tokens::TEXT_NAV };
    button(vcenter(
        row![
            lucide(glyph, 20.0, if active { accent } else { fg }),
            text(label)
                .size(14)
                .font(if active { tokens::UI_SEMIBOLD } else { tokens::UI })
                .color(fg),
        ]
        .spacing(12)
        .align_y(Alignment::Center),
    ))
    .on_press(Message::Tab(tab))
    .width(Length::Fill)
    .height(tokens::TARGET)
    .padding([0, 12])
    .style(flat_button(
        if active { tokens::SELECTED } else { Color::TRANSPARENT },
        if active { tokens::SELECTED } else { tokens::HOVER },
        fg,
        Color::TRANSPARENT,
        8.0,
    ))
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

/// Effects / Layout switch shared by both Lighting views.
fn lighting_tabs(layout: bool) -> Element<'static, Message> {
    let seg = |label: &'static str, on: bool, tab: ShellTab| {
        button(vcenter(
            text(label)
                .size(14)
                .font(if on { tokens::UI_SEMIBOLD } else { tokens::UI_MEDIUM }),
        ))
        .on_press(Message::Tab(tab))
        .height(40)
        .padding([0, 18])
        .style(flat_button(
            if on { tokens::SEGMENT_ON } else { Color::TRANSPARENT },
            if on { tokens::SEGMENT_ON } else { tokens::HOVER },
            if on { tokens::TEXT } else { tokens::TEXT_NAV },
            Color::TRANSPARENT,
            9.0,
        ))
    };
    container(
        row![
            seg("Effects", !layout, ShellTab::Lighting),
            seg("Layout", layout, ShellTab::Canvas),
        ]
        .spacing(4),
    )
    .padding(4)
    .style(boxed(tokens::INSET, Color::TRANSPARENT, 0.0, tokens::RADIUS_TILE))
    .into()
}

/// Page title and one-line explanation, as on every redesigned screen.
fn page_title<'a>(title: &'a str, caption: &'a str) -> iced::widget::Column<'a, Message> {
    column![
        text(title).size(28).font(tokens::UI_SEMIBOLD),
        text(caption).size(14).color(tokens::TEXT_SECONDARY),
    ]
    .spacing(4)
}

fn lighting_header(caption: &'static str, layout: bool) -> Element<'static, Message> {
    row![
        page_title("Lighting", caption),
        Space::new().width(Length::Fill),
        lighting_tabs(layout),
    ]
    .spacing(16)
    .align_y(Alignment::End)
    .into()
}

/// One entry of the merged effect gallery.
#[derive(Debug, Clone, Copy, PartialEq)]
enum Fx {
    /// Runs on the device (profile effect id).
    Device(i32),
    /// Rendered by the background service and streamed to every device.
    Stream(EffectKind),
}

impl Fx {
    /// Static and Off reach every device; the keyboard's own animations and
    /// its per-key Custom mode exist on the keyboard only.
    fn keyboard_only(self) -> bool {
        matches!(self, Fx::Device(id) if id != MODE_STATIC && id != EFFECT_ID_OFF)
    }
}

/// Follows sound or the screen: the gallery's Reactive group.
fn is_reactive(kind: EffectKind) -> bool {
    kind.uses_audio() || kind.uses_screen()
}

fn stream_name(kind: EffectKind) -> &'static str {
    match kind {
        EffectKind::SpectrumBars => "Music",
        EffectKind::ScreenAmbient => "Screen ambient",
        other => other.label(),
    }
}

/// A still of every streamed effect in its default colors, for the gallery.
fn stream_thumbs() -> Vec<(EffectKind, iced::widget::image::Handle)> {
    use cntrl_core::canvas::Frame;
    const W: usize = 80;
    const H: usize = 32;
    // Something to show for the effects that follow sound or the screen.
    let mut input = cntrl_core::canvas_effects::EffectInput::default();
    input.audio.bands = std::array::from_fn(|i| 0.3 + 0.7 * ((i * 7 % 5) as f32 / 4.0));
    input.audio.level = 0.7;
    let mut screen = Frame::new(3, 1);
    screen.px = vec![[0x3b, 0x6b, 0xff], [0xff, 0x8a, 0x3b], [0x2b, 0xd8, 0xa0]];
    input.screen = Some(std::sync::Arc::new(screen));
    EffectKind::ALL
        .into_iter()
        .map(|kind| {
            let mut settings = core_canvas::EffectSettings::default();
            settings.set_effect(kind);
            let mut frame = Frame::new(W, H);
            settings.render_into(&mut frame, 1.3, &input);
            let still = iced::widget::image::Handle::from_rgba(W as u32, H as u32, frame.to_rgba());
            (kind, still)
        })
        .collect()
}

impl App {
    /// The gallery tile that is live right now.
    fn current_fx(&self) -> Fx {
        if self.streaming() {
            Fx::Stream(self.canvas_layout.settings.effect)
        } else {
            Fx::Device(self.effect_id)
        }
    }

    /// Detected device kinds, in chip order, without repeats.
    fn target_kinds(&self) -> Vec<TargetKind> {
        let mut kinds = Vec::new();
        for d in &self.devices {
            let k = TargetKind::from_device_kind(d.kind);
            if !kinds.contains(&k) {
                kinds.push(k);
            }
        }
        kinds
    }

    fn all_targets_on(&self) -> bool {
        self.target_kinds().iter().all(|k| self.targets.get(*k))
    }
}

/// Mix `color` towards the thumbnail dark by the brightness setting.
fn dimmed(color: Color, brightness: f32) -> Color {
    lerp_color(tokens::THUMB_DARK, color, 0.35 + 0.65 * (brightness / 100.0).clamp(0.0, 1.0))
}

/// Tile / preview picture for an on-device effect. `nth` varies the
/// keyboard's animations, which have no artwork of their own.
fn device_thumb(id: i32, nth: usize, color: Color, brightness: f32) -> iced::Background {
    use iced::gradient::Linear;
    let c = dimmed(color, brightness);
    let dark = tokens::THUMB_DARK;
    let linear = |deg: f32, stops: &[(f32, Color)]| {
        let mut g = Linear::new(iced::Radians(deg.to_radians()));
        for (at, color) in stops {
            g = g.add_stop(*at, *color);
        }
        iced::Background::Gradient(g.into())
    };
    match id {
        EFFECT_ID_OFF => tokens::THUMB_OFF.into(),
        MODE_STATIC => c.into(),
        MODE_CUSTOM => linear(
            90.0,
            &[
                (0.0, dimmed(Color::from_rgb8(0xff, 0x3b, 0x3b), brightness)),
                (0.35, dimmed(Color::from_rgb8(0xff, 0xd2, 0x3b), brightness)),
                (0.65, dimmed(Color::from_rgb8(0x3b, 0xd8, 0xff), brightness)),
                (1.0, dimmed(Color::from_rgb8(0x7a, 0x3b, 0xff), brightness)),
            ],
        ),
        _ => {
            let at = 0.25 + 0.5 * ((nth % 4) as f32 / 3.0);
            linear(20.0 + 40.0 * (nth % 9) as f32, &[(0.0, dark), (at, c), (1.0, dark)])
        }
    }
}

/// The picture of `fx`, `height` tall, with the given corner rounding.
fn fx_picture<'a>(
    app: &'a App,
    fx: Fx,
    nth: usize,
    height: f32,
    radius: iced::border::Radius,
    faded: bool,
) -> Element<'a, Message> {
    let lit = Color::from_rgb8(app.current_color.0, app.current_color.1, app.current_color.2);
    match fx {
        Fx::Device(id) => {
            let brightness = if faded { 0.0 } else { f32::from(app.brightness) };
            container(Space::new().width(Length::Fill).height(height))
                .style(boxed(
                    device_thumb(id, nth, lit, brightness),
                    Color::TRANSPARENT,
                    0.0,
                    radius,
                ))
                .into()
        }
        Fx::Stream(kind) => match app.fx_thumbs.iter().find(|(k, _)| *k == kind) {
            Some((_, still)) => iced::widget::image(still.clone())
                .width(Length::Fill)
                .height(height)
                .content_fit(iced::ContentFit::Cover)
                .border_radius(radius)
                .into(),
            None => Space::new().width(Length::Fill).height(height).into(),
        },
    }
}

/// Gallery tile. `enabled: false` dims it and makes it unclickable.
fn fx_tile<'a>(
    app: &'a App,
    fx: Fx,
    nth: usize,
    name: &'a str,
    note: &'a str,
    enabled: bool,
) -> Element<'a, Message> {
    let on = fx == app.current_fx();
    let accent = app.accent();
    let picture = fx_picture(
        app,
        fx,
        nth,
        64.0,
        iced::border::top(tokens::RADIUS_TILE - 2.0),
        !enabled,
    );
    let (title, sub) = if enabled {
        (tokens::TEXT, tokens::TEXT_MUTED)
    } else {
        (tokens::TEXT_DISABLED, tokens::TEXT_DISABLED)
    };
    button(column![
        picture,
        container(
            column![
                text(name).size(14).font(tokens::UI_SEMIBOLD).color(title),
                // Room for two lines whatever the note, so every tile is the same height.
                container(text(note).size(12).color(sub)).height(34),
            ]
            .spacing(2),
        )
        .padding(iced::Padding { top: 10.0, right: 12.0, bottom: 12.0, left: 12.0 }),
    ])
    .on_press_maybe(enabled.then_some(match fx {
        Fx::Device(id) => Message::Effect(id),
        Fx::Stream(kind) => Message::LightingStream(kind),
    }))
    // The 2px selection border is drawn inside the tile.
    .padding(2)
    .width(160)
    .style(move |_, _| button::Style {
        background: Some(tokens::SURFACE.into()),
        text_color: tokens::TEXT,
        border: iced::Border {
            radius: tokens::RADIUS_TILE.into(),
            width: 2.0,
            color: if on { accent } else { tokens::BORDER },
        },
        ..button::Style::default()
    })
    .into()
}

const NOTE_KEYBOARD: &str = "Keyboard only";
const NOTE_STREAM: &str = "All devices · runs in background";

fn lighting_view(app: &App) -> Element<'_, Message> {
    let colors = &app.colors;
    let accent = app.accent();
    let current = app.current_fx();
    let s = &app.canvas_layout.settings;

    // ── Apply to ──
    let mut targets = row![
        text("APPLY TO").size(12).font(tokens::UI_SEMIBOLD).color(tokens::TEXT_SECONDARY),
        Space::new().width(6),
        target_chip("All devices", app.all_targets_on(), accent, Message::ToggleAllTargets),
    ]
    .spacing(10)
    .align_y(Alignment::Center);
    for kind in app.target_kinds() {
        let label = match kind {
            TargetKind::Keyboard => "Keyboard",
            TargetKind::Ite => "Fans & board",
            TargetKind::Gpu => "Graphics card",
            TargetKind::Corsair => "RAM",
        };
        targets = targets.push(target_chip(
            label,
            app.targets.get(kind),
            accent,
            Message::ToggleTarget(kind),
        ));
    }

    // ── Effect gallery ──
    fn group<'a>(
        title: &'a str,
        hint: &'a str,
        tiles: Vec<Element<'a, Message>>,
    ) -> iced::widget::Column<'a, Message> {
        column![
            row![
                text(title).size(15).font(tokens::UI_SEMIBOLD),
                text(hint).size(13).color(tokens::TEXT_MUTED),
            ]
            .spacing(10)
            .align_y(Alignment::End),
            iced::widget::Row::with_children(tiles).spacing(12).wrap().vertical_spacing(12),
        ]
        .spacing(12)
    }
    // Keyboard-only effects are out of reach while the keyboard is deselected.
    let kb = app.targets.keyboard;
    let basic = vec![
        fx_tile(app, Fx::Device(MODE_STATIC), 0, "Static", "", true),
        fx_tile(app, Fx::Device(MODE_CUSTOM), 0, "Custom", NOTE_KEYBOARD, kb),
        fx_tile(app, Fx::Device(EFFECT_ID_OFF), 0, "Off", "", true),
    ];
    // Solid is left out: it is Static with a background service.
    let streamed = |reactive: bool| {
        EffectKind::ALL
            .into_iter()
            .filter(move |k| *k != EffectKind::Solid && is_reactive(*k) == reactive)
            .map(|k| fx_tile(app, Fx::Stream(k), 0, stream_name(k), NOTE_STREAM, true))
    };
    let animated = streamed(false)
        .chain(PRESET_MODES.iter().enumerate().map(|(nth, m)| {
            fx_tile(app, Fx::Device(i32::from(m.effect_id)), nth, m.label, NOTE_KEYBOARD, kb)
        }))
        .collect();
    let gallery = scrollable(
        column![
            group("Basic", "", basic),
            group("Animated", "", animated),
            group("Reactive", "Follows your music or screen", streamed(true).collect()),
        ]
        .spacing(24)
        .padding(iced::Padding { right: 14.0, ..iced::Padding::ZERO }),
    )
    .width(Length::Fill)
    .height(Length::Fill);

    // ── Settings for the chosen effect ──
    let name = match current {
        Fx::Device(id) => effect_name(id),
        Fx::Stream(kind) => stream_name(kind).to_string(),
    };
    let preview = stack![
        fx_picture(app, current, 0, 132.0, tokens::RADIUS_TILE.into(), false),
        container(
            container(text(name).size(13).font(tokens::UI_SEMIBOLD))
                .padding([6, 12])
                .style(boxed(
                    Color::from_rgba8(10, 12, 15, 0.72),
                    Color::TRANSPARENT,
                    0.0,
                    tokens::RADIUS_PILL,
                )),
        )
        .width(Length::Fill)
        .height(132)
        .padding(12)
        .align_x(Alignment::End)
        .align_y(Alignment::End),
    ];

    let note = |why: &'static str| -> Element<'_, Message> {
        container(text(why).size(14).color(tokens::TEXT_NAV))
            .padding(14)
            .width(Length::Fill)
            .style(boxed(tokens::RAISED, Color::TRANSPARENT, 0.0, tokens::RADIUS_CONTROL))
            .into()
    };
    let color_label =
        || text("COLOR").size(12).font(tokens::UI_SEMIBOLD).color(tokens::TEXT_SECONDARY);
    let color_section: Element<'_, Message> = match current {
        Fx::Device(EFFECT_ID_OFF) => note("Lights are off on the selected devices."),
        Fx::Device(_) => column![
            row![
                color_label(),
                Space::new().width(Length::Fill),
                shape(
                    Color::from_rgb8(app.current_color.0, app.current_color.1, app.current_color.2),
                    20.0,
                    6.0
                ),
                hex_field(&app.hex_input, accent, Message::HexChanged, Message::HexSubmit),
            ]
            .spacing(8)
            .align_y(Alignment::Center),
            swatches(Some(app.current_color), Message::Swatch),
            column![
                text("Hue").size(13).color(tokens::TEXT_NAV),
                hue_slider(app.hsl.0, Message::Hue, Message::LightingCommit),
            ]
            .spacing(6),
        ]
        .spacing(12)
        .into(),
        Fx::Stream(kind) if kind.uses_screen() => {
            note("Colors come from whatever is on your screen.")
        }
        Fx::Stream(kind) if kind.controls().colors == 0 => note("This effect uses its own colors."),
        // The effect's own colors: pick one, then change it below.
        Fx::Stream(kind) => {
            let mut chips = row![color_label(), Space::new().width(Length::Fill)]
                .spacing(6)
                .align_y(Alignment::Center);
            for (i, c) in s.palette.iter().take(kind.controls().colors).enumerate() {
                let ring = if i == app.canvas_color_idx { tokens::TEXT } else { Color::TRANSPARENT };
                chips = chips.push(
                    button(shape(Color::from_rgb8(c[0], c[1], c[2]), 24.0, 6.0))
                        .on_press(Message::CanvasColorSelect(i))
                        .padding(3)
                        .style(move |_, _| button::Style {
                            border: iced::Border { radius: 9.0.into(), width: 2.0, color: ring },
                            ..button::Style::default()
                        }),
                );
            }
            let selected = s.palette.get(app.canvas_color_idx).map(|c| (c[0], c[1], c[2]));
            chips = chips.push(Space::new().width(2)).push(hex_field(
                &app.canvas_hex,
                accent,
                Message::CanvasHexChanged,
                Message::CanvasHexSubmit,
            ));
            column![
                chips,
                swatches(selected, |rgb| Message::CanvasColorSet(rgb, true)),
                column![
                    text("Hue").size(13).color(tokens::TEXT_NAV),
                    hue_slider(
                        app.canvas_hsl.0,
                        |h| Message::CanvasColorSet(hsl_to_rgb(h, 1.0, 0.5), false),
                        Message::CanvasDragEnd,
                    ),
                ]
                .spacing(6),
                checkbox(s.accent_from_theme)
                    .label("First color follows the desktop theme")
                    .size(16)
                    .text_size(13)
                    .on_toggle(Message::CanvasThemeAccent),
            ]
            .spacing(12)
            .into()
        }
    };

    // Sliders move the UI as they are dragged; the hardware write (and the
    // save) happens once, on release.
    let mut sliders = column![].spacing(16);
    match current {
        Fx::Stream(kind) => {
            sliders = sliders.push(value_slider(
                "Brightness",
                format!("{:.0}%", s.brightness),
                slider(0.0..=100.0, s.brightness, Message::CanvasBrightness)
                    .on_release(Message::CanvasDragEnd)
                    .style(accent_slider(colors)),
            ));
            if kind.controls().speed {
                sliders = sliders.push(value_slider(
                    "Speed",
                    format!("{:.1}×", s.speed),
                    slider(0.1..=3.0, s.speed, Message::CanvasSpeed)
                        .step(0.1)
                        .on_release(Message::CanvasDragEnd)
                        .style(accent_slider(colors)),
                ));
            }
            if kind.controls().scale {
                sliders = sliders.push(value_slider(
                    "Size",
                    format!("{:.1}×", s.scale),
                    slider(0.3..=3.0, s.scale, Message::CanvasScale)
                        .step(0.1)
                        .on_release(Message::CanvasDragEnd)
                        .style(accent_slider(colors)),
                ));
            }
            if kind.controls().angle {
                sliders = sliders.push(value_slider(
                    "Direction",
                    format!("{:.0}°", s.angle),
                    slider(0.0..=360.0, s.angle, Message::CanvasAngle)
                        .on_release(Message::CanvasDragEnd)
                        .style(accent_slider(colors)),
                ));
            }
        }
        Fx::Device(id) => {
            sliders = sliders.push(value_slider(
                "Brightness",
                format!("{}%", app.brightness),
                slider(0.0..=100.0, f32::from(app.brightness), Message::Brightness)
                    .on_release(Message::LightingCommit)
                    .style(accent_slider(colors)),
            ));
            // Speed only means something to the keyboard's animations.
            if lighting_modes::is_preset_effect_id(id) {
                let speed = app.speed.clamp(1, 5);
                let names = ["Slowest", "Slow", "Medium", "Fast", "Fastest"];
                sliders = sliders.push(value_slider(
                    "Speed",
                    names[usize::from(speed) - 1].to_string(),
                    slider(1.0..=5.0, f32::from(speed), Message::Speed)
                        .on_release(Message::LightingCommit)
                        .style(accent_slider(colors)),
                ));
            }
        }
    }

    let (dot, status) = lighting_status(app, current);
    let footer = column![
        container(Space::new().height(1).width(Length::Fill)).style(boxed(
            tokens::SEGMENT_ON,
            Color::TRANSPARENT,
            0.0,
            0.0
        )),
        row![
            container(shape(dot, 8.0, tokens::RADIUS_PILL)).padding([6, 0]),
            text(status).size(13).color(tokens::TEXT_SECONDARY),
        ]
        .spacing(10),
    ]
    .spacing(16);

    let aside = container(column![preview, color_section, sliders, footer].spacing(22))
        .padding(20)
        .width(340)
        .style(boxed(tokens::SURFACE, tokens::BORDER, 1.0, tokens::RADIUS_CARD));

    column![
        lighting_header(
            "Choose devices, pick an effect, set the color. Changes apply as you go.",
            false,
        ),
        targets,
        row![gallery, aside].spacing(24).height(Length::Fill),
    ]
    .spacing(24)
    .into()
}

/// What the footer of the effect settings says, and its dot color.
fn lighting_status(app: &App, current: Fx) -> (Color, String) {
    if !app.status.is_empty() {
        return (tokens::WARNING, app.status.clone());
    }
    if let Fx::Stream(_) = current {
        return (tokens::SUCCESS, "Live on all devices · runs in the background".into());
    }
    let selected: Vec<TargetKind> =
        app.target_kinds().into_iter().filter(|k| app.targets.get(*k)).collect();
    let saved = format!("saved to {}", profile_name(app));
    if current.keyboard_only() {
        return match (app.targets.keyboard, selected.len()) {
            (false, _) => (
                tokens::TEXT_DISABLED,
                "This effect is keyboard only, and Keyboard is not selected".into(),
            ),
            (true, 1) => (tokens::SUCCESS, format!("Live on the keyboard · {saved}")),
            // Selected, but this effect cannot run there: they are left alone.
            (true, _) => (
                tokens::SUCCESS,
                format!("Live on the keyboard · other devices keep their current effect · {saved}"),
            ),
        };
    }
    match selected.len() {
        0 => (tokens::TEXT_DISABLED, "No devices selected".into()),
        n => (
            tokens::SUCCESS,
            format!("Live on {n} device{} · {saved}", if n == 1 { "" } else { "s" }),
        ),
    }
}

/// "Apply to" chip: outlined pill, accent-ringed with a check when on.
fn target_chip<'a>(label: &'a str, on: bool, accent: Color, msg: Message) -> Element<'a, Message> {
    let mut content = row![].spacing(6).align_y(Alignment::Center);
    if on {
        content = content.push(lucide(ICON_CHECK, 16.0, tokens::TEXT));
    }
    content = content.push(text(label).size(14).font(tokens::UI_MEDIUM));
    button(vcenter(content))
        .on_press(msg)
        .height(40)
        .padding([0, 16])
        .style(flat_button(
            if on { tokens::SEGMENT_ON } else { Color::TRANSPARENT },
            if on { tokens::SEGMENT_ON } else { tokens::HOVER },
            if on { tokens::TEXT } else { tokens::TEXT_SECONDARY },
            if on { accent } else { tokens::BORDER_CONTROL },
            tokens::RADIUS_PILL,
        ))
        .into()
}

/// Label on the left, current value on the right, slider underneath.
fn value_slider<'a>(
    label: &'a str,
    value: String,
    control: impl Into<Element<'a, Message>>,
) -> Element<'a, Message> {
    column![
        row![
            text(label).size(13).color(tokens::TEXT_NAV),
            Space::new().width(Length::Fill),
            text(value).size(13).font(tokens::MONO).color(tokens::TEXT),
        ]
        .align_y(Alignment::Center),
        control.into(),
    ]
    .spacing(6)
    .into()
}

/// Hue picker: a rainbow bar with the slider's handle riding on it.
fn hue_slider<'a>(
    hue: f32,
    on_change: impl Fn(f32) -> Message + 'a,
    on_release: Message,
) -> Element<'a, Message> {
    let mut rainbow = iced::gradient::Linear::new(iced::Radians(std::f32::consts::FRAC_PI_2));
    for (i, rgb) in [0xff0000u32, 0xffff00, 0x00ff00, 0x00ffff, 0x0000ff, 0xff00ff, 0xff0000]
        .into_iter()
        .enumerate()
    {
        let c = Color::from_rgb8((rgb >> 16) as u8, (rgb >> 8) as u8, rgb as u8);
        rainbow = rainbow.add_stop(i as f32 / 6.0, c);
    }
    let bar = container(
        container(Space::new().width(Length::Fill).height(14)).style(boxed(
            iced::Background::Gradient(rainbow.into()),
            Color::TRANSPARENT,
            0.0,
            tokens::RADIUS_PILL,
        )),
    )
    .padding([3, 0]);
    let handle = slider(0.0..=360.0, hue, on_change)
        .on_release(on_release)
        .height(20)
        .style(|theme, status| {
            let mut s = slider::default(theme, status);
            let clear = iced::Background::Color(Color::TRANSPARENT);
            s.rail.backgrounds = (clear, clear);
            s.handle.shape = slider::HandleShape::Circle { radius: 9.0 };
            s.handle.background = tokens::TEXT.into();
            s.handle.border_width = 2.0;
            s.handle.border_color = tokens::SURFACE;
            s
        });
    stack![bar, handle].into()
}

const SWATCH_COLORS: [u32; 16] = [
    0xFF2D2D, 0xFF7A1A, 0xFFC21A, 0xF5FF1A, 0x8CFF1A, 0x1AFF4B, 0x1AFFB0, 0x00C6FF, 0x1A8CFF,
    0x1A2DFF, 0x8C1AFF, 0xE01AFF, 0xFF1AB4, 0xFF1A5A, 0xFFFFFF, 0x88C0D0,
];

/// Two rows of eight; the current color wears a light ring.
fn swatches<'a>(
    current: Option<(u8, u8, u8)>,
    on_pick: impl Fn((u8, u8, u8)) -> Message,
) -> Element<'a, Message> {
    let mut grid = column![].spacing(2);
    for chunk in SWATCH_COLORS.chunks(8) {
        let mut line = row![].spacing(2);
        for &hex in chunk {
            let rgb = ((hex >> 16) as u8, (hex >> 8) as u8, hex as u8);
            let fill = Color::from_rgb8(rgb.0, rgb.1, rgb.2);
            let ring = if Some(rgb) == current { tokens::TEXT } else { Color::TRANSPARENT };
            line = line.push(
                button(
                    container(Space::new().width(Length::Fill).height(26))
                        .style(boxed(fill, Color::TRANSPARENT, 0.0, 6.0)),
                )
                .on_press(on_pick(rgb))
                .padding(3)
                .width(Length::FillPortion(1))
                .style(move |_, _| button::Style {
                    border: iced::Border { radius: 9.0.into(), width: 2.0, color: ring },
                    ..button::Style::default()
                }),
            );
        }
        grid = grid.push(line);
    }
    grid.into()
}

/// Hex color field (type or paste `#rrggbb`).
fn hex_field<'a>(
    value: &'a str,
    accent: Color,
    on_input: impl Fn(String) -> Message + 'a,
    on_submit: Message,
) -> Element<'a, Message> {
    let valid = hex_to_rgb(value).is_ok();
    text_input("#RRGGBB", value)
        .on_input(on_input)
        .on_submit(on_submit)
        .font(tokens::MONO)
        .size(13)
        .padding([9, 10])
        .width(96)
        .style(move |theme, status| {
            let mut s = text_input::default(theme, status);
            s.background = tokens::INSET.into();
            s.value = tokens::TEXT;
            s.placeholder = tokens::TEXT_MUTED;
            s.selection = Color { a: 0.4, ..accent };
            s.border = iced::Border {
                radius: 8.0.into(),
                width: 1.0,
                color: if valid { tokens::BORDER_CONTROL } else { tokens::WARNING },
            };
            s
        })
        .into()
}

/// Compact square button used for −/+ steppers on the Layout tab.
fn stepper<'a>(label: &'a str, msg: Option<Message>) -> Element<'a, Message> {
    button(container(text(label).size(16)).center(Length::Fill))
        .on_press_maybe(msg)
        .width(36)
        .height(36)
        .padding(0)
        .style(flat_button(tokens::SURFACE, tokens::HOVER, tokens::TEXT, tokens::BORDER_CONTROL, 8.0))
        .into()
}

/// Where a layout device is plugged in, in words.
fn layout_via(id: &str) -> String {
    if id == core_canvas::DEV_KEYBOARD {
        "MCHOSE Ace68-II · USB".into()
    } else if id == core_canvas::DEV_GPU {
        "Graphics card · single zone".into()
    } else if id == core_canvas::DEV_RGB12V {
        "Motherboard · 12 V header".into()
    } else if let Some(n) = id.strip_prefix("ram") {
        format!("Corsair · module {}", n.parse::<u32>().map_or(0, |n| n + 1))
    } else if let Some(n) = id.strip_prefix("mbzone") {
        format!("Motherboard · board zone {n}")
    } else if id.starts_with("dled1") {
        "Motherboard · ARGB header 1".into()
    } else {
        "Motherboard · ARGB header 2".into()
    }
}

/// What is daisy-chained on one ARGB header: fans, LEDs per fan, a strip.
fn chain_box<'a>(app: &'a App, header: &'static str, title: &'a str) -> Element<'a, Message> {
    use cntrl_core::canvas::SegmentKind;
    let accent = app.accent();
    let segs = app.canvas_layout.chains.header(header);
    let fans = segs.iter().filter(|s| s.kind == SegmentKind::Fan).count() as u8;
    let fan_leds = segs.iter().find(|s| s.kind == SegmentKind::Fan).map(|s| s.leds);
    let strip = app.canvas_layout.chains.strip_leds(header);
    let total: u32 = segs.iter().map(|s| u32::from(s.leds)).sum();

    let mut rows = column![
        text(title).size(14).font(tokens::UI_SEMIBOLD),
        row![
            text("Fans on this header").size(14).color(tokens::TEXT_NAV).width(Length::Fill),
            stepper("−", (fans > 0).then(|| Message::CanvasChainFans(header, fans - 1))),
            container(text(fans.to_string()).size(14).font(tokens::MONO)).center_x(32),
            stepper("+", (fans < 8).then(|| Message::CanvasChainFans(header, fans + 1))),
        ]
        .spacing(4)
        .align_y(Alignment::Center),
    ]
    .spacing(14);
    if let Some(current) = fan_leds {
        let mut per_fan = row![text("LEDs per fan").size(14).color(tokens::TEXT_NAV).width(Length::Fill)]
            .spacing(4)
            .align_y(Alignment::Center);
        for leds in [8u8, 12, 16] {
            let on = current == leds;
            per_fan = per_fan.push(
                button(vcenter(text(leds.to_string()).size(14)))
                    .on_press(Message::CanvasChainFanLeds(header, leds))
                    .height(36)
                    .padding([0, 12])
                    .style(flat_button(
                        if on { tokens::SURFACE } else { Color::TRANSPARENT },
                        tokens::HOVER,
                        if on { tokens::TEXT } else { tokens::TEXT_SECONDARY },
                        if on { accent } else { tokens::BORDER_CONTROL },
                        8.0,
                    )),
            );
        }
        rows = rows.push(per_fan);
    }
    rows = rows.push(value_slider(
        "Strip after the fans",
        if strip == 0 { "none".to_string() } else { format!("{strip} LEDs") },
        slider(0.0..=120.0, f32::from(strip), move |v| Message::CanvasChainStrip(header, v))
            .on_release(Message::CanvasDragEnd)
            .style(accent_slider(&app.colors)),
    ));
    rows = rows.push(
        text(match (total, strip) {
            (0, _) => "Nothing connected".to_string(),
            (n, 0) => format!("{n} LEDs in total · no strip after the fans"),
            (n, _) => format!("{n} LEDs in total"),
        })
        .size(13)
        .color(tokens::TEXT_SECONDARY),
    );
    container(rows)
        .padding(16)
        .width(Length::Fill)
        .style(boxed(tokens::RAISED, Color::TRANSPARENT, 0.0, tokens::RADIUS_TILE))
        .into()
}

/// Small outlined button of the Layout tab (pressed look when `on`).
fn tool_button<'a>(label: &'a str, on: bool, msg: Message) -> iced::widget::Button<'a, Message> {
    button(vcenter(text(label).size(14)))
        .on_press(msg)
        .height(40)
        .padding([0, 14])
        .style(flat_button(
            if on { tokens::RAISED } else { Color::TRANSPARENT },
            tokens::HOVER,
            if on { tokens::TEXT } else { tokens::TEXT_NAV },
            tokens::BORDER_CONTROL,
            9.0,
        ))
}

/// Left side of the Layout tab: the canvas card over the stream controls.
fn layout_canvas_column(app: &App, canvas_height: f32) -> Element<'_, Message> {
    let colors = &app.colors;
    let accent = app.accent();
    let s = &app.canvas_layout.settings;
    let streaming = app.streaming();
    let rule = || {
        container(Space::new().height(1).width(Length::Fill)).style(boxed(
            tokens::BORDER,
            Color::TRANSPARENT,
            0.0,
            0.0,
        ))
    };

    // ── the canvas card: toolbar, scene, hint ──
    let toolbar = row![
        button(vcenter(
            row![
                shape(
                    if app.canvas_live_preview { tokens::SUCCESS } else { Color::from_rgb8(0x5A, 0x62, 0x6C) },
                    10.0,
                    tokens::RADIUS_PILL
                ),
                text("Live preview").size(14),
            ]
            .spacing(8)
            .align_y(Alignment::Center),
        ))
        .on_press(Message::CanvasLivePreview)
        .height(40)
        .padding([0, 14])
        .style(flat_button(tokens::RAISED, tokens::SEGMENT_ON, tokens::TEXT, tokens::BORDER_CONTROL, 9.0)),
        tool_button("Snap to grid", app.canvas_snap, Message::CanvasSnap),
        Space::new().width(Length::Fill),
        button(vcenter(text("Reset layout").size(14)))
            .on_press(Message::CanvasResetLayout)
            .height(40)
            .padding([0, 14])
            .style(flat_button(Color::TRANSPARENT, tokens::HOVER, tokens::TEXT_NAV, Color::TRANSPARENT, 9.0)),
    ]
    .spacing(8)
    .align_y(Alignment::Center);
    let scene = container(canvas_tab::scene_element(
        &app.canvas_layout,
        app.canvas_live_preview.then_some(&app.canvas_preview),
        app.canvas_selected.as_deref(),
        colors,
    ))
    .padding(16)
    .width(Length::Fill)
    .height(Length::Fill)
    .style(boxed(Color::from_rgb8(0x15, 0x18, 0x1C), Color::TRANSPARENT, 0.0, 0.0));
    let canvas_card = container(column![
        container(toolbar).padding([12, 14]),
        rule(),
        scene,
        rule(),
        container(
            text("Drag to move · click to select · devices take the color of the effect under them.")
                .size(13)
                .color(tokens::TEXT_SECONDARY),
        )
        .padding([12, 16]),
    ])
    .width(Length::Fill)
    .style(boxed(tokens::SURFACE, tokens::BORDER, 1.0, tokens::RADIUS_CARD));

    // ── which devices take part (switched-off ones are not on the canvas) ──
    let mut chips = Vec::new();
    for (i, id, spec, enabled) in canvas_tab::placed_specs(&app.canvas_layout) {
        let label = spec.name.replace("ARGB Header ", "Header ");
        chips.push(
            button(vcenter(
                row![shape(canvas_tab::device_color(i), 8.0, tokens::RADIUS_PILL), text(label).size(13)]
                    .spacing(8)
                    .align_y(Alignment::Center),
            ))
            .on_press(Message::CanvasToggleEnabled(id))
            .height(36)
            .padding([0, 12])
            .style(flat_button(
                if enabled { tokens::SEGMENT_ON } else { Color::TRANSPARENT },
                tokens::HOVER,
                if enabled { tokens::TEXT } else { tokens::TEXT_SECONDARY },
                if enabled { accent } else { tokens::BORDER_CONTROL },
                tokens::RADIUS_PILL,
            ))
            .into(),
        );
    }
    let devices_row: Element<'_, Message> = if chips.is_empty() {
        text("No supported RGB devices detected. See Devices for why.")
            .size(13)
            .color(tokens::TEXT_SECONDARY)
            .into()
    } else {
        iced::widget::Row::with_children(chips).spacing(8).wrap().vertical_spacing(8).into()
    };

    // ── the background stream that runs animated and reactive effects ──
    let mut layer = row![
        text("Second effect on top").size(13).color(tokens::TEXT_NAV),
        pick_list(EffectKind::ALL, s.overlay.map(|o| o.effect), |e| {
            Message::CanvasOverlayEffect(Some(e))
        })
        .placeholder("none")
        .text_size(13)
        .padding([8, 12])
        .style(select_style),
    ]
    .spacing(10)
    .align_y(Alignment::Center);
    if let Some(overlay) = s.overlay {
        layer = layer.push(tool_button("Remove", false, Message::CanvasOverlayEffect(None)));
        for blend in cntrl_core::canvas::Blend::ALL {
            layer = layer.push(tool_button(blend.label(), overlay.blend == blend, Message::CanvasOverlayBlend(blend)));
        }
        layer = layer.push(
            container(value_slider(
                "Opacity",
                format!("{:.0}%", overlay.opacity),
                slider(0.0..=100.0, overlay.opacity, Message::CanvasOverlayOpacity)
                    .on_release(Message::CanvasDragEnd)
                    .style(accent_slider(colors)),
            ))
            .width(170),
        );
    }
    let stream_card = container(
        column![
            row![
                caps("DEVICES ON THE LAYOUT"),
                Space::new().width(Length::Fill),
                text(app.canvas_status.clone()).size(12).color(tokens::TEXT_MUTED),
            ]
            .spacing(12)
            .align_y(Alignment::Center),
            devices_row,
            rule(),
            row![
                column![
                    caps("BACKGROUND STREAM"),
                    text(if streaming {
                        format!("Running {}", stream_name(s.effect))
                    } else {
                        "Stopped. Pick an animated or reactive effect to start it.".to_string()
                    })
                    .size(13)
                    .color(tokens::TEXT_SECONDARY),
                ]
                .spacing(4)
                .width(Length::Fill),
                text("Start at login").size(13).color(tokens::TEXT_NAV),
                switch(
                    app.settings.canvas_autostart,
                    accent,
                    Some(Message::CanvasAutostart(!app.settings.canvas_autostart)),
                ),
                tool_button("Push to keyboard", false, Message::CanvasPushKeyboard),
                button(vcenter(
                    text(if streaming { "Stop" } else { "Start" }).size(14).font(tokens::UI_SEMIBOLD)
                ))
                .on_press(Message::CanvasStreamToggle)
                .height(40)
                .padding([0, 18])
                .style(flat_button(
                    accent,
                    lerp_color(accent, Color::WHITE, 0.12),
                    tokens::ON_ACCENT,
                    Color::TRANSPARENT,
                    tokens::RADIUS_CONTROL,
                )),
            ]
            .spacing(12)
            .align_y(Alignment::Center),
            row![
                container(value_slider(
                    "Frames per second",
                    s.fps.to_string(),
                    slider(5.0..=60.0, s.fps as f32, Message::CanvasFps)
                        .on_release(Message::CanvasDragEnd)
                        .style(accent_slider(colors)),
                ))
                .width(220),
                layer.wrap(),
            ]
            .spacing(16)
            .align_y(Alignment::Center),
            row![
                text("Brightness follows the music").size(13).color(tokens::TEXT_NAV).width(Length::Fill),
                switch(s.audio_brightness, accent, Some(Message::CanvasAudioBrightness(!s.audio_brightness))),
            ]
            .spacing(16)
            .align_y(Alignment::Center),
        ]
        .spacing(14),
    )
    .padding(20)
    .width(Length::Fill)
    .style(boxed(tokens::SURFACE, tokens::BORDER, 1.0, tokens::RADIUS_CARD));

    scrollable(column![canvas_card.height(canvas_height), stream_card].spacing(16)).into()
}

fn canvas_view(app: &App) -> Element<'_, Message> {
    let colors = &app.colors;
    let streaming = app.streaming();
    // The canvas takes the height the controls leave, but never shrinks to
    // nothing in a short window: below that the column scrolls.
    let left = iced::widget::responsive(move |size| {
        const CONTROLS: f32 = 330.0;
        layout_canvas_column(app, (size.height - CONTROLS).max(360.0))
    });

    // ── the selected device ──
    let selected = app
        .canvas_selected
        .as_deref()
        .and_then(|id| Some((id, app.canvas_layout.device(id)?, app.canvas_layout.spec(id)?)));
    let mut aside = column![].spacing(20);
    match selected {
        None => {
            aside = aside
                .push(
                    column![
                        caps("SELECTED"),
                        text("Nothing").size(20).font(tokens::UI_SEMIBOLD),
                        text("Click a device on the canvas to place and tune it.")
                            .size(13)
                            .color(tokens::TEXT_SECONDARY),
                    ]
                    .spacing(2),
                )
                .push(chain_box(app, core_canvas::CHAIN_DLED1, "ARGB header 1"))
                .push(chain_box(app, core_canvas::CHAIN_DLED2, "ARGB header 2"));
        }
        Some((id, d, spec)) => {
            let coord = |label: &'static str, v: f32| {
                column![
                    text(label).size(13).color(tokens::TEXT_NAV),
                    container(text(format!("{v:.0}")).size(14).font(tokens::MONO))
                        .center_y(40)
                        .padding([0, 12])
                        .width(Length::Fill)
                        .style(boxed(tokens::INSET, tokens::BORDER_CONTROL, 1.0, 8.0)),
                ]
                .spacing(6)
                .width(Length::FillPortion(1))
            };
            let turned = d.placement.rotation_deg.rem_euclid(360.0);
            let mut rotation = row![].spacing(4);
            for deg in [0.0f32, 90.0, 180.0, 270.0] {
                let on = (turned - deg).abs() < 1.0;
                rotation = rotation.push(
                    button(container(text(format!("{deg:.0}°")).size(14)).center(Length::Fill))
                        .on_press(Message::CanvasSetRotation(deg))
                        .width(Length::FillPortion(1))
                        .height(36)
                        .padding(0)
                        .style(flat_button(
                            if on { tokens::SEGMENT_ON } else { Color::TRANSPARENT },
                            tokens::HOVER,
                            if on { tokens::TEXT } else { tokens::TEXT_SECONDARY },
                            Color::TRANSPARENT,
                            7.0,
                        )),
                );
            }
            aside = aside
                .push(
                    column![
                        caps("SELECTED"),
                        text(spec.name.clone()).size(20).font(tokens::UI_SEMIBOLD),
                        text(layout_via(id)).size(13).color(tokens::TEXT_SECONDARY),
                    ]
                    .spacing(2),
                )
                .push(row![coord("X", d.placement.x), coord("Y", d.placement.y)].spacing(12))
                .push(
                    column![
                        text("Rotation").size(13).color(tokens::TEXT_NAV),
                        container(rotation)
                            .padding(4)
                            .style(boxed(tokens::INSET, Color::TRANSPARENT, 0.0, tokens::RADIUS_CONTROL)),
                    ]
                    .spacing(6),
                );
            if id.starts_with("dled") {
                let (header, title) = if id.starts_with("dled1") {
                    (core_canvas::CHAIN_DLED1, "Fan chain on header 1")
                } else {
                    (core_canvas::CHAIN_DLED2, "Fan chain on header 2")
                };
                aside = aside.push(chain_box(app, header, title));
            } else {
                aside = aside.push(
                    container(row![
                        text("LEDs").size(14).color(tokens::TEXT_NAV).width(Length::Fill),
                        text(spec.led_positions.len().to_string()).size(14).font(tokens::MONO),
                    ])
                    .padding(16)
                    .style(boxed(tokens::RAISED, Color::TRANSPARENT, 0.0, tokens::RADIUS_TILE)),
                );
            }
            // Not in the mockup, but needed to match real hardware: size on the
            // canvas, wiring direction, and per-device color trim.
            let mut fit = row![
                text("Size").size(14).color(tokens::TEXT_NAV).width(Length::Fill),
                stepper("−", Some(Message::CanvasScaleSelected(1.0 / 1.15))),
                stepper("+", Some(Message::CanvasScaleSelected(1.15))),
                tool_button("Reversed", d.reversed, Message::CanvasDevReverse),
            ]
            .spacing(4)
            .align_y(Alignment::Center);
            if streaming {
                fit = fit.push(tool_button("Identify", app.canvas_identify, Message::CanvasIdentify));
            }
            let trim = |label: &'static str, value: String, range, v: f32, msg: fn(f32) -> Message| {
                value_slider(
                    label,
                    value,
                    slider(range, v, msg).on_release(Message::CanvasDragEnd).style(accent_slider(colors)),
                )
            };
            aside = aside
                .push(fit)
                .push(trim("Brightness", format!("{:.0}%", d.brightness), 0.0..=100.0, d.brightness, Message::CanvasDevBrightness))
                .push(trim("Hue shift", format!("{:.0}°", d.hue_shift), 0.0..=360.0, d.hue_shift, Message::CanvasDevHue))
                .push(trim("Saturation", format!("{:.0}%", d.saturation), 0.0..=200.0, d.saturation, Message::CanvasDevSaturation));
        }
    }
    aside = aside.push(
        text("Basic effects (Static, Off) don’t need a layout. Animated and Reactive effects use it.")
            .size(13)
            .color(tokens::TEXT_SECONDARY),
    );
    let aside = container(scrollable(aside.padding(iced::Padding { right: 12.0, ..iced::Padding::ZERO })))
        .padding(20)
        .width(340)
        .height(Length::Fill)
        .style(boxed(tokens::SURFACE, tokens::BORDER, 1.0, tokens::RADIUS_CARD));

    column![
        lighting_header(
            "Place your devices the way they sit in your case, so animated effects flow across them in order.",
            true,
        ),
        row![left, aside].spacing(24).height(Length::Fill),
    ]
    .spacing(24)
    .into()
}

/// Text link in the mockups' link color ("Change", "Manage").
fn link<'a>(label: &'a str, tab: ShellTab) -> Element<'a, Message> {
    button(text(label).size(14).font(tokens::UI_MEDIUM))
        .on_press(Message::Tab(tab))
        .padding(0)
        .style(|_, status| button::Style {
            text_color: if matches!(status, button::Status::Hovered) {
                Color::from_rgb8(0xC4, 0xEE, 0xFB)
            } else {
                Color::from_rgb8(0x8F, 0xDD, 0xF7)
            },
            ..button::Style::default()
        })
        .into()
}

/// Small uppercase heading of a card or section.
fn caps<'a>(label: &'a str) -> iced::widget::Text<'a> {
    text(label).size(12).font(tokens::UI_SEMIBOLD).color(tokens::TEXT_SECONDARY)
}

/// One-line description of a device for the Overview cards.
fn device_detail(app: &App, kind: TargetKind) -> String {
    use cntrl_core::canvas::SegmentKind;
    let ids = layout_ids(app, kind);
    let plural = |n: usize, one: &str| format!("{n} {one}{}", if n == 1 { "" } else { "s" });
    match kind {
        TargetKind::Keyboard => {
            let keys = app
                .canvas_layout
                .spec(core_canvas::DEV_KEYBOARD)
                .map_or(0, |s| s.led_positions.len());
            format!("{keys} keys · per-key RGB")
        }
        TargetKind::Gpu => "1 zone".into(),
        TargetKind::Corsair => plural(ids.len(), "module"),
        TargetKind::Ite => {
            let chains = &app.canvas_layout.chains;
            let fans = [core_canvas::CHAIN_DLED1, core_canvas::CHAIN_DLED2]
                .into_iter()
                .flat_map(|h| chains.header(h))
                .filter(|s| s.kind == SegmentKind::Fan)
                .count();
            let zones = ids.iter().filter(|id| id.starts_with("mbzone")).count();
            format!("{} · {}", plural(fans, "fan"), plural(zones, "board zone"))
        }
    }
}

fn overview_view(app: &App) -> Element<'_, Message> {
    use cntrl_core::devices::DeviceStatus;
    use cntrl_core::fan_curves::FanMode;
    let accent = app.accent();
    let card_style = || boxed(tokens::SURFACE, tokens::BORDER, 1.0, tokens::RADIUS_CARD);

    let header = row![
        page_title("Overview", "What CNTRL is controlling right now."),
        Space::new().width(Length::Fill),
        outline_button(
            row![
                lucide(ICON_POWER, 16.0, tokens::TEXT),
                text("All lights off").size(14).font(tokens::UI_MEDIUM),
            ]
            .spacing(8)
            .align_y(Alignment::Center),
            Message::AllLightsOff,
        ),
    ]
    .spacing(16)
    .align_y(Alignment::End);

    // ── lighting now ──
    let current = app.current_fx();
    let (name, facts) = match current {
        Fx::Stream(kind) => (
            stream_name(kind).to_string(),
            format!(
                "runs in the background · {:.0}% brightness",
                app.canvas_layout.settings.brightness
            ),
        ),
        Fx::Device(EFFECT_ID_OFF) => ("Off".to_string(), "lights are off".to_string()),
        Fx::Device(id) => (
            effect_name(id),
            format!("{} · {}% brightness", rgb_to_hex(app.current_color).to_uppercase(), app.brightness),
        ),
    };
    // The swatch sits in a ring: a gap in the sidebar color, then a hairline.
    let swatch = container(
        container(fx_picture(app, current, 0, 72.0, 18.0.into(), false))
            .width(72)
            .padding(0),
    )
    .padding(6)
    .style(boxed(tokens::SIDEBAR, tokens::BORDER_CONTROL, 1.0, 24.0));
    let mut where_live = Vec::new();
    for kind in app.target_kinds() {
        let lit = match current {
            Fx::Stream(_) => !layout_ids(app, kind).is_empty(),
            fx => app.targets.get(kind) && (!fx.keyboard_only() || kind == TargetKind::Keyboard),
        };
        let label = match kind {
            TargetKind::Keyboard => "Keyboard",
            TargetKind::Ite => "Fans & board",
            TargetKind::Gpu => "Graphics card",
            TargetKind::Corsair => "RAM",
        };
        where_live.push(
            container(
                row![
                    shape(if lit { accent } else { tokens::TEXT_DISABLED }, 8.0, tokens::RADIUS_PILL),
                    text(label).size(13).color(if lit { tokens::TEXT } else { tokens::TEXT_MUTED }),
                ]
                .spacing(8)
                .align_y(Alignment::Center),
            )
            .padding([6, 12])
            .style(boxed(tokens::RAISED, Color::TRANSPARENT, 0.0, tokens::RADIUS_PILL))
            .into(),
        );
    }
    let lighting = container(
        column![
            row![caps("LIGHTING"), Space::new().width(Length::Fill), link("Change", ShellTab::Lighting)]
                .align_y(Alignment::Center),
            row![
                swatch,
                column![
                    text(name).size(26).font(tokens::UI_SEMIBOLD),
                    text(facts).size(13).font(tokens::MONO).color(tokens::TEXT_SECONDARY),
                ]
                .spacing(2),
            ]
            .spacing(20)
            .align_y(Alignment::Center),
            iced::widget::Row::with_children(where_live).spacing(8).wrap().vertical_spacing(8),
        ]
        .spacing(22),
    )
    .padding(24)
    .width(Length::FillPortion(1))
    .style(card_style());

    // ── cooling now ──
    let cfg = &app.fan_curve_cfg;
    let mut modes = row![].spacing(4);
    for mode in FanMode::ALL {
        let on = cfg.mode == mode && cfg.enabled;
        modes = modes.push(
            button(container(text(mode.label()).size(14).font(tokens::UI_MEDIUM)).center(Length::Fill))
                .on_press(Message::FanSetMode(mode))
                .width(Length::FillPortion(1))
                .height(tokens::TARGET)
                .padding(0)
                .style(flat_button(
                    if on { accent } else { Color::TRANSPARENT },
                    if on { accent } else { tokens::HOVER },
                    if on { tokens::ON_ACCENT } else { tokens::TEXT_NAV },
                    Color::TRANSPARENT,
                    9.0,
                )),
        );
    }
    let sensors = &app.fan_discovery.sensors;
    let temp = |name: &str| {
        sensors
            .iter()
            .find(|s| sensor_name(s) == name)
            .and_then(|s| app.fan_readings.temps_c.get(&s.id))
            .map_or("—".to_string(), |t| format!("{t:.0}°C"))
    };
    // Third figure: the pump if there is one, else the CPU fan.
    let chans = &app.fan_discovery.channels;
    let spinner = chans
        .iter()
        .find(|c| app.assignment_for(&c.id).map_or(c.is_pump(), |a| a.is_pump(c)))
        .or_else(|| chans.iter().find(|c| c.label.eq_ignore_ascii_case("CPU_FAN")))
        .or_else(|| chans.iter().find(|c| c.chip != "amdgpu"));
    let stat = |label: String, value: String| {
        column![
            text(label).size(12).color(tokens::TEXT_SECONDARY),
            text(value).size(24).font(tokens::MONO),
        ]
        .spacing(2)
        .width(Length::FillPortion(1))
    };
    let mut stats = row![stat("CPU".into(), temp("CPU")), stat("GPU".into(), temp("Graphics card"))]
        .spacing(12);
    if let Some(ch) = spinner {
        let rpm = app.fan_readings.rpm.get(&ch.id).map_or("—".to_string(), |r| format!("{r} rpm"));
        stats = stats.push(stat(fan_name(app, ch), rpm));
    }
    let mut cooling_body = column![
        row![caps("COOLING"), Space::new().width(Length::Fill), link("Fine-tune", ShellTab::Fans)]
            .align_y(Alignment::Center),
        container(modes)
            .padding(4)
            .style(boxed(tokens::INSET, Color::TRANSPARENT, 0.0, tokens::RADIUS_TILE)),
        stats,
    ]
    .spacing(22);
    if !cfg.enabled {
        cooling_body = cooling_body.push(
            text("Your BIOS is controlling the fans. Pick a mode to let CNTRL do it.")
                .size(13)
                .color(tokens::TEXT_SECONDARY),
        );
    }
    let cooling = container(cooling_body)
        .padding(24)
        .width(Length::FillPortion(1))
        .style(card_style());

    // ── devices ──
    let mut devices = row![].spacing(16);
    for d in &app.devices {
        let kind = TargetKind::from_device_kind(d.kind);
        let what = match kind {
            TargetKind::Keyboard => "Keyboard",
            TargetKind::Ite => "Motherboard",
            TargetKind::Gpu => "Graphics card",
            TargetKind::Corsair => "Memory",
        };
        let (dot, tone) = match d.status {
            DeviceStatus::Ready => (tokens::SUCCESS, Color::from_rgb8(0x7D, 0xD8, 0xA9)),
            DeviceStatus::Blocked => (tokens::WARNING, tokens::WARNING),
            DeviceStatus::Absent => (tokens::TEXT_DISABLED, tokens::TEXT_MUTED),
        };
        devices = devices.push(
            button(
                column![
                    row![
                        text(what).size(12).color(tokens::TEXT_SECONDARY),
                        Space::new().width(Length::Fill),
                        row![shape(dot, 6.0, tokens::RADIUS_PILL), text(d.status.label()).size(12).color(tone)]
                            .spacing(6)
                            .align_y(Alignment::Center),
                    ]
                    .align_y(Alignment::Center),
                    column![
                        text(d.name.clone()).size(16).font(tokens::UI_SEMIBOLD),
                        text(device_detail(app, kind)).size(13).color(tokens::TEXT_SECONDARY),
                    ]
                    .spacing(2),
                    container(Space::new().width(Length::Fill).height(6)).style(boxed(
                        Color { a: 0.85, ..accent },
                        Color::TRANSPARENT,
                        0.0,
                        tokens::RADIUS_PILL,
                    )),
                ]
                .spacing(14),
            )
            .on_press(Message::Tab(ShellTab::Devices))
            .width(Length::FillPortion(1))
            .padding(18)
            .style(flat_button(tokens::SURFACE, tokens::HOVER, tokens::TEXT, tokens::BORDER, 14.0)),
        );
    }
    let device_section = column![
        row![caps("DEVICES"), Space::new().width(Length::Fill), link("Manage", ShellTab::Devices)]
            .align_y(Alignment::Center),
        devices,
    ]
    .spacing(14);

    column![
        header,
        scrollable(column![row![lighting, cooling].spacing(20), device_section].spacing(28))
            .height(Length::Fill),
    ]
    .spacing(28)
    .into()
}

/// On/off switch from the handoff (52×32). `None` draws it disabled.
fn switch<'a>(on: bool, accent: Color, msg: Option<Message>) -> Element<'a, Message> {
    let live = msg.is_some();
    let knob = if on { tokens::ON_ACCENT } else { Color::from_rgb8(0xC9, 0xCF, 0xD6) };
    let track = if on { accent } else { Color::from_rgb8(0x3A, 0x41, 0x4A) };
    let fade = |c: Color| if live { c } else { Color { a: 0.35, ..c } };
    button(
        container(shape(fade(knob), 24.0, tokens::RADIUS_PILL))
            .width(52)
            .height(32)
            .padding(4)
            .align_x(if on { Alignment::End } else { Alignment::Start })
            .style(boxed(fade(track), Color::TRANSPARENT, 0.0, tokens::RADIUS_PILL)),
    )
    .on_press_maybe(msg)
    .padding(0)
    .style(|_, _| button::Style::default())
    .into()
}

/// Outlined button used for secondary actions on the redesigned screens.
fn outline_button<'a>(
    content: impl Into<Element<'a, Message>>,
    msg: Message,
) -> iced::widget::Button<'a, Message> {
    button(vcenter(content))
        .on_press(msg)
        .height(tokens::TARGET)
        .padding([0, 18])
        .style(flat_button(
            tokens::SURFACE,
            tokens::HOVER,
            tokens::TEXT,
            tokens::BORDER_CONTROL,
            tokens::RADIUS_CONTROL,
        ))
}

fn systemctl(args: &[&str]) -> Option<String> {
    let out = std::process::Command::new("systemctl").arg("--user").args(args).output().ok()?;
    Some(String::from_utf8_lossy(&out.stdout).trim().to_string())
}

const DAEMON_UNIT: &str = "cntrl-daemon.service";

/// Does the background service start at login? `None`: it is not installed.
fn service_enabled() -> Option<bool> {
    match systemctl(&["is-enabled", DAEMON_UNIT])?.as_str() {
        "enabled" => Some(true),
        "disabled" => Some(false),
        _ => None,
    }
}

fn sync_view(app: &App) -> Element<'_, Message> {
    let accent = app.accent();
    let s = &app.settings;

    /// One settings line: label, explanation, and a switch (or a "Soon" pill).
    fn line<'a>(
        label: &'a str,
        desc: &'a str,
        control: Element<'a, Message>,
    ) -> Element<'a, Message> {
        let mut words = column![text(label).size(14).font(tokens::UI_MEDIUM)].spacing(2);
        if !desc.is_empty() {
            words = words.push(text(desc).size(13).color(tokens::TEXT_SECONDARY));
        }
        column![
            container(Space::new().height(1).width(Length::Fill)).style(boxed(
                tokens::RAISED,
                Color::TRANSPARENT,
                0.0,
                0.0
            )),
            container(row![words.width(Length::Fill), control].spacing(20).align_y(Alignment::Center))
                .padding([16, 22]),
        ]
        .into()
    }
    fn section<'a>(title: &'a str, lines: Vec<Element<'a, Message>>) -> Element<'a, Message> {
        let head = container(
            text(title).size(12).font(tokens::UI_SEMIBOLD).color(tokens::TEXT_SECONDARY),
        )
        .padding(iced::Padding { top: 18.0, right: 22.0, bottom: 6.0, left: 22.0 });
        let mut rows = column![head];
        for l in lines {
            rows = rows.push(l);
        }
        container(rows)
            .width(Length::Fill)
            .style(boxed(tokens::SURFACE, tokens::BORDER, 1.0, tokens::RADIUS_CARD))
            .into()
    }

    // The service switches need the systemd user unit from scripts/install.sh.
    let installed = app.service_enabled.is_some();
    let no_unit = "Needs the background service: run scripts/install.sh with INSTALL_SYSTEMD=1.";
    let soon: Element<'_, Message> = container(text("Soon").size(11).color(tokens::TEXT_MUTED))
        .padding([2, 8])
        .style(boxed(Color::TRANSPARENT, tokens::BORDER_CONTROL, 1.0, tokens::RADIUS_PILL))
        .into();
    let startup = section(
        "STARTUP",
        vec![
            line(
                "Start when I log in",
                if installed {
                    "Restores your lighting and fan curves as soon as the desktop loads."
                } else {
                    no_unit
                },
                switch(
                    app.service_enabled == Some(true),
                    accent,
                    installed.then_some(Message::ServiceAutostart(app.service_enabled != Some(true))),
                ),
            ),
            line(
                "Keep running in the background",
                if installed {
                    "Needed for animated and reactive effects, theme sync and fan curves. \
                     Closing the window keeps it running. Turning this off hands the fans \
                     back to the BIOS."
                } else {
                    no_unit
                },
                switch(
                    app.engine_running,
                    accent,
                    installed.then_some(Message::ServiceRun(!app.engine_running)),
                ),
            ),
            line("Show in the system tray", "Quick access to profiles and lights off.", soon),
        ],
    );
    let theme = section(
        "THEME SYNC",
        vec![
            line(
                "Match my desktop theme",
                "When your desktop accent changes, your lighting changes with it.",
                switch(s.theme_sync_enabled, accent, Some(Message::ToggleThemeSync(!s.theme_sync_enabled))),
            ),
            line(
                "Include keyboard",
                "",
                switch(
                    s.theme_sync_keyboard,
                    accent,
                    Some(Message::ToggleThemeSyncKeyboard(!s.theme_sync_keyboard)),
                ),
            ),
            line(
                "Include PC lighting",
                "Fans, motherboard, graphics card and RAM.",
                switch(s.theme_sync_pc, accent, Some(Message::ToggleThemeSyncPc(!s.theme_sync_pc))),
            ),
        ],
    );
    let appearance = section(
        "APPEARANCE",
        vec![line(
            "Tint the app with my lighting color",
            "Buttons and highlights in CNTRL use your current lighting color.",
            switch(s.tint_ui, accent, Some(Message::ToggleTintUi(!s.tint_ui))),
        )],
    );

    // ── desktop accent: where it comes from, and a manual apply ──
    let swatch = match app.desktop_accent {
        Some((r, g, b)) => Color::from_rgb8(r, g, b),
        None => tokens::RAISED,
    };
    let mut source = row![
        text("From ").size(13).color(tokens::TEXT_SECONDARY),
        text(cntrl_core::theme::theme_file_path().display().to_string())
            .size(13)
            .font(tokens::MONO)
            .color(Color::from_rgb8(0xC9, 0xCF, 0xD6)),
    ];
    match (&app.theme_last_applied, app.desktop_accent) {
        (Some(hex), _) => {
            source = source
                .push(text(" · last applied ").size(13).color(tokens::TEXT_SECONDARY))
                .push(text(hex.clone()).size(13).font(tokens::MONO).color(Color::from_rgb8(0xC9, 0xCF, 0xD6)));
        }
        (None, None) => {
            source = source.push(text(" · no accent found there").size(13).color(tokens::TEXT_SECONDARY));
        }
        (None, Some(_)) => {}
    }
    let mut accent_words =
        column![text("Desktop accent").size(14).font(tokens::UI_MEDIUM), source.wrap()].spacing(2);
    if !app.status.is_empty() {
        accent_words = accent_words.push(text(app.status.clone()).size(13).color(tokens::WARNING));
    }
    let accent_card = container(
        row![
            shape(swatch, 44.0, tokens::RADIUS_TILE),
            accent_words.width(Length::Fill),
            button(vcenter(text("Apply now").size(14).font(tokens::UI_SEMIBOLD)))
                .on_press(Message::ApplyThemeNow)
                .height(tokens::TARGET)
                .padding([0, 18])
                .style(flat_button(
                    accent,
                    lerp_color(accent, Color::WHITE, 0.12),
                    tokens::ON_ACCENT,
                    Color::TRANSPARENT,
                    tokens::RADIUS_CONTROL,
                )),
        ]
        .spacing(16)
        .align_y(Alignment::Center),
    )
    .padding([18, 22])
    .width(Length::Fill)
    .style(boxed(tokens::SURFACE, tokens::BORDER, 1.0, tokens::RADIUS_CARD));

    let about = container(
        row![
            column![
                text(concat!("CNTRL ", env!("CARGO_PKG_VERSION"))).size(14).font(tokens::UI_SEMIBOLD),
                text(concat!("Free and open source · ", env!("CARGO_PKG_LICENSE")))
                    .size(13)
                    .color(tokens::TEXT_SECONDARY),
            ]
            .spacing(2)
            .width(Length::Fill),
            outline_button(text("Source code").size(14), Message::OpenUrl(env!("CARGO_PKG_REPOSITORY"))),
            outline_button(
                text("Report a problem").size(14),
                Message::OpenUrl(concat!(env!("CARGO_PKG_REPOSITORY"), "/issues")),
            ),
        ]
        .spacing(16)
        .align_y(Alignment::Center),
    )
    .padding(iced::Padding { top: 4.0, right: 4.0, bottom: 0.0, left: 4.0 });

    column![
        page_title("Settings", "How CNTRL starts, runs and fits in with your desktop."),
        scrollable(
            container(column![startup, theme, appearance, accent_card, about].spacing(20))
                .max_width(860)
        )
        .height(Length::Fill),
    ]
    .spacing(24)
    .into()
}

/// The layout device ids that belong to one kind of hardware.
fn layout_ids(app: &App, kind: TargetKind) -> Vec<&str> {
    app.canvas_layout
        .devices
        .iter()
        .filter(|d| d.present)
        .map(|d| d.id.as_str())
        .filter(|id| match kind {
            TargetKind::Keyboard => *id == core_canvas::DEV_KEYBOARD,
            TargetKind::Gpu => *id == core_canvas::DEV_GPU,
            TargetKind::Corsair => id.starts_with("ram"),
            TargetKind::Ite => {
                id.starts_with("dled") || id.starts_with("mbzone") || *id == core_canvas::DEV_RGB12V
            }
        })
        .collect()
}

/// What the device is made of, as short pills ("Header 2 · 3 fans × 8 LEDs").
fn device_zones(app: &App, kind: TargetKind) -> Vec<String> {
    use cntrl_core::canvas::SegmentKind;
    let ids = layout_ids(app, kind);
    match kind {
        TargetKind::Keyboard => {
            let keys = app
                .canvas_layout
                .spec(core_canvas::DEV_KEYBOARD)
                .map_or(0, |s| s.led_positions.len());
            vec![format!("{keys} keys"), "4 onboard profiles".into()]
        }
        TargetKind::Gpu => vec!["1 zone".into()],
        TargetKind::Corsair => (1..=ids.len()).map(|n| format!("Module {n}")).collect(),
        TargetKind::Ite => {
            let mut zones = Vec::new();
            for (n, header) in
                [core_canvas::CHAIN_DLED1, core_canvas::CHAIN_DLED2].into_iter().enumerate()
            {
                let segs = app.canvas_layout.chains.header(header);
                let fans: Vec<u8> =
                    segs.iter().filter(|s| s.kind == SegmentKind::Fan).map(|s| s.leds).collect();
                let strip: u32 = segs
                    .iter()
                    .filter(|s| s.kind == SegmentKind::Strip)
                    .map(|s| u32::from(s.leds))
                    .sum();
                let mut parts = Vec::new();
                match fans.as_slice() {
                    [] => {}
                    [one] => parts.push(format!("1 fan × {one} LEDs")),
                    [first, rest @ ..] if rest.iter().all(|l| l == first) => {
                        parts.push(format!("{} fans × {first} LEDs", fans.len()))
                    }
                    _ => parts.push(format!("{} fans", fans.len())),
                }
                if strip > 0 {
                    parts.push(format!("{strip}-LED strip"));
                }
                let what = if parts.is_empty() { "empty".to_string() } else { parts.join(" + ") };
                zones.push(format!("Header {} · {what}", n + 1));
            }
            if ids.contains(&core_canvas::DEV_RGB12V) {
                zones.push("12 V strip".into());
            }
            let board: Vec<&str> =
                ids.iter().filter_map(|id| id.strip_prefix("mbzone")).collect();
            if !board.is_empty() {
                zones.push(format!("Board zones {}", board.join(", ")));
            }
            zones
        }
    }
}

fn devices_view(app: &App) -> Element<'_, Message> {
    use cntrl_core::devices::DeviceStatus;
    let header = row![
        page_title("Devices", "Found automatically. No extra drivers or tools needed."),
        Space::new().width(Length::Fill),
        outline_button(
            row![
                lucide(ICON_REFRESH, 16.0, tokens::TEXT),
                text("Scan again").size(14).font(tokens::UI_MEDIUM),
            ]
            .spacing(8)
            .align_y(Alignment::Center),
            Message::RefreshDevices,
        ),
    ]
    .spacing(16)
    .align_y(Alignment::End);

    let mut cards = column![].spacing(12);
    for d in &app.devices {
        let kind = TargetKind::from_device_kind(d.kind);
        let (glyph, what, summary) = match kind {
            TargetKind::Keyboard => (ICON_KEYBOARD, "Keyboard", "per-key RGB".to_string()),
            TargetKind::Ite => (ICON_BOARD, "Motherboard", "fans and board lighting".to_string()),
            TargetKind::Gpu => (ICON_GPU, "Graphics card", "single zone".to_string()),
            TargetKind::Corsair => {
                let n = layout_ids(app, kind).len();
                (ICON_RAM, "Memory", format!("{n} module{}", if n == 1 { "" } else { "s" }))
            }
        };
        let (dot, tone) = match d.status {
            DeviceStatus::Ready => (tokens::SUCCESS, Color::from_rgb8(0x7D, 0xD8, 0xA9)),
            DeviceStatus::Blocked => (tokens::WARNING, tokens::WARNING),
            DeviceStatus::Absent => (tokens::TEXT_DISABLED, tokens::TEXT_MUTED),
        };
        let open = app.device_open == Some(d.backend_id);
        let identifying = app.device_identify == Some(kind);

        let mut top = row![
            container(lucide(glyph, 24.0, tokens::TEXT_NAV))
                .center(48)
                .style(boxed(tokens::RAISED, Color::TRANSPARENT, 0.0, tokens::RADIUS_TILE)),
            column![
                text(d.name.clone()).size(16).font(tokens::UI_SEMIBOLD),
                text(format!("{what} · {summary}")).size(14).color(tokens::TEXT_SECONDARY),
            ]
            .spacing(2)
            .width(Length::Fill),
            row![shape(dot, 7.0, tokens::RADIUS_PILL), text(d.status.label()).size(13).color(tone)]
                .spacing(6)
                .align_y(Alignment::Center),
        ]
        .spacing(16)
        .align_y(Alignment::Center);
        // Identify lights this device alone, through the background stream.
        if d.status == DeviceStatus::Ready && !layout_ids(app, kind).is_empty() {
            top = top.push(
                button(vcenter(text(if identifying { "Stop" } else { "Identify" }).size(14)))
                    .on_press(Message::DeviceIdentify(kind))
                    .height(40)
                    .padding([0, 14])
                    .style(flat_button(
                        if identifying { tokens::SEGMENT_ON } else { Color::TRANSPARENT },
                        tokens::HOVER,
                        tokens::TEXT,
                        tokens::BORDER_CONTROL,
                        9.0,
                    )),
            );
        }
        top = top.push(
            button(vcenter(
                row![
                    text("Details").size(14),
                    lucide(if open { ICON_CHEVRON_UP } else { ICON_CHEVRON_DOWN }, 16.0, tokens::TEXT_NAV),
                ]
                .spacing(6)
                .align_y(Alignment::Center),
            ))
            .on_press(Message::DeviceDetails(d.backend_id))
            .height(40)
            .padding([0, 12])
            .style(flat_button(Color::TRANSPARENT, tokens::HOVER, tokens::TEXT_NAV, Color::TRANSPARENT, 9.0)),
        );

        let pills = device_zones(app, kind).into_iter().map(|zone| {
            container(text(zone).size(13).color(Color::from_rgb8(0xC9, 0xCF, 0xD6)))
                .padding([6, 12])
                .style(boxed(tokens::RAISED, Color::TRANSPARENT, 0.0, tokens::RADIUS_PILL))
                .into()
        });
        let mut body = column![
            top,
            iced::widget::Row::with_children(pills).spacing(8).wrap().vertical_spacing(8),
        ]
        .spacing(14);
        if d.status == DeviceStatus::Blocked {
            body = body.push(
                text(
                    "CNTRL is not allowed to open this device. Install the udev rule \
                     (packaging/udev) and plug it in again.",
                )
                .size(13)
                .color(tokens::WARNING),
            );
        }
        if open {
            let fact = |name: &'static str, value: String| {
                row![
                    text(name).size(13).color(tokens::TEXT_MUTED).width(64),
                    text(value).size(13).font(tokens::MONO),
                ]
                .spacing(20)
            };
            let id = if d.vid != 0 { d.usb_id() } else { "—".to_string() };
            body = body.push(
                container(
                    column![
                        fact("Driver", d.backend_id.to_string()),
                        fact("Path", d.control_path.clone().unwrap_or_else(|| d.detail.clone())),
                        fact("ID", id),
                    ]
                    .spacing(6),
                )
                .padding([14, 16])
                .width(Length::Fill)
                .style(boxed(tokens::INSET, Color::TRANSPARENT, 0.0, tokens::RADIUS_CONTROL)),
            );
        }
        cards = cards.push(
            container(body)
                .padding([18, 20])
                .width(Length::Fill)
                .style(boxed(tokens::SURFACE, tokens::BORDER, 1.0, tokens::RADIUS_CARD)),
        );
    }
    if app.devices.is_empty() {
        cards = cards.push(text("No supported devices found.").size(14).color(tokens::TEXT_SECONDARY));
    }

    // ponytail: solid outline — iced has no dashed borders (the mockup's is dashed).
    let mono = |s: &'static str| text(s).size(14).font(tokens::MONO).color(tokens::TEXT);
    let words = |s: &'static str| text(s).size(14).color(tokens::TEXT_SECONDARY);
    let help = container(
        row![
            column![
                text("Something missing?").size(15).font(tokens::UI_SEMIBOLD),
                row![
                    words("CNTRL talks to hardware over USB (HID) and I²C. RAM and graphics cards need the "),
                    mono("i2c-dev"),
                    words(" module loaded and access to "),
                    mono("/dev/i2c-*"),
                    words("."),
                ]
                .wrap(),
            ]
            .spacing(4)
            .width(Length::Fill),
            button(vcenter(text("Troubleshooting guide").size(14).font(tokens::UI_MEDIUM)))
                .on_press(Message::OpenUrl(concat!(env!("CARGO_PKG_REPOSITORY"), "#readme")))
                .height(tokens::TARGET)
                .padding([0, 18])
                .style(flat_button(
                    tokens::RAISED,
                    tokens::SEGMENT_ON,
                    tokens::TEXT,
                    Color::TRANSPARENT,
                    tokens::RADIUS_CONTROL,
                )),
        ]
        .spacing(16)
        .align_y(Alignment::Center),
    )
    .padding(20)
    .width(Length::Fill)
    .style(boxed(Color::TRANSPARENT, Color::from_rgb8(0x34, 0x3B, 0x43), 1.0, tokens::RADIUS_CARD));

    column![header, scrollable(column![cards, help].spacing(24)).height(Length::Fill)]
        .spacing(24)
        .into()
}

/// What a person would call this fan; the board's own name goes underneath.
fn fan_name(app: &App, ch: &cntrl_core::fan_control::FanChannel) -> String {
    let label = ch.label.to_ascii_uppercase();
    // "This is a pump" decides; without an answer the header's name does.
    let pump = app.assignment_for(&ch.id).map_or(ch.is_pump(), |a| a.is_pump(ch));
    if ch.chip == "amdgpu" {
        "Graphics card".into()
    } else if pump {
        "Pump".into()
    } else if label == "CPU_FAN" {
        "CPU fan".into()
    } else if label == "CPU_OPT" {
        "CPU fan 2".into()
    } else if let Some(n) = label.strip_prefix("SYS_FAN") {
        // "SYS_FAN4_PUMP" with a case fan on it is case fan 4.
        format!("Case fan {}", n.trim_end_matches("_PUMP"))
    } else if ch.label.is_empty() {
        ch.id.clone()
    } else {
        ch.label.clone()
    }
}

/// The name printed on the board (`CPU_FAN`), or the kernel id without one.
fn fan_raw(ch: &cntrl_core::fan_control::FanChannel) -> &str {
    if ch.label.is_empty() { &ch.id } else { &ch.label }
}

/// What a person would call this temperature.
fn sensor_name(s: &cntrl_core::fan_control::TempSensor) -> String {
    match (s.chip.as_str(), s.label.as_str()) {
        ("k10temp" | "zenpower", "Tctl") | ("coretemp", "Package id 0") => "CPU".into(),
        ("amdgpu", "edge") if s.id.starts_with("amdgpu/") => "Graphics card".into(),
        // Everything else keeps its id: several chips repeat the same labels.
        _ => format!("{} · {}", s.label, s.id),
    }
}

/// A graphics card that keeps its own fan curve (it is not in manual mode).
fn fan_on_firmware(app: &App, ch: &cntrl_core::fan_control::FanChannel) -> bool {
    ch.chip == "amdgpu" && !app.fan_readings.manual.contains(&ch.id)
}

/// The fan the right-hand card shows: the picked one, else the first
/// motherboard fan with a curve.
fn selected_fan(app: &App) -> Option<&cntrl_core::fan_control::FanChannel> {
    let chans = &app.fan_discovery.channels;
    app.fan_selected
        .as_deref()
        .and_then(|id| app.fan_discovery.channel(id))
        .or_else(|| {
            chans
                .iter()
                .find(|c| app.assignment_for(&c.id).is_some() && c.chip != "amdgpu")
        })
        .or(chans.first())
}

/// Dropdown look from the handoff (inset fill, control border).
fn select_style(_: &Theme, status: pick_list::Status) -> pick_list::Style {
    pick_list::Style {
        text_color: tokens::TEXT,
        placeholder_color: tokens::TEXT_MUTED,
        handle_color: tokens::TEXT_MUTED,
        background: tokens::INSET.into(),
        border: iced::Border {
            radius: tokens::RADIUS_CONTROL.into(),
            width: 1.0,
            color: if matches!(status, pick_list::Status::Active) {
                tokens::BORDER_CONTROL
            } else {
                tokens::TEXT_MUTED
            },
        },
    }
}

fn fans_view(app: &App) -> Element<'_, Message> {
    use cntrl_core::fan_curves::FanMode;
    let accent = app.accent();
    let cfg = &app.fan_curve_cfg;

    // ── header: title + the one switch that hands fans to CNTRL ──
    let on = cfg.enabled;
    let track = container(shape(
        if on { tokens::ON_ACCENT } else { Color::from_rgb8(0xC9, 0xCF, 0xD6) },
        22.0,
        tokens::RADIUS_PILL,
    ))
    .width(46)
    .height(28)
    .padding(3)
    .align_x(if on { Alignment::End } else { Alignment::Start })
    .style(boxed(
        if on { accent } else { Color::from_rgb8(0x3A, 0x41, 0x4A) },
        Color::TRANSPARENT,
        0.0,
        tokens::RADIUS_PILL,
    ));
    let switch = button(vcenter(
        row![text("Let CNTRL control fans").size(14), track]
            .spacing(12)
            .align_y(Alignment::Center),
    ))
    .on_press(Message::FanToggleEnabled(!on))
    .height(tokens::TARGET)
    .padding(iced::Padding { left: 16.0, right: 6.0, ..iced::Padding::ZERO })
    .style(flat_button(
        tokens::SURFACE,
        tokens::HOVER,
        tokens::TEXT,
        tokens::BORDER_CONTROL,
        tokens::RADIUS_PILL,
    ));
    let header = row![
        page_title(
            "Fans",
            "Pick how your PC should sound. Fine-tune single fans only if you want to.",
        ),
        Space::new().width(Length::Fill),
        switch,
    ]
    .spacing(16)
    .align_y(Alignment::End);

    // ── mode cards ──
    let mut modes = row![].spacing(12);
    for mode in FanMode::ALL {
        let picked = cfg.mode == mode;
        let desc = match mode {
            FanMode::Silent => "Quietest. Fans ramp up late.",
            FanMode::Balanced => "Quiet at idle, cool under load.",
            FanMode::Performance => "Coolest. Fans spin up early.",
            FanMode::Custom => "Your own curve for each fan.",
        };
        let ring = if picked { accent } else { Color::from_rgb8(0x4A, 0x52, 0x5C) };
        let radio = container(shape(
            if picked { accent } else { Color::TRANSPARENT },
            8.0,
            tokens::RADIUS_PILL,
        ))
        .center(18)
        .style(boxed(Color::TRANSPARENT, ring, 2.0, tokens::RADIUS_PILL));
        modes = modes.push(
            button(
                column![
                    row![
                        text(mode.label()).size(16).font(tokens::UI_SEMIBOLD).width(Length::Fill),
                        radio,
                    ]
                    .align_y(Alignment::Center),
                    text(desc).size(13).color(tokens::TEXT_SECONDARY),
                ]
                .spacing(6),
            )
            .on_press(Message::FanSetMode(mode))
            .width(Length::FillPortion(1))
            .height(92)
            .padding([16, 18])
            .style(move |_, _| button::Style {
                background: Some(
                    if picked { Color::from_rgb8(0x1F, 0x25, 0x2B) } else { tokens::SURFACE }.into(),
                ),
                text_color: tokens::TEXT,
                border: iced::Border {
                    radius: 14.0.into(),
                    width: 2.0,
                    color: if picked { accent } else { tokens::BORDER },
                },
                ..button::Style::default()
            }),
        );
    }

    // ── your fans: motherboard headers first, the graphics card last ──
    let selected = selected_fan(app).map(|c| c.id.as_str());
    let mut list = column![].spacing(6);
    let chans = &app.fan_discovery.channels;
    for ch in chans.iter().filter(|c| c.chip != "amdgpu").chain(chans.iter().filter(|c| c.chip == "amdgpu")) {
        let picked = selected == Some(ch.id.as_str());
        let rpm = app.fan_readings.rpm.get(&ch.id).copied();
        let duty = app
            .fan_readings
            .pwm_raw
            .get(&ch.id)
            .map_or(0, |raw| u16::from(cntrl_core::fan_control::raw_to_pct(*raw)));
        let speed = match (fan_on_firmware(app, ch), rpm) {
            (true, Some(0) | None) => "Off · auto".to_string(),
            (true, Some(r)) => format!("{r} rpm · auto"),
            (false, Some(r)) => format!("{r} rpm"),
            (false, None) => "no reading".to_string(),
        };
        let mut bar = row![].height(4);
        if duty > 0 {
            bar = bar.push(
                container(Space::new().height(4))
                    .width(Length::FillPortion(duty))
                    .style(boxed(accent, Color::TRANSPARENT, 0.0, tokens::RADIUS_PILL)),
            );
        }
        if duty < 100 {
            bar = bar.push(Space::new().width(Length::FillPortion(100 - duty)));
        }
        list = list.push(
            button(vcenter(
                row![
                    column![
                        text(fan_name(app, ch)).size(14).font(tokens::UI_SEMIBOLD),
                        text(fan_raw(ch).to_string())
                            .size(12)
                            .font(tokens::MONO)
                            .color(tokens::TEXT_MUTED),
                    ]
                    .spacing(1)
                    .width(Length::Fill),
                    column![
                        text(speed).size(14).font(tokens::MONO),
                        container(bar).width(Length::Fill).style(boxed(
                            tokens::BORDER_PROFILE,
                            Color::TRANSPARENT,
                            0.0,
                            tokens::RADIUS_PILL,
                        )),
                    ]
                    .spacing(6)
                    .width(110)
                    .align_x(Alignment::End),
                ]
                .spacing(16)
                .align_y(Alignment::Center),
            ))
            .on_press(Message::FanSelectChannel(ch.id.clone()))
            .width(Length::Fill)
            .height(60)
            .padding([0, 16])
            .style(flat_button(
                if picked { tokens::SELECTED } else { tokens::SURFACE },
                tokens::SELECTED,
                tokens::TEXT,
                if picked { Color::from_rgb8(0x3A, 0x42, 0x4B) } else { tokens::BORDER },
                tokens::RADIUS_TILE,
            )),
        );
    }
    if chans.is_empty() {
        list = list.push(text("No fans detected.").size(13).color(tokens::TEXT_MUTED));
    }
    let mut left = column![
        text("YOUR FANS").size(12).font(tokens::UI_SEMIBOLD).color(tokens::TEXT_SECONDARY),
        list,
    ]
    .spacing(12)
    .width(Length::FillPortion(2));
    // One warning for the whole board instead of a badge on every row.
    if let Some(ch) = chans.iter().find(|c| c.experimental) {
        let chip = ch.chip.split('_').next().unwrap_or(&ch.chip);
        left = left.push(
            container(
                row![
                    lucide(ICON_WARNING, 18.0, tokens::WARNING),
                    text(format!(
                        "Motherboard fan control on this board ({chip}) is experimental. \
                         If CNTRL closes, your BIOS takes over the fans again."
                    ))
                    .size(13)
                    .color(Color::from_rgb8(0xF3, 0xD9, 0xA8)),
                ]
                .spacing(12),
            )
            .padding(14)
            .width(Length::Fill)
            .style(boxed(
                Color::from_rgb8(0x2A, 0x24, 0x18),
                Color::TRANSPARENT,
                0.0,
                tokens::RADIUS_TILE,
            )),
        );
    }

    let mut root = column![header].spacing(24);
    if !app.fan_status.is_empty() {
        root = root.push(text(app.fan_status.clone()).size(13).color(tokens::WARNING));
    }
    root.push(modes)
        .push(
            row![scrollable(left).height(Length::Fill), fan_card(app)]
                .spacing(24)
                .height(Length::Fill),
        )
        .into()
}

/// The right-hand card: the selected fan's curve and its two settings.
fn fan_card(app: &App) -> Element<'_, Message> {
    use cntrl_core::fan_control::FanChannelStatus;
    use cntrl_core::fan_curves::{CurveKind, FanMode};
    let colors = &app.colors;
    let cfg = &app.fan_curve_cfg;
    fn card<'a>(content: iced::widget::Column<'a, Message>) -> Element<'a, Message> {
        container(content.spacing(18))
            .padding(22)
            .width(Length::FillPortion(3))
            .style(boxed(tokens::SURFACE, tokens::BORDER, 1.0, tokens::RADIUS_CARD))
            .into()
    }
    let note = |msg: String| -> Element<'_, Message> {
        container(text(msg).size(14).color(tokens::TEXT_NAV))
            .padding(16)
            .width(Length::Fill)
            .style(boxed(tokens::RAISED, Color::TRANSPARENT, 0.0, tokens::RADIUS_TILE))
            .into()
    };
    let Some(ch) = selected_fan(app) else {
        return card(column![note("No fans detected.".into())]);
    };
    let firmware = fan_on_firmware(app, ch);
    let assignment = app.assignment_for(&ch.id);

    let duty = app
        .fan_readings
        .pwm_raw
        .get(&ch.id)
        .map_or(0, |raw| cntrl_core::fan_control::raw_to_pct(*raw));
    let summary = match (firmware, app.fan_readings.rpm.get(&ch.id)) {
        (true, _) => "Controlled by the graphics card".to_string(),
        (false, Some(rpm)) => format!("{rpm} rpm right now · {duty}% speed"),
        (false, None) => format!("{duty}% speed"),
    };
    let head = row![
        column![
            text(fan_name(app, ch)).size(20).font(tokens::UI_SEMIBOLD),
            text(summary).size(14).color(tokens::TEXT_SECONDARY),
        ]
        .spacing(2)
        .width(Length::Fill),
        container(
            text(if assignment.is_some_and(|a| a.is_pump(ch)) && cfg.mode != FanMode::Custom {
                "Own curve".to_string()
            } else {
                format!("{} curve", cfg.mode.label())
            })
            .size(13)
            .color(tokens::TEXT_NAV),
        )
            .padding([6, 12])
            .style(boxed(tokens::RAISED, Color::TRANSPARENT, 0.0, tokens::RADIUS_PILL)),
    ]
    .spacing(12);

    let take_over = |label: &'static str| -> Option<Element<'_, Message>> {
        let sensor = default_sensor_for(app, &ch.id)?;
        Some(
            button(text(label).size(14))
                .on_press(Message::FanCreateAssignment(ch.id.clone(), sensor))
                .padding([10, 16])
                .style(flat_button(
                    tokens::RAISED,
                    tokens::SEGMENT_ON,
                    tokens::TEXT,
                    tokens::BORDER_CONTROL,
                    tokens::RADIUS_CONTROL,
                ))
                .into(),
        )
    };

    if ch.status == FanChannelStatus::Blocked {
        return card(column![
            head,
            note(format!(
                "CNTRL is not allowed to control this fan: {}\n\nInstall \
                 packaging/udev/99-cntrl-hwmon.rules, then run: sudo udevadm control \
                 --reload-rules && sudo udevadm trigger --subsystem-match=hwmon",
                ch.detail
            )),
        ]);
    }
    if ch.status == FanChannelStatus::ReadOnly {
        return card(column![
            head,
            note("This fan can be read but not controlled: its driver has no manual mode.".into()),
        ]);
    }
    if firmware {
        let mut body = column![
            head,
            note(
                "The graphics card manages its own fans. They stay off until the GPU warms \
                 up, which is normal."
                    .into()
            ),
        ];
        // Older cards do accept a curve; newer ones ignore it (the list says so).
        if assignment.is_none() && cfg.mode == FanMode::Custom {
            body = body.extend(take_over("Try a curve anyway"));
        }
        return card(body);
    }
    let Some(assignment) = assignment else {
        return card(
            column![head, note("CNTRL is not controlling this fan. Your BIOS is.".into())]
                .extend(take_over("Control this fan")),
        );
    };

    let curve_id = App::curve_id_for(&ch.id);
    let (stored, sensor_id, also_id): (&[cntrl_core::fan_curves::CurvePoint], String, Option<String>) =
        cfg.curve(&curve_id)
            .and_then(|c| match &c.kind {
                CurveKind::Graph { points, sensor_id, also_sensor_id } => {
                    Some((points.as_slice(), sensor_id.clone(), also_sensor_id.clone()))
                }
                _ => None,
            })
            .unwrap_or((&[], String::new(), None));
    // A pump is outside the presets: its own curve, editable in every mode.
    let pump = assignment.is_pump(ch);
    let custom = cfg.mode == FanMode::Custom || pump;
    let points = if custom { stored } else { cfg.mode.preset().unwrap_or(stored) };

    // The curve runs on the hotter of its sensors, so that is what the marker shows.
    let temp_of = |id: &String| app.fan_readings.temps_c.get(id).copied();
    let current_temp = match (temp_of(&sensor_id), also_id.as_ref().and_then(temp_of)) {
        (Some(a), Some(b)) => Some(a.max(b)),
        (a, _) => a,
    };
    let hint = if pump {
        format!(
            "The pump keeps this curve in every mode and never runs below {}%. Drag a point to \
             move it · click empty space to add one · right-click a point to remove it.",
            cntrl_core::fan_control::PUMP_MIN_DUTY_PCT
        )
    } else if custom {
        "Drag a point to move it · click empty space to add one · right-click a point to remove it."
            .to_string()
    } else {
        format!(
            "This is the {} curve. Choose Custom above to drag the points yourself.",
            cfg.mode.label()
        )
    };

    // Every temperature the machine has, by name, with its live reading.
    let sensor_choices: Vec<SensorChoice> = app
        .fan_discovery
        .sensors
        .iter()
        .map(|s| SensorChoice {
            id: s.id.clone(),
            text: match app.fan_readings.temps_c.get(&s.id) {
                Some(t) => format!("{} ({t:.0}°C)", sensor_name(s)),
                None => sensor_name(s),
            },
        })
        .collect();
    fn field<'a>(
        label: &'a str,
        control: Element<'a, Message>,
    ) -> iced::widget::Column<'a, Message> {
        column![text(label).size(13).color(tokens::TEXT_NAV), control]
            .spacing(6)
            .width(Length::FillPortion(1))
    }
    let follow_id = curve_id.clone();
    let follow = pick_list(
        sensor_choices.clone(),
        sensor_choices.iter().find(|c| c.id == sensor_id).cloned(),
        move |choice| Message::FanSetSensor(follow_id.clone(), choice.id),
    )
    .placeholder("sensor not found, pick one")
    .width(Length::Fill)
    .padding([12, 12])
    .text_size(14)
    .style(select_style);

    // The pump's floor starts where the engine's own limit is.
    let (floor_lo, floor_hi) = if pump {
        (f32::from(cntrl_core::fan_control::PUMP_MIN_DUTY_PCT), 100.0)
    } else {
        (0.0, 60.0)
    };
    let min = f32::from(assignment.min_duty_pct).clamp(floor_lo, floor_hi);
    let floor_id = ch.id.clone();
    let floor = column![
        row![
            text("Never slower than").size(13).color(tokens::TEXT_NAV),
            Space::new().width(Length::Fill),
            text(format!("{min:.0}%")).size(13).font(tokens::MONO),
        ],
        container(
            slider(floor_lo..=floor_hi, min, move |v| Message::FanMinDuty(floor_id.clone(), v))
                .style(accent_slider(colors)),
        )
        .center_y(tokens::TARGET),
    ]
    .spacing(6)
    .width(Length::FillPortion(1));

    let mut body = column![
        head,
        crate::fans_tab::curve_editor(curve_id.clone(), points, current_temp, custom, colors),
        text(hint).size(13).color(tokens::TEXT_SECONDARY),
        container(Space::new().height(1).width(Length::Fill)).style(boxed(
            tokens::SEGMENT_ON,
            Color::TRANSPARENT,
            0.0,
            0.0
        )),
        row![field("Follow temperature of", follow.into()), floor].spacing(16),
    ];
    {
        // A second sensor ("whichever is hotter") applies in every mode.
        let none = SensorChoice { id: String::new(), text: "nothing else".into() };
        let also_choices: Vec<SensorChoice> = std::iter::once(none.clone())
            .chain(sensor_choices.iter().filter(|c| c.id != sensor_id).cloned())
            .collect();
        let also_selected = match &also_id {
            Some(id) => also_choices.iter().find(|c| &c.id == id).cloned(),
            None => Some(none),
        };
        let also_curve = curve_id.clone();
        let also = pick_list(also_choices, also_selected, move |choice| {
            Message::FanSetAlsoSensor(also_curve.clone(), (!choice.id.is_empty()).then_some(choice.id))
        })
        .placeholder("sensor not found, pick one")
        .width(Length::Fill)
        .padding([12, 12])
        .text_size(14)
        .style(select_style);
        // Per-fan opt-out, offered where the fan's own curve is in charge.
        let release = column![
            Space::new().height(19),
            button(vcenter(text("Give this fan back to the BIOS").size(14)))
                .on_press_maybe(custom.then(|| Message::FanRemoveAssignment(ch.id.clone())))
                .height(tokens::TARGET)
                .padding([0, 16])
                .style(flat_button(
                    Color::TRANSPARENT,
                    tokens::HOVER,
                    tokens::TEXT_NAV,
                    tokens::BORDER_CONTROL,
                    tokens::RADIUS_CONTROL,
                )),
        ]
        .width(Length::FillPortion(1))
        .align_x(Alignment::End);
        body = body.push(row![field("Or this one, if it is hotter", also.into()), release].spacing(16));
    }
    // A header the board calls a pump header is protected by default, with a
    // switch to lift that (a case fan is often plugged in there). Any other
    // header can be marked as a pump in Custom.
    let floor = cntrl_core::fan_control::PUMP_MIN_DUTY_PCT;
    let pump_switch = if ch.is_pump() {
        Some((
            format!("Allow this pump header to go below {floor}%"),
            "Switch on if a fan, not a pump, is plugged in here. It then follows the modes \
             like any other fan."
                .to_string(),
            !pump,
        ))
    } else if cfg.mode == FanMode::Custom || pump {
        Some((
            "This is a pump".to_string(),
            format!("Never below {floor}%, and it keeps this curve whatever mode is chosen."),
            pump,
        ))
    } else {
        None
    };
    if let Some((label, desc, on)) = pump_switch {
        body = body.push(
            row![
                column![
                    text(label).size(14).font(tokens::UI_MEDIUM),
                    text(desc).size(13).color(tokens::TEXT_SECONDARY),
                ]
                .spacing(2)
                .width(Length::Fill),
                // Either way the switch flips whether the fan counts as a pump.
                switch(on, app.accent(), Some(Message::FanSetPump(ch.id.clone(), !pump))),
            ]
            .spacing(20)
            .align_y(Alignment::Center),
        );
    }
    card(body)
}

/// One entry of the temperature-source list: shown by name, stored by id.
#[derive(Debug, Clone, PartialEq)]
struct SensorChoice {
    id: String,
    text: String,
}

impl std::fmt::Display for SensorChoice {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.text)
    }
}

/// Sensor a new curve starts on. A GPU fan follows its own chip. Anything else
/// (CPU cooler, case fans) follows the CPU: a board chip's first sensor is
/// usually a slow "system" temperature, and a CPU fan tied to it would stay
/// slow while the CPU heats up.
fn default_sensor_for(app: &App, channel_id: &str) -> Option<String> {
    let channel = app.fan_discovery.channel(channel_id)?;
    let sensors = &app.fan_discovery.sensors;
    let own_chip = || sensors.iter().find(|s| s.chip == channel.chip);
    let cpu = || {
        sensors
            .iter()
            .find(|s| ["k10temp", "coretemp", "zenpower"].contains(&s.chip.as_str()))
    };
    let pick = if channel.chip == "amdgpu" {
        own_chip()
    } else {
        cpu().or_else(own_chip)
    };
    pick.or_else(|| sensors.first()).map(|s| s.id.clone())
}


