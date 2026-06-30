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
| GTK compatibility launcher | `cargo run -p cntrl-gtk` (opens ICED panel) |
| ICED only | `cargo run -p cntrl-iced` (same UI as `cntrl panel`) |

> **Native-only:** CNTRL drives all hardware directly via `hidraw`/`i2c`. It does **not** depend on OpenRGB or any external tool.

## If lighting stops working after an OS reinstall

Almost always a **permissions** problem, not a device-ID change: a fresh install ships no udev rule, so `/dev/hidraw*` is root-only and CNTRL gets `Permission denied`. Diagnose and fix:

```bash
cntrl devices    # shows each RGB device, its node, and "Permission blocked" if so
sudo install -Dm644 packaging/udev/99-mchose-ace68.rules /etc/udev/rules.d/99-mchose-ace68.rules
sudo udevadm control --reload-rules && sudo udevadm trigger
# then replug the keyboard (or reboot)
```

`cntrl devices` is a native inventory (keyboard + ITE controller) — green = ready, red = permission-blocked, with the exact node and reason.

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
