mod theme;
mod ui;

use cntrl_core::persistence::read_startup_slot;
use cntrl_core::profile::{self, ProfileDocument};
use std::sync::{Arc, Mutex};

#[derive(Clone)]
pub struct AppShared {
    pub profile: Arc<Mutex<ProfileDocument>>,
    pub active_slot: Arc<Mutex<u8>>,
    pub theme_colors: Arc<Mutex<theme::ThemeColors>>,
}

pub use cntrl_core::persistence::{load_profile_slot, persist_active_slot, persist_profile_slot};

fn main() {
    adw::init().expect("failed to initialize libadwaita");

    let startup_slot = read_startup_slot();
    let initial = load_profile_slot(startup_slot).unwrap_or_else(|_| profile::default_profile_document());
    let initial_theme = Arc::new(Mutex::new(theme::load_theme_from_disk()));

    let shared = AppShared {
        profile: Arc::new(Mutex::new(initial)),
        active_slot: Arc::new(Mutex::new(startup_slot)),
        theme_colors: initial_theme,
    };

    let app = relm4::RelmApp::new("io.cntrl.native");
    app.run::<ui::window::AppModel>(shared);
}
