//! Unified `cntrl` entrypoint: `daemon`, `panel`, `devices`, `apply-theme`, …

mod daemon;

use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(name = "cntrl", version, about = "CNTRL — keyboard lighting control")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Run the Unix socket daemon (foreground; serves HID apply + theme watcher).
    Daemon {
        /// Start listening (default; kept for scripts).
        #[arg(long, default_value_t = true)]
        start: bool,
    },
    /// Full ICED desktop app (same as `cargo run -p cntrl-iced`).
    Panel,
    /// List RGB devices CNTRL can control and their status (native; no external tools).
    Devices,
    /// Re-apply each device's last saved color/effect (run on login to persist RGB).
    Restore,
    /// Apply the current theme accent (from colors.conf) to the lighting.
    ApplyTheme {
        /// Profile slot to recolor (defaults to the persisted active slot).
        #[arg(long)]
        slot: Option<u8>,
        /// Apply only to the keyboard (skip PC RGB backends).
        #[arg(long, conflicts_with = "pc_only")]
        keyboard_only: bool,
        /// Apply only to PC RGB backends (skip keyboard).
        #[arg(long, conflicts_with = "keyboard_only")]
        pc_only: bool,
    },
    /// Probe RGB devices natively, persist mapping, and optionally apply env in-process.
    ProbeRgb {
        /// Force reprobe even if persisted config exists.
        #[arg(long)]
        force: bool,
        /// Skip applying probe values to current process env.
        #[arg(long)]
        no_apply: bool,
    },
    /// Quick lighting test frontend (no ad-hoc socket scripts).
    TestLight {
        /// Profile slot to read/update (0..=3).
        #[arg(long, default_value_t = 0)]
        slot: u8,
        /// Hex color (#rrggbb or rrggbb).
        #[arg(long, default_value = "#ff0000")]
        color: String,
        /// Effect: static | custom | off | <numeric effect id>.
        #[arg(long, default_value = "static")]
        effect: String,
        /// Optional speed override.
        #[arg(long)]
        speed: Option<i32>,
        /// Optional brightness override.
        #[arg(long)]
        brightness: Option<i32>,
        /// Print what would be applied without writing.
        #[arg(long)]
        dry_run: bool,
        /// Bypass daemon and apply locally in this process.
        #[arg(long)]
        local: bool,
        /// Apply only to the keyboard HID path.
        #[arg(long, conflicts_with = "pc_rgb_only")]
        keyboard_only: bool,
        /// Apply only to PC RGB backends (skip keyboard HID).
        #[arg(long, conflicts_with = "keyboard_only")]
        pc_rgb_only: bool,
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
        Commands::Panel => cntrl_iced::run().map_err(|e| format!("{e:?}")),
        Commands::Devices => run_devices(),
        Commands::Restore => run_restore(),
        Commands::ApplyTheme {
            slot,
            keyboard_only,
            pc_only,
        } => run_apply_theme(slot, keyboard_only, pc_only),
        Commands::ProbeRgb { force, no_apply } => run_probe_rgb(force, no_apply),
        Commands::TestLight {
            slot,
            color,
            effect,
            speed,
            brightness,
            dry_run,
            local,
            keyboard_only,
            pc_rgb_only,
        } => run_test_light(
            slot,
            &color,
            &effect,
            speed,
            brightness,
            dry_run,
            local,
            keyboard_only,
            pc_rgb_only,
        ),
    }
}

fn run_probe_rgb(force: bool, no_apply: bool) -> Result<(), String> {
    use cntrl_core::rgb_probe;

    let cfg = if force {
        let cfg = rgb_probe::probe_native()?;
        rgb_probe::write_probe_config(&cfg)?;
        cfg
    } else if let Some(cfg) = rgb_probe::read_probe_config()? {
        cfg
    } else {
        let cfg = rgb_probe::probe_native()?;
        rgb_probe::write_probe_config(&cfg)?;
        cfg
    };

    if !no_apply {
        rgb_probe::apply_probe_env(&cfg);
    }

    let path = rgb_probe::probe_config_path()?;
    println!("rgb probe config: {}", path.display());
    println!(
        "ite_hid_path={}",
        cfg.ite_hid_path.unwrap_or_else(|| "<none>".to_string())
    );
    println!(
        "gpu_i2c_bus={} gpu_i2c_addr={}",
        cfg.gpu_i2c_bus.unwrap_or_else(|| "<none>".to_string()),
        cfg.gpu_i2c_addr.unwrap_or_else(|| "<none>".to_string())
    );
    println!(
        "corsair_i2c_buses={}",
        if cfg.corsair_i2c_buses.is_empty() {
            "<none>".to_string()
        } else {
            cfg.corsair_i2c_buses.join(",")
        }
    );
    println!("env_applied={}", if no_apply { "false" } else { "true" });
    Ok(())
}

fn run_devices() -> Result<(), String> {
    use cntrl_core::devices::{self, DeviceStatus};

    let list = devices::enumerate();
    println!("CNTRL device inventory (native detection):\n");
    let mut blocked = false;
    for d in &list {
        let mark = match d.status {
            DeviceStatus::Ready => "●",
            DeviceStatus::Blocked => "✗",
            DeviceStatus::Absent => "○",
        };
        if d.status == DeviceStatus::Blocked {
            blocked = true;
        }
        let id = if d.vid != 0 {
            format!("[{}]  ", d.usb_id())
        } else {
            String::new()
        };
        println!("  {mark} {name}  {id}{kind}", name = d.name, kind = d.kind.label());
        println!(
            "      backend={backend}  status={status}",
            backend = d.backend_id,
            status = d.status.label(),
        );
        println!("      {}", d.detail);
    }
    if blocked {
        println!(
            "\nA device is permission-blocked. Install the udev rule:\n  \
             sudo install -Dm644 packaging/udev/99-mchose-ace68.rules \
             /etc/udev/rules.d/99-mchose-ace68.rules \\\n    \
             && sudo udevadm control --reload-rules && sudo udevadm trigger\n  \
             then replug the device."
        );
    }
    Ok(())
}

fn run_restore() -> Result<(), String> {
    let msg = cntrl_core::device_state::restore_all()?;
    println!("{msg}");
    Ok(())
}

/// Record the applied lighting per backend so `cntrl restore` can bring it back.
fn remember_targets(doc: &cntrl_core::profile::ProfileDocument, keyboard: bool, pc: bool) {
    let cfg = &doc.data.light.config;
    let mut backends: Vec<String> = Vec::new();
    if keyboard {
        backends.push("keyboard".into());
    }
    if pc {
        // Respect the currently selected PC backends (CNTRL_PC_RGB_BACKENDS), else all.
        let raw = std::env::var("CNTRL_PC_RGB_BACKENDS")
            .unwrap_or_else(|_| "corsair_ram,ite_rgb,gpu_rgb".to_string());
        for b in raw.split(',').map(str::trim).filter(|s| !s.is_empty()) {
            backends.push(b.to_string());
        }
    }
    if !backends.is_empty() {
        cntrl_core::device_state::remember(
            &backends,
            &cfg.hex_color,
            cfg.effect,
            cfg.brightness,
            cfg.speed,
        );
    }
}

fn run_apply_theme(slot: Option<u8>, keyboard_only: bool, pc_only: bool) -> Result<(), String> {
    use cntrl_core::ipc::ApplyOptions;
    use cntrl_core::persistence::read_startup_slot;
    use cntrl_core::theme_sync;

    let slot = slot.unwrap_or_else(read_startup_slot);
    if slot > 3 {
        return Err("slot must be 0..=3".to_string());
    }
    let options = if keyboard_only {
        ApplyOptions { keyboard: true, pc_rgb: false }
    } else if pc_only {
        ApplyOptions { keyboard: false, pc_rgb: true }
    } else {
        ApplyOptions::ALL
    };

    let (r, g, b) = theme_sync::apply_theme(slot, options)?;
    // Record so the theme color survives a reboot (via `cntrl restore`).
    if let Ok(doc) = cntrl_core::persistence::load_profile_slot(slot) {
        remember_targets(&doc, options.keyboard, options.pc_rgb);
    }
    println!("applied theme color #{r:02x}{g:02x}{b:02x} to slot {slot}");
    Ok(())
}

fn normalize_hex_color(input: &str) -> Result<String, String> {
    let t = input.trim().trim_start_matches('#');
    if t.len() != 6 || !t.chars().all(|c| c.is_ascii_hexdigit()) {
        return Err(format!(
            "invalid color {input:?}; expected #rrggbb or rrggbb"
        ));
    }
    Ok(format!("#{}", t.to_ascii_lowercase()))
}

fn parse_effect(effect: &str) -> Result<i32, String> {
    use cntrl_core::lighting_modes::{EFFECT_ID_OFF, MODE_CUSTOM, MODE_STATIC};
    let t = effect.trim().to_ascii_lowercase();
    match t.as_str() {
        "static" => Ok(MODE_STATIC),
        "custom" => Ok(MODE_CUSTOM),
        "off" => Ok(EFFECT_ID_OFF),
        _ => t
            .parse::<i32>()
            .map_err(|_| format!("invalid effect {effect:?}; use static|custom|off|<number>")),
    }
}

fn run_test_light(
    slot: u8,
    color: &str,
    effect: &str,
    speed: Option<i32>,
    brightness: Option<i32>,
    dry_run: bool,
    local: bool,
    keyboard_only: bool,
    pc_rgb_only: bool,
) -> Result<(), String> {
    use cntrl_core::ipc::{self, ApplyOptions};
    use cntrl_core::persistence::load_profile_slot;

    if slot > 3 {
        return Err("slot must be 0..=3".to_string());
    }

    let color = normalize_hex_color(color)?;
    let effect = parse_effect(effect)?;
    let mut doc = load_profile_slot(slot)?;

    doc.data.light.config.hex_color = color.clone();
    doc.data.light.config.effect = effect;
    if let Some(v) = speed {
        doc.data.light.config.speed = v;
    }
    if let Some(v) = brightness {
        doc.data.light.config.brightness = v;
    }

    if dry_run {
        println!(
            "dry-run slot={slot} effect={} color={} speed={} brightness={}",
            doc.data.light.config.effect,
            doc.data.light.config.hex_color,
            doc.data.light.config.speed,
            doc.data.light.config.brightness
        );
        return Ok(());
    }

    let isolated = keyboard_only || pc_rgb_only;
    let options = if keyboard_only {
        ApplyOptions {
            keyboard: true,
            pc_rgb: false,
        }
    } else if pc_rgb_only {
        ApplyOptions {
            keyboard: false,
            pc_rgb: true,
        }
    } else {
        ApplyOptions::ALL
    };

    if local || isolated {
        ipc::apply_local_with_options(&doc, slot, options)?;
    } else {
        ipc::apply_local_or_daemon(&doc, slot)?;
    }
    remember_targets(&doc, options.keyboard, options.pc_rgb);
    println!(
        "applied slot={slot} effect={} color={} speed={} brightness={} path={}",
        doc.data.light.config.effect,
        doc.data.light.config.hex_color,
        doc.data.light.config.speed,
        doc.data.light.config.brightness,
        if keyboard_only {
            "keyboard_only"
        } else if pc_rgb_only {
            "pc_rgb_only"
        } else if local {
            "local"
        } else {
            "daemon_or_local"
        }
    );
    Ok(())
}
