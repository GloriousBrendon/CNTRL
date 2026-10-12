# Milestone 4 — Rice-native theming (ICED)

## Goal

Load the same pywal-style theme file the GTK shell uses (`~/.config/themes/colors.conf`), map it to an ICED [`Theme::custom`](https://docs.rs/iced/latest/iced/enum.Theme.html) palette, and hot-reload when the file changes—without pulling UI dependencies into `cntrl-core`.

## Implementation

| Piece | Location |
|--------|-----------|
| Parse `$key=rgba(rrggbbaa)` → `#rrggbb` keys `bg1`, `bg2`, `surface`, `accent`, `text` | `cntrl-core/src/theme.rs` |
| GTK reuses core types; inotify watcher only | `cntrl-gtk/src/theme.rs` |
| `Theme::custom("cntrl-pywal", Palette { … })`, mtime poll every 1s | `cntrl-iced/src/app.rs` |

### Config path

- Default: `dirs::config_dir()/themes/colors.conf` (on Linux typically `~/.config/themes/colors.conf`).
- Same layout as documented for GTK in earlier milestones.

### ICED palette mapping

| `Palette` field | Source |
|-----------------|--------|
| `background` | `$bg1` |
| `text` | `$text` |
| `primary` | `$accent` |
| `success` / `warning` / `danger` | Built-in dark semantic colors, lightly blended (25%) toward `primary` so alerts stay readable but match accent |

If the file is missing or no keys match, parsing falls back to defaults (`ThemeColors::default` in core), which approximate the previous hard-coded ICED dark look.

### Hot reload

- ICED: `Message::ThemePoll` from `iced::time::every(Duration::from_secs(1))`; reload when `metadata().modified()` changes.
- GTK: unchanged (inotify on the parent directory).

## Manual proof (PASS / FAIL)

1. **Baseline:** With no `colors.conf` (or empty), run `cntrl-iced` — UI should match the default palette (dark, accent pink-toned defaults).
2. **Load:** Create `~/.config/themes/colors.conf` with at least one valid line, e.g. `$accent=rgba(d88f92ff)`, reload ICED — accent / primary controls should follow.
3. **Hot reload:** Edit `colors.conf` on disk while ICED is open — within ~1s the shell chrome (and keyboard canvas frame/text) should update without restart.
4. **GTK parity (optional):** Run `cntrl-gtk` and confirm theme file edits still apply CSS + keyboard accent sync as before.

Record result and date in this section when validated.

**Status:** ☐ PASS ☐ FAIL — validated: _______________

## Notes

- `$surface` is parsed for parity with GTK templates but is not currently mapped to a distinct ICED palette slot (reserved for future surfaces / cards).
- For Wayland/X11 display, the `cntrl-iced` crate still needs the appropriate `iced` backend features (see workspace `Cargo.toml`).
