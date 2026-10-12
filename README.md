# CNTRL

Native RGB lighting and fan control for Linux, in one app: a keyboard, the
motherboard's ARGB headers, GPU and RAM lit as one canvas, and temperature-driven
fan curves. No OpenRGB, no vendor software, no browser engine — CNTRL talks to
the hardware directly over `hidraw`, `i2c` and `hwmon`.

## Supported hardware

CNTRL only shows and drives what it detects. This is what it knows today:

| Device | What works | Notes |
|--------|------------|-------|
| **MCHOSE Ace68-II** keyboard (`41e4:2116`) | Static colour, built-in effects, per-key live streaming (60+ fps) | Needs the udev rule |
| **ITE IT5702** motherboard RGB (`048d:5702`, Gigabyte RGB Fusion 2) | Two ARGB headers per LED (fans and strips), 12V RGB header, the board's own zones | Tested on a B650 AORUS ELITE AX V2 |
| **AMD Radeon** GPU with Gigabyte RGB over i2c | One colour for the whole card | Streams colour only; never writes the card's flash |
| **Corsair DDR5** RGB RAM (SMBus) | Per-LED colour, about 9 updates a second | Rate-limited: the controller rejects faster writes |
| **Fans** on any `hwmon` chip with `pwmN` + `pwmN_enable` | Temperature curves | `amdgpu` up to RX 6000 out of the box; Gigabyte boards need the out-of-tree `it87` driver (see Fan control) |

Other hardware is not supported yet. Adding a device means a backend in
`cntrl-core/src/` (see `gpu_rgb.rs` for the smallest one) and a line in the
catalog in `devices.rs`.

## Install

```bash
git clone https://github.com/GloriousBrendon/CNTRL && cd CNTRL
INSTALL_SYSTEMD=1 ./scripts/install.sh    # builds, installs /usr/local/bin/cntrl, udev rules, services
cntrl devices                             # what was detected, and why anything is blocked
cntrl panel                               # the app
```

Needs a Rust toolchain and `libudev`. `INSTALL_SYSTEMD=1` also sets up the
background daemon (theme sync, fan engine, canvas stream) and the units that
restore lighting at login and after suspend. Add `INSTALL_IT87=1` for
motherboard fan headers.

> **Fans are safety-relevant.** CNTRL hands every fan back to firmware control
> when it stops, is disabled, or loses the temperature sensor a curve depends
> on — but a `kill -9` cannot be caught. `cntrl fans restore` recovers from that.

The repo is a **Cargo workspace**: `cntrl-core`, `cntrl-cli` (unified **`cntrl`** binary: daemon / panel / devices / apply-theme), and `cntrl-iced` (the UI — full desktop panel). iced is the only frontend; the earlier GTK app was retired (see ADR 0001). Build and run from the repository root.

| Entry | Command |
|-------|---------|
| **CLI** | `cargo run -p cntrl-cli -- <subcommand>` → binary **`cntrl`** |
| `cntrl panel` | Full ICED desktop app |
| `cntrl devices` | Native RGB device inventory + status |
| `cntrl apply-theme` | Apply the theme accent to the lighting |
| `cntrl daemon` | Unix socket daemon (foreground; HID apply + theme watcher) |
| `cntrl canvas` | Stream a canvas effect across ARGB devices (headless; layout from Lighting › Layout) |
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

- **27 effects** (`cntrl canvas --list`): everyday (Solid, Gradient, Breathing, Spectrum Cycle, rainbows), ambient (Fire, Lava, Rain, Matrix, Twinkle, Aurora, Plasma, Waves), motion (Comet, Scanner, Radar Sweep, Ripple, Neon Pulse, Bouncing Balls) audio-reactive (Spectrum Bars, Audio Wave, Beat Pulse) and Screen Ambient. Each shows only the controls it uses (speed, scale, direction, one to three colours) and remembers its own settings. The first colour can follow your theme accent.
- **Layers:** draw a second effect over the main one with Add / Screen / Multiply / Normal blending and an opacity slider — Twinkle over a Gradient, for example. The overlay uses that effect's own saved settings.
- **Audio:** audio effects and the "Brightness follows the music" option listen to whatever your default output is playing. Capture uses `parec` (part of `pipewire-pulse` / PulseAudio) and runs only while something needs it.
- **Screen Ambient:** the devices take their colours from what is on screen. Capture uses `grim`, so it needs a wlroots compositor (Hyprland, Sway); it grabs 10 times a second and runs only while the effect is active. By default it follows the whole desktop; to follow one monitor set `"screen_output": "DP-1"` under `settings` in `~/.config/cntrl/canvas_layout.json`.
- **Live streaming:** each device family writes on its own thread at its own pace. At 60 fps the keyboard gets true **per-key** frames, the ARGB headers per-LED frames (about 50 a second), the GPU a sampled colour (never saved to its flash) and Corsair RAM per-LED SMBus writes (rate-limited). A device that fails or is unplugged is retried every two seconds. When a stream stops, every device returns to its saved lighting.
- **Runs in the daemon:** with `cntrl daemon` running, the stream lives there — it survives closing the panel, and **Start at login** under Lighting › Layout brings it back after a reboot. Without the daemon, the panel streams by itself. Picking Static or Off under Lighting stops the stream; a theme change while streaming is saved but does not write over the effect.
- **Arrangement:** drag devices under Lighting › Layout; a selected device can be rotated and resized. Layout + effect settings persist to `~/.config/cntrl/canvas_layout.json`.
- **ARGB chains:** tell CNTRL what is plugged along each header — how many fans, how many LEDs per fan (8, 12 or 16), and an optional strip after them; a header can also be empty. Every fan becomes its own draggable ring. **Identify** lights only the selected device (first LED on the wire in red), so you can match each ring to its physical fan and toggle **Reverse** for backwards wiring.
- **Board zones:** the motherboard's own lit zones appear as "Board zone N" and start disabled; enable one and use Identify to see which part of the board it is.
- **Per-element trim:** each placed device has its own brightness, hue shift, and saturation sliders on top of the global effect settings.
- **Headless:** `cntrl canvas --effect fire --fps 30` starts a stream with the saved layout (in the daemon when it runs: stop it with `cntrl canvas --stop`); `--seconds N` for a timed run.

### Board-specific settings

Detection covers the common case. These environment variables override it for other boards:

| Variable | Default | Meaning |
|----------|---------|---------|
| `CNTRL_ITE_PATH` | auto | hidraw node of the ITE controller |
| `CNTRL_ITE_STRIP_COLOR_ORDER` | board default | Colour order of the ARGB headers (e.g. `grb`) |
| `CNTRL_ITE_RGB12V_ZONE` | `2` | ITE zone of the 12V RGB header |
| `CNTRL_GPU_I2C_BUS` | auto | i2c bus of the GPU's RGB controller (e.g. `/dev/i2c-8`) |
| `CNTRL_GPU_I2C_ZONES` | `5` | Number of GPU lighting zones |
| `CNTRL_CORSAIR_RAM_LED_COUNT` | `10` | LEDs per DIMM |
| `CNTRL_PC_RGB_BACKENDS` | all | Limit static applies to some backends (`ite_rgb,gpu_rgb,corsair_ram`) |
| `CNTRL_CANVAS_LAYOUT_PATH` | `~/.config/cntrl/canvas_layout.json` | Use another layout file |

## Fan control (temp→duty curves, native)

The **Fans** tab is a native equivalent of Windows' *Fan Control*: it reads temperature sensors from `/sys/class/hwmon` and drives fan PWM channels with custom curves. No external tools — plain sysfs, same "native-only" philosophy as the lighting.

- **What's controllable:** anything hwmon exposes as `pwmN` + `pwmN_enable`. The **GPU fan** (`amdgpu`) works out of the box on cards up to the RX 6000 series (RX 7000 and newer keep their own fan curve and ignore `pwm1`; not supported yet); motherboard headers need the `it87` driver (below). Every temperature the kernel exposes (CPU `k10temp`, GPU, NVMe, board `gigabyte_wmi`, DIMM `spd5118`, …) is available as a **curve input**, even for sensors with no controllable fan.
- **Curves:** drag points on the graph (temp on X, duty% on Y), click empty space to add a point, right-click to delete. Linear interpolation with clamped ends, exactly like Fan Control. Each channel also has a **min/max duty** envelope. Temps are EMA-smoothed (the "Response Time" knob) and a small change threshold prevents fan hunting.
- **The engine:** enable it with the **Enable engine** toggle (or `enabled: true` in `~/.config/cntrl/fan_curves.json`). It runs inside `cntrl daemon`, or standalone in the foreground via `cntrl fans run`. The GUI only edits the JSON; the engine picks up changes on its next tick — the same relationship the theme watcher has with `settings.json`.
- **Safety:** before the engine first drives a fan it records the fan's original mode. It hands the fan back to firmware control on Ctrl-C, SIGTERM or a panic, when you untick **Enable engine**, when you remove the fan's curve, and when the sensor its curve reads disappears (it takes the fan back when the sensor returns). Only one engine can run at a time. `SIGKILL` (`kill -9`) can't be trapped — if that happens, `cntrl fans restore` puts the fans back.

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

CPU and case fan headers hang off the board's Super I/O chip (ITE, `it87` driver). The driver is not loaded automatically, and **which `it87` you load matters**:

- The kernel's built-in `it87` reads temperatures and RPM, but on recent Gigabyte boards (tested: B650 AORUS ELITE AX V2, IT8689E) the board's own fan controller ignores its speed writes. Fans can be switched to manual, but the speed never follows.
- The out-of-tree driver ([frankcrawford/it87](https://github.com/frankcrawford/it87), AUR `it87-dkms-git`) works. DKMS only builds it for kernels whose **headers are installed**, so install the headers package that matches `uname -r` (for example `linux-cachyos-bore-headers`), or you will silently keep the built-in driver.

```bash
paru -S it87-dkms-git                     # plus the headers for your running kernel
INSTALL_IT87=1 ./scripts/install.sh       # load it87 at every boot + hwmon udev rule
modinfo it87 | grep filename              # must contain updates/dkms
cntrl fans list                           # warns if the built-in driver is loaded
```

Channel ids contain the chip name the driver reports (`it8689/pwm3` built-in, `it8689_90060606/pwm3` out-of-tree), so curves saved under one driver do not apply under the other.

**Validate before trusting it** on a new board:

```bash
cntrl fans it87-spike --channel it8689_90060606/pwm3 --value 255 --seconds 15
cntrl fans it87-spike --channel it8689_90060606/pwm3 --value 80 --seconds 15
```

The spike waits for the fan to settle, drives it to a fixed speed and watches the RPM: **PASS** = the RPM moved and held; **FAIL** = it stayed put (writes are being ignored); **INCONCLUSIVE** = no tach wired. The channel is handed back to the firmware afterwards. Check both a high and a low value: a pass must go up for one and down for the other. Motherboard channels always show an **Experimental** badge in the Fans tab.

## Theme switcher sync

CNTRL follows a pywal-style `~/.config/themes/colors.conf`; its `accent` key drives the lighting. Two ways to use it:

- **Direct (call from your switcher):** add one line to your theme script —

  ```bash
  cntrl apply-theme            # recolor the active slot to the new accent
  cntrl apply-theme --keyboard-only --slot 1
  ```

- **Automatic (daemon watcher):** enable **Match my desktop theme** in Settings (or set `theme_sync_enabled` in `~/.config/cntrl/settings.json`). While `cntrl daemon` runs, it watches `colors.conf` and re-applies on every change. The existing effect is preserved — presets keep animating, just recolored.

## Migration baseline (GTK → ICED, historical)

The migration is finished and the GTK app is gone. These record how it was done:

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
   ```

3. **Lighting**: choose **Static**, **Custom**, **Off**, or a named keyboard effect; set the color where relevant. Changes apply as you make them. **Static** is not firmware id `3` — see baseline doc. Optional: `MCHOSE_HIDRAW=/dev/hidrawN` if auto-detection picks the wrong interface.

**Legacy Phase 1 notes (Tauri era):** [PHASE1_LIGHTING.md](docs/protocol/PHASE1_LIGHTING.md) — prefer **baseline** docs above for current behavior.

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
so with the `users` group access the udev rules grant).

`INSTALL_SYSTEMD=1 ./scripts/install.sh` installs and enables both, plus the
daemon's user service, and points all of them at the one installed binary
(`$PREFIX/bin/cntrl`). It also replaces an older `~/.local/bin/cntrl` with a link,
so your shell and the services never run different versions.

Note that `Off` is a state like any other: if the last thing you applied to a
device was the Off effect, that is what boot and resume restore. Applying any
color (including via theme sync) records the new state and clears it.

That reprobes with OpenRGB and persists mapping to `~/.config/cntrl/pc_rgb_probe.json`. Runtime apply paths then auto-load this mapping.

## License

Licensed under either of [Apache License, Version 2.0](LICENSE-APACHE) or
[MIT license](LICENSE-MIT) at your option. Contributions are accepted under the
same terms.

## Development

Rust + [rust-analyzer](https://marketplace.visualstudio.com/items?itemName=rust-lang.rust-analyzer); `libudev` development files and `pkg-config` (see `.github/workflows/ci.yml`).
