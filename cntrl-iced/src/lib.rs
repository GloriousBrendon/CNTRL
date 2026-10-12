//! CNTRL ICED UI (full desktop panel).

mod app;
mod canvas_tab;
mod fans_tab;
mod tokens;

/// Launch the full-size ICED shell.
pub fn run() -> iced::Result {
    app::run()
}
