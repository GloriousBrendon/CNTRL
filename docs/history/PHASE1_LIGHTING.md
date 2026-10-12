# Phase 1 — USB lighting (Linux + Tauri)

CNTRL can push **lighting only** to the MCHOSE Ace68-II (`41e4:2116`) on Linux: static color, firmware presets **1–22**, and **off (255)**. Keymap, macros, performance (RT/polling), GIF timelines, per-key matrix upload, and reading real device state are **not** Phase 1.

## Smoke test

1. Install udev rule from `packaging/udev/99-mchose-ace68.rules` into `/etc/udev/rules.d/`, then `udevadm control --reload-rules && udevadm trigger` (or replug the keyboard).
2. Run the **Tauri** app (not browser-only Vite):

   ```bash
   npm install
   npm run tauri dev
   ```

3. Open **Lighting**. Confirm the status bar shows a hidraw path (or a clear error if the device is missing).
4. Optional: if the wrong node is used, set `MCHOSE_HIDRAW=/dev/hidrawN` and restart.

## Regression checklist (manual)

| Step | Pass criterion |
|------|------------------|
| Static | Apply **effect 3** with **Direct mode = Direct**; change color at least **5** distinct times; keyboard matches each time. |
| Presets | Apply at least **6** of IDs **1, 4, 6, 8, 14, 22** (or any six in **1–22**); keyboard shows the effect. |
| Off | **255** repeatedly; keyboard LEDs off each time. |
| Stress | **Apply effect** ~10× in a row; UI stays responsive (HID runs on a worker thread). |
| Errors | Unplug keyboard or break permissions; UI shows an error string, not a silent no-op. |

## Ground truth (implementation)

- **Linux:** `real_hid` → `mchose_lighting` (hidraw `read`/`write`).
- **Non-Linux:** `mock_hid` (no USB).
- **Effect mapping:** `0`, `3`, `255` → `set_solid_rgb`; `1..=22` → `set_effect` + `effects_light_v2.json`.
- **Speed:** not on the wire (`profile` only).
- **Brightness:** scaled for presets **1–22**; not applied on-wire for solid DIY (see `mchose_lighting.rs`).
- **Direct mode:** `direct=true` skips the GET_DIY_LIGHT pre-read in `run_pipeline`.

## Docs

- Short overview: [README.md](README.md) (Phase 1 section).
- In-app: **Lighting** page copy and **HID diagnostics** (report descriptors).
