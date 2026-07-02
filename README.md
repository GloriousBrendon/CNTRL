# CNTRL

ICED-first Linux app for the **MCHOSE Ace68-II** (`41e4:2116`) with unified keyboard + PC RGB apply. On **Linux**, CNTRL pushes **lighting** over vendor HID/hidraw plus optional PC RGB backends (ITE/GPU/Corsair). Other profile areas (keymap, macros, performance) remain **local JSON** until explicitly implemented.

The repo is a **Cargo workspace**: `cntrl-core`, `cntrl-cli` (unified **`cntrl`** binary: daemon / panel / devices / apply-theme), `cntrl-iced` (primary UI — full desktop panel), and `cntrl-gtk` (legacy compatibility launcher that opens the ICED panel). Build and run from the repository root.

| Entry | Command |
|-------|---------|
| **CLI** | `cargo run -p cntrl-cli -- <subcommand>` → binary **`cntrl`** |
| `cntrl panel` | Full ICED desktop app |
| `cntrl devices` | Native RGB device inventory + status |
| `cntrl apply-theme` | Apply the theme accent to the lighting |
| `cntrl daemon` | Unix socket daemon (foreground; HID apply + theme watcher) |
| `cntrl canvas` | Stream a canvas effect across ARGB devices (headless; layout from the Canvas tab) |
| GTK compatibility launcher | `cargo run -p cntrl-gtk` (opens ICED panel) |
| ICED only | `cargo run -p cntrl-iced` (same UI as `cntrl panel`) |

> **Native-only:** CNTRL drives all hardware directly via `hidraw`/`i2c`. It does **not** depend on OpenRGB or any external tool.

> **Fast keyboard path:** applies use the native Ace68 protocol client (`mchose_direct`, ~135 ms end-to-end) with the legacy capture-replay pipeline as automatic fallback; set `CNTRL_KB_LEGACY=1` to force the old path.

## If lighting stops working after an OS reinstall

Almost always a **permissions** problem, not a device-ID change: a fresh install ships no udev rule, so `/dev/hidraw*` is root-only and CNTRL gets `Permission denied`. Diagnose and fix:

```bash
cntrl devices    # shows each RGB device, its node, and "Permission blocked" if so
sudo install -Dm644 packaging/udev/99-mchose-ace68.rules /etc/udev/rules.d/99-mchose-ace68.rules
sudo udevadm control --reload-rules && sudo udevadm trigger
# then replug the keyboard (or reboot)
```

`cntrl devices` is a native inventory (keyboard + ITE controller) — green = ready, red = permission-blocked, with the exact node and reason.

## LED canvas (SignalRGB-style effects, fully native)

The **Canvas** tab maps every device onto one virtual effect surface: each device declares its LED geometry (per-key for the keyboard, per-LED for the ARGB strips), you drag/rotate/scale devices to mirror your desk, and effects render onto the canvas while every LED samples the pixel underneath it — so one animation flows across strips, GPU and RAM in physical order. No browser engine, no external tools: effects are pure Rust functions and the sampler is the native equivalent of SignalRGB's `device.color(x, y)`.

- **Demo effects:** Rainbow Wave, Plasma, Ripple, Comet, Aurora (`cntrl canvas --list`).
- **Live streaming:** the keyboard gets true **per-key** frames (native CZ protocol, ~7 ms/frame — up to 143 fps); ITE D_LED1/D_LED2 strips get per-LED frames; GPU gets a sampled zone color (never saved to its flash); Corsair RAM gets per-LED SMBus writes (rate-limited). When a stream stops, every device returns to its saved lighting.
- **Arrangement:** drag devices on the Canvas tab; selection supports rotate (90°) and scale. Layout + effect settings persist to `~/.config/cntrl/canvas_layout.json`.
- **Headless:** `cntrl canvas --effect plasma --fps 20` streams with the saved layout; `--seconds N` for a timed run.

## Theme switcher sync

CNTRL follows a pywal-style `~/.config/themes/colors.conf`; its `accent` key drives the lighting. Two ways to use it:

- **Direct (call from your switcher):** add one line to your theme script —

  ```bash
  cntrl apply-theme            # recolor the active slot to the new accent
  cntrl apply-theme --keyboard-only --slot 1
  ```

- **Automatic (daemon watcher):** enable **Theme sync** in the Lighting tab (or set `theme_sync_enabled` in `~/.config/cntrl/settings.json`). While `cntrl daemon` runs, it watches `colors.conf` and re-applies on every change. The existing effect is preserved — presets keep animating, just recolored.

## Migration baseline (GTK → ICED)

Authoritative pre-migration snapshot and gates:

| Artifact | Path |
|----------|------|
| Behavior & mappings | [docs/baseline/BASELINE_BEHAVIOR.md](docs/baseline/BASELINE_BEHAVIOR.md) |
| Manual smoke checklist | [docs/baseline/SMOKE_LIGHTING_CHECKLIST.md](docs/baseline/SMOKE_LIGHTING_CHECKLIST.md) |
| ADR (strategy) | [docs/adr/0001-migrate-to-iced-daemon-and-theming.md](docs/adr/0001-migrate-to-iced-daemon-and-theming.md) |
| Build smoke script | `./scripts/smoke_lighting.sh` |

## Phase 1 — lighting (Linux + native app)

**In scope:** static / custom / off, firmware presets via **named** mapping (`lighting_modes::PRESET_MODES`), clear errors when hidraw fails.  
**Out of scope:** key remaps, macros, RT/polling/actuation upload, reading real device state, per-key matrix upload, GIF/timeline upload.

1. **udev:** copy `packaging/udev/99-mchose-ace68.rules` to `/etc/udev/rules.d/`, reload rules, replug the keyboard.
2. **Run the app:**

   ```bash
   cargo run -p cntrl-cli -- panel
   # compatibility alias: cargo run -p cntrl-gtk
   ```

3. **Lighting** tab: choose **Static**, **Custom**, **Off**, or a **labeled** preset; set color where relevant; **Apply To Keyboard**. **Static** is not firmware id `3` — see baseline doc. Optional: `MCHOSE_HIDRAW=/dev/hidrawN` if auto-detection picks the wrong interface.

**Legacy Phase 1 notes (Tauri era):** [PHASE1_LIGHTING.md](PHASE1_LIGHTING.md) — prefer **baseline** docs above for current behavior.

## Scripts

| Command | Purpose |
|---------|---------|
| `./scripts/smoke_lighting.sh` | `cargo build` + pointers to manual checklist |

## Arch boot-time RGB probe (optional)

If you want bus/device mapping refreshed automatically at boot (no manual env setup), use the systemd unit template at `packaging/systemd/cntrl-rgb-probe.service`.

1. Make sure `cntrl` is reachable at the path used by `ExecStart` (default `/usr/bin/cntrl`).
2. Install the unit:

   ```bash
   sudo cp packaging/systemd/cntrl-rgb-probe.service /etc/systemd/system/
   sudo systemctl daemon-reload
   ```

3. Enable at boot and run once now:

   ```bash
   sudo systemctl enable --now cntrl-rgb-probe.service
   ```

4. Verify result:

   ```bash
   systemctl status cntrl-rgb-probe.service --no-pager
   cat ~/.config/cntrl/pc_rgb_probe.json
   ```

This unit runs:

```bash
cntrl probe-rgb --force --no-apply
```

That reprobes with OpenRGB and persists mapping to `~/.config/cntrl/pc_rgb_probe.json`. Runtime apply paths then auto-load this mapping.

## Development

Rust + [rust-analyzer](https://marketplace.visualstudio.com/items?itemName=rust-lang.rust-analyzer); GTK/libadwaita dev packages per your distro.
