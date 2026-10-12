//! Design tokens from `docs/design/HANDOFF.md`. One place for every color,
//! radius and font of the redesign; views never spell these out themselves.

use iced::font::{Family, Weight};
use iced::{Color, Font};

const fn hex(v: u32) -> Color {
    Color::from_rgb8((v >> 16) as u8, (v >> 8) as u8, v as u8)
}

pub const BG: Color = hex(0x0F1114);
pub const SIDEBAR: Color = hex(0x14171B);
pub const SIDEBAR_EDGE: Color = hex(0x23282E);
pub const SURFACE: Color = hex(0x1A1E23);
/// Raised surface / chip / note background.
pub const RAISED: Color = hex(0x22272D);
/// Active nav item.
pub const SELECTED: Color = hex(0x22282F);
/// Selected segment / pressed chip.
pub const SEGMENT_ON: Color = hex(0x262C33);
pub const HOVER: Color = hex(0x1E2329);
/// Inputs and the track behind segmented controls.
pub const INSET: Color = hex(0x121519);
pub const BORDER: Color = hex(0x252A30);
pub const BORDER_CONTROL: Color = hex(0x2E343B);
pub const BORDER_PROFILE: Color = hex(0x2A3037);
pub const TEXT: Color = hex(0xE8EBEE);
pub const TEXT_SECONDARY: Color = hex(0x9AA3AE);
pub const TEXT_NAV: Color = hex(0xB4BCC6);
pub const TEXT_MUTED: Color = hex(0x8E97A2);
pub const TEXT_DISABLED: Color = hex(0x6F7883);
/// Text drawn on the accent color.
pub const ON_ACCENT: Color = hex(0x08171D);
pub const SUCCESS: Color = hex(0x4CC38A);
pub const WARNING: Color = hex(0xF2A541);
/// Thumbnails fade to this, not to pure black.
pub const THUMB_DARK: Color = hex(0x0D0F12);
pub const THUMB_OFF: Color = hex(0x0B0D10);

/// Accent when no lighting color is usable (and the mockups' default).
pub const DEFAULT_ACCENT: (u8, u8, u8) = (0x00, 0xC6, 0xFF);

pub const BG_HEX: &str = "#0F1114";
pub const SIDEBAR_HEX: &str = "#14171B";
pub const SURFACE_HEX: &str = "#1A1E23";
pub const TEXT_HEX: &str = "#E8EBEE";

pub const RADIUS_CARD: f32 = 16.0;
pub const RADIUS_TILE: f32 = 12.0;
pub const RADIUS_CONTROL: f32 = 10.0;
pub const RADIUS_PILL: f32 = 99.0;
/// Smallest clickable height.
pub const TARGET: f32 = 44.0;

pub const UI_FONT_NAME: &str = "Instrument Sans";

const fn ui(weight: Weight) -> Font {
    Font {
        family: Family::Name(UI_FONT_NAME),
        weight,
        ..Font::DEFAULT
    }
}

pub const UI: Font = ui(Weight::Normal);
pub const UI_MEDIUM: Font = ui(Weight::Medium);
pub const UI_SEMIBOLD: Font = ui(Weight::Semibold);
pub const UI_BOLD: Font = ui(Weight::Bold);
/// Numbers, ids and paths. Not bundled: falls back to the default monospace
/// font where JetBrains Mono is not installed.
pub const MONO: Font = Font::with_name("JetBrains Mono");

/// The Instrument Sans weights the UI uses (OFL, see `resources/fonts`).
pub const UI_FONT_FILES: [&[u8]; 4] = [
    include_bytes!("../../resources/fonts/InstrumentSans-Regular.ttf"),
    include_bytes!("../../resources/fonts/InstrumentSans-Medium.ttf"),
    include_bytes!("../../resources/fonts/InstrumentSans-SemiBold.ttf"),
    include_bytes!("../../resources/fonts/InstrumentSans-Bold.ttf"),
];
