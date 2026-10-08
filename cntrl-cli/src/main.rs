//! Unified `cntrl` entrypoint: `daemon`, `panel`, `devices`, `apply-theme`, …

mod daemon;

use clap::{Args, Parser, Subcommand};

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
    /// Stream a canvas effect across ARGB devices using the saved layout
    /// (arrange devices in the panel's Canvas tab). Runs until Ctrl-C.
    Canvas {
        /// Effect: wave | plasma | ripple | comet | aurora (default: saved).
        #[arg(long)]
        effect: Option<String>,
        /// Frames per second (default: saved layout setting).
        #[arg(long)]
        fps: Option<u32>,
        /// Stop after N seconds (default: run until Ctrl-C).
        #[arg(long)]
        seconds: Option<u64>,
        /// List available effects and exit.
        #[arg(long)]
        list: bool,
    },
    /// Fan control: discover sensors/channels, validate, and drive curves.
    Fans(FanArgs),
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

#[derive(Args)]
struct FanArgs {
    #[command(subcommand)]
    command: FanCommands,
}

#[derive(Subcommand)]
enum FanCommands {
    /// List discovered temperature sensors and fan channels with status.
    List,
    /// Load the it87 Super I/O driver (experimental; requires root).
    It87Load {
        /// Forced chip id, e.g. 0x8689 for IT8689E.
        #[arg(long)]
        force_id: String,
    },
    /// Drive one channel to a fixed PWM and watch its RPM to see if it holds.
    It87Spike {
        /// Channel id from `fans list`, e.g. `it8689/pwm1`.
        #[arg(long)]
        channel: String,
        /// Raw PWM value to command (0..=255).
        #[arg(long, default_value_t = 200)]
        value: u8,
        /// How long to watch the tach, in seconds.
        #[arg(long, default_value_t = 30)]
        seconds: u64,
    },
    /// Print the saved fan curve config (~/.config/cntrl/fan_curves.json).
    Curves,
    /// Run one control tick and print the per-channel report.
    Apply {
        /// Compute targets but don't write to hardware.
        #[arg(long)]
        dry_run: bool,
    },
    /// Run the fan control engine in the foreground until Ctrl-C.
    Run,
    /// Restore all driven fans to their pre-engine state (recovery path).
    Restore,
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
        Commands::Canvas {
            effect,
            fps,
            seconds,
            list,
        } => run_canvas(effect.as_deref(), fps, seconds, list),
        Commands::Fans(args) => run_fans(args),
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

fn run_canvas(
    effect: Option<&str>,
    fps: Option<u32>,
    seconds: Option<u64>,
    list: bool,
) -> Result<(), String> {
    use cntrl_core::canvas_effects::EffectKind;

    if list {
        println!("canvas effects:");
        for kind in EffectKind::all() {
            println!("  {:<8} {}", kind.slug(), kind.label());
        }
        return Ok(());
    }

    let mut layout = cntrl_core::canvas::load_layout();
    if let Some(name) = effect {
        layout.settings.effect = EffectKind::from_slug(name)
            .ok_or_else(|| format!("unknown effect {name:?}; see `cntrl canvas --list`"))?;
    }
    if let Some(f) = fps {
        layout.settings.fps = f.clamp(1, 60);
    }

    let enabled: Vec<&str> = layout
        .devices
        .iter()
        .filter(|d| d.enabled)
        .map(|d| d.id.as_str())
        .collect();
    println!(
        "streaming {} at {} fps to [{}] — layout: {}",
        layout.settings.effect.label(),
        layout.settings.fps,
        enabled.join(", "),
        cntrl_core::canvas::layout_path()?.display(),
    );
    println!("Ctrl-C to stop.");

    let handle = cntrl_core::canvas_stream::start(layout);
    let deadline = seconds.map(|s| std::time::Instant::now() + std::time::Duration::from_secs(s));

    // Report stats once a second until the deadline / Ctrl-C.
    loop {
        std::thread::sleep(std::time::Duration::from_secs(1));
        {
            let stats = handle.stats.lock().map_err(|e| e.to_string())?;
            let err = stats
                .last_error
                .as_deref()
                .map(|e| format!(" · last error: {e}"))
                .unwrap_or_default();
            println!(
                "  {:.0} fps · frame {} · {}{}",
                stats.actual_fps, stats.frames, stats.backends, err
            );
        }
        if let Some(d) = deadline {
            if std::time::Instant::now() >= d {
                break;
            }
        }
        if !handle.is_running() {
            return Err("stream thread exited unexpectedly".to_string());
        }
    }
    handle.stop();
    println!("stream stopped.");
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

fn run_fans(args: FanArgs) -> Result<(), String> {
    match args.command {
        FanCommands::List => run_fans_list(),
        FanCommands::It87Load { force_id } => {
            let msg = cntrl_core::fan_control::it87_load(&force_id)?;
            println!("{msg}");
            println!(
                "Now refresh hwmon permissions:\n  \
                 sudo udevadm control --reload-rules && sudo udevadm trigger --subsystem-match=hwmon\n\
                 then run `cntrl fans list` to see the new it87 channels."
            );
            Ok(())
        }
        FanCommands::It87Spike {
            channel,
            value,
            seconds,
        } => run_fans_spike(&channel, value, seconds),
        FanCommands::Curves => run_fans_curves(),
        FanCommands::Apply { dry_run } => run_fans_apply(dry_run),
        FanCommands::Run => run_fans_engine(),
        FanCommands::Restore => {
            let msg = cntrl_core::fan_curves::restore_all();
            println!("{msg}");
            Ok(())
        }
    }
}

fn run_fans_curves() -> Result<(), String> {
    let path = cntrl_core::fan_curves::curves_path()?;
    match std::fs::read_to_string(&path) {
        Ok(raw) => {
            println!("# {}", path.display());
            print!("{raw}");
            Ok(())
        }
        Err(_) => {
            println!("no fan curve config yet at {}", path.display());
            println!("(create curves in the panel's Fans tab, or edit the JSON directly)");
            Ok(())
        }
    }
}

fn run_fans_apply(dry_run: bool) -> Result<(), String> {
    use cntrl_core::{fan_control, fan_curves};

    let cfg = fan_curves::load();
    if cfg.assignments.is_empty() {
        println!("no fan assignments configured — nothing to apply");
        return Ok(());
    }
    let discovery = fan_control::discover();
    // A real apply leaves fans in manual mode: refuse while an engine owns them,
    // and save their original state first so `cntrl fans restore` can undo it.
    let _lock = if dry_run {
        None
    } else {
        let lock = cntrl_core::fan_engine::acquire_lock()?;
        fan_curves::capture_originals(&cfg, &discovery)?;
        Some(lock)
    };
    let mut state = fan_curves::TickState::default();
    let summary = fan_curves::tick(&cfg, &discovery, &mut state, dry_run);

    println!("{}", summary.short_message());
    for r in &summary.reports {
        let target = r
            .target_pct
            .map(|p| format!("{p}%"))
            .unwrap_or_else(|| "—".to_string());
        println!("  {} → {target}  [{:?}]  {}", r.channel_id, r.result, r.reason);
    }
    if !dry_run {
        println!("fans now hold these speeds; nothing follows temperature until");
        println!("`cntrl fans run` or the daemon takes over. Undo with `cntrl fans restore`.");
    }
    Ok(())
}

fn run_fans_engine() -> Result<(), String> {
    use cntrl_core::{fan_curves, fan_engine};

    let cfg = fan_curves::load();
    if !cfg.enabled {
        println!("note: fan control is disabled (enabled=false in fan_curves.json).");
        println!("      The engine will idle until you enable it (e.g. in the Fans tab).");
    }

    let _lock = fan_engine::acquire_lock()?;
    fan_engine::install_panic_hook();
    fan_engine::install_signal_handlers();

    println!("fan engine running — Ctrl-C to stop and restore fans.");
    fan_engine::run_loop(|summary| {
        let has_activity = summary
            .reports
            .iter()
            .any(|r| !matches!(r.result, cntrl_core::fan_curves::ApplyResult::Skipped));
        if has_activity {
            println!("  {}", summary.short_message());
            for r in &summary.reports {
                if !matches!(r.result, cntrl_core::fan_curves::ApplyResult::Skipped) {
                    println!("    {} {}", r.channel_id, r.reason);
                }
            }
        }
    });
    Ok(())
}

fn run_fans_list() -> Result<(), String> {
    use cntrl_core::fan_control::{self, FanChannelStatus};

    let disc = fan_control::discover();
    let mut blocked = false;

    println!("Temperature sensors ({}):\n", disc.sensors.len());
    for s in &disc.sensors {
        let temp = s
            .read_celsius()
            .map(|c| format!("{c:.1}°C"))
            .unwrap_or_else(|_| "—".to_string());
        println!("  ● {id}  [{chip}]  {label}  {temp}", id = s.id, chip = s.chip, label = s.label);
    }

    println!("\nFan channels ({}):\n", disc.channels.len());
    for c in &disc.channels {
        let mark = match c.status {
            FanChannelStatus::Controllable => "●",
            FanChannelStatus::Blocked => "✗",
            FanChannelStatus::ReadOnly => "◐",
        };
        if c.status == FanChannelStatus::Blocked {
            blocked = true;
        }
        let rpm = c
            .read_rpm()
            .map(|r| format!("{r} RPM"))
            .unwrap_or_else(|| "no tach".to_string());
        let duty = c
            .read_pwm()
            .map(|raw| format!("{}%", fan_control::raw_to_pct(raw)))
            .unwrap_or_else(|_| "—".to_string());
        let exp = if c.experimental { "  [experimental]" } else { "" };
        println!("  {mark} {id}  [{chip}]  {rpm}  duty={duty}{exp}", id = c.id, chip = c.chip);
        println!("      status={}", c.status.label());
        println!("      {}", c.detail);
    }

    if let Some(warning) = fan_control::it87_in_tree_warning() {
        println!("\nWARNING: {warning}");
    }
    if blocked {
        println!(
            "\nA channel is permission-blocked. Install the hwmon udev rule:\n  \
             sudo install -Dm644 packaging/udev/99-cntrl-hwmon.rules \
             /etc/udev/rules.d/99-cntrl-hwmon.rules \\\n    \
             && sudo udevadm control --reload-rules \
             && sudo udevadm trigger --subsystem-match=hwmon"
        );
    }
    Ok(())
}

fn run_fans_spike(channel_id: &str, value: u8, seconds: u64) -> Result<(), String> {
    use cntrl_core::fan_control;

    let disc = fan_control::discover();
    let channel = disc
        .channel(channel_id)
        .ok_or_else(|| format!("unknown channel {channel_id:?}; see `cntrl fans list`"))?;

    println!(
        "Spiking {channel_id} to raw PWM {value} ({}%) for {seconds}s — watching {}...",
        fan_control::raw_to_pct(value as u32),
        channel
            .tach_file
            .as_ref()
            .map(|_| "tach")
            .unwrap_or("(no tach)"),
    );
    if let Some(b) = channel.read_rpm() {
        println!("  baseline: {b} RPM");
    }

    let report = fan_control::it87_spike(channel, value, seconds, |elapsed, rpm| {
        let rpm = rpm.map(|r| format!("{r} RPM")).unwrap_or_else(|| "—".to_string());
        println!("  t={elapsed:>3}s  {rpm}");
    })?;

    println!("\n  verdict: {}", report.verdict.label());
    println!("  {}", report.note);
    println!("  (channel restored to its original state)");
    Ok(())
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

    // `apply_theme` records the applied state itself (device_state::remember_applied);
    // re-saving the on-disk slot here would clobber it with the pre-promotion effect.
    let (r, g, b) = theme_sync::apply_theme(slot, options)?;
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
    cntrl_core::device_state::remember_applied(&doc, options);
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
