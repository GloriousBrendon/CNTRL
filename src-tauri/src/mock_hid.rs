//! Non-Linux / dev: no hardware; profile I/O uses embedded default + in-memory state.

use crate::profile::ProfileDocument;

pub fn keyboard_connected() -> bool {
    false
}

pub fn read_profile_from_device() -> Result<ProfileDocument, String> {
    Ok(crate::profile::default_profile_document())
}

pub fn write_profile_to_device(_profile: &ProfileDocument) -> Result<(), String> {
    Ok(())
}
