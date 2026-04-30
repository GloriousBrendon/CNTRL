# Baseline behavior snapshot (pre–ICED migration)

**Captured for:** GTK4 + libadwaita + Relm4 desktop (`cntrl-gtk` crate) and ICED (`cntrl-iced` / `cntrl panel`).  
**Purpose:** Reproducible description of Linux HID lighting, effect mapping, profile persistence, and theming so reviewers can compare before/after migration.

## Runtime and platform

| Item | Value |
|------|--------|
| **Stack** | Rust 2021, `gtk4` 0.9, `libadwaita` 0.7, `relm4` 0.9 |
| **App ID** | `io.cntrl.native` |
| **Linux HID** | MCHOSE Ace68-II USB `41e4:2116`, vendor lighting on **HID interface 1** (`hidraw`) |
| **Non-Linux** | `mock_hid` — no USB; lighting apply is a no-op success path per mock |

## Entry points (source of truth files)

| Area | Module / path | Role |
|------|----------------|------|
| HID pipeline | `cntrl-core/src/mchose_lighting.rs` | 64-byte `0x55` frames, `read`/`write` on hidraw, checksum, `run_pipeline` |
| Effect templates | `resources/effects_light_v2.json` | Keys `"1"`…`"22"` → 64-byte hex templates; zones optional |
| Preset **names** → firmware **effect_id** | `cntrl-core/src/lighting_modes.rs` (`PRESET_MODES`) | **Single UI mapping table** — display labels are not raw slot numbers |
| Profile → wire | `cntrl-core/src/hid/real_hid.rs` (`push_profile_lighting`) | Maps `ProfileDocument.data.light.config` to `set_solid_rgb` / `set_effect` |
| Profile JSON | `cntrl-core/src/profile.rs` | `KeyboardProfile`-style document |
| Profile slot I/O | `cntrl-core/src/persistence.rs` | `~/.config/cntrl/profiles/slot_*.json`, `active_slot.txt` |
| App theme file | `cntrl-gtk/src/theme.rs` | Loads `~/.config/themes/colors.conf` (`$key=rgba(...)`); inotify hot-reload on Linux |

## Config and persistence paths

- **Config root:** `dirs::config_dir()/cntrl` (typically `~/.config/cntrl`).
- **Profiles:** `profiles/slot_{0..3}.json`.
- **Active slot:** `active_slot.txt` (single digit `0`–`3`).
- **Theme:** `~/.config/themes/colors.conf` (see `theme::theme_file_path()`).
- **Override hidraw:** environment variable `MCHOSE_HIDRAW=/dev/hidrawN` (see `mchose_lighting::vendor_hidraw_path`).

## Lighting semantics (must not regress without explicit decision)

### Effect identifiers in profile / UI state

| `config.effect` (i32) | Wire behavior | Notes |
|------------------------|---------------|--------|
| `MODE_STATIC` (`-2`) | `set_solid_rgb` — **no** `effect_id`; DIY profile template from RGB | **Not** “preset 3” — static/custom are distinct from numbered presets |
| `MODE_CUSTOM` (`-1`) | `set_solid_rgb` | Same wire path as static; per-key preview colors in profile JSON |
| `0` (legacy) | Treated as custom when loading | Backward compatibility |
| `1`–`22` (only if present in `PRESET_MODES`) | `set_effect(effect_id, color, brightness_scaled, …)` | Brightness 0–100 scaled to 0–255 for presets |
| `EFFECT_ID_OFF` (`255`) | `set_solid_rgb(0,0,0)` | LEDs off |

**Mandatory constraint:** Do **not** assume effect id **3** means “static” in the default/firmware sense — in the current **named** table, firmware id `3` is **“Serene Realm”** (preset slot 8). **Static** is `MODE_STATIC` (`-2`), not `3`.

### Direct mode

- `config.direct != 0` → `direct=true` in `mchose_lighting::run_pipeline`, which **skips** the `GET_DIY_LIGHT` stage (see `mchose_lighting.rs`).

### Brightness and speed

- **Presets (1–22):** brightness from profile (0–100) scaled to wire 0–255.
- **Solid DIY (`set_solid_rgb`):** brightness parameter exists in API but solid path matches Python — not applied on wire for solid (see comments in `mchose_lighting.rs`).
- **Speed:** stored in profile; logged as not on wire yet in `run_pipeline`.

## UI behavior (Lighting page — GTK: `cntrl-gtk/src/ui/lighting.rs`; ICED: `cntrl-iced/src/app.rs`)

- **Effects:** FlowBox of pills: **Static**, **Custom**, **Off**, then each `PRESET_MODES` entry by **label** (not “Effect 6” only — labels are human-readable).
- **Color:** HSL picker, hex label, swatches; updates in-memory state and optional per-key paint on keyboard preview.
- **Apply:** “Apply To Keyboard” → updates in-memory profile, persists slot JSON, calls `hid::write_profile_to_device` (Linux: `real_hid::write_profile_to_device`).
- **Keyboard preview:** GTK `cntrl-gtk/src/ui/keyboard.rs`; ICED uses `cntrl-core::keyboard_layout` — 68-key layout; not claimed to match full per-key USB upload in Phase 1 (lighting push uses global effect + color from config).

## HID diagnostics

- **API (implemented):** `real_hid::hid_diagnostics()` — enumerates VID/PID devices, report descriptor hex per path, errors on open/read. `vendor_hidraw_path_status()` — resolved path or error string.
- **GTK UI (current baseline):** these helpers are **not** wired into `cntrl-gtk/src/ui/window.rs` or the Lighting page; the compiler reports them as unused. Operational feedback is mainly **stderr** from `mchose_lighting` during apply (`run_pipeline` logs path, stages, `effect_id`) and **toasts** on persist/HID failure from Apply.
- **Gap vs older docs:** `PHASE1_LIGHTING.md` mentions a status bar / in-app diagnostics — treat that as aspirational until UI is connected or ICED exposes it.

## udev

- Rule file: `packaging/udev/99-mchose-ace68.rules` — install to `/etc/udev/rules.d/`, reload rules, replug device.

## Known documentation drift

- Root `README.md` may still mention Tauri/npm in places; this file and `docs/baseline/SMOKE_LIGHTING_CHECKLIST.md` are the **authoritative** baseline for the GTK/native stack unless superseded by a later milestone doc.
