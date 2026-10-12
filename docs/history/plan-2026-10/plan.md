# Implementation Plan: fans working, RGB and Canvas reliable, SignalRGB-level

- **Date:** 2026-10-08
- **Branch:** `feature/led-canvas` (uncommitted fan + RGB work on top of `4f1010b`)
- **Task list:** `tasks/todo.md` (full acceptance criteria per task)
- **Context used:** `docs/reviews/2026-10-08-fan-control-and-rgb-review.md`,
  `graphify-out/GRAPH_REPORT.md`, the canvas/stream/daemon source, and the live
  state of this machine (read-only checks only).

## Overview

Three goals, in this order: (1) fan control that is safe and that covers the
motherboard fans, not only the GPU fan; (2) RGB apply/restore with no silent
misses; (3) a Canvas that maps LEDs correctly, stays smooth, survives closing
the panel, and then grows the effects that make it comparable to SignalRGB on
this machine's hardware.

## What I found (this is why the tasks exist)

**Fans**

- Only one fan is controllable today: `amdgpu/pwm1`. The motherboard headers
  (CPU, case) are invisible because the `it87` module is not loaded.
- The in-tree `it87` module on this kernel mentions IT8689, and the kernel
  command line already has `acpi_enforce_resources=lax`. So `sudo modprobe it87`
  is likely enough. Not tested: it needs root, and Gigabyte firmware may ignore
  PWM writes.
- No `~/.config/cntrl/fan_curves.json` exists, so the engine in the running
  daemon is idle. Nobody has run the fan engine for real yet.
- The four must-fix findings from the review (1, 2, 3, 4) are all "fan left in
  manual mode with nobody watching temperature".

**Canvas (new findings, not in the review)**

- **C-A. Wrong LEDs per fan.** The saved layout has header 2 as 3 fans × 12
  LEDs. The fans are Corsair AR120, 8 LEDs each. So "fan 1" on the canvas
  drives all of fan 1 and half of fan 2. `Chains::set_fans` always splits the
  header total (36 / 3 = 12) and throws away the per-fan count.
- **C-B. One slow device slows all devices.** One thread writes keyboard, ITE,
  GPU and RAM in sequence. The layout asks for 40 fps; ITE was only verified
  at 20 fps. The keyboard (143 fps capable) waits for the slowest bus.
- **C-C. A device that fails once is never retried.** `kb_err` / `ite_err` /
  `gpu_err` / `ram_err` are set on the first failed open and never cleared.
  After a write error the session is reopened on every frame with no pause. A
  RAM write error never drops the session.
- **C-D. Preview and hardware are out of step.** The preview uses the panel's
  clock, the stream uses its own start time. The preview also runs at 10 fps.
- **C-E. FPS limits disagree.** Slider 5–60, handler clamps to 40, worker
  clamps to 60.
- **C-F. Every slider move writes the layout file** on the UI thread.
- **C-G. The stream lives in the panel.** Close the panel and the effect stops.
  Nothing starts it at login. The daemon's theme sync and the Lighting tab's
  Apply write to the same devices while a stream is running.
- **C-H. No colour control.** `settings.color` feeds Ripple and Comet but the
  Canvas tab has no way to change it.
- **C-I. No test** for chain assembly (order, reverse, disabled segment).
- **C-J.** `layout.spec(id)` rebuilds every device spec on each call, several
  times per frame. Small waste, fixed for free in the C-B task.

**RGB**

- Review findings 15 and 16 stand.
- **Correction to review finding 14:** the installed binaries are *not* stale.
  `/usr/local/bin/cntrl` and `~/.local/bin/cntrl` are byte-identical to the
  current release build (the source was last edited Aug 31, same day). What
  remains of 14: two install paths, and the hard-coded path in the resume unit.

## Architecture Decisions

- **Test fans against a fake hwmon directory.** `fan_control::discover()` reads
  `/sys/class/hwmon` directly. Give it a root-path variant so safety logic is
  tested without hardware. Every fan safety fix lands with a test.
- **Fail safe means firmware control** (your choice). When the engine cannot
  compute a speed, or stops, or is disabled, it hands the fan back to the
  board or GPU. It never leaves a fan in manual mode unattended.
- **Effects stay native.** SignalRGB effects are HTML/JS under its licence and
  cannot be copied. Parity comes from a native library, layers, audio and
  screen input.
- **Audio and screen capture sit behind cargo features**, so the project still
  builds without PipeWire.
- **The daemon owns the hardware stream.** The panel becomes a client over the
  existing Unix socket, with the current in-process stream as fallback when no
  daemon runs. This solves "stops when the panel closes", autostart, and two
  processes writing the same hidraw device.
- **One thread per backend, each at its own safe rate**, all reading the same
  rendered frame. Only after measuring (task 16) confirms the bottleneck.
- **A chain is a list of segments; its total is the sum.** The header total is
  no longer forced to 32/36.
- **No new dependencies until Phase 6**, and there only with your go-ahead.

## What "SignalRGB level" means here

In scope: correct per-LED mapping, smooth streaming, always-on (login, resume),
about 25 native effects with their own parameters, layers, audio visualiser,
screen ambient, onboard motherboard zones, and a repo that is fit to publish.

Not planned: support for hardware you do not own (the code will detect and
skip it, Task 35), running SignalRGB's own effect files, game integrations,
macros. A user-scriptable effect format is the natural next step after
publishing; it is not in this plan.

## Dependency graph

```
T1 fake hwmon + test
 ├── T2 lost sensor ─┐
 ├── T3 disable/remove restores ─┤
 ├── T7 write-skip reads hardware ─┤
 └── T6 flock lock ──────────────┤
T4 daemon joins fan thread ──────┤
T5 `fans apply` captures state ──┴─► Checkpoint A/B ─► T9 it87 spike (you, sudo)
                                                        └── T10 it87 at boot
T12 panel Apply env leak ── T13 install paths
T14 chain model ── T15 identify ── T18 chain test
T16 measure ── T17 threads + reconnect ── T19 clock/fps
T17 ──► T23 daemon-owned stream ── T24 apply vs stream ── T25 autostart
T21 colour ── T22 basic effects ── (T26, T27, T28 parity)
```

## Task List

### Phase 0: Baseline
- [x] Task 0: Commit the current working tree (`8aa133d`, `5ff39bd`)

### Phase 1: Fan safety
- [x] Task 1: Fake-hwmon test root + first `apply_assignment` test
- [x] Task 2: Lost sensor hands the fan back to firmware
- [x] Task 3: Disabling the engine or removing a curve hands the fan back
- [x] Task 4: Daemon waits for the fan thread on stop
- [x] Task 5: `cntrl fans apply` captures state and takes the lock

### Checkpoint A: fans are safe on the GPU fan

### Phase 2: Fans robust + motherboard fans
- [x] Task 6: OS file lock instead of pid file
- [x] Task 7: Write-skip checks the hardware value
- [x] Task 8: Restore writes PWM only in manual mode
- [x] Task 9: it87 validation spike on the real board (you run it, sudo)
- [x] Task 10: Load it87 at boot (only if Task 9 passes)
- [x] Task 11: Fans tab reads once per poll; numeric hwmon sort; delete dead code

### Checkpoint B: all fans controlled, or the limit is documented

### Phase 3: RGB apply/restore
- [x] Task 12: Panel Apply stops leaking the backend list
- [x] Task 13: One install path; resume unit follows `PREFIX`

### Phase 4: Canvas correctness
- [x] Task 14: Chain segments keep their own LED count
- [x] Task 15: Identify button for the selected device
- [x] Task 16: Per-backend timing in stream stats
- [x] Task 17: One thread per backend, with reconnect and backoff
- [x] Task 18: Test for chain assembly
- [x] Task 19: Preview and hardware share one clock; one FPS range
- [x] Task 20: Save the layout on slider release
- [x] Task 21: Effect colour control on the Canvas tab
- [x] Task 22: Effect parameters and palettes

### Checkpoint C: Canvas maps correctly and stays smooth

### Phase 5: Canvas always on
- [x] Task 23: Daemon owns the stream; panel is a client
- [x] Task 24: Apply, theme sync and restore cooperate with a running stream
- [x] Task 25: Canvas starts at login and after resume

### Checkpoint D: effect survives closing the panel, reboot, suspend

### Phase 6: Dynamic canvas
- [x] Task 26: Onboard motherboard zones as canvas devices
- [x] Task 27: Effect library, batch 1 (everyday)
- [x] Task 28: Effect library, batch 2 (ambient)
- [x] Task 29: Layers
- [x] Task 30: Effect inputs (live data into effects)
- [x] Task 31: Audio capture to frequency bands
- [x] Task 32: Audio effects
- [x] Task 33: Screen capture to a canvas frame
- [x] Task 34: Screen Ambient effect

### Checkpoint E: dynamic canvas

### Phase 7: Ready to publish
- [x] Task 35: Only show hardware that is present
- [ ] Task 36: Repo clean-up for publishing
- [x] Task 37: README, supported hardware list, CI

### Checkpoint: Complete

## Risks and Mitigations

| Risk | Impact | Mitigation |
|------|--------|------------|
| The Linux `it87` driver cannot drive this board (the hardware can: Fan Control does it on Windows) | High: case fans cannot be controlled | Task 9 is a spike before any work depends on it. If the in-tree driver fails, try the out-of-tree `it87` (frankcrawford) before giving up. |
| The screencast portal does not work from a background service | Med: no screen ambient | Task 33 starts with a spike. Fallback: capture in the panel process. |
| Dumps in the repo or its history hold serials or personal data | Med once public | Task 36 checks before publishing. |
| A fan bug leaves a fan slow under load | High | Phase 1 first, every fix with a test, Checkpoint A on the GPU fan before touching motherboard fans. |
| amdgpu rejects PWM writes in auto mode (review 5) | Low | Task 8 starts by reading the real restore message; skip the change if there is no error. |
| Per-backend threads add races | Med | Threads share only a frame snapshot behind one mutex; each owns its device. Measure first (Task 16); skip Task 17's threading if the numbers do not justify it. |
| Moving the stream into the daemon breaks the working panel flow | Med | Keep the in-process path as fallback when no daemon answers. |
| ITE zone latching strobes the board when onboard zones stream | Med | Task 26 is change-detected like the 12V header, never per frame. |
| Flash wear on GPU (0xAA save) | Med | Unchanged rule: stream never sends the save. |
| Large uncommitted tree makes fixes hard to review or revert | Med | Task 0. |

## Decisions (2026-10-08)

1. Commit first: done.
2. Motherboard fans work on Windows with Fan Control, so the hardware is fine; Task 9 tests the Linux driver.
3. Audio visualiser and screen ambient: yes.
4. Effects: a full native library, because the repo will go public.
5. Fans are 8 LEDs each; the header 1 strip is gone (Task 14 allows an empty header).
6. Lost sensor: hand the fan back to firmware.

7. Licence: dual MIT OR Apache-2.0 (the usual Rust choice).
8. Root dump files: untracked and ignored (done in the working tree). They stay in git history until Task 36 decides about rewriting it.

## Open Questions

1. **Task 9, second half:** reinstall the udev rule, then run the spike on `it8689/pwm1`.
