//! Native fan/temperature discovery and raw PWM I/O over `/sys/class/hwmon`.
//!
//! No external tools, no extra crates — hwmon is plain sysfs file I/O. Mirrors
//! `devices.rs`'s "report presence + accessibility, and explain *why* something
//! is not controllable" philosophy: each fan channel carries a status and a
//! human-readable `detail` so the UI/CLI can tell the user what to fix (usually a
//! missing udev rule → `Permission denied` on the `pwm*` attribute files).
//!
//! GPU fans (`amdgpu`) are controllable via the stock kernel driver. Motherboard
//! Super I/O headers (`it87`) are experimental: the chip is often not
//! auto-detected and, even when loaded, the board's embedded controller may
//! re-assert its own fan curve and ignore PWM writes — see `it87_spike`.

use std::collections::HashMap;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

/// hwmon `pwmN_enable` value meaning "software/manual control".
pub const PWM_ENABLE_MANUAL: u32 = 1;
/// hwmon `pwmN_enable` value meaning "firmware/automatic control" (amdgpu, it87).
pub const PWM_ENABLE_AUTO: u32 = 2;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FanChannelStatus {
    /// `pwmN` and `pwmN_enable` both exist and are openable read/write.
    Controllable,
    /// Files exist but can't be opened read/write (almost always udev/perms).
    Blocked,
    /// `pwmN` exists but has no `pwmN_enable` sibling — can't force manual mode safely.
    ReadOnly,
}

impl FanChannelStatus {
    pub fn label(self) -> &'static str {
        match self {
            FanChannelStatus::Controllable => "Controllable",
            FanChannelStatus::Blocked => "Permission blocked",
            FanChannelStatus::ReadOnly => "Read-only (no enable)",
        }
    }
}

/// A temperature input that can drive a fan curve.
#[derive(Debug, Clone)]
pub struct TempSensor {
    /// Stable id used in curve refs + persisted config, e.g. `amdgpu/temp1`.
    pub id: String,
    /// hwmon `name` file contents, e.g. `amdgpu`, `k10temp`, `gigabyte_wmi`.
    pub chip: String,
    /// `tempN_label` if present, else `temp{N}`.
    pub label: String,
    pub input_file: PathBuf,
}

impl TempSensor {
    /// Read the current temperature in °C (hwmon reports millidegrees).
    pub fn read_celsius(&self) -> Result<f32, String> {
        read_sysfs_i64(&self.input_file).map(|milli| milli as f32 / 1000.0)
    }
}

/// A controllable (or would-be-controllable) fan PWM channel.
#[derive(Debug, Clone)]
pub struct FanChannel {
    /// Stable id, e.g. `amdgpu/pwm1`.
    pub id: String,
    pub chip: String,
    pub pwm_file: PathBuf,
    pub enable_file: Option<PathBuf>,
    /// `fanN_input` (RPM tach) if a matching one exists.
    pub tach_file: Option<PathBuf>,
    pub status: FanChannelStatus,
    /// Why the channel is (not) controllable — actionable text for the user.
    pub detail: String,
    /// True for non-GPU chips (`it87`, …): drives a permanent "Experimental" badge.
    pub experimental: bool,
}

impl FanChannel {
    /// Read the raw PWM duty (0..=255).
    pub fn read_pwm(&self) -> Result<u32, String> {
        read_sysfs_u32(&self.pwm_file)
    }

    /// Read the tach RPM if this channel has a `fanN_input`.
    pub fn read_rpm(&self) -> Option<u32> {
        self.tach_file
            .as_ref()
            .and_then(|p| read_sysfs_u32(p).ok())
    }

    /// Read the current `pwmN_enable` mode if present.
    pub fn read_enable(&self) -> Option<u32> {
        self.enable_file
            .as_ref()
            .and_then(|p| read_sysfs_u32(p).ok())
    }

    /// Hand the channel back to firmware (automatic) control.
    pub fn set_auto(&self) -> Result<(), String> {
        match &self.enable_file {
            Some(p) => write_sysfs(p, &PWM_ENABLE_AUTO.to_string()),
            None => Err(format!("{}: no pwm enable file", self.id)),
        }
    }

    /// Set manual mode and verify it actually took. Some drivers accept the write
    /// at the syscall level but silently keep automatic control — notably RDNA3
    /// `amdgpu` when overdrive is disabled — which would otherwise surface only as
    /// a confusing `EINVAL` on the following `pwm` write.
    pub fn set_manual_verified(&self) -> Result<(), String> {
        let Some(p) = &self.enable_file else {
            return Err(format!("{}: no pwm enable file", self.id));
        };
        write_sysfs(p, &PWM_ENABLE_MANUAL.to_string())?;
        match read_sysfs_u32(p) {
            Ok(v) if v == PWM_ENABLE_MANUAL => Ok(()),
            Ok(other) => Err(format!(
                "driver ignored manual-mode request (pwm_enable stayed {other}, wanted 1){}",
                self.manual_hint()
            )),
            Err(e) => Err(e),
        }
    }

    /// Chip-specific hint for why manual mode may be refused.
    fn manual_hint(&self) -> String {
        if self.chip == "amdgpu" {
            " — amdgpu overdrive is likely disabled; boot with \
             amdgpu.ppfeaturemask=0xffffffff to unlock fan control"
                .to_string()
        } else {
            String::new()
        }
    }

    /// Write a duty percent (0..=100), converting to hwmon's 0..=255 range.
    pub fn write_duty_pct(&self, pct: u8) -> Result<(), String> {
        let raw = pct_to_raw(pct);
        write_sysfs(&self.pwm_file, &raw.to_string())
    }

    /// Write a raw PWM value (0..=255) directly (used by the spike/test path).
    pub fn write_raw(&self, raw: u8) -> Result<(), String> {
        write_sysfs(&self.pwm_file, &raw.to_string())
    }

    /// Restore a previously-captured enable mode, then the raw PWM value if
    /// that mode is manual — drivers reject PWM writes in automatic mode
    /// (it87: EBUSY), and the firmware owns the duty there anyway.
    pub fn restore(&self, enable_value: Option<u32>, pwm_value: u32) -> Result<(), String> {
        if let (Some(p), Some(v)) = (&self.enable_file, enable_value) {
            write_sysfs(p, &v.to_string())?;
            if v != PWM_ENABLE_MANUAL {
                return Ok(());
            }
        }
        write_sysfs(&self.pwm_file, &pwm_value.to_string())
    }
}

/// Convert a duty percent (0..=100) to a raw hwmon PWM value (0..=255).
pub fn pct_to_raw(pct: u8) -> u8 {
    let pct = pct.min(100) as u32;
    ((pct * 255 + 50) / 100) as u8
}

/// Convert a raw hwmon PWM value (0..=255) to a duty percent (0..=100).
pub fn raw_to_pct(raw: u32) -> u8 {
    ((raw.min(255) * 100 + 127) / 255) as u8
}

#[derive(Debug, Clone, Default)]
pub struct FanDiscovery {
    pub sensors: Vec<TempSensor>,
    pub channels: Vec<FanChannel>,
}

/// One snapshot of every live value, keyed by sensor/channel id — so a UI can
/// read sysfs once per poll instead of on every redraw.
#[derive(Debug, Clone, Default)]
pub struct FanReadings {
    pub temps_c: HashMap<String, f32>,
    pub rpm: HashMap<String, u32>,
    pub pwm_raw: HashMap<String, u32>,
}

impl FanDiscovery {
    pub fn channel(&self, id: &str) -> Option<&FanChannel> {
        self.channels.iter().find(|c| c.id == id)
    }

    pub fn sensor(&self, id: &str) -> Option<&TempSensor> {
        self.sensors.iter().find(|s| s.id == id)
    }

    pub fn read_all(&self) -> FanReadings {
        let mut r = FanReadings::default();
        for s in &self.sensors {
            if let Ok(c) = s.read_celsius() {
                r.temps_c.insert(s.id.clone(), c);
            }
        }
        for c in &self.channels {
            if let Some(rpm) = c.read_rpm() {
                r.rpm.insert(c.id.clone(), rpm);
            }
            if let Ok(pwm) = c.read_pwm() {
                r.pwm_raw.insert(c.id.clone(), pwm);
            }
        }
        r
    }
}

const HWMON_ROOT: &str = "/sys/class/hwmon";

/// Enumerate every temperature sensor and fan PWM channel on this machine.
/// Pure read-only: openability is checked by opening+dropping, never writing.
pub fn discover() -> FanDiscovery {
    discover_in(Path::new(HWMON_ROOT))
}

/// `discover` over an arbitrary hwmon root (tests point this at a fake tree).
pub fn discover_in(root: &Path) -> FanDiscovery {
    let mut sensors = Vec::new();
    let mut channels = Vec::new();

    let entries = match fs::read_dir(root) {
        Ok(e) => e,
        Err(_) => return FanDiscovery::default(),
    };

    let mut dirs: Vec<PathBuf> = entries
        .filter_map(|e| e.ok().map(|e| e.path()))
        .collect();
    // Numeric order (hwmon2 before hwmon10), so `chip#2` follows kernel order.
    dirs.sort_by_key(|d| hwmon_index(d));

    // Multiple hwmon dirs can share a `name` (e.g. two `nvme`, two `amdgpu`).
    // The first occurrence keeps the bare chip name, later ones get `chip#2`,
    // etc. Kernel hwmon numbering can change between boots, so ids of duplicate
    // chips are stable within a boot only.
    let mut chip_counts: HashMap<String, u32> = HashMap::new();

    for dir in dirs {
        let chip = read_sysfs_string(&dir.join("name")).unwrap_or_else(|_| "unknown".to_string());
        let experimental = chip != "amdgpu";
        let count = chip_counts.entry(chip.clone()).or_insert(0);
        *count += 1;
        let chip_key = if *count == 1 {
            chip.clone()
        } else {
            format!("{chip}#{count}")
        };

        // Sensors: tempN_input (+ optional tempN_label).
        for n in sysfs_indices(&dir, "temp", "_input") {
            let input = dir.join(format!("temp{n}_input"));
            let label = read_sysfs_string(&dir.join(format!("temp{n}_label")))
                .unwrap_or_else(|_| format!("temp{n}"));
            sensors.push(TempSensor {
                id: format!("{chip_key}/temp{n}"),
                chip: chip.clone(),
                label,
                input_file: input,
            });
        }

        // Channels: pwmN (+ optional pwmN_enable, + optional fanN_input tach).
        for n in sysfs_indices(&dir, "pwm", "") {
            let pwm_file = dir.join(format!("pwm{n}"));
            let enable_path = dir.join(format!("pwm{n}_enable"));
            let enable_file = enable_path.exists().then_some(enable_path);
            let tach_path = dir.join(format!("fan{n}_input"));
            let tach_file = tach_path.exists().then_some(tach_path);

            let (status, detail) = classify_channel(&pwm_file, enable_file.as_deref());
            channels.push(FanChannel {
                id: format!("{chip_key}/pwm{n}"),
                chip: chip.clone(),
                pwm_file,
                enable_file,
                tach_file,
                status,
                detail,
                experimental,
            });
        }
    }

    FanDiscovery { sensors, channels }
}

fn hwmon_index(dir: &Path) -> u32 {
    dir.file_name()
        .and_then(|n| n.to_str())
        .and_then(|n| n.strip_prefix("hwmon"))
        .and_then(|n| n.parse().ok())
        .unwrap_or(u32::MAX)
}

/// The in-tree `it87` driver reads Gigabyte boards' fans but its PWM writes are
/// ignored by the board's fan controller; the out-of-tree driver is needed.
/// `Some(warning)` when the in-tree one is loaded (the kernel marks out-of-tree
/// modules with an `O` taint flag).
pub fn it87_in_tree_warning() -> Option<&'static str> {
    let taint = fs::read_to_string("/sys/module/it87/taint").ok()?;
    (!taint.contains('O')).then_some(
        "the kernel's built-in it87 driver is loaded. On recent Gigabyte boards it \
         reads fans but speed writes are silently ignored. Install the out-of-tree \
         driver (AUR: it87-dkms-git) plus the headers for your running kernel, then \
         `sudo modprobe -r it87 && sudo modprobe it87`.",
    )
}

/// Decide a channel's controllability + a human-readable reason.
fn classify_channel(pwm_file: &Path, enable_file: Option<&Path>) -> (FanChannelStatus, String) {
    if enable_file.is_none() {
        return (
            FanChannelStatus::ReadOnly,
            format!("{}: no pwm enable sibling — cannot switch to manual", pwm_file.display()),
        );
    }
    match node_writable(pwm_file) {
        Ok(()) => (
            FanChannelStatus::Controllable,
            pwm_file.display().to_string(),
        ),
        Err(e) => (
            FanChannelStatus::Blocked,
            format!("{}: {e} — install packaging/udev/99-cntrl-hwmon.rules", pwm_file.display()),
        ),
    }
}

/// Enumerate the numeric indices for a `<prefix>N<suffix>` sysfs family (sorted).
fn sysfs_indices(dir: &Path, prefix: &str, suffix: &str) -> Vec<u32> {
    let mut out = Vec::new();
    let Ok(entries) = fs::read_dir(dir) else {
        return out;
    };
    for e in entries.filter_map(|e| e.ok()) {
        let name = e.file_name();
        let name = name.to_string_lossy();
        let Some(rest) = name.strip_prefix(prefix) else {
            continue;
        };
        // For pwm we want the bare `pwmN` (suffix ""), not `pwmN_enable` etc.
        let core = if suffix.is_empty() {
            rest
        } else {
            match rest.strip_suffix(suffix) {
                Some(c) => c,
                None => continue,
            }
        };
        if let Ok(n) = core.parse::<u32>() {
            out.push(n);
        }
    }
    out.sort_unstable();
    out.dedup();
    out
}

/// Can the current user open this sysfs attribute read+write? (Opens and drops —
/// no write, so hardware state is untouched.) Mirrors `devices::node_openable`.
fn node_writable(path: &Path) -> Result<(), String> {
    fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open(path)
        .map(|_| ())
        .map_err(|e| e.to_string())
}

fn read_sysfs_string(path: &Path) -> Result<String, String> {
    fs::read_to_string(path)
        .map(|s| s.trim().to_string())
        .map_err(|e| format!("{}: {e}", path.display()))
}

fn read_sysfs_u32(path: &Path) -> Result<u32, String> {
    read_sysfs_string(path)?
        .parse::<u32>()
        .map_err(|e| format!("{}: {e}", path.display()))
}

fn read_sysfs_i64(path: &Path) -> Result<i64, String> {
    read_sysfs_string(path)?
        .parse::<i64>()
        .map_err(|e| format!("{}: {e}", path.display()))
}

fn write_sysfs(path: &Path, value: &str) -> Result<(), String> {
    let mut f = fs::OpenOptions::new()
        .write(true)
        .truncate(true) // no-op on sysfs; keeps regular files (tests) exact
        .open(path)
        .map_err(|e| format!("{}: {e}", path.display()))?;
    f.write_all(value.as_bytes())
        .map_err(|e| format!("{}: {e}", path.display()))
}

/// Load the `it87` Super I/O driver with a forced chip id (e.g. `0x8689`).
/// Explicit, user-invoked only — never called by the daemon. Requires root.
pub fn it87_load(force_id: &str) -> Result<String, String> {
    let force_id = force_id.trim();
    if !force_id.starts_with("0x") || u32::from_str_radix(&force_id[2..], 16).is_err() {
        return Err(format!("invalid force_id {force_id:?}; expected hex like 0x8689"));
    }
    let out = std::process::Command::new("modprobe")
        .args([
            "it87",
            "ignore_resource_conflict=1",
            &format!("force_id={force_id}"),
        ])
        .output()
        .map_err(|e| format!("failed to run modprobe: {e}"))?;
    if out.status.success() {
        Ok(format!("it87 loaded (force_id={force_id})"))
    } else {
        let stderr = String::from_utf8_lossy(&out.stderr);
        Err(format!("modprobe it87 failed: {}", stderr.trim()))
    }
}

/// Result of the it87/PWM validation spike.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SpikeVerdict {
    /// RPM converged toward and held near the commanded duty.
    Pass,
    /// RPM drifted back to baseline — the EC re-asserted its own curve.
    Fail,
    /// No tach reading available — can't tell whether the write held.
    Inconclusive,
}

impl SpikeVerdict {
    pub fn label(self) -> &'static str {
        match self {
            SpikeVerdict::Pass => "PASS",
            SpikeVerdict::Fail => "FAIL",
            SpikeVerdict::Inconclusive => "INCONCLUSIVE",
        }
    }
}

#[derive(Debug, Clone)]
pub struct SpikeReport {
    pub verdict: SpikeVerdict,
    pub baseline_rpm: Option<u32>,
    /// (elapsed_secs, rpm) samples over the window.
    pub samples: Vec<(u64, Option<u32>)>,
    pub commanded_raw: u8,
    pub note: String,
}

/// Drive a single channel to a fixed raw PWM value and watch its tach for
/// `seconds` to decide whether the write actually holds. Saves and restores the
/// channel's original enable/pwm regardless of outcome.
///
/// `sample`, if provided, is called once per second with (elapsed_secs, rpm) so
/// a CLI can stream progress live.
pub fn it87_spike(
    channel: &FanChannel,
    value: u8,
    seconds: u64,
    mut sample: impl FnMut(u64, Option<u32>),
) -> Result<SpikeReport, String> {
    if channel.enable_file.is_none() {
        return Err(format!("{}: no pwm enable file; cannot force manual", channel.id));
    }
    let seconds = seconds.max(3);

    let orig_enable = channel.read_enable();
    let orig_pwm = channel.read_pwm()?;
    let baseline_rpm = steady_rpm(channel);

    // Ensure we always restore, even on an early error mid-window.
    let mut run = || -> Result<Vec<(u64, Option<u32>)>, String> {
        channel.set_manual_verified()?;
        channel.write_raw(value)?;
        let mut samples = Vec::new();
        for s in 1..=seconds {
            std::thread::sleep(std::time::Duration::from_secs(1));
            let rpm = channel.read_rpm();
            sample(s, rpm);
            samples.push((s, rpm));
        }
        Ok(samples)
    };

    let result = run();
    // Restore no matter what happened above.
    let restore_err = channel.restore(orig_enable, orig_pwm).err();

    let samples = match result {
        Ok(s) => s,
        Err(e) => {
            let mut msg = e;
            if let Some(re) = restore_err {
                msg = format!("{msg}; ALSO failed to restore: {re}");
            }
            return Err(msg);
        }
    };

    let (verdict, note) = judge_spike(baseline_rpm, value, &samples);
    let note = match restore_err {
        Some(re) => format!("{note}; WARNING restore failed: {re}"),
        None => note,
    };

    Ok(SpikeReport {
        verdict,
        baseline_rpm,
        samples,
        commanded_raw: value,
        note,
    })
}

/// Baseline RPM once the fan has settled: a fan still ramping (e.g. just handed
/// back to firmware by a previous spike) makes any later reading look like a
/// response. Waits up to 12 s for two readings 2 s apart to agree within 3%.
fn steady_rpm(channel: &FanChannel) -> Option<u32> {
    let mut prev = channel.read_rpm()?;
    for _ in 0..6 {
        std::thread::sleep(std::time::Duration::from_secs(2));
        let now = channel.read_rpm()?;
        if now.abs_diff(prev) * 100 <= prev.max(1) * 3 {
            return Some(now);
        }
        prev = now;
    }
    Some(prev)
}

/// Decide PASS/FAIL/INCONCLUSIVE from the tach samples.
///
/// We look at the back half of the window (steady state). If no tach data is
/// present, it's INCONCLUSIVE. Otherwise we compare the steady-state average
/// against the pre-write baseline: a meaningful, sustained move away from
/// baseline in the commanded direction is a PASS; staying near baseline is a
/// FAIL (the EC ignored the write).
fn judge_spike(
    baseline_rpm: Option<u32>,
    commanded_raw: u8,
    samples: &[(u64, Option<u32>)],
) -> (SpikeVerdict, String) {
    let steady: Vec<u32> = samples
        .iter()
        .skip(samples.len() / 2)
        .filter_map(|(_, rpm)| *rpm)
        .collect();

    if steady.is_empty() {
        return (
            SpikeVerdict::Inconclusive,
            "no tach (fanN_input) readings — cannot verify the write held".to_string(),
        );
    }

    let steady_avg = steady.iter().sum::<u32>() as f32 / steady.len() as f32;

    let Some(baseline) = baseline_rpm.filter(|b| *b > 0) else {
        // No usable baseline; treat a nonzero commanded value producing spin as a weak pass.
        if commanded_raw > 0 && steady_avg > 100.0 {
            return (
                SpikeVerdict::Pass,
                format!("no baseline, but fan spins at {steady_avg:.0} RPM under command"),
            );
        }
        return (
            SpikeVerdict::Inconclusive,
            format!("no usable baseline RPM; steady-state {steady_avg:.0} RPM"),
        );
    };

    let baseline = baseline as f32;
    // Commanded higher duty than the fan was doing? Expect RPM up, and vice-versa.
    // Require the steady-state to move at least 12% away from baseline to count.
    let rel_change = (steady_avg - baseline) / baseline;
    let moved_enough = rel_change.abs() >= 0.12;

    if moved_enough {
        (
            SpikeVerdict::Pass,
            format!(
                "steady-state {steady_avg:.0} RPM vs baseline {baseline:.0} RPM ({:+.0}%) — write holds",
                rel_change * 100.0
            ),
        )
    } else {
        (
            SpikeVerdict::Fail,
            format!(
                "steady-state {steady_avg:.0} RPM stayed near baseline {baseline:.0} RPM ({:+.0}%) — EC likely overriding the write",
                rel_change * 100.0
            ),
        )
    }
}
