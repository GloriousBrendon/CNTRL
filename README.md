# CNTRL

Native **GTK4 + libadwaita** (Relm4) app for the **MCHOSE Ace68-II** (`41e4:2116`). On **Linux**, CNTRL pushes **lighting** over the vendor HID/hidraw interface (aligned with CNTRL_Python `set_rgb_rw.py`). Other profile areas (keymap, macros, performance) remain **local JSON** until explicitly implemented.

The repo is a **Cargo workspace**: `cntrl-core`, `cntrl-cli` (unified **`cntrl`** binary: daemon / panel / popup), `cntrl-gtk` (GTK fallback), and `cntrl-iced` (ICED library + standalone binary). Build and run from the repository root.

| Entry | Command |
|-------|---------|
| **CLI** | `cargo run -p cntrl-cli -- <subcommand>` → binary **`cntrl`** |
| `cntrl panel` | Full ICED shell |
| `cntrl popup` / `cntrl popup --toggle` | Compact ICED / Waybar toggle |
| `cntrl daemon` | Unix socket daemon (foreground) |
| GTK | `cargo run -p cntrl-gtk` |
| ICED only | `cargo run -p cntrl-iced` (same UI as `cntrl panel`) |

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
   cargo run -p cntrl-gtk
   # or ICED: cargo run -p cntrl-cli -- panel
   ```

3. **Lighting** tab: choose **Static**, **Custom**, **Off**, or a **labeled** preset; set color where relevant; **Apply To Keyboard**. **Static** is not firmware id `3` — see baseline doc. Optional: `MCHOSE_HIDRAW=/dev/hidrawN` if auto-detection picks the wrong interface.

**Legacy Phase 1 notes (Tauri era):** [PHASE1_LIGHTING.md](PHASE1_LIGHTING.md) — prefer **baseline** docs above for current behavior.

## Scripts

| Command | Purpose |
|---------|---------|
| `./scripts/smoke_lighting.sh` | `cargo build` + pointers to manual checklist |

## Development

Rust + [rust-analyzer](https://marketplace.visualstudio.com/items?itemName=rust-lang.rust-analyzer); GTK/libadwaita dev packages per your distro.
