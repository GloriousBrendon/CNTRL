//! PC RGB backend abstraction and fan-out apply orchestration.
//!
//! M5.2 scope: define the contract and registry/config shape used by future
//! device implementations. Backends are currently stubs until M5.3+.

use crate::profile::ProfileDocument;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ApplyResult {
    Ok,
    Unsupported,
    Error,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BackendCapabilities {
    pub static_mode: bool,
    pub custom_fallback: bool,
    pub off_mode: bool,
    pub preset_subset: bool,
}

#[derive(Debug, Clone)]
pub struct BackendReport {
    pub backend_id: &'static str,
    pub result: ApplyResult,
    pub reason: String,
    pub capabilities: BackendCapabilities,
}

#[derive(Debug, Clone)]
pub struct PcRgbApplySummary {
    pub reports: Vec<BackendReport>,
}

impl PcRgbApplySummary {
    pub fn is_ok(&self) -> bool {
        self.reports.iter().all(|r| r.result == ApplyResult::Ok)
    }

    pub fn has_partial_failure(&self) -> bool {
        self.reports
            .iter()
            .any(|r| r.result == ApplyResult::Unsupported || r.result == ApplyResult::Error)
    }

    pub fn short_message(&self) -> String {
        if self.reports.is_empty() {
            return "pc-rgb: no enabled backends".to_string();
        }
        let ok = self
            .reports
            .iter()
            .filter(|r| r.result == ApplyResult::Ok)
            .count();
        let unsupported = self
            .reports
            .iter()
            .filter(|r| r.result == ApplyResult::Unsupported)
            .count();
        let err = self
            .reports
            .iter()
            .filter(|r| r.result == ApplyResult::Error)
            .count();
        format!("pc-rgb: ok={ok}, unsupported={unsupported}, error={err}")
    }
}

pub trait PcRgbBackend: Send + Sync {
    fn id(&self) -> &'static str;
    fn capabilities(&self) -> BackendCapabilities;
    fn apply(&self, _doc: &ProfileDocument) -> BackendReport;
}

#[derive(Debug, Clone, Default)]
pub struct BackendRegistry {
    /// Backend IDs enabled on this machine.
    pub enabled_ids: Vec<String>,
}

impl BackendRegistry {
    /// Comma-separated list, e.g.:
    /// `CNTRL_PC_RGB_BACKENDS=corsair_ram,ite_rgb,gpu_rgb`
    pub fn from_env() -> Self {
        let raw = std::env::var("CNTRL_PC_RGB_BACKENDS")
            .unwrap_or_else(|_| "corsair_ram,ite_rgb,gpu_rgb".to_string());
        let enabled_ids = raw
            .split(',')
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(ToOwned::to_owned)
            .collect();
        Self { enabled_ids }
    }

    pub fn run(&self, doc: &ProfileDocument) -> PcRgbApplySummary {
        let mut reports = Vec::new();
        for id in &self.enabled_ids {
            let report = match id.as_str() {
                "corsair_ram" => CorsairRamBackend.apply(doc),
                "ite_rgb" => IteRgbBackend.apply(doc),
                "gpu_rgb" => GpuRgbBackend.apply(doc),
                other => BackendReport {
                    backend_id: "unknown",
                    result: ApplyResult::Unsupported,
                    reason: format!("unknown backend id: {other}"),
                    capabilities: BackendCapabilities {
                        static_mode: false,
                        custom_fallback: false,
                        off_mode: false,
                        preset_subset: false,
                    },
                },
            };
            reports.push(report);
        }
        PcRgbApplySummary { reports }
    }
}

struct CorsairRamBackend;
struct IteRgbBackend;
struct GpuRgbBackend;

impl PcRgbBackend for CorsairRamBackend {
    fn id(&self) -> &'static str {
        "corsair_ram"
    }
    fn capabilities(&self) -> BackendCapabilities {
        BackendCapabilities {
            static_mode: true,
            custom_fallback: true,
            off_mode: true,
            preset_subset: false,
        }
    }
    fn apply(&self, _doc: &ProfileDocument) -> BackendReport {
        match crate::corsair_ram::apply_profile(_doc) {
            Ok(msg) => BackendReport {
                backend_id: self.id(),
                result: ApplyResult::Ok,
                reason: msg,
                capabilities: self.capabilities(),
            },
            Err(e) => BackendReport {
                backend_id: self.id(),
                result: if e.contains("unsupported effect")
                    || e.contains("no Corsair DRAM controllers found")
                    || e.contains("no SMBus PIIX4")
                    || e.contains("requires Linux")
                {
                    ApplyResult::Unsupported
                } else {
                    ApplyResult::Error
                },
                reason: e,
                capabilities: self.capabilities(),
            },
        }
    }
}

impl PcRgbBackend for IteRgbBackend {
    fn id(&self) -> &'static str {
        "ite_rgb"
    }
    fn capabilities(&self) -> BackendCapabilities {
        BackendCapabilities {
            static_mode: true,
            custom_fallback: true,
            off_mode: true,
            preset_subset: false,
        }
    }
    fn apply(&self, doc: &ProfileDocument) -> BackendReport {
        match crate::ite_rgb::apply_profile(doc) {
            Ok(msg) => BackendReport {
                backend_id: self.id(),
                result: ApplyResult::Ok,
                reason: msg,
                capabilities: self.capabilities(),
            },
            Err(e) => BackendReport {
                backend_id: self.id(),
                result: if e.contains("not set")
                    || e.contains("unsupported effect")
                    || e.contains("no ITE HID candidates")
                {
                    ApplyResult::Unsupported
                } else {
                    ApplyResult::Error
                },
                reason: e,
                capabilities: self.capabilities(),
            },
        }
    }
}

impl PcRgbBackend for GpuRgbBackend {
    fn id(&self) -> &'static str {
        "gpu_rgb"
    }
    fn capabilities(&self) -> BackendCapabilities {
        BackendCapabilities {
            static_mode: true,
            custom_fallback: true,
            off_mode: true,
            preset_subset: false,
        }
    }
    fn apply(&self, doc: &ProfileDocument) -> BackendReport {
        match crate::gpu_rgb::apply_profile(doc) {
            Ok(msg) => BackendReport {
                backend_id: self.id(),
                result: ApplyResult::Ok,
                reason: msg,
                capabilities: self.capabilities(),
            },
            Err(e) => BackendReport {
                backend_id: self.id(),
                result: if e.contains("unsupported effect")
                {
                    ApplyResult::Unsupported
                } else {
                    ApplyResult::Error
                },
                reason: e,
                capabilities: self.capabilities(),
            },
        }
    }
}

