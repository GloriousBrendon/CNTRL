#[cfg(target_os = "linux")]
pub mod real_hid;
#[cfg(not(target_os = "linux"))]
pub mod mock_hid;

#[cfg(target_os = "linux")]
pub use real_hid as backend;
#[cfg(not(target_os = "linux"))]
pub use mock_hid as backend;

use serde::Serialize;

/// One HID interface node for the Ace68-II (same VID/PID can expose multiple hidraw devices).
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HidInterfaceDiagnostic {
    pub path: String,
    pub interface_number: i32,
    pub product_string: Option<String>,
    pub report_descriptor_hex: Option<String>,
    pub open_or_read_error: Option<String>,
}
