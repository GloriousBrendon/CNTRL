# ADR 0001: Migrate CNTRL UI to ICED with optional daemon, rice-native theming, and Waybar integration

## Status

Accepted (Milestone 0 — strategy only; implementation follows later milestones).

## Context

- CNTRL today is a **GTK4 + libadwaita + Relm4** Linux desktop app with **working** MCHOSE Ace68-II lighting over `hidraw` (`mchose_lighting`, `real_hid`, `profile`, `theme`).
- Goals: **ICED** frontend, **optional daemon** for panel/popup split, **Unix socket IPC**, **Waybar-friendly** commands (`cntrl popup --toggle`, `cntrl daemon --start`, `cntrl panel`), and **rice-native** theming (pywal-style / file-driven tokens) with hot reload — **without rewriting** the HID protocol pipeline except where unavoidable.

## Decision

1. **Extract a `core` crate** (Milestone 1): profile model + persistence, HID entry points (`write_profile_to_device` semantics), and effect mapping (`lighting_modes` + effects JSON) **with zero UI dependencies**. GTK remains on `core` until ICED parity is accepted.
2. **Add `cntrl-iced` (or similarly named) binary/crate** (Milestone 2+): Lighting page parity first — named effects, color, apply, keyboard preview — calling **only** `core` APIs.
3. **Daemon + IPC** (Milestone 3): long-running `cntrl daemon` on a Unix socket; `cntrl popup` connects (or spawns daemon) for Waybar one-shot actions.
4. **Theming** (Milestone 4): tokenized theme loaded from user rice sources (start with pywal-style files); file watch + hot reload for ICED **and** popup/panel; **no** hardcoded libadwaita assumptions in ICED.
5. **GTK retirement** (Milestone 5): feature parity matrix, rollback docs, **ICED as default** when accepted; **GTK removal** is a separate, reversible step after explicit approval.

## Effect mapping rule (non-negotiable)

- **Named mapping table** (`PRESET_MODES` + resources) remains the **single source of truth** for labels → firmware ids.
- Do **not** treat firmware id `3` as “static” by default — static is `MODE_STATIC`; preset “Serene Realm” maps to id `3` in the current table.
- **Static** vs **custom** remain distinct semantics in profile/UI until explicitly unified with device-captured behavior.

## Consequences

- **Positive:** Clear layering for tests, Waybar, and multiple frontends; theming aligned with dotfiles workflows.
- **Negative:** Workspace complexity, IPC versioning maintenance, two UIs temporarily (GTK + ICED).
- **Risks:** IPC surface drift vs direct in-process calls; theme token coverage gaps; Wayland/X11 focus behavior for popup — each milestone has explicit pass/fail gates.

## Compliance

- No destructive git operations as part of migration work.
- Prefer minimal refactors; preserve Linux HID behavior unless a milestone explicitly changes it with documentation.
