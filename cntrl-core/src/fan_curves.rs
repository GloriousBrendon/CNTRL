//! Fan curve model, evaluation, persistence, and the per-tick control engine.
//!
//! A curve maps a smoothed temperature to a duty percent. Fans are driven by
//! assigning a curve to a channel. This mirrors Windows "Fan Control": graph
//! curves (draggable temp→duty points, linear interp, clamped ends) and mix
//! curves (`max()` of referenced curves).
//!
//! The engine is intentionally *not* a trait+registry like `rgb_backends.rs` —
//! GPU and motherboard fans differ only in *which* hwmon chip, not in *how* they
//! are driven (uniform sysfs I/O in [`crate::fan_control`]). What is worth
//! mirroring is the typed per-channel report shape, for consistent CLI/log UX.

use crate::fan_control::{FanChannel, FanDiscovery};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::collections::HashMap;
use std::path::PathBuf;

const MAX_MIX_DEPTH: usize = 8;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CurvePoint {
    pub temp_c: f32,
    pub duty_pct: u8,
}

/// How the fans should sound. A preset replaces the shape of every graph
/// curve (each fan keeps its sensor and limits); `Custom` uses the stored points.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FanMode {
    Silent,
    Balanced,
    Performance,
    #[default]
    Custom,
}

const fn pt(temp_c: f32, duty_pct: u8) -> CurvePoint {
    CurvePoint { temp_c, duty_pct }
}

impl FanMode {
    pub const ALL: [FanMode; 4] =
        [FanMode::Silent, FanMode::Balanced, FanMode::Performance, FanMode::Custom];

    pub fn label(self) -> &'static str {
        match self {
            FanMode::Silent => "Silent",
            FanMode::Balanced => "Balanced",
            FanMode::Performance => "Performance",
            FanMode::Custom => "Custom",
        }
    }

    /// The preset's curve (docs/design/HANDOFF.md); `None` for `Custom`.
    pub fn preset(self) -> Option<&'static [CurvePoint]> {
        const SILENT: [CurvePoint; 5] =
            [pt(30.0, 20), pt(50.0, 25), pt(65.0, 40), pt(80.0, 70), pt(90.0, 100)];
        const BALANCED: [CurvePoint; 5] =
            [pt(30.0, 20), pt(50.0, 40), pt(70.0, 75), pt(85.0, 100), pt(95.0, 100)];
        const PERFORMANCE: [CurvePoint; 5] =
            [pt(30.0, 35), pt(45.0, 55), pt(60.0, 80), pt(75.0, 100), pt(90.0, 100)];
        match self {
            FanMode::Silent => Some(&SILENT),
            FanMode::Balanced => Some(&BALANCED),
            FanMode::Performance => Some(&PERFORMANCE),
            FanMode::Custom => None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum CurveKind {
    /// Temp→duty graph over a sensor, or over the hotter of two.
    Graph {
        sensor_id: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        also_sensor_id: Option<String>,
        points: Vec<CurvePoint>,
    },
    /// The max() of the referenced member curves (Fan Control's "Mix Curve").
    Mix { member_ids: Vec<String> },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Curve {
    pub id: String,
    pub name: String,
    #[serde(flatten)]
    pub kind: CurveKind,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FanAssignment {
    pub channel_id: String,
    pub curve_id: String,
    #[serde(default = "default_min_duty")]
    pub min_duty_pct: u8,
    #[serde(default = "default_max_duty")]
    pub max_duty_pct: u8,
}

fn default_min_duty() -> u8 {
    20
}
fn default_max_duty() -> u8 {
    100
}
fn default_tick_ms() -> u64 {
    2000
}
fn default_alpha() -> f32 {
    0.3
}
fn default_min_change() -> u8 {
    3
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct FanCurveConfig {
    pub curves: Vec<Curve>,
    pub assignments: Vec<FanAssignment>,
    /// Control loop cadence. Slower than theme_watcher's 1s — fan hunting costs more.
    pub tick_interval_ms: u64,
    /// EMA smoothing factor for temps (Fan Control's "Response Time"), 0..=1.
    pub temp_smoothing_alpha: f32,
    /// Skip writing a channel if the target duty moved less than this many points.
    pub min_change_pct: u8,
    /// Master enable for the control engine.
    pub enabled: bool,
    pub mode: FanMode,
    /// The panel has put each fan on its default sensors once (first mode pick).
    pub sensors_seeded: bool,
}

impl Default for FanCurveConfig {
    fn default() -> Self {
        Self {
            curves: Vec::new(),
            assignments: Vec::new(),
            tick_interval_ms: default_tick_ms(),
            temp_smoothing_alpha: default_alpha(),
            min_change_pct: default_min_change(),
            enabled: false,
            mode: FanMode::default(),
            sensors_seeded: false,
        }
    }
}

impl FanCurveConfig {
    pub fn curve(&self, id: &str) -> Option<&Curve> {
        self.curves.iter().find(|c| c.id == id)
    }

    /// Evaluate a curve id to a duty percent given current smoothed temps.
    /// Returns `None` if the curve/sensor is missing or a mix cycle is detected.
    pub fn eval_curve(&self, curve_id: &str, temps: &HashMap<String, f32>) -> Option<u8> {
        self.eval_curve_depth(curve_id, temps, self.mode, 0)
    }

    /// Like [`eval_curve`], but always on the stored points (no preset).
    pub fn eval_own_curve(&self, curve_id: &str, temps: &HashMap<String, f32>) -> Option<u8> {
        self.eval_curve_depth(curve_id, temps, FanMode::Custom, 0)
    }

    fn eval_curve_depth(
        &self,
        curve_id: &str,
        temps: &HashMap<String, f32>,
        mode: FanMode,
        depth: usize,
    ) -> Option<u8> {
        if depth > MAX_MIX_DEPTH {
            return None;
        }
        let curve = self.curve(curve_id)?;
        match &curve.kind {
            CurveKind::Graph { sensor_id, also_sensor_id, points } => {
                let mut t = *temps.get(sensor_id)?;
                // A lost second sensor fails the curve too: the fan goes back
                // to firmware rather than ignoring the part it was watching.
                if let Some(also) = also_sensor_id {
                    t = t.max(*temps.get(also)?);
                }
                Some(eval_graph(mode.preset().unwrap_or(points), t))
            }
            CurveKind::Mix { member_ids } => {
                let mut best = 0u8;
                let mut any = false;
                for m in member_ids {
                    if let Some(d) = self.eval_curve_depth(m, temps, mode, depth + 1) {
                        best = best.max(d);
                        any = true;
                    }
                }
                any.then_some(best)
            }
        }
    }
}

/// Linear interpolation between ordered points with clamped extrapolation.
/// Below the first point → first point's duty; above the last → last point's duty.
pub fn eval_graph(points: &[CurvePoint], temp_c: f32) -> u8 {
    if points.is_empty() {
        return 0;
    }
    // Points are kept sorted by temp on save, but sort defensively for eval.
    let mut pts: Vec<&CurvePoint> = points.iter().collect();
    pts.sort_by(|a, b| a.temp_c.partial_cmp(&b.temp_c).unwrap_or(std::cmp::Ordering::Equal));

    if temp_c <= pts[0].temp_c {
        return pts[0].duty_pct;
    }
    let last = pts[pts.len() - 1];
    if temp_c >= last.temp_c {
        return last.duty_pct;
    }
    for w in pts.windows(2) {
        let (a, b) = (w[0], w[1]);
        if temp_c >= a.temp_c && temp_c <= b.temp_c {
            let span = b.temp_c - a.temp_c;
            if span <= f32::EPSILON {
                return b.duty_pct;
            }
            let frac = (temp_c - a.temp_c) / span;
            let duty = a.duty_pct as f32 + frac * (b.duty_pct as f32 - a.duty_pct as f32);
            return duty.round().clamp(0.0, 100.0) as u8;
        }
    }
    last.duty_pct
}

// ---- Reporting (mirrors rgb_backends::ApplyResult / BackendReport shape) ----

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ApplyResult {
    /// Duty written to hardware this tick.
    Written,
    /// Target unchanged (within min_change threshold) — no write needed.
    Skipped,
    /// Channel/curve/sensor missing or not controllable.
    Unsupported,
    /// The curve could not be evaluated (lost sensor), so the channel was handed
    /// back to firmware control until it can be.
    HandedBack,
    /// A write or read failed.
    Error,
}

#[derive(Debug, Clone)]
pub struct FanApplyReport {
    pub channel_id: String,
    pub target_pct: Option<u8>,
    pub result: ApplyResult,
    pub reason: String,
}

#[derive(Debug, Clone, Default)]
pub struct FanTickSummary {
    pub reports: Vec<FanApplyReport>,
}

impl FanTickSummary {
    pub fn short_message(&self) -> String {
        let written = self.reports.iter().filter(|r| r.result == ApplyResult::Written).count();
        let skipped = self.reports.iter().filter(|r| r.result == ApplyResult::Skipped).count();
        let err = self
            .reports
            .iter()
            .filter(|r| matches!(r.result, ApplyResult::Error | ApplyResult::Unsupported))
            .count();
        let handed_back = self
            .reports
            .iter()
            .filter(|r| r.result == ApplyResult::HandedBack)
            .count();
        if handed_back > 0 {
            return format!(
                "fans: written={written}, skipped={skipped}, error={err}, handed back to firmware={handed_back}"
            );
        }
        format!("fans: written={written}, skipped={skipped}, error={err}")
    }
}

/// Long-lived control state owned by the tick loop — must NOT be reloaded from
/// disk each tick (that would reset the smoothing and re-trigger writes).
#[derive(Debug, Clone, Default)]
pub struct TickState {
    /// sensor_id → smoothed temperature (EMA).
    smoothed: HashMap<String, f32>,
    /// channel_id → last duty% actually written.
    last_written: HashMap<String, u8>,
}

/// Run one control tick: read + smooth temps, evaluate each assignment, and write
/// duty to channels whose target moved enough. Shared by the daemon thread and
/// `cntrl fans apply/run`.
pub fn tick(
    cfg: &FanCurveConfig,
    discovery: &FanDiscovery,
    state: &mut TickState,
    dry_run: bool,
) -> FanTickSummary {
    // A channel that lost its assignment must be written afresh if it returns.
    state
        .last_written
        .retain(|id, _| cfg.assignments.iter().any(|a| &a.channel_id == id));

    // 1. Read + smooth every sensor referenced by any graph curve.
    update_smoothed(cfg, discovery, state);
    let temps = state.smoothed.clone();

    // 2. Per assignment, evaluate and (maybe) write.
    let mut reports = Vec::new();
    for a in &cfg.assignments {
        reports.push(apply_assignment(cfg, discovery, state, &temps, a, dry_run));
    }
    FanTickSummary { reports }
}

fn update_smoothed(cfg: &FanCurveConfig, discovery: &FanDiscovery, state: &mut TickState) {
    let alpha = cfg.temp_smoothing_alpha.clamp(0.01, 1.0);
    let mut wanted: Vec<String> = Vec::new();
    for c in &cfg.curves {
        if let CurveKind::Graph { sensor_id, also_sensor_id, .. } = &c.kind {
            wanted.push(sensor_id.clone());
            wanted.extend(also_sensor_id.clone());
        }
    }
    wanted.sort();
    wanted.dedup();

    for sid in wanted {
        // A sensor we can't read has no temperature: drop the stale value so
        // its curves stop evaluating instead of holding the last duty forever.
        let Some(raw) = discovery.sensor(&sid).and_then(|s| s.read_celsius().ok()) else {
            state.smoothed.remove(&sid);
            continue;
        };
        let next = match state.smoothed.get(&sid) {
            Some(prev) => alpha * raw + (1.0 - alpha) * prev,
            None => raw, // seed with the first real reading
        };
        state.smoothed.insert(sid, next);
    }
}

fn apply_assignment(
    cfg: &FanCurveConfig,
    discovery: &FanDiscovery,
    state: &mut TickState,
    temps: &HashMap<String, f32>,
    a: &FanAssignment,
    dry_run: bool,
) -> FanApplyReport {
    let unsupported = |reason: String| FanApplyReport {
        channel_id: a.channel_id.clone(),
        target_pct: None,
        result: ApplyResult::Unsupported,
        reason,
    };

    let Some(channel) = discovery.channel(&a.channel_id) else {
        return unsupported(format!("channel {} not present", a.channel_id));
    };
    // A pump is not part of the presets: it keeps its own curve.
    let pump = channel.is_pump();
    let evaluated = if pump {
        cfg.eval_own_curve(&a.curve_id, temps)
    } else {
        cfg.eval_curve(&a.curve_id, temps)
    };
    let Some(raw_target) = evaluated else {
        let why = format!(
            "curve {} could not be evaluated (missing sensor or cycle)",
            a.curve_id
        );
        if dry_run || !matches!(channel.status, crate::fan_control::FanChannelStatus::Controllable) {
            return unsupported(why);
        }
        // Already handed back on an earlier tick: nothing to do (or to log).
        if channel.read_enable() != Some(crate::fan_control::PWM_ENABLE_MANUAL) {
            return unsupported(format!("{why}; under firmware control"));
        }
        // Never leave a fan in manual mode with nothing watching temperature.
        state.last_written.remove(&a.channel_id);
        return match channel.set_auto() {
            Ok(()) => FanApplyReport {
                channel_id: a.channel_id.clone(),
                target_pct: None,
                result: ApplyResult::HandedBack,
                reason: format!("{why}; handed back to firmware control"),
            },
            Err(e) => FanApplyReport {
                channel_id: a.channel_id.clone(),
                target_pct: None,
                result: ApplyResult::Error,
                reason: format!("{why}; AND failed to hand back to firmware: {e}"),
            },
        };
    };

    // Clamp to the assignment's duty envelope.
    let floor = if pump { crate::fan_control::PUMP_MIN_DUTY_PCT } else { 0 };
    let lo = a.min_duty_pct.max(floor).min(100);
    let hi = a.max_duty_pct.min(100).max(lo);
    let target = raw_target.clamp(lo, hi);

    // Dry-run: report the computed target without writing, even if the channel
    // isn't currently controllable (useful for previewing curve output).
    if dry_run {
        let note = if matches!(
            channel.status,
            crate::fan_control::FanChannelStatus::Controllable
        ) {
            format!("dry-run: would write {target}%")
        } else {
            format!("dry-run: would write {target}% (but channel not controllable: {})", channel.detail)
        };
        return FanApplyReport {
            channel_id: a.channel_id.clone(),
            target_pct: Some(target),
            result: ApplyResult::Skipped,
            reason: note,
        };
    }

    if !matches!(
        channel.status,
        crate::fan_control::FanChannelStatus::Controllable
    ) {
        return unsupported(format!("channel not controllable: {}", channel.detail));
    }

    // Write-skip: only when the target barely moved AND the hardware still
    // holds what we last wrote — something else (`cntrl fans restore`, the
    // firmware) may have changed the fan behind our back.
    if let Some(prev) = state.last_written.get(&a.channel_id) {
        let untouched = channel.read_enable() == Some(crate::fan_control::PWM_ENABLE_MANUAL)
            && channel.read_pwm().is_ok_and(|raw| {
                // ±2 raw: some drivers round the duty they report back.
                raw.abs_diff(u32::from(crate::fan_control::pct_to_raw(*prev))) <= 2
            });
        if untouched && target.abs_diff(*prev) < cfg.min_change_pct {
            return FanApplyReport {
                channel_id: a.channel_id.clone(),
                target_pct: Some(target),
                result: ApplyResult::Skipped,
                reason: format!("target {target}% within {}pt of last", cfg.min_change_pct),
            };
        }
    }

    match write_channel(channel, target) {
        Ok(()) => {
            state.last_written.insert(a.channel_id.clone(), target);
            FanApplyReport {
                channel_id: a.channel_id.clone(),
                target_pct: Some(target),
                result: ApplyResult::Written,
                reason: format!("wrote {target}%"),
            }
        }
        Err(e) => FanApplyReport {
            channel_id: a.channel_id.clone(),
            target_pct: Some(target),
            result: ApplyResult::Error,
            reason: e,
        },
    }
}

/// Ensure manual mode then write the duty. Manual mode is idempotent, so it's
/// cheap to re-assert each write (guards against the EC flipping it back). The
/// verified variant catches drivers that silently ignore the manual switch.
fn write_channel(channel: &FanChannel, target_pct: u8) -> Result<(), String> {
    channel.set_manual_verified()?;
    channel.write_duty_pct(target_pct)
}

// ---------------------------- persistence ----------------------------

fn config_dir() -> Result<PathBuf, String> {
    dirs::config_dir()
        .map(|p| p.join("cntrl"))
        .ok_or_else(|| "Could not resolve config directory".to_string())
}

pub fn curves_path() -> Result<PathBuf, String> {
    Ok(config_dir()?.join("fan_curves.json"))
}

/// Load the curve config, returning defaults if missing/unreadable.
pub fn load() -> FanCurveConfig {
    let Ok(path) = curves_path() else {
        return FanCurveConfig::default();
    };
    match std::fs::read_to_string(&path) {
        Ok(raw) => serde_json::from_str(&raw).unwrap_or_default(),
        Err(_) => FanCurveConfig::default(),
    }
}

pub fn save(cfg: &FanCurveConfig) -> Result<(), String> {
    std::fs::create_dir_all(config_dir()?).map_err(|e| e.to_string())?;
    let raw = serde_json::to_string_pretty(cfg).map_err(|e| e.to_string())?;
    let final_path = curves_path()?;
    let tmp = final_path.with_file_name(format!("fan_curves.json.tmp.{}", std::process::id()));
    std::fs::write(&tmp, raw).map_err(|e| e.to_string())?;
    std::fs::rename(&tmp, &final_path).map_err(|e| e.to_string())
}

// ------------------------ safety / restore state ------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OriginalPwmState {
    pub enable_value: Option<u32>,
    pub pwm_value: u32,
}

/// channel_id → its pre-engine enable/pwm values, so we can put fans back exactly.
pub type OriginalStates = BTreeMap<String, OriginalPwmState>;

pub fn original_state_path() -> Result<PathBuf, String> {
    Ok(config_dir()?.join("fan_original_state.json"))
}

pub fn load_original_states() -> OriginalStates {
    let Ok(path) = original_state_path() else {
        return OriginalStates::new();
    };
    match std::fs::read_to_string(&path) {
        Ok(raw) => serde_json::from_str(&raw).unwrap_or_default(),
        Err(_) => OriginalStates::new(),
    }
}

pub fn save_original_states(states: &OriginalStates) -> Result<(), String> {
    std::fs::create_dir_all(config_dir()?).map_err(|e| e.to_string())?;
    let raw = serde_json::to_string_pretty(states).map_err(|e| e.to_string())?;
    let final_path = original_state_path()?;
    let tmp = final_path.with_file_name(format!(
        "fan_original_state.json.tmp.{}",
        std::process::id()
    ));
    std::fs::write(&tmp, raw).map_err(|e| e.to_string())?;
    std::fs::rename(&tmp, &final_path).map_err(|e| e.to_string())
}

pub fn clear_original_states() {
    if let Ok(path) = original_state_path() {
        let _ = std::fs::remove_file(path);
    }
}

/// Record the current enable/pwm for every assigned+controllable channel that we
/// don't already have on file. Call before any tick that writes: the tick itself
/// switches a channel to manual, only once it has a duty to write.
pub fn capture_originals(cfg: &FanCurveConfig, discovery: &FanDiscovery) -> Result<(), String> {
    let mut states = load_original_states();
    if capture_into(&mut states, cfg, discovery)? {
        save_original_states(&states)?;
    }
    Ok(())
}

fn capture_into(
    states: &mut OriginalStates,
    cfg: &FanCurveConfig,
    discovery: &FanDiscovery,
) -> Result<bool, String> {
    let mut changed = false;
    for a in &cfg.assignments {
        let Some(channel) = discovery.channel(&a.channel_id) else {
            continue;
        };
        if !matches!(
            channel.status,
            crate::fan_control::FanChannelStatus::Controllable
        ) {
            continue;
        }
        if !states.contains_key(&a.channel_id) {
            let pwm_value = channel.read_pwm()?;
            states.insert(
                a.channel_id.clone(),
                OriginalPwmState {
                    enable_value: channel.read_enable(),
                    pwm_value,
                },
            );
            changed = true;
        }
    }
    Ok(changed)
}

/// Restore every recorded channel to its captured enable/pwm, then delete the
/// state file. Idempotent — safe to call from a signal handler, panic hook, or
/// `cntrl fans restore`. Returns a human-readable summary.
pub fn restore_all() -> String {
    restore_where(|_| true).unwrap_or_else(|| "no fan state to restore".to_string())
}

/// Restore channels the engine once drove but `cfg` no longer assigns (curve
/// removed in the UI), so they don't sit in manual mode unwatched. `None` when
/// there was nothing to release.
pub fn release_unassigned(cfg: &FanCurveConfig) -> Option<String> {
    restore_where(|id| !cfg.assignments.iter().any(|a| a.channel_id == id))
}

/// Restore + forget every recorded channel matching `pick`. `None` if none match.
fn restore_where(pick: impl Fn(&str) -> bool) -> Option<String> {
    let mut states = load_original_states();
    if !states.keys().any(|id| pick(id)) {
        return None;
    }
    let discovery = crate::fan_control::discover();
    let (ok, errors) = restore_states(&mut states, &discovery, pick);
    if states.is_empty() {
        clear_original_states();
    } else if let Err(e) = save_original_states(&states) {
        eprintln!("cntrl fans: could not update saved fan state: {e}");
    }
    Some(if errors.is_empty() {
        format!("restored {ok} fan channel(s)")
    } else {
        format!("restored {ok}; errors: {}", errors.join(" | "))
    })
}

pub(crate) fn restore_states(
    states: &mut OriginalStates,
    discovery: &FanDiscovery,
    pick: impl Fn(&str) -> bool,
) -> (usize, Vec<String>) {
    let mut ok = 0usize;
    let mut errors: Vec<String> = Vec::new();
    states.retain(|id, orig| {
        if !pick(id) {
            return true;
        }
        match discovery.channel(id) {
            Some(channel) => match channel.restore(orig.enable_value, orig.pwm_value) {
                Ok(()) => ok += 1,
                Err(e) => errors.push(format!("{id}: {e}")),
            },
            None => errors.push(format!("{id}: channel no longer present")),
        }
        false
    });
    (ok, errors)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pt(temp_c: f32, duty_pct: u8) -> CurvePoint {
        CurvePoint { temp_c, duty_pct }
    }

    #[test]
    fn graph_interpolates_and_clamps() {
        let pts = vec![pt(30.0, 20), pt(50.0, 40), pt(70.0, 80)];
        // Below first point → clamped to first duty.
        assert_eq!(eval_graph(&pts, 10.0), 20);
        // Above last point → clamped to last duty.
        assert_eq!(eval_graph(&pts, 90.0), 80);
        // Exactly on a point.
        assert_eq!(eval_graph(&pts, 50.0), 40);
        // Midway 30→50 (20→40) at 40°C → 30.
        assert_eq!(eval_graph(&pts, 40.0), 30);
        // Midway 50→70 (40→80) at 60°C → 60.
        assert_eq!(eval_graph(&pts, 60.0), 60);
    }

    #[test]
    fn graph_handles_unsorted_points() {
        let pts = vec![pt(70.0, 80), pt(30.0, 20), pt(50.0, 40)];
        assert_eq!(eval_graph(&pts, 40.0), 30);
    }

    #[test]
    fn graph_follows_the_hotter_of_two_sensors() {
        let mut cfg = FanCurveConfig::default();
        cfg.curves.push(Curve {
            id: "c".into(),
            name: "c".into(),
            kind: CurveKind::Graph {
                sensor_id: "cpu".into(),
                also_sensor_id: Some("gpu".into()),
                points: vec![pt(0.0, 0), pt(100.0, 100)],
            },
        });
        let mut temps = HashMap::from([("cpu".to_string(), 40.0), ("gpu".to_string(), 70.0)]);
        assert_eq!(cfg.eval_curve("c", &temps), Some(70));
        temps.insert("cpu".into(), 90.0);
        assert_eq!(cfg.eval_curve("c", &temps), Some(90));
        temps.remove("gpu");
        assert_eq!(cfg.eval_curve("c", &temps), None);
    }

    #[test]
    fn pump_keeps_its_own_curve_and_a_60_percent_floor() {
        let root = fake_hwmon("pump");
        let label = [("fakechip".to_string(), "pwm1".to_string(), "SYS_FAN4_PUMP".to_string())];
        let d = crate::fan_control::discover_in(&root, &label);
        assert!(d.channel("fakechip/pwm1").unwrap().is_pump());

        // Own curve says 40% at 40°C; Silent would say 22%. Both are under the floor.
        let mut cfg = fake_cfg();
        cfg.mode = FanMode::Silent;
        tick(&cfg, &d, &mut TickState::default(), false);
        assert_eq!(read(&root, "pwm1"), crate::fan_control::pct_to_raw(60).to_string());

        // Above the floor the pump follows its own curve, not the preset.
        if let CurveKind::Graph { points, .. } = &mut cfg.curves[0].kind {
            *points = vec![pt(30.0, 20), pt(50.0, 100)];
        }
        std::fs::write(root.join("hwmon0/temp1_input"), "50000\n").unwrap();
        tick(&cfg, &d, &mut TickState::default(), false);
        assert_eq!(read(&root, "pwm1"), crate::fan_control::pct_to_raw(100).to_string(), "Silent would give 25%");

        // The same header without the pump name follows the preset.
        let plain = crate::fan_control::discover_in(&root, &[]);
        tick(&cfg, &plain, &mut TickState::default(), false);
        assert_eq!(read(&root, "pwm1"), crate::fan_control::pct_to_raw(25).to_string());
        let _ = std::fs::remove_dir_all(root);
    }

    /// What the watchdog does after a hard kill: the engine left the fan in
    /// manual mode, its lock vanished with the process, the fan goes back.
    #[test]
    fn watchdog_restores_fans_once_the_engine_is_gone() {
        let root = fake_hwmon("watchdog");
        let lock = std::env::temp_dir().join(format!("cntrl-wd-lock-{}", std::process::id()));
        let engine = crate::fan_engine::acquire_lock_at(&lock).unwrap();
        // The engine took the fan over (was: firmware mode, pwm 66) ...
        let mut states = OriginalStates::new();
        states.insert(
            "fakechip/pwm1".into(),
            OriginalPwmState { enable_value: Some(2), pwm_value: 66 },
        );
        std::fs::write(root.join("hwmon0/pwm1_enable"), "1\n").unwrap();
        std::fs::write(root.join("hwmon0/pwm1"), "200\n").unwrap();

        let (root2, lock2) = (root.clone(), lock.clone());
        let watchdog = std::thread::spawn(move || {
            crate::fan_engine::wait_for_engine_exit(&lock2).unwrap();
            let d = crate::fan_control::discover_in(&root2, &[]);
            restore_states(&mut states, &d, |_| true)
        });
        // ... and while it lives the watchdog must not touch anything.
        std::thread::sleep(std::time::Duration::from_millis(150));
        assert_eq!(read(&root, "pwm1_enable"), "1");

        // The kernel drops the lock when the process dies, however it dies.
        drop(engine);
        let (restored, errors) = watchdog.join().unwrap();
        assert_eq!((restored, errors.len()), (1, 0));
        assert_eq!(read(&root, "pwm1_enable"), "2");
        let _ = std::fs::remove_dir_all(root);
        let _ = std::fs::remove_file(lock);
    }

    #[test]
    fn preset_mode_replaces_the_curve_shape_and_custom_keeps_it() {
        let mut cfg = FanCurveConfig::default();
        cfg.curves.push(Curve {
            id: "c".into(),
            name: "c".into(),
            kind: CurveKind::Graph {
                sensor_id: "cpu".into(),
                also_sensor_id: None,
                points: vec![pt(0.0, 90), pt(100.0, 90)],
            },
        });
        let temps = HashMap::from([("cpu".to_string(), 50.0)]);
        assert_eq!(cfg.eval_curve("c", &temps), Some(90));
        cfg.mode = FanMode::Silent;
        assert_eq!(cfg.eval_curve("c", &temps), Some(25));
        cfg.mode = FanMode::Performance;
        assert_eq!(cfg.eval_curve("c", &temps), Some(63));
        // A config written before modes existed loads as Custom.
        let old: FanCurveConfig = serde_json::from_str("{\"enabled\":true}").unwrap();
        assert_eq!(old.mode, FanMode::Custom);
    }

    #[test]
    fn mix_takes_max_of_members() {
        let mut cfg = FanCurveConfig::default();
        cfg.curves.push(Curve {
            id: "a".into(),
            name: "a".into(),
            kind: CurveKind::Graph {
                sensor_id: "s1".into(),
                also_sensor_id: None,
                points: vec![pt(0.0, 10), pt(100.0, 30)],
            },
        });
        cfg.curves.push(Curve {
            id: "b".into(),
            name: "b".into(),
            kind: CurveKind::Graph {
                sensor_id: "s2".into(),
                also_sensor_id: None,
                points: vec![pt(0.0, 50), pt(100.0, 60)],
            },
        });
        cfg.curves.push(Curve {
            id: "mix".into(),
            name: "mix".into(),
            kind: CurveKind::Mix {
                member_ids: vec!["a".into(), "b".into()],
            },
        });
        let mut temps = HashMap::new();
        temps.insert("s1".to_string(), 50.0);
        temps.insert("s2".to_string(), 50.0);
        // a→20, b→55 → max = 55.
        assert_eq!(cfg.eval_curve("mix", &temps), Some(55));
    }

    #[test]
    fn mix_cycle_is_detected() {
        let mut cfg = FanCurveConfig::default();
        cfg.curves.push(Curve {
            id: "m".into(),
            name: "m".into(),
            kind: CurveKind::Mix {
                member_ids: vec!["m".into()],
            },
        });
        // Self-reference must not loop forever and must not evaluate.
        assert_eq!(cfg.eval_curve("m", &HashMap::new()), None);
    }

    /// A fake hwmon tree with one chip: firmware-controlled fan at raw 66, 40°C.
    fn fake_hwmon(tag: &str) -> PathBuf {
        let root = std::env::temp_dir().join(format!("cntrl-hwmon-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let chip = root.join("hwmon0");
        std::fs::create_dir_all(&chip).unwrap();
        for (file, value) in [
            ("name", "fakechip"),
            ("pwm1", "66"),
            ("pwm1_enable", "2"),
            ("fan1_input", "900"),
            ("temp1_input", "40000"),
        ] {
            std::fs::write(chip.join(file), format!("{value}\n")).unwrap();
        }
        root
    }

    fn read(root: &std::path::Path, file: &str) -> String {
        std::fs::read_to_string(root.join("hwmon0").join(file))
            .unwrap()
            .trim()
            .to_string()
    }

    /// 30°C→20%, 50°C→60% on fakechip/temp1, driving fakechip/pwm1.
    fn fake_cfg() -> FanCurveConfig {
        FanCurveConfig {
            enabled: true,
            curves: vec![Curve {
                id: "c".into(),
                name: "c".into(),
                kind: CurveKind::Graph {
                    sensor_id: "fakechip/temp1".into(),
                    also_sensor_id: None,
                    points: vec![pt(30.0, 20), pt(50.0, 60)],
                },
            }],
            assignments: vec![FanAssignment {
                channel_id: "fakechip/pwm1".into(),
                curve_id: "c".into(),
                min_duty_pct: 20,
                max_duty_pct: 100,
            }],
            ..FanCurveConfig::default()
        }
    }

    #[test]
    fn tick_writes_duty_and_engages_manual() {
        let root = fake_hwmon("write");
        let d = crate::fan_control::discover_in(&root, &[]);
        let mut state = TickState::default();
        let summary = tick(&fake_cfg(), &d, &mut state, false);
        assert_eq!(summary.reports[0].result, ApplyResult::Written);
        // 40°C is midway 30→50, so 40% → raw 102.
        assert_eq!(read(&root, "pwm1"), "102");
        assert_eq!(read(&root, "pwm1_enable"), "1");
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn steady_temperature_skips_unless_fan_was_changed_behind_us() {
        let root = fake_hwmon("skip");
        let cfg = fake_cfg();
        let d = crate::fan_control::discover_in(&root, &[]);
        let mut state = TickState::default();
        tick(&cfg, &d, &mut state, false);
        assert_eq!(tick(&cfg, &d, &mut state, false).reports[0].result, ApplyResult::Skipped);

        // Someone else rewrites the duty: the next tick must put it back.
        std::fs::write(root.join("hwmon0/pwm1"), "30\n").unwrap();
        assert_eq!(tick(&cfg, &d, &mut state, false).reports[0].result, ApplyResult::Written);
        assert_eq!(read(&root, "pwm1"), "102");

        // Someone hands the fan to firmware (`cntrl fans restore`): re-engage.
        std::fs::write(root.join("hwmon0/pwm1_enable"), "2\n").unwrap();
        assert_eq!(tick(&cfg, &d, &mut state, false).reports[0].result, ApplyResult::Written);
        assert_eq!(read(&root, "pwm1_enable"), "1");
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn lost_sensor_hands_fan_back_then_recovers() {
        let root = fake_hwmon("sensor");
        let cfg = fake_cfg();
        let mut state = TickState::default();
        tick(&cfg, &crate::fan_control::discover_in(&root, &[]), &mut state, false);
        assert_eq!(read(&root, "pwm1_enable"), "1");

        // Sensor vanishes (driver reload): firmware takes the fan back.
        std::fs::remove_file(root.join("hwmon0/temp1_input")).unwrap();
        let summary = tick(&cfg, &crate::fan_control::discover_in(&root, &[]), &mut state, false);
        assert_eq!(summary.reports[0].result, ApplyResult::HandedBack);
        assert_eq!(read(&root, "pwm1_enable"), "2");

        // Sensor returns at the same temperature: the engine must re-engage
        // even though the target equals the last duty it wrote.
        std::fs::write(root.join("hwmon0/temp1_input"), "40000\n").unwrap();
        let summary = tick(&cfg, &crate::fan_control::discover_in(&root, &[]), &mut state, false);
        assert_eq!(summary.reports[0].result, ApplyResult::Written);
        assert_eq!(read(&root, "pwm1_enable"), "1");
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn restore_puts_removed_assignment_back_in_firmware_mode() {
        let root = fake_hwmon("restore");
        let cfg = fake_cfg();
        let d = crate::fan_control::discover_in(&root, &[]);
        let mut states = OriginalStates::new();
        assert!(capture_into(&mut states, &cfg, &d).unwrap());
        // Capturing alone must not touch the fan.
        assert_eq!(read(&root, "pwm1_enable"), "2");
        tick(&cfg, &d, &mut TickState::default(), false);
        assert_eq!(read(&root, "pwm1_enable"), "1");

        // Still assigned: nothing is released.
        let assigned = |id: &str| !cfg.assignments.iter().any(|a| a.channel_id == id);
        assert_eq!(restore_states(&mut states, &d, assigned), (0, vec![]));
        assert_eq!(states.len(), 1);

        // Assignment gone (or engine disabled): back to automatic, and the PWM
        // value is left to the firmware rather than rewritten.
        assert_eq!(restore_states(&mut states, &d, |_| true), (1, vec![]));
        assert!(states.is_empty());
        assert_eq!(read(&root, "pwm1_enable"), "2");
        assert_eq!(read(&root, "pwm1"), "102");
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn pct_raw_roundtrip_endpoints() {
        assert_eq!(crate::fan_control::pct_to_raw(0), 0);
        assert_eq!(crate::fan_control::pct_to_raw(100), 255);
        assert_eq!(crate::fan_control::raw_to_pct(0), 0);
        assert_eq!(crate::fan_control::raw_to_pct(255), 100);
    }
}
