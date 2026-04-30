//! CNTRL ICED UI (panel + popup widgets).

mod app;

/// Shell layout: full panel or one of four compact Waybar-oriented popups.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum UiMode {
    /// Full shell (default size).
    #[default]
    Panel,
    /// Color / HSL + swatches (`keeb-color`).
    PopupKeebColor,
    /// Effect selection (`keeb-effects`).
    PopupKeebEffects,
    /// Brightness + speed (`keeb-controls`).
    PopupKeebControls,
    /// Profile slot picker (`keeb-profiles`).
    PopupKeebProfiles,
}

impl UiMode {
    /// CLI / pid filename segment: `popup-{slug}.pid`.
    pub fn slug(self) -> &'static str {
        match self {
            Self::Panel => "panel",
            Self::PopupKeebColor => "keeb-color",
            Self::PopupKeebEffects => "keeb-effects",
            Self::PopupKeebControls => "keeb-controls",
            Self::PopupKeebProfiles => "keeb-profiles",
        }
    }

    /// Parse `cntrl popup <widget>` positional (`keeb-color`, …).
    pub fn from_slug(s: &str) -> Result<Self, String> {
        match s.trim() {
            "keeb-color" => Ok(Self::PopupKeebColor),
            "keeb-effects" => Ok(Self::PopupKeebEffects),
            "keeb-controls" => Ok(Self::PopupKeebControls),
            "keeb-profiles" => Ok(Self::PopupKeebProfiles),
            _ => Err(format!(
                "unknown popup widget {:?}: expected keeb-color | keeb-effects | keeb-controls | keeb-profiles",
                s.trim()
            )),
        }
    }
}

/// Full-size ICED shell (same as Milestone 2 default).
pub fn run() -> iced::Result {
    run_with(UiMode::Panel)
}

pub fn run_with(mode: UiMode) -> iced::Result {
    app::run(mode)
}
