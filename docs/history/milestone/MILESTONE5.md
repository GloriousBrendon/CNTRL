# Milestone 5 — PC RGB parity roadmap (single-system scope)

## Goal

Extend CNTRL from keyboard-only lighting to full **local PC RGB control** on this machine, using the same core behavior already working for the keyboard:

- color apply
- existing basic modes (`Static`, `Custom`, `Off`, current preset mapping)
- profile-driven apply path

No "fancy" new effects are required in this milestone set.

## Non-goals (for now)

- Public hardware compatibility matrix
- Cross-platform support
- Full OpenRGB feature parity
- New animation engine or advanced per-led choreography

## Constraints

- CNTRL remains the only user-facing control surface.
- Design can be hardware-specific to this exact PC.
- `cntrl-core` remains UI-agnostic; frontends call core APIs.
- Daemon/IPC remains the single apply gate when running.

## Milestone breakdown

### M5.1 — Device inventory and control surface contract

Define exactly which non-keyboard RGB devices CNTRL must control on this PC, and lock a minimal shared control contract.

Reference doc: `docs/milestone/M5_1_DEVICE_INVENTORY_AND_PARITY_CONTRACT.md`

**Deliverables**

- Hardware inventory doc (`vendor/product`, bus type, expected controllable zones).
- "Parity contract" doc: mapping from CNTRL lighting model to PC devices:
  - `Static` -> solid color write
  - `Custom` -> initial fallback behavior (solid color until per-zone is added)
  - `Off` -> black/off write
  - current preset names -> either mapped or explicitly unsupported per device
- Decision on update semantics: full apply only vs full apply + fast recolor.

**Pass criteria**

- [ ] Every target device is listed with an owner driver plan (`native` or `deferred`).
- [ ] Unsupported functions are explicit (no hidden behavior differences).
- [ ] Contract approved and referenced by implementation milestones below.

**Status:** Complete (targets confirmed; see inventory/contract doc for GPU discovery note).

---

### M5.2 — Core backend abstraction for PC RGB

Add a backend interface in `cntrl-core` for non-keyboard RGB targets, without changing frontend ownership.

Implementation snapshot:

- `cntrl-core/src/rgb_backends.rs` adds backend capabilities, apply result reporting, and env-driven backend registry (`CNTRL_PC_RGB_BACKENDS`).
- `cntrl-core/src/ipc.rs` local apply now fans out to optional PC RGB backends after keyboard HID.
- `cntrl-cli/src/daemon.rs` now routes `apply_profile` through `cntrl_core::ipc::apply_local` to keep daemon/local behavior aligned.

**Deliverables**

- Core trait/interface for RGB backends (apply-by-profile style).
- Backend registry/config for this machine (enable/disable specific targets).
- Apply orchestrator that executes keyboard + PC targets in one CNTRL apply call.
- Structured result reporting (`ok`, partial failure, per-backend error message).

**Pass criteria**

- [x] Existing keyboard apply behavior is unchanged.
- [x] One apply action can fan out to multiple RGB backends.
- [x] Failure in one backend does not crash CNTRL; error is surfaced cleanly.

**Status:** Complete (scaffold + fan-out path; backend protocol work starts in M5.3/M5.4).

---

### M5.3 — Corsair RAM native backend (priority target)

Implement a CNTRL-native Corsair RAM backend that binds to the correct HID/interface and performs solid color + off control.

**Deliverables**

- Reliable device selection logic (correct interface, not wrong HID node).
- Minimal protocol implementation for:
  - init/handshake (if required)
  - set solid color
  - set off/black
- Safety guardrails (exact VID/PID/interface checks for this machine).
- Optional diagnostic command/log output for backend health.

Implementation snapshot:

- `cntrl-core/src/corsair_ram.rs` added:
  - strict HID discovery for Corsair VID `1b1c`
  - scored interface selection (name hints + optional `CNTRL_CORSAIR_RAM_INTERFACE`)
  - SMBus/I2C candidate discovery fallback (`/sys/bus/i2c/devices`) for RAM paths that are not HID
  - profile-mode guard (`Static`, `Custom`, `Off` only)
  - template-driven packet writer for `init` + `apply` packets
- `cntrl-core/src/rgb_backends.rs` now routes `corsair_ram` backend through this implementation and reports `ok/unsupported/error`.

Environment contract (current):

- `CNTRL_CORSAIR_RAM_APPLY_TEMPLATE` (required to send writes)
  - space-separated bytes, supports placeholders `rr gg bb`
  - example shape: `00 08 12 rr gg bb 00 00`
- `CNTRL_CORSAIR_RAM_INIT_TEMPLATES` (optional)
  - `;`-separated packet templates (same format) sent before apply
- `CNTRL_CORSAIR_RAM_INTERFACE` (optional)
  - preferred HID interface number to force target selection

**Pass criteria**

- [ ] Repeated applies update RAM color deterministically. (pending packet template validation)
- [ ] `Off` always turns RAM LEDs off. (pending packet template validation)
- [x] Backend rejects non-matching devices instead of writing blindly.

**Status:** In progress (target selection + guarded write path done; packet templates and on-device validation next).

Current finding from local OpenRGB logs:

- OpenRGB strongly indicates DIMM logic on I2C/SMBus ("Detecting I2C DIMM modules"), while HID is used for motherboard controller (`/dev/hidraw3`).
- This means Corsair RAM support may require SMBus path work in CNTRL, not only HID endpoint correction.

---

### M5.4 — Additional local devices (GPU/motherboard/case) as needed

Add only the remaining devices on this PC that are required for "whole-system parity."

**Deliverables**

- One backend per required device family (or explicit defer note).
- Same minimal command set per backend (`Static`, `Custom` fallback, `Off`, mapped preset subset).
- Per-device capability declaration (supported modes/features).

Implementation snapshot (early start):

- `cntrl-core/src/ite_rgb.rs` added for motherboard/case path (`048d:5702`):
  - strict ITE HID discovery with optional interface pinning (`CNTRL_ITE_INTERFACE`)
  - template-driven init/apply packets with `rr gg bb` placeholders
  - base mode guard (`Static`/`Custom` fallback/`Off`)
- `cntrl-core/src/rgb_backends.rs` now routes `ite_rgb` through this backend.

Environment contract (ITE path):

- `CNTRL_ITE_APPLY_TEMPLATE` (required to send writes)
- `CNTRL_ITE_INIT_TEMPLATES` (optional, `;`-separated pre-apply packets)
- `CNTRL_ITE_INTERFACE` (optional preferred HID interface number)

**Pass criteria**

- [ ] All required local devices respond to base color/off controls.
- [ ] Unsupported presets are handled gracefully (no silent no-op).
- [ ] No external RGB UI is required for day-to-day use.

**Status:** In progress (ITE backend scaffold wired; packet templates + hardware validation pending).

Candidate packet templates from local usbmon capture (`/tmp/20260430_131859_openrgb_usbmon_full.pcapng`, host -> `1.3.0`):

- Repeating baseline/init family observed before mode writes:
  - `cc60`, `cc20`..`cc28`, `cc31`, `cc32`, `cc34`
- Direct red writes (`ff0000`) observed in:
  - `cc58*` / `cc59*` packets with repeated RGB triplets
  - followed by `cc23`, `cc24`, `cc28` commit-style packets

Ready-to-test env candidates (M5.4 validation):

- `CNTRL_ITE_INIT_TEMPLATES` candidate:
  - `CC 60 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00;CC 20 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00;CC 21 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00;CC 22 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00;CC 23 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00;CC 24 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00;CC 25 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00;CC 26 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00;CC 27 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00;CC 28 FF 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00;CC 31 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00;CC 32 1B 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00;CC 34 10 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00`
- `CNTRL_ITE_APPLY_TEMPLATE` candidate:
  - `CC 23 08 00 00 00 00 00 00 00 01 rr gg bb 00 00 rr gg bb 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00`

Validation note:

- If color does not latch with only `CC 23 ...`, test this commit tail as additional init templates after apply:
  - `CC 24 10 00 00 00 00 00 00 00 01 rr gg bb 00 00 rr gg bb 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00`
  - `CC 28 18 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00`

---

### M5.5 — IPC + UI integration for unified apply

Expose PC RGB control through existing daemon/UI flows so CNTRL remains one place to control everything.

**Deliverables**

- IPC extension (or profile embedding) so daemon apply includes PC RGB state.
- UI toggle/surface to enable PC RGB sync and show backend status.
- Clear status reporting in panel/popup after apply (success/partial/failure).

**Pass criteria**

- [ ] `cntrl panel` and `cntrl popup` apply keyboard + PC RGB together.
- [ ] With daemon running, apply uses daemon path only.
- [ ] With daemon unavailable, local fallback still applies keyboard + PC targets.

---

### M5.6 — Validation and hardening

Ship a repeatable smoke procedure proving parity behavior across reboots and repeated applies.

**Deliverables**

- `docs/baseline` smoke checklist for PC RGB parity.
- Optional script for repeated color/off cycle tests.
- Failure matrix with known limits per backend.

**Pass criteria**

- [ ] Cold boot -> CNTRL apply restores expected state.
- [ ] Repeated apply loop does not drift or hang target devices.
- [ ] Known limitations are documented and acceptable for daily use.

## Definition of done for this roadmap

CNTRL can apply the same practical lighting intent already used for the keyboard (color + base modes) to the keyboard **and** required PC RGB devices on this machine, from one UI/CLI flow, with no dependency on another interactive RGB application.
