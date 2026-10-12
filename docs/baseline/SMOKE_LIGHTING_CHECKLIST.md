# Smoke checklist — lighting flow

Use this **before and after** migration milestones to confirm behavior is unchanged unless intentionally documented.

**Prerequisites**

- [ ] Linux host, keyboard connected.
- [ ] udev: `packaging/udev/99-mchose-ace68.rules` installed; rules reloaded; device replugged if needed.
- [ ] Build: `cargo build` (or run `./scripts/smoke_lighting.sh` for build-only gate).

**Launch**

```bash
cargo run -p cntrl-cli -- panel
```

**Checks**

| # | Step | Pass criterion |
|---|------|------------------|
| 1 | Open **Lighting**. | Page loads: effects grid, controls, color picker, keyboard preview, **Apply To Keyboard**. |
| 2 | Apply / stderr | Run Apply with device connected: stderr includes `mchose_lighting: run_pipeline start path=...` and `effect_id=...`. If device missing or permission denied, Apply toast shows **HID write failed** (or similar) and stderr has error text — **no silent success**. |
| 3 | **Static** | Select **Static**, pick a color, **Apply**. Keyboard shows solid color matching selection. |
| 4 | **Named preset** | Select a **labeled** preset (e.g. “Aurora mapping”), set color if needed, **Apply**. Effect matches preset (not a raw number-only label in the grid). |
| 5 | **Off** | Select **Off**, **Apply**. LEDs off. |
| 6 | **Direct** (if exposed in profile/UI) | With Direct enabled in profile, Apply still completes or surfaces error — compare to `PHASE1` / profile `direct` field. |
| 7 | **Wrong hidraw** | Optional: `MCHOSE_HIDRAW=/dev/hidrawN` wrong node → clear error or wrong behavior documented (not crash). |
| 8 | **HID diagnostics API** | Optional dev check: call `crate::hid::backend::hid_diagnostics()` from a temporary `#[test]` or `cargo run --example` (not shipped in GTK UI today). Expect entries for Ace68-II when enumerated. *Skip if not set up — baseline gap documented in BASELINE_BEHAVIOR.md.* |
| 9 | **Repeat apply** | Apply ~10× in a row; UI stays responsive (HID work must not block UI thread — if GTK still blocks, record as known baseline). |

**Evidence to record (optional)**

- Screenshot or note of selected effect **label** and resulting keyboard appearance.
- Copy of stderr lines starting with `mchose_lighting:` for one successful apply (shows `effect_id`, `direct`, path).

**Failure handling**

- Unplug keyboard mid-session: user-visible error string on apply, not panic.
- Permission denied on hidraw: clear message.
