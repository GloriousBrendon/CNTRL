# Changelog

## 0.7.0 — first public preview

CNTRL has so far run on one machine (see "Supported hardware" in the README). This release is
for trying it on others.

### Lighting
- One canvas across keyboard, motherboard ARGB headers, 12 V strip, onboard zones, graphics card
  and RAM: 27 native effects, a second effect layer, audio-reactive effects and Screen Ambient.
- The stream runs in the background service: it survives closing the panel, can start at login,
  and comes back after suspend.
- Static colour, the keyboard's 22 built-in effects, per-key Custom and Off, applied as you
  change them and saved to the active profile.
- Lighting can follow the desktop theme's accent colour.

### Fans
- Temperature curves for any `hwmon` fan with a manual mode; Silent, Balanced, Performance and
  Custom modes; a curve can follow the hotter of two sensors.
- Fans go back to the BIOS when CNTRL stops, is switched off, loses a sensor, crashes or is killed.
- Gigabyte boards with ITE chips work through the out-of-tree `it87` driver (experimental).

### App
- New panel: Overview, Lighting (Effects and Layout), Fans, Devices, Settings.

### Known gaps
- Fan control on Radeon RX 7000 and newer graphics cards is not supported (they ignore `pwm1`).
- Screen Ambient needs a wlroots compositor (Hyprland, Sway) and `grim`.
- No tray icon and no System screen yet.
- Only the hardware in the README has been tested.
