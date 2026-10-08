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
- **Per-fan ARGB:** each header's daisy chain can be split into individual fans (**ARGB CHAINS** presets) — every fan becomes its own draggable ring, so chains that light "out of order" are fixed by placing each fan where it physically sits (or toggling **Reverse** for backwards wiring; the accent dot marks the chain's first LED).
- **Per-element trim:** each placed device has its own brightness, hue shift, and saturation sliders on top of the global effect settings.
- **Headless:** `cntrl canvas --effect plasma --fps 20` streams with the saved layout; `--seconds N` for a timed run.

## Fan control (temp→duty curves, native)

The **Fans** tab is a native equivalent of Windows' *Fan Control*: it reads temperature sensors from `/sys/class/hwmon` and drives fan PWM channels with custom curves. No external tools — plain sysfs, same "native-only" philosophy as the lighting.

- **What's controllable:** anything hwmon exposes as `pwmN` + `pwmN_enable`. On this machine the **GPU fan** (`amdgpu`) works out of the box. Every temperature the kernel exposes (CPU `k10temp`, GPU, NVMe, board `gigabyte_wmi`, DIMM `spd5118`, …) is available as a **curve input**, even for sensors with no controllable fan.
- **Curves:** drag points on the graph (temp on X, duty% on Y), click empty space to add a point, right-click to delete. Linear interpolation with clamped ends, exactly like Fan Control. Each channel also has a **min/max duty** envelope. Temps are EMA-smoothed (the "Response Time" knob) and a small change threshold prevents fan hunting.
- **The engine:** enable it with the **Enable engine** toggle (or `enabled: true` in `~/.config/cntrl/fan_curves.json`). It runs inside `cntrl daemon`, or standalone in the foreground via `cntrl fans run`. The GUI only edits the JSON; the engine picks up changes on its next tick — the same relationship the theme watcher has with `settings.json`.
- **Safety:** on start the engine records each driven fan's original mode/PWM and flips it to manual. On Ctrl-C, SIGTERM, or a panic it **restores** every fan to exactly what it was. `SIGKILL` (`kill -9`) can't be trapped — if that happens, `cntrl fans restore` puts the fans back.

### First-time setup (permissions)

hwmon PWM files are root-only by default. Install the udev rule so CNTRL can drive fans as your user:

```bash
sudo install -Dm644 packaging/udev/99-cntrl-hwmon.rules /etc/udev/rules.d/99-cntrl-hwmon.rules
sudo udevadm control --reload-rules && sudo udevadm trigger --subsystem-match=hwmon
cntrl fans list      # the GPU fan should now read "Controllable"
```

(hwmon devices don't "replug" like USB — the `udevadm trigger` re-applies perms; a reboot also works.)

### CLI

```bash
cntrl fans list                 # sensors + channels, with status and why-blocked detail
cntrl fans apply --dry-run      # show the duty each curve would command right now
cntrl fans run                  # run the engine in the foreground (Ctrl-C restores fans)
cntrl fans restore              # recover fans after a SIGKILL / crash
```

### Motherboard headers (experimental)

Case-fan headers hang off the board's Super I/O chip (ITE `it87`), which Gigabyte boards often don't advertise, so the driver isn't auto-loaded. Load it, then **validate before trusting it** — on many Gigabyte boards the embedded controller re-asserts its own fan curve and silently ignores PWM writes:

```bash
sudo cntrl fans it87-load --force-id 0x8689     # chip id varies by board (IT8689E here)
sudo udevadm control --reload-rules && sudo udevadm trigger --subsystem-match=hwmon
cntrl fans it87-spike --channel it8689/pwm1 --value 200 --seconds 30
```

The spike drives one header to a fixed duty and watches its tach: **PASS** = the RPM moved and held, so control is real; **FAIL** = the RPM drifted back to baseline (the EC is overriding writes — control isn't usable on this board); **INCONCLUSIVE** = no tach wired. The channel is restored regardless. Motherboard channels always show an **Experimental** badge in the Fans tab.

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

## Restoring lighting after boot, suspend and hibernate

Saved per-device lighting lives in `~/.config/cntrl/device_state.json`; `cntrl restore`
re-applies every entry. Two units drive it, and you want **both** — they cover
different events:

| Unit | Manager | Covers |
|------|---------|--------|
| `packaging/systemd/cntrl-restore.service` | user (`systemctl --user`) | login / boot |
| `packaging/systemd/cntrl-resume@.service` | system (`sudo systemctl`) | resume from suspend, hibernate, hybrid-sleep |

Sleep targets only exist in the *system* manager, so the resume hook cannot be a
user unit — it is a template instantiated for your username, running as you (and
so with the `users` group access the udev rules grant):

```bash
sudo install -Dm644 packaging/systemd/cntrl-resume@.service \
    /etc/systemd/system/cntrl-resume@.service
sudo systemctl daemon-reload
sudo systemctl enable "cntrl-resume@$USER.service"
```

`INSTALL_SYSTEMD=1 ./scripts/install.sh` does this for you.

Note that `Off` is a state like any other: if the last thing you applied to a
device was the Off effect, that is what boot and resume restore. Applying any
color (including via theme sync) records the new state and clears it.

That reprobes with OpenRGB and persists mapping to `~/.config/cntrl/pc_rgb_probe.json`. Runtime apply paths then auto-load this mapping.

## Development

Rust + [rust-analyzer](https://marketplace.visualstudio.com/items?itemName=rust-lang.rust-analyzer); GTK/libadwaita dev packages per your distro.
