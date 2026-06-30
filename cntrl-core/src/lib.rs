//! CNTRL core: profile model, persistence, HID entry points, and lighting protocol.
//! No UI dependencies.

pub mod color;
pub mod corsair_ram;
pub mod device_state;
pub mod devices;
pub mod gpu_rgb;
pub mod hid;
pub mod ipc;
pub mod ite_rgb;
pub mod keyboard_layout;
pub mod lighting_modes;
pub mod mchose_lighting;
pub mod persistence;
pub mod profile;
pub mod rgb_probe;
pub mod rgb_backends;
pub mod settings;
pub mod theme;
pub mod theme_sync;
