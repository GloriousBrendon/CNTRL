# M5.1 — Device inventory and parity contract

## Purpose

Define the exact local hardware CNTRL must control for PC RGB parity, and lock the minimum behavior contract that matches current keyboard functionality (`Static`, `Custom`, `Off`, plus existing preset naming where possible).

This document is intentionally single-system and not intended as a public compatibility matrix.

## Inventory (current machine)

### Confirmed USB/HID RGB-relevant devices

| Device | VID:PID | Bus/protocol | Current evidence | Driver plan |
|--------|---------|--------------|------------------|-------------|
| MCHOSE Ace68-II keyboard | `41e4:2116` | USB HID (`hidraw0-2`) | Already controlled by CNTRL | `native` (done) |
| ITE RGB LED Controller | `048d:5702` | USB HID (`hidraw3`) | Present as `ITE Tech. Inc. ITE Device` | `native` (planned in M5.4) |
| Glorious Model O 2 PRO mouse | `258a:2033` | USB HID (`hidraw4-6`) | Detected; RGB not required in current scope | `deferred` (out of scope) |

### Expected but not yet identified in USB HID scan

| Device | Bus/protocol expectation | Current evidence gap | Driver plan |
|--------|--------------------------|----------------------|-------------|
| Corsair RAM | Likely SMBus/I2C (not standard USB HID) or alternate HID interface | Not visible in `lsusb`/`hidraw` output | `native` (priority in M5.3) |
| AMD Radeon RX 7700 XT / 7800 XT RGB | GPU vendor path (likely non-USB; may be board-vendor specific) | Not visible in current USB HID scan; OpenRGB can control it | `native` if protocol is practical, else `deferred` with explicit reason |

## Required targets for "PC RGB parity"

This section is the source of truth for what "everything" means on this machine.

- [x] Keyboard (already complete in current milestones)
- [x] Corsair RAM (required)
- [x] ITE RGB controller (required; likely motherboard/case/fans path)
- [ ] GPU RGB (investigate and include if practically controllable for this rig)
- [ ] Glorious mouse (explicitly out of scope)

If a device is not marked required, it is explicitly out of parity scope.

## CNTRL parity contract (v1)

For every **required** PC RGB target backend:

1. `Static` -> write one solid color to all controllable LEDs/zones for that target.
2. `Custom` -> same as `Static` for v1 (fallback behavior), until per-zone/per-led support exists.
3. `Off` -> write black/off state deterministically.
4. Presets -> map from existing CNTRL preset names only when target can represent them; otherwise return explicit `unsupported` (never silent no-op).

## Apply/update semantics

v1 decision for M5.1:

- Primary path: **full apply** (same path as keyboard apply).
- Optional optimization later: **fast recolor** command that skips non-color profile fields.

Rationale: full apply minimizes branching while new backends are stabilized.

## Error and capability contract

Each backend must report:

- `supported_modes`: subset of `{static, custom_fallback, off, preset_subset}`
- `apply_result`: `ok | partial | error`
- `reason`: stable message for UI/CLI status

Unsupported mode/preset requests must return a structured unsupported reason.

## M5.1 completion checklist

- [x] Every target device is listed with `native` or `deferred` driver plan.
- [x] Unsupported behavior is explicit in the contract (no hidden fallbacks).
- [x] Full apply vs fast recolor decision documented.
- [x] Final required-target list confirmed by user (Corsair + ITE required; mouse out of scope; GPU discovery required).

## OpenRGB-derived discovery inputs

OpenRGB local config path found:

- `~/.config/OpenRGB/OpenRGB.json`

Useful signals already confirmed:

- `CorsairDominatorSettings.model = "CMT"` (useful hint for Corsair RAM family assumptions).
- OpenRGB detector list includes many SMBus/GPU handlers; this supports using OpenRGB behavior as a reference while keeping CNTRL as the only control surface.

Initial GPU fact for this machine:

- `lspci` reports `AMD Radeon RX 7700 XT / 7800 XT` (`Navi 32`).

## Notes for next milestone

- M5.2 should implement backend capability/result types exactly from this contract.
- M5.3 starts with Corsair RAM driver spike and deterministic `Static`/`Off`.
