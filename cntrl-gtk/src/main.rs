//! Legacy compatibility launcher.
//!
//! `cntrl-gtk` now forwards to the ICED panel so existing scripts and habits
//! continue to work while CNTRL stays ICED-first.

fn main() {
    if let Err(e) = cntrl_iced::run() {
        eprintln!("cntrl-gtk compatibility launcher failed: {e:?}");
        std::process::exit(1);
    }
}
