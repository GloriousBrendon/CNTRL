//! Linux: HID access for MCHOSE Ace68-II (VID 0x41e4, PID 0x2116).
//! Protocol for read/write is not implemented yet; we still round-trip the JSON profile in-app.

use crate::profile::ProfileDocument;

const VENDOR_ID: u16 = 0x41e4;
const PRODUCT_ID: u16 = 0x2116;

pub fn keyboard_connected() -> bool {
    hidapi::HidApi::new()
        .ok()
        .and_then(|api| api.device_list().find(|d| d.vendor_id() == VENDOR_ID && d.product_id() == PRODUCT_ID))
        .is_some()
}

pub fn read_profile_from_device() -> Result<ProfileDocument, String> {
    // TODO: HID transfer — until then use the same default as mock.
    Ok(crate::profile::default_profile_document())
}

pub fn write_profile_to_device(_profile: &ProfileDocument) -> Result<(), String> {
    // TODO: pack + send over HID
    Ok(())
}
