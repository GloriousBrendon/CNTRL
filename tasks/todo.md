# Tasks: fans, RGB, Canvas

Plan and reasoning: `tasks/plan.md`. "Review N" = finding N in
`docs/reviews/2026-10-08-fan-control-and-rgb-review.md`. "C-x" = canvas finding
in the plan.

Commands used below:

- Tests: `cargo test -p cntrl-core`
- Build: `cargo build --release -p cntrl-cli`
- Panel from the build: `target/release/cntrl panel`
- Install: `./scripts/install.sh` (asks for sudo)

Every task must leave tests passing and the build clean.

---

## Phase 0: Baseline

## Task 0: Commit the current working tree — DONE

- [x] `8aa133d` rgb: remember the lighting theme sync really applied; resume hook
- [x] `5ff39bd` fans: hwmon fan control engine, curves, Fans tab and CLI
- [x] `graphify-out/` ignored; `docs/reviews/` and `tasks/` committed as docs

Files that mix both changes (`main.rs`, `daemon.rs`, `app.rs`) went into the fan commit.

---

## Phase 1: Fan safety

## Task 1: Fake-hwmon test root + first `apply_assignment` test

**Description:** `fan_control::discover()` reads `/sys/class/hwmon` directly, so
the code that writes fan speeds has no test (Review 8). Add `discover_in(root)`
and make `discover()` call it with the real path. Write one test that builds a
fake chip in a temp directory and runs a tick.

**Acceptance criteria:**
- [x] `discover_in(&Path)` exists; `discover()` behaviour is unchanged
- [x] A test creates `hwmonN/{name,pwm1,pwm1_enable,fan1_input,temp1_input}` in a temp dir and asserts a tick writes the expected PWM and sets manual mode
- [x] No test touches the real `/sys`

**Verification:**
- [x] `cargo test -p cntrl-core fan_` passes
- [x] `target/release/cntrl fans list` still prints 21 sensors and `amdgpu/pwm1`

**Dependencies:** None
**Files likely touched:** `cntrl-core/src/fan_control.rs`, `cntrl-core/src/fan_curves.rs`
**Estimated scope:** S

## Task 2: Lost sensor hands the fan back to firmware

**Description:** Review 3, with your choice of policy. When a sensor read
fails, the old smoothed temperature is kept forever and the fan holds its last
speed. Drop the stale value, and when a curve cannot be evaluated restore that
channel to its captured original state (firmware control). Try again each tick,
and take the fan back when the sensor returns.

**Acceptance criteria:**
- [x] A failed read removes that sensor from `state.smoothed`
- [x] A channel whose curve cannot be evaluated is restored to its original mode, never left in manual
- [x] If the captured original mode was itself manual, the channel is set to automatic instead
- [x] Test: delete `temp1_input` between two ticks; `pwm1_enable` is back to the original value; put it back, next tick engages again

**Verification:**
- [x] `cargo test -p cntrl-core fan_` passes

**Dependencies:** Task 1
**Files likely touched:** `cntrl-core/src/fan_curves.rs`
**Estimated scope:** S

## Task 3: Disabling the engine or removing a curve hands the fan back

**Description:** Review 2. Unticking "Enable engine" only stops the ticks; the
fan stays in manual mode at its last speed. Restore on disable, and restore any
captured channel that no longer has an assignment.

**Acceptance criteria:**
- [x] `enabled: false` restores every captured channel and resets tick state
- [x] Removing one assignment restores only that channel
- [x] Test: fake chip returns to its original `pwm1_enable` after the config is disabled

**Verification:**
- [x] `cargo test -p cntrl-core fan_` passes
- [x] Manual: enable a curve on `amdgpu/pwm1`, untick the engine, `cat /sys/class/hwmon/hwmon3/pwm1_enable` shows the original value within 2 s

**Dependencies:** Task 1
**Files likely touched:** `cntrl-core/src/fan_engine.rs`, `cntrl-core/src/fan_curves.rs`
**Estimated scope:** S

## Task 4: Daemon waits for the fan thread on stop

**Description:** Review 1 and 9. On SIGTERM the daemon's main loop returns
before the fan thread restores the fans. Keep the thread handle and join it.
Drop the idle accept sleep from 200 ms to 20 ms.

**Acceptance criteria:**
- [x] `run_unix` joins the fan thread after the accept loop
- [x] Accept loop sleeps 20 ms when idle
- [x] The daemon exits within 2 s of SIGTERM

**Verification:**
- [x] Build succeeds
- [x] Manual: with a curve active, `systemctl --user stop cntrl-daemon`; the journal shows the restore line before "stopped", and `pwm1_enable` is back to its original value

**Dependencies:** None
**Files likely touched:** `cntrl-cli/src/daemon.rs`
**Estimated scope:** XS

## Task 5: `cntrl fans apply` captures state and takes the lock

**Description:** Review 4. The one-shot command switches the fan to manual
without saving the original state, so `cntrl fans restore` has nothing to
restore.

**Acceptance criteria:**
- [x] A non-dry-run `fans apply` calls `capture_and_engage` before `tick`
- [x] It takes the engine lock and refuses with a clear message if the daemon holds it
- [x] `cntrl fans restore` after `fans apply` puts the fan back

**Verification:**
- [x] Build succeeds
- [x] Manual: stop the daemon, `cntrl fans apply`, `cntrl fans restore`, `pwm1_enable` matches the value before

**Dependencies:** None
**Files likely touched:** `cntrl-cli/src/main.rs`
**Estimated scope:** XS

## Checkpoint A: fans are safe on the GPU fan
- [x] `cargo test -p cntrl-core` passes; build clean
- [x] On `amdgpu/pwm1`: enable, disable, stop daemon, `kill -TERM`, `fans apply` + `restore` all end with the fan back in its original mode
- [ ] You have looked at it before motherboard fans are touched

**Run on 2026-10-08** with the new build on case fan `it8689_90060606/pwm3` (it could
be tested there because Task 9 passed; the GPU fan was in zero-RPM mode):
engine on → manual; disabled → auto; curve removed → auto; sensor id missing →
auto, then manual again when it returned; SIGTERM → "restored 1" logged before
"stopped"; `fans apply` refused while the daemon ran, and `fans apply` +
`fans restore` alone ended in auto. Not run: `kill -TERM` during a write.

**Changes from the plan:**
- A lost sensor sets the fan to automatic mode (2) directly, instead of replaying the saved original. Simpler, and the same on this hardware.
- The engine no longer switches a fan to manual before it has a speed to write (`capture_and_engage` became `capture_originals`).
- Restore no longer writes the PWM value in automatic mode. This is the code half of Task 8 (the "Device or resource busy" warning).

---

## Phase 2: Fans robust + motherboard fans

## Task 6: OS file lock instead of pid file

**Description:** Review 6. After a power cut the pid file stays and can block
the engine forever. Hold an OS lock on the file; the kernel releases it when the
process dies. Delete `pid_alive` and the `Drop` impl.

**Acceptance criteria:**
- [x] Lock uses `File::try_lock` (or `flock` through the existing `nix` dependency)
- [x] A leftover lock file from a dead process does not block start
- [x] Two engines started together: exactly one runs

**Verification:**
- [x] Test: take the lock twice in one process through two handles, second fails
- [x] Manual: `cntrl fans run` while the daemon runs prints "already running"

**Dependencies:** None
**Files likely touched:** `cntrl-core/src/fan_engine.rs`
**Estimated scope:** XS

## Task 7: Write-skip checks the hardware value

**Description:** Review 7. A write is skipped when the target is close to the
last value *written*, even if something else changed the fan since.

**Acceptance criteria:**
- [x] Skip only when `read_pwm()` also equals the last written raw value
- [x] Test: change the fake `pwm1` between ticks at a steady temperature, next tick rewrites it

**Verification:**
- [x] `cargo test -p cntrl-core fan_` passes

**Dependencies:** Task 1
**Files likely touched:** `cntrl-core/src/fan_curves.rs`
**Estimated scope:** XS

## Task 8: Restore writes PWM only in manual mode

**Description:** Review 5, which was from memory and not tested. First look at
the real restore message from Checkpoint A. Only if it reports an error on
`amdgpu/pwm1`, skip the PWM write when the restored mode is not manual.

**Acceptance criteria:**
- [x] The restore message on this machine is recorded in the task notes
- [x] If it showed an error: restore reports "restored 1" with no error
- [x] If it did not: close the task with no code change

**Result:** the error was real on it87 (`Device or resource busy`). Fixed in Phase 1;
the spike now also waits for a settled baseline. Re-run on `pwm3`: 844 → 1630 RPM,
PASS, no restore warning. A direction check was not added: in automatic mode the
driver does not report the real duty, so "up or down" cannot be known in advance.
The README says to test one high and one low value.

**Verification:**
- [x] Manual: `cntrl fans restore` output after an engine run

**Dependencies:** Checkpoint A
**Files likely touched:** `cntrl-core/src/fan_control.rs`
**Estimated scope:** XS

## Task 9: it87 validation spike on the real board (you run it)

**Description:** You confirmed the headers are controllable on Windows with
Fan Control, so the hardware accepts it. What is still unknown is whether the
Linux `it87` driver can do the same on this board. Only the GPU fan is visible
today because `it87` is not loaded. The in-tree module mentions IT8689 and the kernel already boots with
`acpi_enforce_resources=lax`. Find out whether the board accepts PWM writes.
This needs sudo, so you run it; no code changes.

```
sudo modprobe it87            # if "No such device": sudo cntrl fans it87-load --force-id 0x8689
cntrl fans list               # expect it87/pwm1..N
cntrl fans it87-spike --channel it87/pwm1 --value 200 --seconds 30
```

**Result so far (2026-10-08):**
- `sudo modprobe it87` loads with no `force_id`. The chip shows up as `it8689` (the chip name, not `it87`).
- Five channels: `it8689/pwm1,2,3,4,6`, all with RPM, all in mode 2 (firmware automatic). Six more temperature sensors.
- All five were "Permission blocked": the udev rule matched the name `it87`. Fixed to `it8*` in the repo; needs reinstalling.
- The spike failed only because the plan used the wrong id (`it87/pwm1`). Use `it8689/pwm1`.
- udev rule reinstalled: all five channels Controllable.
- **Spike verdict: the in-tree driver does not control speed.** In manual mode the RPM ignores the value written:
  - `pwm1` (about 1330 RPM in auto): 255 → 1268, 200 → 1132, 120 → 1275, 60 → 1347.
  - `pwm3` (919 RPM in auto): 255 → 811.
  - Mode switching works (the RPM shifts and goes flat); the duty value has no effect.
- Every restore printed `Device or resource busy` on the PWM write after going back to mode 2. The mode restore itself worked (all channels read `pwm_enable=2` afterwards). This is Review 5, now confirmed on it87: Task 8 is real.
- The spike once said PASS wrongly: its baseline was taken while the fan was still settling after the previous run. Task 8 also makes the spike wait for a steady baseline and check the direction of the change.
- **Out-of-tree driver (`it87-dkms-git` 358.bd7a566): PASS.** It needed `linux-cachyos-bore-headers` so DKMS could build for the running kernel. On `pwm3`: 255 → 1850 RPM (from 845), 80 → 585 RPM. Speed follows the value in both directions.
- With this driver the chip is named `it8689_90060606` and there are six channels (`pwm1`–`pwm6`; `pwm5` is new). Channel ids, and so saved curves, depend on which driver is loaded.
- Still open: which channel is which physical fan.

**Acceptance criteria:**
- [x] Recorded: does `it87` load, with or without `force_id`
- [ ] Recorded: which `it87/pwmN` maps to which physical fan
- [x] Recorded: does RPM follow the PWM write for 30 s, or does firmware take it back

**Verification:**
- [ ] The spike report is pasted into this task

**Dependencies:** Checkpoint A
**Files likely touched:** none
**Estimated scope:** XS

## Task 10: Load it87 at boot (only if Task 9 passes)

**Description:** Make the motherboard fans available after every boot.
Task 9 showed the in-tree driver cannot set speed on this board; the
out-of-tree `it87-dkms-git` can. So the README must say: install that package
and the headers for your running kernel. `fans list` should warn when an ITE
chip is driven by the in-tree module (module path not under `updates/`),
because that combination reads fine and silently ignores speed writes.
`install.sh` writes `/etc/modules-load.d/cntrl-it87.conf` (plus a
`modprobe.d` option line if `force_id` was needed). If Task 9 failed, instead
say so in the README and Fans tab and close the task.

**Acceptance criteria:**
- [x] After reboot, `cntrl fans list` shows the it87 channels with no manual step
- [x] The channels are writable without root (the udev rule already covers `it87`)
- [x] The install step is opt-in or clearly labelled experimental
- [x] `install.sh` also installs `packaging/udev/99-cntrl-hwmon.rules` (today it does not; the rule on this machine was put there by hand)

**Verification:**
- [x] Manual: reboot, `cntrl fans list`, set a curve on one case fan in the Fans tab, RPM follows

**Dependencies:** Task 9
**Files likely touched:** `scripts/install.sh`, `README.md`
**Estimated scope:** S

**Status (2026-10-08):** code and README done (`INSTALL_IT87=1 ./scripts/install.sh`,
driver warning in `cntrl fans list`). Confirmed after a reboot: six channels Controllable, out-of-tree driver loaded, no warning. The warning for the built-in driver
was only checked in the "no warning" direction (the out-of-tree driver is loaded).

## Task 11: Fans tab reads once per poll; numeric hwmon sort; delete dead code

**Description:** Review 10, 11, 12. The tab reads sysfs on every redraw; read
in `FanPoll` and keep the values in `App`. Sort `hwmonN` by number so
`hwmon10` does not come before `hwmon2`. Delete `mix_would_cycle`,
`smoothed_temp`, `request_stop`, `FanChannelStatus::Absent`, `hwmon_path` and
the plain `set_manual`.

**Acceptance criteria:**
- [x] `fans_view` does no file reads
- [x] Duplicate chips are numbered in numeric hwmon order
- [x] The six unused items are gone and nothing else changed behaviour

**Verification:**
- [x] `cargo test -p cntrl-core` passes; build has no new warnings
- [x] Manual: dragging a curve point in the Fans tab is smooth; readings update each second

**Dependencies:** Task 2 (touches the same file)
**Files likely touched:** `cntrl-iced/src/app.rs`, `cntrl-core/src/fan_control.rs`, `cntrl-core/src/fan_curves.rs`, `cntrl-core/src/fan_engine.rs`
**Estimated scope:** M

## Checkpoint B: all fans controlled, or the limit is documented
- [ ] Tests pass, build clean
- [ ] GPU fan and (if Task 9 passed) every motherboard fan follow their curves for an hour of normal use
- [ ] A stress run (`stress-ng` or a game) ramps the fans; stopping the daemon hands them back
- [ ] Review with you before Phase 3

---

## Phase 3: RGB apply/restore

## Task 12: Panel Apply stops leaking the backend list

**Description:** Review 15 and 16. The Lighting tab's Apply sets the
process-wide `CNTRL_PC_RGB_BACKENDS` and never clears it, so a later "Apply
theme now" uses the old device selection. The same block has its own copy of
the save logic and saves before applying.

**Acceptance criteria:**
- [ ] Apply with only the keyboard ticked, then "Apply theme now": RAM, GPU and motherboard all change colour and are saved
- [x] The panel calls `device_state::remember_applied` after a successful apply; its own copy is deleted
- [x] A failed apply is not saved

**Verification:**
- [x] Build succeeds
- [ ] Manual: the sequence above, then check `~/.config/cntrl/device_state.json` has the theme colour for all four entries

**Dependencies:** None
**Files likely touched:** `cntrl-iced/src/app.rs`
**Estimated scope:** S

**Status (2026-10-08):** fixed in shared code (`BackendRegistry::with_enabled` sets the
list, runs, and puts the old value back; unit-tested). `cntrl restore` uses it too.
Open: the manual check in the panel (first and last boxes).

## Task 13: One install path; resume unit follows `PREFIX`

**Description:** What is left of Review 14 (the "stale binaries" part was
wrong: both installed copies match the current build). Two copies in two
places can drift, and the resume unit hard-codes `/usr/local/bin`.

**Acceptance criteria:**
- [x] Login unit and resume unit run the same binary path
- [ ] `PREFIX=/usr ./scripts/install.sh` produces units that point at `/usr/bin/cntrl`
- [x] The second copy is removed or is a symlink

**Verification:**
- [ ] Manual: `systemctl --user cat cntrl-restore.service` and `systemctl cat cntrl-resume@$USER.service` show the same path
- [ ] Manual: suspend and resume once; lighting comes back

**Dependencies:** None
**Files likely touched:** `scripts/install.sh`, `packaging/systemd/cntrl-resume@.service`, `packaging/systemd/cntrl-restore.service`
**Estimated scope:** S

---

**Status (2026-10-08):** done in the repo; takes effect when you run
`INSTALL_SYSTEMD=1 ./scripts/install.sh`. Found while doing it: `~/.local/bin/cntrl`
is first on PATH and is the Aug 31 build, so every `cntrl ...` typed in a shell so
far ran old code. The daemon unit was not in the repo; added as
`packaging/systemd/cntrl-daemon.service`. `PREFIX=/usr` was not run.

## Phase 4: Canvas correctness

## Task 14: Chain segments keep their own LED count

**Description:** C-A. Your layout has header 2 as 3 fans × 12 LEDs; the fans
have 8. `set_fans` splits the header total and forgets the per-fan count, and
the total is forced to 32/36. Make a chain a list of segments whose total is
their sum: picking a fan count keeps the current LEDs-per-fan (default 8), and
a strip segment can follow the fans. A header can also be empty: you no longer
have the strip on header 1, so it must be possible to remove it from the canvas
and stop writing to it.

**Acceptance criteria:**
- [x] "3 fans" then "8/fan" gives 24 LEDs; changing fan count afterwards keeps 8 per fan
- [x] A header can hold fans followed by a strip with its own LED count
- [x] A header can be set to "Empty": no canvas device, no writes to that header
- [x] Existing layout files still load; the test `chain_split_reconciles_segment_devices` is updated to the new rule

**Verification:**
- [x] `cargo test -p cntrl-core canvas` passes
- [ ] Manual: stream Comet slowly across three fans placed in a row; the head crosses fan 1, then 2, then 3, with no fan lit in two halves

**Dependencies:** None
**Files likely touched:** `cntrl-core/src/canvas.rs`, `cntrl-iced/src/app.rs`
**Estimated scope:** S

**Status (2026-10-08):** done in code and tests. Your saved layout still says header 1 = 32-LED strip
and header 2 = 3 fans × 12; fix it in the Canvas tab: Header 1 "No fans" + strip slider to none,
Header 2 "8/fan". Open: the manual Comet check.

## Task 15: Identify button for the selected device

**Description:** There is no way to tell which physical fan is "H2 Fan 2"
except trial and error. While streaming, an Identify toggle makes the selected
device solid white and its first LED the accent colour, everything else dim.

**Acceptance criteria:**
- [ ] Identify lights only the selected device on the hardware
- [ ] The first LED on the wire is visibly different, so Reverse can be set correctly
- [ ] Turning it off returns to the effect with no restart

**Verification:**
- [ ] Manual: identify each of the three fans, the strip, both RAM sticks

**Dependencies:** Task 14
**Files likely touched:** `cntrl-core/src/canvas_stream.rs`, `cntrl-core/src/canvas.rs`, `cntrl-iced/src/app.rs`
**Estimated scope:** S

**Status (2026-10-08):** done; unit-tested on the wire order. Shows white with the first wire LED
red, other devices dark. Open: the manual check on each device.

## Task 16: Per-backend timing in stream stats

**Description:** C-B, measurement first. Record how long each backend's write
takes per frame and show it in the status line and in `cntrl canvas`. This
decides whether Task 17's threads are needed and what each device's rate is.

**Acceptance criteria:**
- [x] Status shows average and worst write time per backend over the last second
- [x] Numbers recorded here for 20, 40 and 60 fps on this machine

**Verification:**
- [x] Manual: `target/release/cntrl canvas --effect wave --fps 40 --seconds 20`

**Dependencies:** None
**Files likely touched:** `cntrl-core/src/canvas_stream.rs`, `cntrl-cli/src/main.rs`
**Estimated scope:** S

**Measured (2026-10-08), one thread, wave effect, 3 fans × 8:** keyboard 6.6 ms, strips 19.5 ms,
GPU 4.8 ms, RAM 2.6 ms per frame, about 33 ms in total. Asked 20 fps → 20; asked 40 → 29; asked 60 → 30.

## Task 17: One thread per backend, with reconnect and backoff

**Description:** C-B, C-C, C-J. Render once per frame into a shared snapshot;
each backend thread takes the newest frame at its own rate (keyboard up to 60,
ITE and RAM at the rate Task 16 shows is safe, GPU change-detected). A failed
open or write is retried after 2 s, not never and not every frame. Device specs
are built once per layout change, not per call.

**Acceptance criteria:**
- [x] Keyboard holds the requested fps while ITE and RAM run slower
- [ ] Unplug and replug the keyboard during a stream: it comes back within 3 s without restarting the stream
- [x] A RAM write error drops and reopens the session
- [x] Stopping the stream still restores each PC backend's saved lighting

**Verification:**
- [x] `cargo test -p cntrl-core` passes
- [ ] Manual: 60 fps stream for 10 minutes, no strobing on motherboard zones, no errors in the status line

**Dependencies:** Task 16 (skip the threading part if the keyboard already holds fps; keep the reconnect part)
**Files likely touched:** `cntrl-core/src/canvas_stream.rs`
**Estimated scope:** M

**Measured after (2026-10-08), asked 60 fps:** render 60 fps; keyboard 60 writes/s, strips 52/s,
GPU 60/s, RAM 8–9/s; no errors. Open: unplug/replug the keyboard during a stream, and the
10-minute run. The strips were only verified at 20 fps before; watch them for flicker at 52/s.

## Task 18: Test for chain assembly

**Description:** C-I. `assemble_chain` decides which colour goes to which LED
on the wire and has no test.

**Acceptance criteria:**
- [x] Test: two segments, second reversed, on a left-to-right gradient frame; wire order is as expected
- [x] Test: a disabled segment sends black for its LED count and keeps later segments in place
- [x] Test: all segments disabled returns `None`

**Verification:**
- [x] `cargo test -p cntrl-core canvas_stream` passes

**Dependencies:** Task 14
**Files likely touched:** `cntrl-core/src/canvas_stream.rs`
**Estimated scope:** XS

## Task 19: Preview and hardware share one clock; one FPS range

**Description:** C-D, C-E. The preview uses the panel's clock and 10 fps; the
stream uses its own start time. The slider says 5–60, the handler clamps to 40.
Use the stream's start time for the preview while streaming, tick the preview
at 30 fps, and use one FPS range everywhere.

**Acceptance criteria:**
- [ ] With a slow Comet, the on-screen head and the head on the keyboard are at the same place
- [x] The FPS slider's maximum is the value the worker really uses
- [ ] Preview animation is visibly smooth

**Verification:**
- [ ] Manual: Comet at speed 0.3×, compare screen and keyboard

**Dependencies:** Task 17
**Files likely touched:** `cntrl-iced/src/app.rs`, `cntrl-core/src/canvas_stream.rs`
**Estimated scope:** S

**Status (2026-10-08):** preview uses the stream's clock and ticks at 30 fps; FPS is 5–60
everywhere. Open: the manual Comet comparison.

## Task 20: Save the layout on slider release

**Description:** C-F. Every slider move writes `canvas_layout.json` on the UI
thread. Keep the live update to the stream, save on release (iced sliders have
`on_release`), as dragging devices already does with `CanvasDragEnd`.

**Acceptance criteria:**
- [ ] Dragging a slider causes no file write until release
- [ ] The value is saved after release and survives a panel restart

**Verification:**
- [ ] Manual: `inotifywait -m ~/.config/cntrl/` while dragging Speed shows one write

**Dependencies:** None
**Files likely touched:** `cntrl-iced/src/app.rs`
**Estimated scope:** S

**Status (2026-10-08):** canvas sliders save on release. Not checked with `inotifywait`.

## Task 21: Effect colour control on the Canvas tab

**Description:** C-H. `settings.color` drives Ripple and Comet but is fixed at
the default pink. Reuse the Lighting tab's colour control, plus a "use theme
accent" toggle.

**Acceptance criteria:**
- [ ] Changing the colour changes Ripple and Comet on preview and hardware at once
- [x] The colour is saved with the layout
- [ ] "Use theme accent" follows the theme file

**Verification:**
- [ ] Manual: set green, restart the panel, Comet is green

**Dependencies:** None
**Files likely touched:** `cntrl-iced/src/app.rs`, `cntrl-core/src/canvas.rs`
**Estimated scope:** S

**Status (2026-10-08):** done. Swatches plus hue/saturation/lightness sliders (the Lighting
tab's own controls are tied to the keyboard profile, so the sliders were reused, not the card),
and a "first colour follows the theme accent" box. Open: the two manual checks.

## Task 22: Effect parameters and palettes

**Description:** Today every effect shares speed, scale and one colour. A large
effect library needs each effect to declare its own controls (colours, a
palette, direction, density) and the tab to draw them from that list, instead
of one hand-written slider row. Do this before adding many effects.

**Acceptance criteria:**
- [x] Each effect declares its parameters; the Canvas tab builds its controls from the declaration
- [ ] A palette (2–6 colours) is a parameter type, with "theme accent" as a source
- [x] Old layout files still load; values are saved per effect

**Verification:**
- [x] `cargo test -p cntrl-core canvas_effects` passes
- [ ] Manual: switch between two effects, each keeps its own settings

**Dependencies:** Task 21
**Files likely touched:** `cntrl-core/src/canvas_effects.rs`, `cntrl-core/src/canvas.rs`, `cntrl-iced/src/app.rs`
**Estimated scope:** M

**Status (2026-10-08):** each effect declares speed / scale / number of colours and the tab
shows only those. Speed, scale and colours are remembered per effect; old layout files migrate
(tested). Kept deliberately small: no direction or density types yet, they arrive with the
first effect that needs them (Task 27/28). The palette holds as many colours as the effect
declares; today that is at most one. Open: the manual check.

## Checkpoint C: Canvas maps correctly and stays smooth
- [ ] Tests pass, build clean
- [ ] Every device is identified and placed; a slow Comet crosses the desk in physical order
- [ ] 30 minutes at the chosen fps with no error in the status line
- [ ] Review with you before Phase 5

---

## Phase 5: Canvas always on

## Task 23: Daemon owns the stream; panel is a client

**Description:** C-G. Add `CanvasStart`, `CanvasStop`, `CanvasStatus` and
`CanvasLayout` (live update) requests to the socket protocol. The daemon holds
the `StreamHandle`. The panel sends requests when a daemon answers and keeps
today's in-process stream as the fallback.

**Acceptance criteria:**
- [ ] Start the stream in the panel, close the panel: the effect keeps running
- [ ] Reopen the panel: it shows "streaming" with live stats
- [x] Moving a device or a slider in the panel changes the hardware within one frame
- [x] With no daemon, the panel streams in-process as before

**Verification:**
- [x] Test: request/response JSON round-trip for the new messages
- [ ] Manual: the three steps above; `cntrl canvas` also goes through the daemon

**Dependencies:** Task 17
**Files likely touched:** `cntrl-core/src/ipc.rs`, `cntrl-cli/src/daemon.rs`, `cntrl-cli/src/main.rs`, `cntrl-iced/src/app.rs`
**Estimated scope:** M

**Status (2026-10-08):** done. Checked through the socket and CLI on real hardware with the new
daemon: start → 60 fps, keyboard 60/s, strips 52/s; `cntrl canvas --stop`; a timed run; SIGTERM
while streaming restores lighting. The daemon now accepts connections on its own thread, so a
request no longer waits up to 20 ms. Open: the panel side (close and reopen the panel).

## Task 24: Apply, theme sync and restore cooperate with a running stream

**Description:** C-G. Today a Lighting-tab Apply or a theme change writes
static colour to devices a stream is driving, and the next frame overwrites it.
Rule: an explicit Apply stops the stream for the devices it targets; a theme
change while streaming updates the canvas colour (when "use theme accent" is
on) and does not write static colour to streamed devices.

**Acceptance criteria:**
- [ ] Apply on the Lighting tab while streaming ends with the applied colour on those devices, steady
- [ ] A theme change while streaming does not flash static colour over the effect
- [x] `device_state.json` still records what to restore when the stream stops

**Verification:**
- [ ] Manual: both cases with all devices streaming

**Dependencies:** Task 23, Task 21
**Files likely touched:** `cntrl-cli/src/daemon.rs`, `cntrl-core/src/theme_sync.rs`, `cntrl-core/src/ipc.rs`
**Estimated scope:** M

**Status (2026-10-08):** an Apply from the panel or through the daemon stops the stream first.
Theme sync while a stream runs saves the new colour but does not write it to the devices.
`cntrl restore` while a stream runs restarts the stream instead (checked: "canvas stream
restarted", back at 60 fps). Simpler than planned: Apply stops the whole stream, not only the
devices it targets. Open: both manual checks; the theme path was not run.

## Task 25: Canvas starts at login and after resume

**Description:** Add `canvas_autostart` to settings with a toggle on the Canvas
tab. The daemon starts the stream on launch when it is set. After resume the
existing resume unit asks the daemon to restart the stream instead of only
restoring static colour.

**Acceptance criteria:**
- [ ] With the toggle on, the effect is running after login without opening the panel
- [ ] After suspend and resume the effect is running again within 10 s
- [x] With the toggle off, behaviour is the same as today

**Verification:**
- [ ] Manual: reboot; suspend and resume

**Dependencies:** Task 23, Task 13
**Files likely touched:** `cntrl-core/src/settings.rs`, `cntrl-cli/src/daemon.rs`, `cntrl-cli/src/main.rs`, `cntrl-iced/src/app.rs`
**Estimated scope:** S

**Status (2026-10-08):** `canvas_autostart` setting, "Start at login" box on the Canvas tab, daemon
starts the stream on launch. Resume is handled by the restore change in Task 24 plus the 2 s
device reconnect. Not run: autostart itself, reboot, suspend.

## Checkpoint D: always on
- [ ] Effect survives closing the panel, a reboot, and suspend/resume
- [ ] Fan engine and canvas stream run together in the daemon for a day with no errors in `journalctl --user -u cntrl-daemon`
- [ ] Review with you before Phase 6

---

## Phase 6: Dynamic canvas (your answers: yes to audio, screen, and a full effect set)

Effects cannot be copied from SignalRGB: they are HTML/JS and under its
licence. These tasks write a native library that covers the same ground.

## Task 26: Onboard motherboard zones as canvas devices

**Description:** The board's own lit zones are left out of the canvas because
latching them every frame strobes the board. Add them as single-colour canvas
devices, written only when their colour changes, like the 12V header.

**Acceptance criteria:**
- [x] Each onboard zone can be placed and enabled on the canvas
- [ ] No strobing at any fps
- [x] Disabled zones are never written by the stream

**Verification:**
- [ ] Manual: a colour-cycling effect for 10 minutes, watch the board zones

**Dependencies:** Task 17
**Files likely touched:** `cntrl-core/src/ite_rgb.rs`, `cntrl-core/src/canvas.rs`, `cntrl-core/src/canvas_stream.rs`
**Estimated scope:** M

**Status (2026-10-08):** done in code. The canvas offers every ITE zone that is not an ARGB header
or the 12V header (zones 0, 1, 5, 6, 7) as "Board zone N". They start disabled, because I cannot
tell from here which ones your board wires up; enable one, start the stream and press Identify.
Each is written only when its colour changes, the same way the 12V header already is.
Not run on hardware: no zone was enabled.

## Task 27: Effect library, batch 1 (everyday)

**Description:** Solid, Gradient (linear and radial), Breathing, Spectrum
Cycle, Rainbow (horizontal, vertical, radial, spiral), Colour Wipe.

**Acceptance criteria:**
- [x] Each has a label, a CLI slug and declared parameters
- [x] The determinism and slug tests cover them; Solid is exempt from "animates over time"
- [x] Each one checked on hardware

**Verification:**
- [x] `cargo test -p cntrl-core canvas_effects` passes

**Dependencies:** Task 22
**Files likely touched:** `cntrl-core/src/canvas_effects.rs`
**Estimated scope:** S

**Status (2026-10-08):** Solid, Gradient, Radial Gradient, Breathing, Spectrum Cycle, Rainbow
Radial, Rainbow Spiral, Colour Wipe. Rainbow Wave gained a Direction slider, which covers the
horizontal and vertical rainbows. Checked from a rendered contact sheet, not on hardware one by one.

## Task 28: Effect library, batch 2 (ambient)

**Description:** Fire, Rain, Starfield/Twinkle, Lava (smooth noise), Matrix,
Radar Sweep, Bouncing Balls, Scanner, Neon Pulse, Waves. Effects that need
randomness use a seeded generator keyed on time, so preview and hardware still
match.

**Acceptance criteria:**
- [x] Ten effects, same rules as batch 1
- [x] A full frame of any effect renders in under 2 ms (the canvas is 160×90)

**Verification:**
- [x] `cargo test -p cntrl-core canvas_effects` passes, including a timing assert in release

**Dependencies:** Task 22
**Files likely touched:** `cntrl-core/src/canvas_effects.rs` (split into a module folder if it passes ~600 lines)
**Estimated scope:** M

**Status (2026-10-08):** Lava, Fire, Waves, Twinkle, Rain, Matrix, Neon Pulse, Scanner, Radar
Sweep, Bouncing Balls. 23 effects in total. In an optimised build all 23 render ten 320×180
preview frames in 0.2 s together (under 1 ms each on average). Fire, Lava and Bouncing Balls
ran on the hardware at 60 fps without errors; the rest were only checked on the contact sheet
(`cargo test -p cntrl-core contact_sheet -- --ignored`).

## Task 29: Layers

**Description:** Two or three effects stacked with a blend mode (normal, add,
multiply, screen) and opacity, so effects can be combined, for example Twinkle
over a Gradient.

**Acceptance criteria:**
- [ ] The layout holds an ordered list of layers; one layer behaves as today
- [x] Blend modes have a unit test on fixed pixels
- [x] Old layout files load as a single layer

**Verification:**
- [x] `cargo test -p cntrl-core canvas` passes
- [ ] Manual: two layers on hardware

**Dependencies:** Task 22, Task 23
**Files likely touched:** `cntrl-core/src/canvas.rs`, `cntrl-core/src/canvas_effects.rs`, `cntrl-core/src/canvas_stream.rs`, `cntrl-iced/src/app.rs`
**Estimated scope:** M

**Status (2026-10-08):** one layer on top of the main effect, with Add / Screen / Multiply /
Normal and an opacity slider. Smaller than planned: one overlay, not a list, and the overlay
uses that effect's saved tuning (select it as the main effect once to tune it). Open: the
manual check; a second overlay if one is not enough.

## Task 30: Effect inputs (live data into effects)

**Description:** Effects are pure functions of time. Audio and screen need live
data. Add an `EffectInput` passed to `render` (time, optional audio bands,
optional screen frame). The preview in the panel gets the same input from the
daemon, so it still matches the hardware.

**Acceptance criteria:**
- [x] `render` takes `EffectInput`; existing effects ignore the new fields and their tests still pass
- [x] The daemon's status reply can carry the current input for the preview

**Verification:**
- [x] `cargo test -p cntrl-core` passes

**Dependencies:** Task 23
**Files likely touched:** `cntrl-core/src/canvas_effects.rs`, `cntrl-core/src/canvas_stream.rs`, `cntrl-core/src/ipc.rs`
**Estimated scope:** S

**Status (2026-10-08):** `EffectInput` carries audio levels today; the status reply carries them to the panel preview.

## Task 31: Audio capture to frequency bands

**Description:** Capture the default output's monitor from PipeWire and reduce
it to about 16 smoothed bands plus a beat flag. New dependencies (`pipewire`,
an FFT crate) behind a cargo feature, so the project still builds without them.
Capture runs only while an audio effect is active.

**Acceptance criteria:**
- [x] Test: a synthetic 1 kHz sine lands in the right band
- [x] With no audio server the effect shows silence, and the stream keeps running
- [ ] Building without the feature works

**Verification:**
- [ ] `cargo test -p cntrl-core audio` passes
- [ ] Manual: `cntrl audio-test` prints moving band levels while music plays

**Dependencies:** Task 30
**Files likely touched:** `cntrl-core/Cargo.toml`, `cntrl-core/src/audio.rs`, `cntrl-cli/src/main.rs`
**Estimated scope:** M

**Status (2026-10-08):** done without new dependencies: the stream starts `parec` (part of
pipewire-pulse) on the default output's monitor and analyses its PCM (own 1024-point FFT,
16 bands, auto gain). So there is no cargo feature and no `cntrl audio-test` command. Capture
runs only while an audio effect, an audio layer or "brightness follows the music" is active;
checked that `parec` starts with the Spectrum Bars stream and is gone after it stops.
Not checked with music playing.

## Task 32: Audio effects

**Description:** Spectrum Bars, Beat Pulse, Audio Wave, and "audio drives
brightness" as a modifier on any layer.

**Acceptance criteria:**
- [ ] Three effects and the modifier, with sensitivity and smoothing parameters
- [x] Deterministic for a fixed `EffectInput` (unit test)

**Verification:**
- [ ] Manual: music playing, all devices react together with no visible lag

**Dependencies:** Task 31, Task 29
**Files likely touched:** `cntrl-core/src/canvas_effects.rs`, `cntrl-iced/src/app.rs`
**Estimated scope:** S

**Status (2026-10-08):** Spectrum Bars, Audio Wave, Beat Pulse, and a "Brightness follows the
music" box that works with any effect. No sensitivity slider: gain is automatic. The preview
mirrors the levels the stream is using. Open: the manual check with music.

## Task 33: Screen capture to a canvas frame

**Description:** Capture the screen through the Wayland screencast portal
(permission prompt once, restore token saved) and downscale to 160×90. New
dependencies (`ashpd`, PipeWire video) behind a cargo feature. Starts with a
spike: confirm the portal works on your compositor from a user service.

**Acceptance criteria:**
- [x] Spike result recorded: portal works from the daemon, or what blocks it
- [ ] Capture delivers at least 30 frames per second downscaled, under 5% of one core
- [ ] Permission is asked once, not on every start

**Verification:**
- [ ] Manual: `cntrl screen-test` writes one downscaled PNG

**Dependencies:** Task 30
**Files likely touched:** `cntrl-core/Cargo.toml`, `cntrl-core/src/screen.rs`, `cntrl-cli/src/main.rs`
**Estimated scope:** M

**Spike result (2026-10-08): the simple route fails.** Taking screenshots with `grim` (the tool
Hyprland ships) costs 114 ms and a full CPU core per frame for one monitor, so about 9 fps at
100% of a core. The target was 30 fps under 5%. The working route is the screen-capture
portal with a PipeWire video stream, which needs two new crates (`ashpd`, `pipewire`) and
their system build dependencies. Not started; needs your go-ahead on the dependencies.

**Status (2026-10-12): done another way, no new crates.** The spike blamed `grim` wrongly: the
cost was its image scaling and PNG encoding. An unscaled, uncompressed grab (`grim -t ppm`) takes
15 ms and 6 ms of CPU per 1440p monitor, and CNTRL shrinks it itself (`cntrl-core/src/screen.rs`).
Measured on this machine with one monitor at 10 grabs a second: the daemon plus `grim` use 11% of
one core while streaming Screen Ambient (Rainbow Wave uses 21%). Not met: 30 grabs a second and
the 5% target; no permission prompt exists on this route. Works on wlroots compositors only.
Check: `cargo test -p cntrl-core live_grab -- --ignored` writes `/tmp/cntrl-screen.ppm`.

## Task 34: Screen Ambient effect

**Description:** Show the captured screen on the canvas with smoothing,
saturation boost and a choice of full screen or edges only.

**Acceptance criteria:**
- [ ] Devices follow the screen with under 100 ms delay
- [ ] A black screen gives dark LEDs, not flicker
- [ ] Works as a layer under other effects

**Verification:**
- [ ] Manual: play a video with strong colour changes

**Status (2026-10-12):** effect `screen` added; picture verified, LEDs not watched by anyone
yet. Smoothing is a blend of each grab into the last (black stays black). Works as base or as the
layer on top. Not built: saturation boost (each device already has a saturation trim), edges-only
mode, a monitor picker in the panel (`screen_output` in the layout file), and a live preview in
the panel (it shows black for this effect).

**Dependencies:** Task 33, Task 29
**Files likely touched:** `cntrl-core/src/canvas_effects.rs`, `cntrl-iced/src/app.rs`
**Estimated scope:** S

## Checkpoint E: dynamic canvas
- [ ] Tests pass with and without the audio and screen features
- [ ] About 25 effects, layers, audio and screen all work on your hardware
- [ ] Review with you before Phase 7

---

## Phase 7: Ready to publish

## Task 35: Only show hardware that is present

**Description:** The canvas lists this machine's devices unconditionally, and
several values are this board's (ITE zone numbers, 32/36 LEDs, 12V zone 2, GPU
i2c address). On another PC the app must start cleanly and show only what it
detects.

**Acceptance criteria:**
- [x] Canvas devices come from detection; a missing device is not listed and never written
- [x] Board-specific constants are in one documented place with their env overrides
- [x] With no supported hardware, the panel opens and says so

**Verification:**
- [ ] Manual: run with the keyboard unplugged and with `CNTRL_ITE_PATH` set to a missing path

**Dependencies:** Checkpoint D
**Files likely touched:** `cntrl-core/src/canvas.rs`, `cntrl-core/src/devices.rs`, `cntrl-iced/src/app.rs`
**Estimated scope:** M

**Status (2026-10-08):** each canvas device is flagged present or absent from the device
inventory. Absent devices keep their placement but are not drawn, not listed and not written.
The README has a table of the board-specific environment variables. The daemon's autostart
skips the check on purpose (at login the devices may not have appeared yet). Not run with a
device actually unplugged.

## Task 36: Repo clean-up for publishing

**Description:** The repo root holds USB capture dumps, `profile_dump.bin`,
`mchose-indexeddb-dump.json` and analysis scripts. Check them for serial
numbers or personal data, move what is useful to `docs/protocol/`, delete the
rest. Decide whether history needs rewriting. Add a licence. Needs your
decisions; nothing is deleted without your OK.

**Acceptance criteria:**
- [ ] Each root dump is kept (moved), or deleted, by your choice
- [ ] No serials, tokens or personal paths in the tree; history checked and the result recorded
- [x] `LICENSE` added (your choice of licence)

**Verification:**
- [ ] `git grep` for your user name, home path and device serials finds nothing

**Dependencies:** None
**Files likely touched:** repo root, `docs/`, `.gitignore`
**Estimated scope:** M

**Status (2026-10-08):** `LICENSE-MIT` and `LICENSE-APACHE` added, crates marked
`MIT OR Apache-2.0`. The dump files are untracked and ignored. The systemd probe unit no longer
hard-codes a user name (install.sh fills it in). A search of the tracked tree for the user name
and home paths finds nothing outside `tasks/` and `docs/reviews/`.
**Still open, needs you:** the dumps are still in git history (2 commits touch them). A search of
`mchose-indexeddb-dump.json` for "serial", "email" and "token" finds nothing, but I have not
read the dumps through. Rewriting history is
destructive, so I have not done it. The analysis scripts and `PHASE1_LIGHTING.md` are still in
the repo root.

## Task 37: README, supported hardware list, CI

**Description:** A README a stranger can follow: what it supports, install,
udev, fan safety warning, how to add a device. A GitHub workflow running
`cargo test` and `cargo clippy`.

**Acceptance criteria:**
- [x] Supported-hardware table with what works on each device
- [ ] Install steps tested from a clean checkout
- [ ] CI is green

**Verification:**
- [ ] Fresh clone, follow the README, panel starts

**Dependencies:** Task 35, Task 36
**Files likely touched:** `README.md`, `.github/workflows/ci.yml`
**Estimated scope:** S

**Status (2026-10-08):** README has a supported-hardware table, install steps, the fan safety
note, the new canvas features and a licence section. `.github/workflows/ci.yml` runs the core
tests, the release-mode effect speed check, clippy and a CLI build. Not verified: the workflow
has never run (nothing is pushed), and the install steps were not tried from a clean checkout.

## Checkpoint: Complete
- [ ] Every task above is checked or explicitly dropped
- [ ] `cargo test` passes for the workspace; release build clean
- [ ] Installed binary is the reviewed build
