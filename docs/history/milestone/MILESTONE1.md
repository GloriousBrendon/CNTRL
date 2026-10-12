# Milestone 1 — Workspace and core extraction (checkpoint)

## Workspace layout

```
CNTRL/
├── Cargo.toml                 # [workspace] members = ["cntrl-core", "cntrl"]
├── cntrl-core/
│   ├── Cargo.toml
│   └── src/
│       ├── lib.rs
│       ├── persistence.rs     # profile slot + active_slot.txt I/O
│       ├── profile.rs
│       ├── lighting_modes.rs
│       ├── mchose_lighting.rs
│       └── hid/
│           ├── mod.rs
│           ├── real_hid.rs
│           └── mock_hid.rs
├── cntrl/                     # GTK4 + libadwaita binary (package name `cntrl`)
│   ├── Cargo.toml
│   └── src/
│       ├── main.rs
│       ├── theme.rs
│       └── ui/
├── resources/                 # shared JSON (include_str paths from cntrl-core)
└── scripts/smoke_lighting.sh
```

## Public `cntrl-core` API (summary)

| Module | Primary exports |
|--------|------------------|
| `profile` | `ProfileDocument`, `default_profile_document`, `normalize`, `code_to_color_index`, … |
| `persistence` | `load_profile_slot`, `persist_profile_slot`, `persist_active_slot`, `read_startup_slot` |
| `lighting_modes` | `PRESET_MODES`, `EFFECT_ID_OFF`, `MODE_STATIC`, `MODE_CUSTOM`, `is_preset_effect_id` |
| `mchose_lighting` | `vendor_hidraw_path`, `run_pipeline`, `set_effect`, `set_solid_rgb` |
| `hid` | `backend` (OS-specific `real_hid` / `mock_hid`), `HidInterfaceDiagnostic` |

The GTK crate re-exports `load_profile_slot`, `persist_profile_slot`, `persist_active_slot` from `main.rs` for minimal UI churn (`crate::persist_profile_slot`).

## Build

```bash
cargo build              # entire workspace
cargo build -p cntrl-core
cargo build -p cntrl
cargo test -p cntrl-core
cargo run -p cntrl
```

## Pass criteria

- [x] Workspace builds.
- [x] `cntrl-core` has no GTK/libadwaita/relm4 dependencies (`cargo tree -p cntrl-core`).
- [ ] GTK lighting behavior unchanged on device (manual smoke per `docs/baseline/SMOKE_LIGHTING_CHECKLIST.md`).
