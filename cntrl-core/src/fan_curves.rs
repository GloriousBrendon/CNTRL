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

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum CurveKind {
    /// Temp→duty graph over a single sensor.
    Graph {
        sensor_id: String,
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
        self.eval_curve_depth(curve_id, temps, 0)
    }

    fn eval_curve_depth(
        &self,
        curve_id: &str,
        temps: &HashMap<String, f32>,
        depth: usize,
    ) -> Option<u8> {
        if depth > MAX_MIX_DEPTH {
            return None;
        }
        let curve = self.curve(curve_id)?;
        match &curve.kind {
            CurveKind::Graph { sensor_id, points } => {
                let t = *temps.get(sensor_id)?;
                Some(eval_graph(points, t))
            }
            CurveKind::Mix { member_ids } => {
                let mut best = 0u8;
                let mut any = false;
                for m in member_ids {
                    if let Some(d) = self.eval_curve_depth(m, temps, depth + 1) {
                        best = best.max(d);
                        any = true;
                    }
                }
                any.then_some(best)
            }
        }
    }

    /// Reject a would-be mix edge that creates a cycle (checked at save time).
    pub fn mix_would_cycle(&self, from_curve: &str, new_member: &str) -> bool {
        if from_curve == new_member {
            return true;
        }
        // Walk new_member's transitive members; if we reach from_curve, it cycles.
        let mut stack = vec![new_member.to_string()];
        let mut seen = 0usize;
        while let Some(id) = stack.pop() {
            seen += 1;
            if seen > 256 {
                return true; // pathological — treat as cycle
            }
            if id == from_curve {
                return true;
            }
            if let Some(Curve {
                kind: CurveKind::Mix { member_ids },
                ..
            }) = self.curve(&id)
            {
                stack.extend(member_ids.iter().cloned());
            }
        }
        false
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

impl TickState {
    pub fn smoothed_temp(&self, sensor_id: &str) -> Option<f32> {
        self.smoothed.get(sensor_id).copied()
    }
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
        if let CurveKind::Graph { sensor_id, .. } = &c.kind {
            wanted.push(sensor_id.clone());
        }
    }
    wanted.sort();
    wanted.dedup();

    for sid in wanted {
        let Some(sensor) = discovery.sensor(&sid) else {
            continue;
        };
        let Ok(raw) = sensor.read_celsius() else {
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
    let Some(raw_target) = cfg.eval_curve(&a.curve_id, temps) else {
        return unsupported(format!(
            "curve {} could not be evaluated (missing sensor or cycle)",
            a.curve_id
        ));
    };

    // Clamp to the assignment's duty envelope.
    let lo = a.min_duty_pct.min(100);
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

    // Write-skip: only write when the target moved enough vs. last written.
    if let Some(prev) = state.last_written.get(&a.channel_id) {
        if target.abs_diff(*prev) < cfg.min_change_pct {
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
/// don't already have on file, then flip it to manual mode. Call on engine start.
pub fn capture_and_engage(cfg: &FanCurveConfig, discovery: &FanDiscovery) -> Result<(), String> {
    let mut states = load_original_states();
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
        // Engage manual mode now so the first tick's write takes effect.
        let _ = channel.set_manual();
    }
    if changed {
        save_original_states(&states)?;
    }
    Ok(())
}

/// Restore every recorded channel to its captured enable/pwm, then delete the
/// state file. Idempotent — safe to call from a signal handler, panic hook, or
/// `cntrl fans restore`. Returns a human-readable summary.
pub fn restore_all() -> String {
    let states = load_original_states();
    if states.is_empty() {
        return "no fan state to restore".to_string();
    }
    let discovery = crate::fan_control::discover();
    let mut ok = 0usize;
    let mut errors: Vec<String> = Vec::new();
    for (id, orig) in &states {
        match discovery.channel(id) {
            Some(channel) => match channel.restore(orig.enable_value, orig.pwm_value) {
                Ok(()) => ok += 1,
                Err(e) => errors.push(format!("{id}: {e}")),
            },
            None => errors.push(format!("{id}: channel no longer present")),
        }
    }
    clear_original_states();
    if errors.is_empty() {
        format!("restored {ok} fan channel(s)")
    } else {
        format!("restored {ok}; errors: {}", errors.join(" | "))
    }
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
    fn mix_takes_max_of_members() {
        let mut cfg = FanCurveConfig::default();
        cfg.curves.push(Curve {
            id: "a".into(),
            name: "a".into(),
            kind: CurveKind::Graph {
                sensor_id: "s1".into(),
                points: vec![pt(0.0, 10), pt(100.0, 30)],
            },
        });
        cfg.curves.push(Curve {
            id: "b".into(),
            name: "b".into(),
            kind: CurveKind::Graph {
                sensor_id: "s2".into(),
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
        assert!(cfg.mix_would_cycle("m", "m"));
    }

    #[test]
    fn pct_raw_roundtrip_endpoints() {
        assert_eq!(crate::fan_control::pct_to_raw(0), 0);
        assert_eq!(crate::fan_control::pct_to_raw(100), 255);
        assert_eq!(crate::fan_control::raw_to_pct(0), 0);
        assert_eq!(crate::fan_control::raw_to_pct(255), 100);
    }
}
