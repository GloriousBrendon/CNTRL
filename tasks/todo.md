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
- [ ] `discover_in(&Path)` exists; `discover()` behaviour is unchanged
- [ ] A test creates `hwmonN/{name,pwm1,pwm1_enable,fan1_input,temp1_input}` in a temp dir and asserts a tick writes the expected PWM and sets manual mode
- [ ] No test touches the real `/sys`

**Verification:**
- [ ] `cargo test -p cntrl-core fan_` passes
- [ ] `target/release/cntrl fans list` still prints 21 sensors and `amdgpu/pwm1`

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
- [ ] A failed read removes that sensor from `state.smoothed`
- [ ] A channel whose curve cannot be evaluated is restored to its original mode, never left in manual
- [ ] If the captured original mode was itself manual, the channel is set to automatic instead
- [ ] Test: delete `temp1_input` between two ticks; `pwm1_enable` is back to the original value; put it back, next tick engages again

**Verification:**
- [ ] `cargo test -p cntrl-core fan_` passes

**Dependencies:** Task 1
**Files likely touched:** `cntrl-core/src/fan_curves.rs`
**Estimated scope:** S

## Task 3: Disabling the engine or removing a curve hands the fan back

**Description:** Review 2. Unticking "Enable engine" only stops the ticks; the
fan stays in manual mode at its last speed. Restore on disable, and restore any
captured channel that no longer has an assignment.

**Acceptance criteria:**
- [ ] `enabled: false` restores every captured channel and resets tick state
- [ ] Removing one assignment restores only that channel
- [ ] Test: fake chip returns to its original `pwm1_enable` after the config is disabled

**Verification:**
- [ ] `cargo test -p cntrl-core fan_` passes
- [ ] Manual: enable a curve on `amdgpu/pwm1`, untick the engine, `cat /sys/class/hwmon/hwmon3/pwm1_enable` shows the original value within 2 s

**Dependencies:** Task 1
**Files likely touched:** `cntrl-core/src/fan_engine.rs`, `cntrl-core/src/fan_curves.rs`
**Estimated scope:** S

## Task 4: Daemon waits for the fan thread on stop

**Description:** Review 1 and 9. On SIGTERM the daemon's main loop returns
before the fan thread restores the fans. Keep the thread handle and join it.
Drop the idle accept sleep from 200 ms to 20 ms.

**Acceptance criteria:**
- [ ] `run_unix` joins the fan thread after the accept loop
- [ ] Accept loop sleeps 20 ms when idle
- [ ] The daemon exits within 2 s of SIGTERM

**Verification:**
- [ ] Build succeeds
- [ ] Manual: with a curve active, `systemctl --user stop cntrl-daemon`; the journal shows the restore line before "stopped", and `pwm1_enable` is back to its original value

**Dependencies:** None
**Files likely touched:** `cntrl-cli/src/daemon.rs`
**Estimated scope:** XS

## Task 5: `cntrl fans apply` captures state and takes the lock

**Description:** Review 4. The one-shot command switches the fan to manual
without saving the original state, so `cntrl fans restore` has nothing to
restore.

**Acceptance criteria:**
- [ ] A non-dry-run `fans apply` calls `capture_and_engage` before `tick`
- [ ] It takes the engine lock and refuses with a clear message if the daemon holds it
- [ ] `cntrl fans restore` after `fans apply` puts the fan back

**Verification:**
- [ ] Build succeeds
- [ ] Manual: stop the daemon, `cntrl fans apply`, `cntrl fans restore`, `pwm1_enable` matches the value before

**Dependencies:** None
**Files likely touched:** `cntrl-cli/src/main.rs`
**Estimated scope:** XS

## Checkpoint A: fans are safe on the GPU fan
- [ ] `cargo test -p cntrl-core` passes; build clean
- [ ] On `amdgpu/pwm1`: enable, disable, stop daemon, `kill -TERM`, `fans apply` + `restore` all end with the fan back in its original mode
- [ ] You have looked at it before motherboard fans are touched

---

## Phase 2: Fans robust + motherboard fans

## Task 6: OS file lock instead of pid file

**Description:** Review 6. After a power cut the pid file stays and can block
the engine forever. Hold an OS lock on the file; the kernel releases it when the
process dies. Delete `pid_alive` and the `Drop` impl.

**Acceptance criteria:**
- [ ] Lock uses `File::try_lock` (or `flock` through the existing `nix` dependency)
- [ ] A leftover lock file from a dead process does not block start
- [ ] Two engines started together: exactly one runs

**Verification:**
- [ ] Test: take the lock twice in one process through two handles, second fails
- [ ] Manual: `cntrl fans run` while the daemon runs prints "already running"

**Dependencies:** None
**Files likely touched:** `cntrl-core/src/fan_engine.rs`
**Estimated scope:** XS

## Task 7: Write-skip checks the hardware value

**Description:** Review 7. A write is skipped when the target is close to the
last value *written*, even if something else changed the fan since.

**Acceptance criteria:**
- [ ] Skip only when `read_pwm()` also equals the last written raw value
- [ ] Test: change the fake `pwm1` between ticks at a steady temperature, next tick rewrites it

**Verification:**
- [ ] `cargo test -p cntrl-core fan_` passes

**Dependencies:** Task 1
**Files likely touched:** `cntrl-core/src/fan_curves.rs`
**Estimated scope:** XS

## Task 8: Restore writes PWM only in manual mode

**Description:** Review 5, which was from memory and not tested. First look at
the real restore message from Checkpoint A. Only if it reports an error on
`amdgpu/pwm1`, skip the PWM write when the restored mode is not manual.

**Acceptance criteria:**
- [ ] The restore message on this machine is recorded in the task notes
- [ ] If it showed an error: restore reports "restored 1" with no error
- [ ] If it did not: close the task with no code change

**Verification:**
- [ ] Manual: `cntrl fans restore` output after an engine run

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

**Acceptance criteria:**
- [ ] Recorded: does `it87` load, with or without `force_id`
- [ ] Recorded: which `it87/pwmN` maps to which physical fan
- [ ] Recorded: does RPM follow the PWM write for 30 s, or does firmware take it back

**Verification:**
- [ ] The spike report is pasted into this task

**Dependencies:** Checkpoint A
**Files likely touched:** none
**Estimated scope:** XS

## Task 10: Load it87 at boot (only if Task 9 passes)

**Description:** Make the motherboard fans available after every boot.
`install.sh` writes `/etc/modules-load.d/cntrl-it87.conf` (plus a
`modprobe.d` option line if `force_id` was needed). If Task 9 failed, instead
say so in the README and Fans tab and close the task.

**Acceptance criteria:**
- [ ] After reboot, `cntrl fans list` shows the it87 channels with no manual step
- [ ] The channels are writable without root (the udev rule already covers `it87`)
- [ ] The install step is opt-in or clearly labelled experimental
- [ ] `install.sh` also installs `packaging/udev/99-cntrl-hwmon.rules` (today it does not; the rule on this machine was put there by hand)

**Verification:**
- [ ] Manual: reboot, `cntrl fans list`, set a curve on one case fan in the Fans tab, RPM follows

**Dependencies:** Task 9
**Files likely touched:** `scripts/install.sh`, `README.md`
**Estimated scope:** S

## Task 11: Fans tab reads once per poll; numeric hwmon sort; delete dead code

**Description:** Review 10, 11, 12. The tab reads sysfs on every redraw; read
in `FanPoll` and keep the values in `App`. Sort `hwmonN` by number so
`hwmon10` does not come before `hwmon2`. Delete `mix_would_cycle`,
`smoothed_temp`, `request_stop`, `FanChannelStatus::Absent`, `hwmon_path` and
the plain `set_manual`.

**Acceptance criteria:**
- [ ] `fans_view` does no file reads
- [ ] Duplicate chips are numbered in numeric hwmon order
- [ ] The six unused items are gone and nothing else changed behaviour

**Verification:**
- [ ] `cargo test -p cntrl-core` passes; build has no new warnings
- [ ] Manual: dragging a curve point in the Fans tab is smooth; readings update each second

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
- [ ] The panel calls `device_state::remember_applied` after a successful apply; its own copy is deleted
- [ ] A failed apply is not saved

**Verification:**
- [ ] Build succeeds
- [ ] Manual: the sequence above, then check `~/.config/cntrl/device_state.json` has the theme colour for all four entries

**Dependencies:** None
**Files likely touched:** `cntrl-iced/src/app.rs`
**Estimated scope:** S

## Task 13: One install path; resume unit follows `PREFIX`

**Description:** What is left of Review 14 (the "stale binaries" part was
wrong: both installed copies match the current build). Two copies in two
places can drift, and the resume unit hard-codes `/usr/local/bin`.

**Acceptance criteria:**
- [ ] Login unit and resume unit run the same binary path
- [ ] `PREFIX=/usr ./scripts/install.sh` produces units that point at `/usr/bin/cntrl`
- [ ] The second copy is removed or is a symlink

**Verification:**
- [ ] Manual: `systemctl --user cat cntrl-restore.service` and `systemctl cat cntrl-resume@$USER.service` show the same path
- [ ] Manual: suspend and resume once; lighting comes back

**Dependencies:** None
**Files likely touched:** `scripts/install.sh`, `packaging/systemd/cntrl-resume@.service`, `packaging/systemd/cntrl-restore.service`
**Estimated scope:** S

---

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
- [ ] "3 fans" then "8/fan" gives 24 LEDs; changing fan count afterwards keeps 8 per fan
- [ ] A header can hold fans followed by a strip with its own LED count
- [ ] A header can be set to "Empty": no canvas device, no writes to that header
- [ ] Existing layout files still load; the test `chain_split_reconciles_segment_devices` is updated to the new rule

**Verification:**
- [ ] `cargo test -p cntrl-core canvas` passes
- [ ] Manual: stream Comet slowly across three fans placed in a row; the head crosses fan 1, then 2, then 3, with no fan lit in two halves

**Dependencies:** None
**Files likely touched:** `cntrl-core/src/canvas.rs`, `cntrl-iced/src/app.rs`
**Estimated scope:** S

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

## Task 16: Per-backend timing in stream stats

**Description:** C-B, measurement first. Record how long each backend's write
takes per frame and show it in the status line and in `cntrl canvas`. This
decides whether Task 17's threads are needed and what each device's rate is.

**Acceptance criteria:**
- [ ] Status shows average and worst write time per backend over the last second
- [ ] Numbers recorded here for 20, 40 and 60 fps on this machine

**Verification:**
- [ ] Manual: `target/release/cntrl canvas --effect wave --fps 40 --seconds 20`

**Dependencies:** None
**Files likely touched:** `cntrl-core/src/canvas_stream.rs`, `cntrl-cli/src/main.rs`
**Estimated scope:** S

## Task 17: One thread per backend, with reconnect and backoff

**Description:** C-B, C-C, C-J. Render once per frame into a shared snapshot;
each backend thread takes the newest frame at its own rate (keyboard up to 60,
ITE and RAM at the rate Task 16 shows is safe, GPU change-detected). A failed
open or write is retried after 2 s, not never and not every frame. Device specs
are built once per layout change, not per call.

**Acceptance criteria:**
- [ ] Keyboard holds the requested fps while ITE and RAM run slower
- [ ] Unplug and replug the keyboard during a stream: it comes back within 3 s without restarting the stream
- [ ] A RAM write error drops and reopens the session
- [ ] Stopping the stream still restores each PC backend's saved lighting

**Verification:**
- [ ] `cargo test -p cntrl-core` passes
- [ ] Manual: 60 fps stream for 10 minutes, no strobing on motherboard zones, no errors in the status line

**Dependencies:** Task 16 (skip the threading part if the keyboard already holds fps; keep the reconnect part)
**Files likely touched:** `cntrl-core/src/canvas_stream.rs`
**Estimated scope:** M

## Task 18: Test for chain assembly

**Description:** C-I. `assemble_chain` decides which colour goes to which LED
on the wire and has no test.

**Acceptance criteria:**
- [ ] Test: two segments, second reversed, on a left-to-right gradient frame; wire order is as expected
- [ ] Test: a disabled segment sends black for its LED count and keeps later segments in place
- [ ] Test: all segments disabled returns `None`

**Verification:**
- [ ] `cargo test -p cntrl-core canvas_stream` passes

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
- [ ] The FPS slider's maximum is the value the worker really uses
- [ ] Preview animation is visibly smooth

**Verification:**
- [ ] Manual: Comet at speed 0.3×, compare screen and keyboard

**Dependencies:** Task 17
**Files likely touched:** `cntrl-iced/src/app.rs`, `cntrl-core/src/canvas_stream.rs`
**Estimated scope:** S

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

## Task 21: Effect colour control on the Canvas tab

**Description:** C-H. `settings.color` drives Ripple and Comet but is fixed at
the default pink. Reuse the Lighting tab's colour control, plus a "use theme
accent" toggle.

**Acceptance criteria:**
- [ ] Changing the colour changes Ripple and Comet on preview and hardware at once
- [ ] The colour is saved with the layout
- [ ] "Use theme accent" follows the theme file

**Verification:**
- [ ] Manual: set green, restart the panel, Comet is green

**Dependencies:** None
**Files likely touched:** `cntrl-iced/src/app.rs`, `cntrl-core/src/canvas.rs`
**Estimated scope:** S

## Task 22: Effect parameters and palettes

**Description:** Today every effect shares speed, scale and one colour. A large
effect library needs each effect to declare its own controls (colours, a
palette, direction, density) and the tab to draw them from that list, instead
of one hand-written slider row. Do this before adding many effects.

**Acceptance criteria:**
- [ ] Each effect declares its parameters; the Canvas tab builds its controls from the declaration
- [ ] A palette (2–6 colours) is a parameter type, with "theme accent" as a source
- [ ] Old layout files still load; values are saved per effect

**Verification:**
- [ ] `cargo test -p cntrl-core canvas_effects` passes
- [ ] Manual: switch between two effects, each keeps its own settings

**Dependencies:** Task 21
**Files likely touched:** `cntrl-core/src/canvas_effects.rs`, `cntrl-core/src/canvas.rs`, `cntrl-iced/src/app.rs`
**Estimated scope:** M

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
- [ ] Moving a device or a slider in the panel changes the hardware within one frame
- [ ] With no daemon, the panel streams in-process as before

**Verification:**
- [ ] Test: request/response JSON round-trip for the new messages
- [ ] Manual: the three steps above; `cntrl canvas` also goes through the daemon

**Dependencies:** Task 17
**Files likely touched:** `cntrl-core/src/ipc.rs`, `cntrl-cli/src/daemon.rs`, `cntrl-cli/src/main.rs`, `cntrl-iced/src/app.rs`
**Estimated scope:** M

## Task 24: Apply, theme sync and restore cooperate with a running stream

**Description:** C-G. Today a Lighting-tab Apply or a theme change writes
static colour to devices a stream is driving, and the next frame overwrites it.
Rule: an explicit Apply stops the stream for the devices it targets; a theme
change while streaming updates the canvas colour (when "use theme accent" is
on) and does not write static colour to streamed devices.

**Acceptance criteria:**
- [ ] Apply on the Lighting tab while streaming ends with the applied colour on those devices, steady
- [ ] A theme change while streaming does not flash static colour over the effect
- [ ] `device_state.json` still records what to restore when the stream stops

**Verification:**
- [ ] Manual: both cases with all devices streaming

**Dependencies:** Task 23, Task 21
**Files likely touched:** `cntrl-cli/src/daemon.rs`, `cntrl-core/src/theme_sync.rs`, `cntrl-core/src/ipc.rs`
**Estimated scope:** M

## Task 25: Canvas starts at login and after resume

**Description:** Add `canvas_autostart` to settings with a toggle on the Canvas
tab. The daemon starts the stream on launch when it is set. After resume the
existing resume unit asks the daemon to restart the stream instead of only
restoring static colour.

**Acceptance criteria:**
- [ ] With the toggle on, the effect is running after login without opening the panel
- [ ] After suspend and resume the effect is running again within 10 s
- [ ] With the toggle off, behaviour is the same as today

**Verification:**
- [ ] Manual: reboot; suspend and resume

**Dependencies:** Task 23, Task 13
**Files likely touched:** `cntrl-core/src/settings.rs`, `cntrl-cli/src/daemon.rs`, `cntrl-cli/src/main.rs`, `cntrl-iced/src/app.rs`
**Estimated scope:** S

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
- [ ] Each onboard zone can be placed and enabled on the canvas
- [ ] No strobing at any fps
- [ ] Disabled zones are never written by the stream

**Verification:**
- [ ] Manual: a colour-cycling effect for 10 minutes, watch the board zones

**Dependencies:** Task 17
**Files likely touched:** `cntrl-core/src/ite_rgb.rs`, `cntrl-core/src/canvas.rs`, `cntrl-core/src/canvas_stream.rs`
**Estimated scope:** M

## Task 27: Effect library, batch 1 (everyday)

**Description:** Solid, Gradient (linear and radial), Breathing, Spectrum
Cycle, Rainbow (horizontal, vertical, radial, spiral), Colour Wipe.

**Acceptance criteria:**
- [ ] Each has a label, a CLI slug and declared parameters
- [ ] The determinism and slug tests cover them; Solid is exempt from "animates over time"
- [ ] Each one checked on hardware

**Verification:**
- [ ] `cargo test -p cntrl-core canvas_effects` passes

**Dependencies:** Task 22
**Files likely touched:** `cntrl-core/src/canvas_effects.rs`
**Estimated scope:** S

## Task 28: Effect library, batch 2 (ambient)

**Description:** Fire, Rain, Starfield/Twinkle, Lava (smooth noise), Matrix,
Radar Sweep, Bouncing Balls, Scanner, Neon Pulse, Waves. Effects that need
randomness use a seeded generator keyed on time, so preview and hardware still
match.

**Acceptance criteria:**
- [ ] Ten effects, same rules as batch 1
- [ ] A full frame of any effect renders in under 2 ms (the canvas is 160×90)

**Verification:**
- [ ] `cargo test -p cntrl-core canvas_effects` passes, including a timing assert in release

**Dependencies:** Task 22
**Files likely touched:** `cntrl-core/src/canvas_effects.rs` (split into a module folder if it passes ~600 lines)
**Estimated scope:** M

## Task 29: Layers

**Description:** Two or three effects stacked with a blend mode (normal, add,
multiply, screen) and opacity, so effects can be combined, for example Twinkle
over a Gradient.

**Acceptance criteria:**
- [ ] The layout holds an ordered list of layers; one layer behaves as today
- [ ] Blend modes have a unit test on fixed pixels
- [ ] Old layout files load as a single layer

**Verification:**
- [ ] `cargo test -p cntrl-core canvas` passes
- [ ] Manual: two layers on hardware

**Dependencies:** Task 22, Task 23
**Files likely touched:** `cntrl-core/src/canvas.rs`, `cntrl-core/src/canvas_effects.rs`, `cntrl-core/src/canvas_stream.rs`, `cntrl-iced/src/app.rs`
**Estimated scope:** M

## Task 30: Effect inputs (live data into effects)

**Description:** Effects are pure functions of time. Audio and screen need live
data. Add an `EffectInput` passed to `render` (time, optional audio bands,
optional screen frame). The preview in the panel gets the same input from the
daemon, so it still matches the hardware.

**Acceptance criteria:**
- [ ] `render` takes `EffectInput`; existing effects ignore the new fields and their tests still pass
- [ ] The daemon's status reply can carry the current input for the preview

**Verification:**
- [ ] `cargo test -p cntrl-core` passes

**Dependencies:** Task 23
**Files likely touched:** `cntrl-core/src/canvas_effects.rs`, `cntrl-core/src/canvas_stream.rs`, `cntrl-core/src/ipc.rs`
**Estimated scope:** S

## Task 31: Audio capture to frequency bands

**Description:** Capture the default output's monitor from PipeWire and reduce
it to about 16 smoothed bands plus a beat flag. New dependencies (`pipewire`,
an FFT crate) behind a cargo feature, so the project still builds without them.
Capture runs only while an audio effect is active.

**Acceptance criteria:**
- [ ] Test: a synthetic 1 kHz sine lands in the right band
- [ ] With no audio server the effect shows silence, and the stream keeps running
- [ ] Building without the feature works

**Verification:**
- [ ] `cargo test -p cntrl-core audio` passes
- [ ] Manual: `cntrl audio-test` prints moving band levels while music plays

**Dependencies:** Task 30
**Files likely touched:** `cntrl-core/Cargo.toml`, `cntrl-core/src/audio.rs`, `cntrl-cli/src/main.rs`
**Estimated scope:** M

## Task 32: Audio effects

**Description:** Spectrum Bars, Beat Pulse, Audio Wave, and "audio drives
brightness" as a modifier on any layer.

**Acceptance criteria:**
- [ ] Three effects and the modifier, with sensitivity and smoothing parameters
- [ ] Deterministic for a fixed `EffectInput` (unit test)

**Verification:**
- [ ] Manual: music playing, all devices react together with no visible lag

**Dependencies:** Task 31, Task 29
**Files likely touched:** `cntrl-core/src/canvas_effects.rs`, `cntrl-iced/src/app.rs`
**Estimated scope:** S

## Task 33: Screen capture to a canvas frame

**Description:** Capture the screen through the Wayland screencast portal
(permission prompt once, restore token saved) and downscale to 160×90. New
dependencies (`ashpd`, PipeWire video) behind a cargo feature. Starts with a
spike: confirm the portal works on your compositor from a user service.

**Acceptance criteria:**
- [ ] Spike result recorded: portal works from the daemon, or what blocks it
- [ ] Capture delivers at least 30 frames per second downscaled, under 5% of one core
- [ ] Permission is asked once, not on every start

**Verification:**
- [ ] Manual: `cntrl screen-test` writes one downscaled PNG

**Dependencies:** Task 30
**Files likely touched:** `cntrl-core/Cargo.toml`, `cntrl-core/src/screen.rs`, `cntrl-cli/src/main.rs`
**Estimated scope:** M

## Task 34: Screen Ambient effect

**Description:** Show the captured screen on the canvas with smoothing,
saturation boost and a choice of full screen or edges only.

**Acceptance criteria:**
- [ ] Devices follow the screen with under 100 ms delay
- [ ] A black screen gives dark LEDs, not flicker
- [ ] Works as a layer under other effects

**Verification:**
- [ ] Manual: play a video with strong colour changes

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
- [ ] Canvas devices come from detection; a missing device is not listed and never written
- [ ] Board-specific constants are in one documented place with their env overrides
- [ ] With no supported hardware, the panel opens and says so

**Verification:**
- [ ] Manual: run with the keyboard unplugged and with `CNTRL_ITE_PATH` set to a missing path

**Dependencies:** Checkpoint D
**Files likely touched:** `cntrl-core/src/canvas.rs`, `cntrl-core/src/devices.rs`, `cntrl-iced/src/app.rs`
**Estimated scope:** M

## Task 36: Repo clean-up for publishing

**Description:** The repo root holds USB capture dumps, `profile_dump.bin`,
`mchose-indexeddb-dump.json` and analysis scripts. Check them for serial
numbers or personal data, move what is useful to `docs/protocol/`, delete the
rest. Decide whether history needs rewriting. Add a licence. Needs your
decisions; nothing is deleted without your OK.

**Acceptance criteria:**
- [ ] Each root dump is kept (moved), or deleted, by your choice
- [ ] No serials, tokens or personal paths in the tree; history checked and the result recorded
- [ ] `LICENSE` added (your choice of licence)

**Verification:**
- [ ] `git grep` for your user name, home path and device serials finds nothing

**Dependencies:** None
**Files likely touched:** repo root, `docs/`, `.gitignore`
**Estimated scope:** M

## Task 37: README, supported hardware list, CI

**Description:** A README a stranger can follow: what it supports, install,
udev, fan safety warning, how to add a device. A GitHub workflow running
`cargo test` and `cargo clippy`.

**Acceptance criteria:**
- [ ] Supported-hardware table with what works on each device
- [ ] Install steps tested from a clean checkout
- [ ] CI is green

**Verification:**
- [ ] Fresh clone, follow the README, panel starts

**Dependencies:** Task 35, Task 36
**Files likely touched:** `README.md`, `.github/workflows/ci.yml`
**Estimated scope:** S

## Checkpoint: Complete
- [ ] Every task above is checked or explicitly dropped
- [ ] `cargo test` passes for the workspace; release build clean
- [ ] Installed binary is the reviewed build
