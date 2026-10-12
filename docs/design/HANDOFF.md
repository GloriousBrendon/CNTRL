# CNTRL redesign — implementation handoff

The `.dc.html` files are design mockups (HTML + inline styles + a small JS state class).
Treat them as the visual/behavioural spec for the iced frontend (`cntrl-iced`), which is the only UI:
the GTK app was retired (docs/adr/0001) and there is no GTK port.
Ignore `support.js`, `<x-dc>`, `<sc-for>`, `<sc-if>` — they are the mockup runtime only.

## Navigation (sidebar)
Brand · Profile switcher (named profiles, maps to keyboard slots 1–4) · Overview · Lighting · Fans · Devices · System (disabled, "Soon") · — spacer — · Settings · status line ("4 devices · engine running").

| Screen | File | Purpose |
|---|---|---|
| Overview | Main.dc.html | Current lighting + cooling mode + device cards |
| Lighting › Effects | Lighting.dc.html | Targets → effect → color/brightness/speed |
| Lighting › Layout | Layout.dc.html | Spatial arrangement for animated/reactive effects (was "Canvas") |
| Fans | Fans.dc.html | Mode presets, fan list, curve editor |
| Devices | Devices.dc.html | Detected hardware, zones, expandable technical details |
| Settings | Settings.dc.html | Startup, theme sync (was "Sync"), appearance, about |

## Behaviour changes vs current app
- No Apply button: lighting changes apply live (debounce hardware writes ~100 ms) and autosave to the active profile.
- One merged effect gallery: Basic (Static, Breathing, Spectrum cycle, Off), Animated (on-device), Reactive (Typing ripple — keyboard only; Music, Screen ambient — need background service).
- "Apply to" target chips at the top of Lighting: All devices / Keyboard / Fans & board / Graphics card / RAM.
- Color section hidden for effects with their own palette (Spectrum, Rainbow, Fire, Matrix, Screen, Off); Speed hidden for Static/Off.
- Fans: four modes — Silent, Balanced, Performance, Custom. Curve is read-only unless Custom. Friendly names with raw sensor id underneath. One experimental warning instead of a badge per row. GPU fan shows "controlled by the graphics card".
- Settings: start at login, background service, tray icon, theme sync (+ keyboard / PC lighting), tint UI with lighting color.

## Preset curves (temp °C, duty %)
- Silent: (30,20) (50,25) (65,40) (80,70) (90,100)
- Balanced: (30,20) (50,40) (70,75) (85,100) (95,100)
- Performance: (30,35) (45,55) (60,80) (75,100) (90,100)

## Tokens
| Token | Value |
|---|---|
| bg | #0F1114 |
| sidebar | #14171B |
| surface | #1A1E23 |
| surface-raised / chip | #22272D |
| active nav / selected | #22282F |
| inset (inputs, segmented bg) | #121519 |
| border | #252A30 (cards), #2E343B (controls) |
| text | #E8EBEE |
| text-secondary | #9AA3AE / #B4BCC6 |
| text-muted | #8E97A2 |
| accent | current lighting color (default #00C6FF); text on accent #08171D |
| success | #4CC38A |
| warning | #F2A541 on #2A2418 |
| radius | 16 cards · 12 tiles · 10 buttons/inputs · 99 pills |
| spacing | page padding 32/40 · card padding 20–24 · gaps 12/16/20/24 |
| min target | 44 px |
| fonts | Instrument Sans (UI), JetBrains Mono (numbers, ids, paths) |
