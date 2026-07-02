//! CNTRL ICED UI (full desktop panel).

mod app;
mod canvas_tab;

/// Launch the full-size ICED shell.
pub fn run() -> iced::Result {
    app::run()
}
