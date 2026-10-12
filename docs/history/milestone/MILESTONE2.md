# Milestone 2 — ICED shell + Lighting parity (checkpoint)

## Run

```bash
cargo run -p cntrl-iced
```

GTK frontend is unchanged: `cargo run -p cntrl`.

## UI structure (textual)

- **Shell:** Left sidebar — **Lighting** | **Profiles** | **Performance** (latter two are placeholders with copy to use GTK until later milestones).
- **Lighting:** Top **Canvas** keyboard (68-key geometry from `cntrl-core::keyboard_layout`, same hit-test math as GTK). Below: **Effects** — **Static**, **Custom**, **Off** (named, not numbers) + scrollable list of `PRESET_MODES` labels; **Controls** — hex line, brightness/speed sliders; **Color (HSL)** — hue/sat/light sliders + preset swatches; **Apply To Keyboard** + status line.

## Effect id evidence (stderr)

On **Apply**, the binary logs lines such as:

- `cntrl-iced: apply profile.data.light.config.effect=<n>` — always (profile field).
- `cntrl-iced: HID uses firmware effect_id=<n> (set_effect)` — when a **named preset** is selected (`is_preset_effect_id`).
- `cntrl-iced: HID solid DIY (set_solid_rgb); static/custom are not firmware id 3` — **Static** / **Custom**.
- `cntrl-iced: HID off (solid black)` — **Off**.

This preserves the **static ≠ firmware id 3** rule: static uses `MODE_STATIC` (`-2`), not `3`.

## Parity checklist

| Item | ICED |
|------|------|
| Named effects only in grid (no “Effect 7”-only labels) | Yes — `PRESET_MODES[].label` + Static/Custom/Off |
| Color → profile `hex_color` + apply | Yes |
| Brightness / speed in profile | Yes |
| Per-key preview colors | Yes — click key, change color, apply |
| Off | `EFFECT_ID_OFF` → `set_solid_rgb(0,0,0)` via core HID |
| Hardware | Manual: run `SMOKE_LIGHTING_CHECKLIST.md` against `cntrl-iced` |

## Changed / added (summary)

- `cntrl-iced/` crate (`app.rs`, `main.rs`).
- `cntrl-core`: `color.rs` (`hex_to_rgb`, `rgb_to_hex`), `keyboard_layout.rs` (shared layout + hit test).
