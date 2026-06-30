//! Native RGB device inventory — no external tools (no OpenRGB).
//!
//! Enumerates the RGB hardware CNTRL knows how to drive on this machine using
//! `hidapi` only, and reports presence + accessibility so the UI/CLI can show a
//! SignalRGB-style "Devices" list and explain *why* a device is not controllable
//! (usually a missing udev rule → `Permission denied`).

use std::fs::OpenOptions;

/// Stable identifier for the backend that drives a device class.
pub const BACKEND_KEYBOARD: &str = "keyboard";
pub const BACKEND_ITE: &str = "ite_rgb";
pub const BACKEND_GPU: &str = "gpu_rgb";
pub const BACKEND_CORSAIR: &str = "corsair_ram";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeviceKind {
    Keyboard,
    Motherboard,
    GraphicsCard,
    Memory,
}

impl DeviceKind {
    pub fn label(self) -> &'static str {
        match self {
            DeviceKind::Keyboard => "Keyboard",
            DeviceKind::Motherboard => "Motherboard / ARGB (fans)",
            DeviceKind::GraphicsCard => "Graphics card",
            DeviceKind::Memory => "Memory (RAM)",
        }
    }

    /// Nerd Font glyph (Material Design Icons) for this device class.
    pub fn icon(self) -> &'static str {
        match self {
            DeviceKind::Keyboard => "\u{F030C}",     // md-keyboard
            DeviceKind::Motherboard => "\u{F061A}",  // md-chip
            DeviceKind::GraphicsCard => "\u{F08AE}", // md-expansion-card
            DeviceKind::Memory => "\u{F035B}",       // md-memory
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeviceStatus {
    /// Present and the control node is openable by this user.
    Ready,
    /// Present but the control node can't be opened (almost always udev/perms).
    Blocked,
    /// Not plugged in / not enumerated.
    Absent,
}

impl DeviceStatus {
    pub fn label(self) -> &'static str {
        match self {
            DeviceStatus::Ready => "Ready",
            DeviceStatus::Blocked => "Permission blocked",
            DeviceStatus::Absent => "Not detected",
        }
    }
}

#[derive(Debug, Clone)]
pub struct RgbDevice {
    pub name: String,
    pub vid: u16,
    pub pid: u16,
    pub kind: DeviceKind,
    pub backend_id: &'static str,
    pub status: DeviceStatus,
    /// hidraw node CNTRL would use to control this device, if found.
    pub control_path: Option<String>,
    pub interface: Option<i32>,
    /// Human-readable detail (resolved path, or the reason it is blocked/absent).
    pub detail: String,
}

impl RgbDevice {
    pub fn is_controllable(&self) -> bool {
        self.status == DeviceStatus::Ready
    }

    /// `41e4:2116` style id.
    pub fn usb_id(&self) -> String {
        format!("{:04x}:{:04x}", self.vid, self.pid)
    }
}

/// Catalog of devices CNTRL natively supports on this machine.
struct Catalog {
    name: &'static str,
    vid: u16,
    pid: u16,
    kind: DeviceKind,
    backend_id: &'static str,
}

const CATALOG: &[Catalog] = &[
    Catalog {
        name: "MCHOSE Ace68-II",
        vid: 0x41e4,
        pid: 0x2116,
        kind: DeviceKind::Keyboard,
        backend_id: BACKEND_KEYBOARD,
    },
    Catalog {
        name: "ITE RGB LED Controller",
        vid: 0x048d,
        pid: 0x5702,
        kind: DeviceKind::Motherboard,
        backend_id: BACKEND_ITE,
    },
];

/// Can the current user open this hidraw node read/write? (Opens and immediately
/// drops the handle — no writes, so device state is untouched.)
fn node_openable(path: &str) -> Result<(), String> {
    OpenOptions::new()
        .read(true)
        .write(true)
        .open(path)
        .map(|_| ())
        .map_err(|e| e.to_string())
}

/// Resolve the control node for a catalog entry: prefer the keyboard's interface-1
/// resolver, otherwise the lowest interface number for the vid/pid.
fn resolve_control_node(
    api: &hidapi::HidApi,
    entry: &Catalog,
) -> Option<(String, i32)> {
    if entry.backend_id == BACKEND_KEYBOARD {
        if let Ok(p) = crate::mchose_lighting::vendor_hidraw_path() {
            // interface_number for the resolved path, best-effort.
            let iface = api
                .device_list()
                .find(|d| d.path().to_string_lossy() == p.to_string_lossy())
                .map(|d| d.interface_number())
                .unwrap_or(-1);
            return Some((p.to_string_lossy().into_owned(), iface));
        }
    }
    let mut best: Option<(i32, String)> = None;
    for d in api.device_list() {
        if d.vendor_id() != entry.vid || d.product_id() != entry.pid {
            continue;
        }
        let iface = d.interface_number();
        let path = d.path().to_string_lossy().into_owned();
        match &best {
            Some((bi, _)) if *bi <= iface => {}
            _ => best = Some((iface, path)),
        }
    }
    best.map(|(i, p)| (p, i))
}

/// Probe every catalog device and report status. Pure read-only.
pub fn enumerate() -> Vec<RgbDevice> {
    let api = match hidapi::HidApi::new() {
        Ok(a) => a,
        Err(e) => {
            // hidapi itself failed — report all as absent with the reason.
            return CATALOG
                .iter()
                .map(|c| RgbDevice {
                    name: c.name.to_string(),
                    vid: c.vid,
                    pid: c.pid,
                    kind: c.kind,
                    backend_id: c.backend_id,
                    status: DeviceStatus::Absent,
                    control_path: None,
                    interface: None,
                    detail: format!("hidapi init failed: {e}"),
                })
                .collect();
        }
    };

    let mut list: Vec<RgbDevice> = CATALOG
        .iter()
        .map(|c| match resolve_control_node(&api, c) {
            Some((path, iface)) => match node_openable(&path) {
                Ok(()) => RgbDevice {
                    name: c.name.to_string(),
                    vid: c.vid,
                    pid: c.pid,
                    kind: c.kind,
                    backend_id: c.backend_id,
                    status: DeviceStatus::Ready,
                    control_path: Some(path.clone()),
                    interface: Some(iface),
                    detail: path,
                },
                Err(e) => RgbDevice {
                    name: c.name.to_string(),
                    vid: c.vid,
                    pid: c.pid,
                    kind: c.kind,
                    backend_id: c.backend_id,
                    status: DeviceStatus::Blocked,
                    control_path: Some(path.clone()),
                    interface: Some(iface),
                    detail: format!("{path}: {e} — install packaging/udev rule"),
                },
            },
            None => RgbDevice {
                name: c.name.to_string(),
                vid: c.vid,
                pid: c.pid,
                kind: c.kind,
                backend_id: c.backend_id,
                status: DeviceStatus::Absent,
                control_path: None,
                interface: None,
                detail: "not enumerated on USB/hidraw".to_string(),
            },
        })
        .collect();

    list.push(probe_gpu());
    list.push(probe_ram());
    list
}

/// GPU RGB (AMD Fusion2 over i2c) as a Devices-view entry.
fn probe_gpu() -> RgbDevice {
    use crate::gpu_rgb::GpuProbe;
    let (status, detail) = match crate::gpu_rgb::probe() {
        GpuProbe::Ready(bus) => (DeviceStatus::Ready, bus),
        GpuProbe::Blocked(d) => (DeviceStatus::Blocked, d),
        GpuProbe::Absent(d) => (DeviceStatus::Absent, d),
    };
    RgbDevice {
        name: "Graphics card".to_string(),
        vid: 0,
        pid: 0,
        kind: DeviceKind::GraphicsCard,
        backend_id: BACKEND_GPU,
        status,
        control_path: None,
        interface: None,
        detail,
    }
}

/// Corsair DDR5 RAM (SMBus) as a Devices-view entry.
fn probe_ram() -> RgbDevice {
    use crate::corsair_ram::RamProbe;
    let (status, name, detail) = match crate::corsair_ram::probe() {
        RamProbe::Ready { modules, detail } => (
            DeviceStatus::Ready,
            format!("Corsair RAM ({modules} module{})", if modules == 1 { "" } else { "s" }),
            detail,
        ),
        RamProbe::Blocked(d) => (DeviceStatus::Blocked, "Corsair RAM".to_string(), d),
        RamProbe::Absent(d) => (DeviceStatus::Absent, "Corsair RAM".to_string(), d),
    };
    RgbDevice {
        name,
        vid: 0,
        pid: 0,
        kind: DeviceKind::Memory,
        backend_id: BACKEND_CORSAIR,
        status,
        control_path: None,
        interface: None,
        detail,
    }
}
