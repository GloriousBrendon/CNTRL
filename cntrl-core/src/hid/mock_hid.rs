//! Non-Linux / dev: no hardware; profile I/O uses embedded default + in-memory state.

use crate::profile::ProfileDocument;
use crate::hid::HidInterfaceDiagnostic;

pub fn vendor_hidraw_path_status() -> (Option<String>, Option<String>) {
    (None, None)
}

pub fn keyboard_connected() -> bool {
    false
}

pub fn read_profile_from_device() -> Result<ProfileDocument, String> {
    Ok(crate::profile::default_profile_document())
}

pub fn write_profile_to_device(_profile: &ProfileDocument) -> Result<(), String> {
    Ok(())
}

pub fn profile_updates_reach_keyboard() -> bool {
    false
}

pub fn hid_diagnostics() -> Result<Vec<HidInterfaceDiagnostic>, String> {
    Ok(vec![])
}
