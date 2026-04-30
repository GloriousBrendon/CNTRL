//! Unified `cntrl` entrypoint: `daemon`, `panel`, `popup <widget> [--toggle]`.

mod daemon;
mod popup_pid;

use clap::{Parser, Subcommand};
use cntrl_iced::UiMode;

#[derive(Parser)]
#[command(name = "cntrl", version, about = "CNTRL — keyboard lighting control")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Run the Unix socket daemon (foreground; serves HID apply + ping).
    Daemon {
        /// Start listening (default; kept for Waybar/scripts).
        #[arg(long, default_value_t = true)]
        start: bool,
    },
    /// Full ICED shell (same as `cargo run -p cntrl-iced`).
    Panel,
    /// Compact ICED window; use `--toggle` for Waybar show/hide.
    Popup {
        /// Widget: keeb-color | keeb-effects | keeb-controls | keeb-profiles
        widget: String,
        #[arg(long)]
        toggle: bool,
    },
}

fn main() {
    let code = match run() {
        Ok(()) => 0,
        Err(e) => {
            eprintln!("{e}");
            1
        }
    };
    std::process::exit(code);
}

fn run() -> Result<(), String> {
    let cli = Cli::parse();
    match cli.command {
        Commands::Daemon { start: _ } => daemon::run(),
        Commands::Panel => cntrl_iced::run_with(UiMode::Panel).map_err(|e| format!("{e:?}")),
        Commands::Popup { widget, toggle } => {
            let mode = UiMode::from_slug(&widget)?;
            if toggle {
                match popup_pid::try_close_existing_popup(mode) {
                    Ok(true) => Ok(()),
                    Ok(false) => {
                        spawn_popup_detached(mode);
                        Ok(())
                    }
                    Err(e) => Err(e),
                }
            } else {
                popup_pid::write_pid_file(mode)?;
                let res = cntrl_iced::run_with(mode).map_err(|e| format!("{e:?}"));
                popup_pid::remove_pid_file(mode);
                res
            }
        }
    }
}

fn spawn_popup_detached(mode: UiMode) {
    let Ok(exe) = std::env::current_exe() else {
        return;
    };
    if let Err(e) = std::process::Command::new(exe)
        .args(["popup", mode.slug()])
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
    {
        eprintln!("cntrl: spawn popup: {e}");
    }
}
