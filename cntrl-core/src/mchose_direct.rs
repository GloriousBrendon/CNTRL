//! Fast native protocol client for the MCHOSE Ace68-II — no handshake replay.
//!
//! Derived from the vendor CZ-SDK (the iframe app MCHOSE HUB embeds for the
//! cizhou/magnetic family). Every command is one 64-byte interrupt frame:
//!
//! `55 <cmd> 00 <cksum> <len> <off_lo> <off_hi> 00 <data…>`
//!
//! cksum = sum(bytes[4..64]) & 0xFF. A frame with data is a write, without is
//! a read; the reply echoes `aa <cmd> …` with the payload at bytes 8..8+len.
//! Long transfers are paged in 56-byte chunks by advancing the offset.
//!
//! Verified on hardware 2026-07-02: live frames render instantly at 143 fps,
//! `CMD_REALTIME_READ` returns the rendered colors byte-for-byte.
//!
//! Never send flag 0x5F or the eraseRom/sendRomData family — that is the
//! firmware updater.

use crate::keyboard_layout;
use crate::mchose_lighting;
use nix::poll::{poll, PollFd, PollFlags, PollTimeout};
use serde::Deserialize;
use std::fs::File;
use std::io::{Read, Write};
use std::os::unix::io::AsFd;
use std::sync::LazyLock;

/// Live streaming frame (music/GIF overlay): RGB triplets per LED slot,
/// rendered immediately, keyboard reverts to its own effect once frames stop.
pub const CMD_LIVE_FRAME: u8 = 0xDD;
/// Read back the colors currently rendered on the keys (384 bytes).
pub const CMD_REALTIME_READ: u8 = 0xDE;
/// Performance/settings page (read). The apply frames write its twin 0x06.
pub const CMD_SETTINGS_READ: u8 = 0x05;
/// Persistent per-key custom colors (512-byte table per profile).
pub const CMD_CUSTOM_READ: u8 = 0x0A;
pub const CMD_CUSTOM_WRITE: u8 = 0x0B;

/// The 0xDD frame carries this many LED slots (× 3 bytes RGB).
pub const LIVE_FRAME_SLOTS: usize = 128;
pub const LIVE_FRAME_BYTES: usize = LIVE_FRAME_SLOTS * 3;

const ACK_TIMEOUT_MS: u16 = 60;

// ── LED slot map (from the vendor `_te` table) ───────────────────────

#[derive(Deserialize)]
struct LedIndexFile {
    keys: Vec<LedIndexEntry>,
}

#[derive(Deserialize)]
struct LedIndexEntry {
    code: u16,
    index: usize,
}

/// HID usage code → LED slot in the 0xDD frame.
static LED_INDEX: LazyLock<Vec<(u16, usize)>> = LazyLock::new(|| {
    let json = include_str!("../../resources/ace68_led_index.json");
    let f: LedIndexFile = serde_json::from_str(json).expect("ace68_led_index.json");
    f.keys.iter().map(|k| (k.code, k.index)).collect()
});

pub fn led_index_for_usage(code: u16) -> Option<usize> {
    LED_INDEX.iter().find(|(c, _)| *c == code).map(|(_, i)| *i)
}

/// LED slot for each key of `keyboard_layout::get_keyboard_layout()`, in
/// layout order — the bridge from canvas key sampling to the wire frame.
pub fn layout_led_indices() -> Vec<Option<usize>> {
    keyboard_layout::hid_usage_codes()
        .iter()
        .map(|&code| led_index_for_usage(code))
        .collect()
}

// ── protocol client ──────────────────────────────────────────────────

/// Open session on the vendor hidraw node. Cheap to open (no handshake);
/// keep it open for streaming.
pub struct Ace68Direct {
    file: File,
    path: String,
}

fn checksum(frame: &mut [u8; 64]) {
    frame[3] = frame[4..].iter().copied().fold(0u8, |a, b| a.wrapping_add(b));
}

impl Ace68Direct {
    pub fn open() -> Result<Self, String> {
        let path = mchose_lighting::vendor_hidraw_path()?;
        let file = File::options()
            .read(true)
            .write(true)
            .open(&path)
            .map_err(|e| format!("open {}: {e}", path.display()))?;
        let mut s = Self {
            file,
            path: path.to_string_lossy().into_owned(),
        };
        s.drain_stale();
        Ok(s)
    }

    pub fn path(&self) -> &str {
        &self.path
    }

    fn read_reply(&mut self, timeout_ms: u16) -> Option<[u8; 64]> {
        let pfd = PollFd::new(self.file.as_fd(), PollFlags::POLLIN);
        let n = poll(&mut [pfd], PollTimeout::from(timeout_ms)).ok()?;
        if n == 0 {
            return None;
        }
        let mut raw = [0u8; 65];
        let nread = self.file.read(&mut raw).ok()?;
        if nread == 0 {
            return None;
        }
        let buf = &raw[..nread];
        let mut out = [0u8; 64];
        if buf.len() >= 65 && buf[0] == 0 && matches!(buf[1], 0xAA | 0xAB | 0x55) {
            out.copy_from_slice(&buf[1..65]);
        } else {
            let take = buf.len().min(64);
            out[..take].copy_from_slice(&buf[..take]);
        }
        Some(out)
    }

    fn drain_stale(&mut self) {
        while self.read_reply(2).is_some() {}
    }

    /// Send one frame and wait for its `aa <cmd>` ack.
    fn xfer(&mut self, frame: &[u8; 64]) -> Result<[u8; 64], String> {
        self.file
            .write_all(frame)
            .map_err(|e| format!("write {}: {e}", self.path))?;
        // Skip unrelated frames (key events never appear on this interface,
        // but stale acks from an interrupted session might).
        for _ in 0..4 {
            match self.read_reply(ACK_TIMEOUT_MS) {
                Some(rep) if rep[0] == 0xAA && rep[1] == frame[1] => return Ok(rep),
                Some(_) => continue,
                None => break,
            }
        }
        Err(format!("no ack for cmd {:02x} on {}", frame[1], self.path))
    }

    /// Send a raw prebuilt 64-byte packet (templates from `mchose_lighting`).
    fn xfer_raw(&mut self, pkt: [u8; 64]) -> Result<(), String> {
        self.xfer(&pkt).map(|_| ())
    }

    /// Paged write: `data` at `offset`, 56 bytes per frame, ack each.
    pub fn write_cmd(&mut self, cmd: u8, offset: u16, data: &[u8]) -> Result<(), String> {
        let mut off = offset as usize;
        for chunk in data.chunks(56) {
            let mut f = [0u8; 64];
            f[0] = 0x55;
            f[1] = cmd;
            f[4] = chunk.len() as u8;
            f[5] = (off & 0xFF) as u8;
            f[6] = (off >> 8) as u8;
            f[8..8 + chunk.len()].copy_from_slice(chunk);
            checksum(&mut f);
            self.xfer(&f)?;
            off += chunk.len();
        }
        Ok(())
    }

    /// Paged read of `len` bytes at `offset`.
    pub fn read_cmd(&mut self, cmd: u8, offset: u16, len: usize) -> Result<Vec<u8>, String> {
        let mut out = Vec::with_capacity(len);
        let mut off = offset as usize;
        while out.len() < len {
            let n = (len - out.len()).min(56);
            let mut f = [0u8; 64];
            f[0] = 0x55;
            f[1] = cmd;
            f[4] = n as u8;
            f[5] = (off & 0xFF) as u8;
            f[6] = (off >> 8) as u8;
            checksum(&mut f);
            let rep = self.xfer(&f)?;
            out.extend_from_slice(&rep[8..8 + n]);
            off += n;
        }
        Ok(out)
    }

    // ── live per-key streaming ──

    /// Push one live frame: `colors[slot]` in wire LED order (see
    /// `layout_led_indices`). Renders immediately; ~7 ms round trip.
    pub fn write_live_frame(&mut self, colors: &[[u8; 3]]) -> Result<(), String> {
        let mut buf = [0u8; LIVE_FRAME_BYTES];
        for (i, c) in colors.iter().take(LIVE_FRAME_SLOTS).enumerate() {
            buf[i * 3] = c[0];
            buf[i * 3 + 1] = c[1];
            buf[i * 3 + 2] = c[2];
        }
        self.write_cmd(CMD_LIVE_FRAME, 0, &buf)
    }

    /// Colors currently rendered on the keys (RGB triplets, wire LED order).
    pub fn read_realtime_colors(&mut self) -> Result<Vec<u8>, String> {
        self.read_cmd(CMD_REALTIME_READ, 0, LIVE_FRAME_BYTES)
    }

    // ── fast solid / preset apply (no handshake, no extra gets) ──

    /// Raw settings page 0 reply frame — the template for apply A/B.
    fn read_settings_frame(&mut self) -> Result<[u8; 64], String> {
        let mut f = [0u8; 64];
        f[0] = 0x55;
        f[1] = CMD_SETTINGS_READ;
        f[4] = 0x38;
        checksum(&mut f);
        let rep = self.xfer(&f)?;
        if rep[4] != 0x38 {
            return Err(format!("unexpected settings reply header: {:02x?}", &rep[..8]));
        }
        Ok(rep)
    }

    /// Solid color via the proven 4-frame chain (~30 ms): light@v2 blob +
    /// zone + apply A/B templated from a fresh settings read.
    pub fn apply_solid(&mut self, r: u8, g: u8, b: u8) -> Result<(), String> {
        let perf = self.read_settings_frame()?;
        self.xfer_raw(mchose_lighting::profile_template(r, g, b))?;
        self.xfer_raw(mchose_lighting::zone_template(r, g, b))?;
        self.xfer_raw(mchose_lighting::build_apply_a(&perf, (r, g, b), None))?;
        self.xfer_raw(mchose_lighting::build_apply_b(&perf, (r, g, b), None))?;
        Ok(())
    }

    /// Preset effect (ids 1..=22) with color + brightness, same fast chain.
    pub fn apply_effect(
        &mut self,
        effect_id: u8,
        r: u8,
        g: u8,
        b: u8,
        brightness: u8,
    ) -> Result<(), String> {
        if !(1..=22).contains(&effect_id) {
            return Err("effect_id must be 1..=22".into());
        }
        let perf = self.read_settings_frame()?;
        let (templates, zones) = &*mchose_lighting::EFFECT_DATA;
        let blob =
            mchose_lighting::effect_profile_packet(effect_id, r, g, b, brightness, templates)?;
        self.xfer_raw(blob)?;
        let zone = match zones.get(&effect_id) {
            Some(z) => {
                let mut zp = *z;
                checksum(&mut zp);
                zp
            }
            None => mchose_lighting::zone_template(r, g, b),
        };
        self.xfer_raw(zone)?;
        self.xfer_raw(mchose_lighting::build_apply_a(&perf, (r, g, b), Some(effect_id)))?;
        self.xfer_raw(mchose_lighting::build_apply_b(&perf, (r, g, b), Some(effect_id)))?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn led_index_covers_every_layout_key() {
        let indices = layout_led_indices();
        // Every key the vendor table lights is in the layout: none left dark.
        assert_eq!(indices.len(), LED_INDEX.len());
        assert_eq!(indices.len(), 68);
        for (i, idx) in indices.iter().enumerate() {
            assert!(idx.is_some(), "layout key {i} has no LED slot");
            assert!(idx.unwrap() < LIVE_FRAME_SLOTS);
        }
    }

    #[test]
    fn led_slots_are_unique() {
        let mut seen = std::collections::HashSet::new();
        for idx in layout_led_indices().into_iter().flatten() {
            assert!(seen.insert(idx), "duplicate LED slot {idx}");
        }
    }

    #[test]
    fn frame_checksum_matches_protocol() {
        let mut f = [0u8; 64];
        f[0] = 0x55;
        f[1] = 0x08;
        f[4] = 0x38;
        f[5] = 0x18;
        f[6] = 0x01;
        checksum(&mut f);
        // Known-good frame from the captured handshake: 55 08 00 51 38 18 01.
        assert_eq!(f[3], 0x51);
    }
}
