# Code review: fan control + RGB restore (uncommitted changes)

- **Date:** 2026-10-08
- **Branch:** `feature/led-canvas`, uncommitted working tree on top of `4f1010b`
- **Format:** `/ponytail:ponytail-review` (correct, safe, holds under load, tested, fast, lean)
- **Assumed load:** one user, one desktop, one daemon
- **Status:** findings only. No code was changed by this review.

## What this change does

It adds fan control. The engine reads temperatures from `/sys/class/hwmon`, turns
them into a fan speed through a curve, and writes that speed to the fan. It runs
inside `cntrl daemon` or as `cntrl fans run`, with a new Fans tab to edit curves.

It also changes RGB control. After theme sync applies a colour, it now saves the
lighting it really applied, so a slot that was "Off" no longer comes back dark
after reboot. A new system unit re-runs `cntrl restore` after suspend or hibernate.

## Files reviewed

| Area | Files |
|------|-------|
| Fan core | `cntrl-core/src/fan_control.rs`, `fan_curves.rs`, `fan_engine.rs` |
| Fan UI / CLI | `cntrl-iced/src/fans_tab.rs`, `cntrl-iced/src/app.rs`, `cntrl-cli/src/main.rs`, `cntrl-cli/src/daemon.rs` |
| RGB | `cntrl-core/src/device_state.rs`, `cntrl-core/src/theme_sync.rs`, callers of `apply_theme` |
| Packaging | `packaging/systemd/cntrl-resume@.service`, `packaging/udev/99-cntrl-hwmon.rules`, `scripts/install.sh` |

## Summary

| # | Finding | Group | Action |
|---|---------|-------|--------|
| 1 | Daemon exits before the fans are restored | Must fix | Fix |
| 2 | Turning the engine off does not give the fans back | Must fix | Fix |
| 3 | A lost sensor freezes the fan | Must fix | Fix |
| 4 | `cntrl fans apply` leaves a fan stuck with no way back | Must fix | Fix |
| 5 | Restore probably reports an error on the GPU fan every time | Should fix | Fix |
| 6 | A stale lock file can block the engine after a crash | Should fix | Fix (deletes code) |
| 7 | The engine does not notice when someone else changes the fan | Should fix | Fix |
| 8 | No test for the code that writes fan speeds | Should fix | Add test |
| 9 | Every daemon request now waits up to 200 ms | Should fix | Make better |
| 10 | The Fans tab reads hardware while drawing | Nice to have | Make better |
| 11 | Ids for duplicate chips can swap | Nice to have | Make better |
| 12 | Code nobody calls | Nice to have | Remove |
| 13 | The resume hook has a fixed binary path | Nice to have | Merged into 14 |
| 14 | The installed binaries do not contain this fix | Should fix | Fix |
| 15 | "Apply theme now" can skip the PC devices | Should fix | Fix |
| 16 | The panel still has its own copy of the save logic | Nice to have | Remove |

The RGB core fix itself is correct: `apply_theme` saves only after a successful
apply, all three callers (CLI, daemon watcher, panel) get the fix, and the old
`remember_targets` has no callers left.

## Must fix

1. **Daemon exits before the fans are restored** (`cntrl-cli/src/daemon.rs:L44-L66`)
   - **What this is:** On SIGTERM the main loop stops and `run_unix` returns, while
     the fan thread is supposed to put the fans back.
   - **Problem:** Nobody waits for the fan thread. Main wakes within 200 ms and
     exits; the fan thread wakes within 200 ms and then still has to scan hwmon and
     write, so it usually loses the race.
   - **Fix:** Keep the handle, `let fan = std::thread::spawn(fan_watcher);`, and
     call `let _ = fan.join();` after the accept loop.
   - **If we skip it:** Every logout or `systemctl stop` can leave fans in manual
     mode at their last speed, with nothing watching temperature.

2. **Turning the engine off does not give the fans back** (`cntrl-core/src/fan_engine.rs:L126-L133`)
   - **What this is:** The loop only works when `cfg.enabled` is true, and the Fans
     tab header says "the engine restores fans on exit".
   - **Problem:** Unticking "Enable engine" just stops the ticks. The fan stays in
     manual mode at its last speed, for example 20% from a cool idle. Removing a
     curve does the same for that channel.
   - **Fix:** In `run_loop`, add
     `else { fan_curves::restore_all(); state = TickState::default(); }`. For
     removed curves, restore any saved channel that no longer has an assignment.
   - **If we skip it:** The off switch is the most dangerous state: fixed speed, no
     control, and the user believes the system has taken over.

3. **A lost sensor freezes the fan** (`cntrl-core/src/fan_curves.rs:L289-L301`, `L322-L327`)
   - **What this is:** Each tick reads the sensor, smooths it, and computes the fan
     speed.
   - **Problem:** If the read fails, the old temperature is kept forever, so the fan
     holds its last speed while the real temperature climbs. If the sensor was never
     readable, the channel is already in manual mode (`capture_and_engage`) and the
     tick returns "Unsupported" without writing.
   - **Fix:** Remove the sensor from `state.smoothed` when a read fails. When a
     curve cannot be evaluated, write `max_duty_pct` instead of returning.
   - **If we skip it:** A driver reload or a renamed sensor id leaves a GPU fan
     stuck, possibly at 0%.

4. **`cntrl fans apply` leaves a fan stuck with no way back** (`cntrl-cli/src/main.rs:L401-L423`)
   - **What this is:** It runs one control tick and exits.
   - **Problem:** It switches the fan to manual and writes a speed, but never saves
     the original state and takes no lock. `cntrl fans restore` then says "no fan
     state to restore". If the engine starts later, it records this stuck state as
     the "original".
   - **Fix:** Call `fan_curves::capture_and_engage(&cfg, &discovery)` before `tick`
     when not a dry run, and take `acquire_lock()` first.
   - **If we skip it:** One documented command leaves the fan at a fixed speed until
     reboot.

## Should fix

5. **Restore probably reports an error on the GPU fan every time** (`cntrl-core/src/fan_control.rs:L151-L156`)
   - **What this is:** `restore` writes the old mode, then the old PWM value.
   - **Problem:** If the old mode was automatic, the `amdgpu` driver rejects a PWM
     write outside manual mode. This is from memory of the kernel code, not tested
     on hardware. The message would be "restored 0; errors: …" although the restore
     worked.
   - **Fix:** Write PWM only when the restored mode is `PWM_ENABLE_MANUAL`.
   - **If we skip it:** Every clean stop looks like a failure, so real failures get
     ignored.

6. **A stale lock file can block the engine after a crash** (`cntrl-core/src/fan_engine.rs:L91-L112`)
   - **What this is:** A pid file stops two engines from fighting over one fan.
   - **Problem:** After a power cut the file stays. On the next boot that pid often
     belongs to another process, so the daemon prints "already running" once and
     never controls the fans. Two engines starting together can also both pass the
     check.
   - **Fix:** Hold an OS lock on the file (`File::try_lock`, stable since Rust 1.89,
     or `flock`). The kernel drops it when the process dies, and `pid_alive` and the
     `Drop` impl can be deleted.
   - **If we skip it:** Fan control silently does not start after an unclean
     shutdown.

7. **The engine does not notice when someone else changes the fan** (`cntrl-core/src/fan_curves.rs:L360-L370`)
   - **What this is:** A write is skipped when the target is within 3 points of the
     last value written.
   - **Problem:** It compares with memory, not with the hardware. Run
     `cntrl fans restore` while the engine runs (the lock error suggests exactly
     this): the next tick flips the fan to manual again but skips the write. The fan
     sits at a stray speed until the temperature moves.
   - **Fix:** Skip only if `channel.read_pwm()` also equals `pct_to_raw(prev)`.
   - **If we skip it:** The fan holds a wrong speed for as long as the temperature
     is steady.

8. **No test for the code that writes fan speeds** (`cntrl-core/src/fan_curves.rs:L304-L389`)
   - **What this is:** `apply_assignment` holds the clamp, skip and fail-safe
     decisions.
   - **Problem:** Tests cover only curve maths. The cases in findings 3 and 7 have
     nothing that fails when they break.
   - **Fix:** One test using a temp directory as a fake hwmon chip: a missing sensor
     must write `max_duty_pct`.
   - **If we skip it:** The safety behaviour can regress without anyone seeing it.

9. **Every daemon request now waits up to 200 ms** (`cntrl-cli/src/daemon.rs:L49-L62`)
   - **What this is:** The accept loop became non-blocking and sleeps 200 ms when
     idle, so it can see a stop request.
   - **Problem:** A client that connects during the sleep waits for it. Before,
     accept was instant.
   - **Fix:** Sleep 20 ms instead.
   - **If we skip it:** Lighting applies through the daemon feel laggy, about 100 ms
     extra on average.

14. **The installed binaries do not contain this fix** (`/usr/local/bin/cntrl`, `~/.local/bin/cntrl`, `packaging/systemd/cntrl-resume@.service:L27`)
    - **What this is:** The resume hook runs `/usr/local/bin/cntrl`; the login unit
      runs `~/.local/bin/cntrl`. Both units are enabled on the dev machine.
    - **Problem:** Both files were dated Aug 31 at review time, older than this
      work, so the dark-keyboard fix and `cntrl fans` are not live. Two copies in
      two places will drift apart. The resume unit's path is also hard-coded, so
      `PREFIX=/usr ./scripts/install.sh` breaks it (this was finding 13).
    - **Correction (2026-10-08, later the same day):** the "older than this work"
      part is wrong. Both installed files are byte-identical to the current
      release build, and the running daemon holds the fan engine lock. Only the
      two-paths and hard-coded-path problems remain.
    - **Fix:** Reinstall after merging. Point both units at one path, and have
      `install.sh` rewrite the unit's path with `sed "s|/usr/local/bin|$BINDIR|"`.
    - **If we skip it:** The keyboard still comes back dark, and login and resume
      can run different versions.

15. **"Apply theme now" can skip the PC devices** (`cntrl-iced/src/app.rs:L776`, older code that the new save relies on)
    - **What this is:** The panel's Apply button sets the process-wide
      `CNTRL_PC_RGB_BACKENDS` variable to the ticked devices. `remember_applied`
      reads that same variable.
    - **Problem:** The variable stays set. Apply with only the keyboard ticked, then
      press "Apply theme now" with PC sync on: the saved list is empty, so nothing
      is recorded for the PC devices. `push_to_devices` was not opened in this
      review, but if it reads the same variable (as `cntrl restore` assumes), they
      are not recoloured either.
    - **Fix:** Call `std::env::remove_var("CNTRL_PC_RGB_BACKENDS")` at the end of
      the Apply task. A cleaner fix is to pass the backend list inside
      `ApplyOptions`.
    - **If we skip it:** Theme sync from the panel silently misses RAM, GPU and
      motherboard lighting, and `cntrl restore` brings back the old colour.

## Nice to have

10. **The Fans tab reads hardware while drawing** (`cntrl-iced/src/app.rs:L2096-L2105`, `L2218-L2221`)
    - **What this is:** `fans_view` reads RPM, PWM and temperature from sysfs on
      every render.
    - **Problem:** Dragging a curve point redraws on every mouse move, so each move
      does several file reads on the UI thread.
    - **Fix:** Read the values once in `FanPoll` and keep them in `App`.
    - **If we skip it:** Dragging may stutter on slow sensor chips.

11. **Ids for duplicate chips can swap** (`cntrl-core/src/fan_control.rs:L199-L218`)
    - **What this is:** Two chips with the same name get `nvme` and `nvme#2`, in
      sorted directory order.
    - **Problem:** The sort is by text, so `hwmon10` comes before `hwmon2`, and the
      kernel numbers can change between boots. A curve on `nvme/temp1` may read the
      other drive.
    - **Fix:** Sort by the number after `hwmon`. It is still not stable across
      boots, so say so in the doc comment.
    - **If we skip it:** Only curves on duplicate chips are affected.

12. **Code nobody calls** (`fan_curves.rs:L141-L166`, `L251-L255`; `fan_engine.rs:L22-L24`; `fan_control.rs:L30`, `L69`, `L102-L107`)
    - **What this is:** `mix_would_cycle`, `smoothed_temp`, `request_stop`, the
      `Absent` status, `hwmon_path`, and `set_manual` next to `set_manual_verified`.
    - **Problem:** The first three have no callers outside tests. `Absent` is never
      created. `hwmon_path` is never read. The two `set_manual` functions are near
      copies.
    - **Fix:** Delete them and use `set_manual_verified` everywhere.
    - **If we skip it:** About 50 extra lines to read and maintain.

13. **The resume hook has a fixed binary path** — merged into finding 14.

16. **The panel still has its own copy of the save logic** (`cntrl-iced/src/app.rs:L779-L800`)
    - **What this is:** The Apply button builds its own backend list and calls
      `remember` directly.
    - **Problem:** It duplicates `remember_applied`, and it saves before applying,
      so a failed apply is still recorded.
    - **Fix:** Delete the block and call
      `device_state::remember_applied(&doc, options)` after a successful apply.
    - **If we skip it:** Two code paths must change together; a failed apply is
      restored at next boot.

## Verdict

Fix 1, 2, 3 and 4 first. Nothing in the RGB part blocks shipping.

Lean: -70 lines possible (about 50 from finding 12, about 20 from finding 16).

## Not checked

- Real fan hardware. Finding 5 (the `amdgpu` PWM write) is from memory of the
  kernel driver, not a test.
- The resume unit on a real suspend and resume.
- `push_to_devices` internals (affects the second half of finding 15).
- The README text.
- The 8 commits already on `feature/led-canvas` (canvas, per-fan chains, 12V
  header, ITE zone writes, about 4,100 lines). Only uncommitted changes were
  reviewed.

## What was run

`cargo test -p cntrl-core -p cntrl-cli -p cntrl-iced`: 23 passed, 0 failed (all
in `cntrl-core`; the other crates have no tests).
