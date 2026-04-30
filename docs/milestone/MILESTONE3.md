# Milestone 3 — Daemon + IPC + panel/popup CLI (checkpoint)

## IPC protocol (line-delimited JSON)

Default socket: `$XDG_RUNTIME_DIR/cntrl/daemon.sock` (override with `CNTRL_SOCKET`).

**Client → server** (one JSON object + newline per request):

| `cmd` | Fields | Meaning |
|-------|--------|--------|
| `ping` | — | Health check |
| `apply_profile` | `document` (full `ProfileDocument`), `slot` (`0`…`3`) | Persist to `slot_*.json` then HID lighting |

**Server → client** (one JSON object + newline):

```json
{ "ok": true, "message": "pong" }
{ "ok": false, "error": "…" }
```

Implementation: `cntrl-core/src/ipc.rs`.

## Commands (unified `cntrl` binary)

Built from package `cntrl-cli` (`target/debug/cntrl` after `cargo build`).

| Command | Role |
|---------|------|
| `cntrl daemon` / `cntrl daemon --start` | Foreground Unix socket server (HID + persist on `apply_profile`). |
| `cntrl panel` | Full ICED shell (same as `cntrl_iced::run_with(Panel)`). |
| `cntrl popup` | Compact ICED window; writes `$XDG_RUNTIME_DIR/cntrl/popup.pid`. |
| `cntrl popup --toggle` | If popup pid exists → `SIGTERM` and remove pid; else spawn `cntrl popup` detached (Waybar-friendly). |

**GTK fallback:** `cargo run -p cntrl-gtk` → binary `cntrl-gtk`.

**Direct ICED (no CLI):** `cargo run -p cntrl-iced` still runs full panel.

## Apply path (daemon vs local)

`cntrl_core::ipc::apply_local_or_daemon` is used by **ICED** and **GTK** Apply:

1. If the socket exists and accepts a connection → send `apply_profile` to daemon.
2. Otherwise → `persist_profile_slot` + `hid::write_profile_to_device` locally.

So lighting works **without** a daemon; with a daemon running, applies go through it (single HID gate).

## Example manual tests

```bash
# Terminal A
cargo run -p cntrl-cli -- daemon

# Terminal B — ping (adjust path if CNTRL_SOCKET set)
printf '%s\n' '{"cmd":"ping"}' | socat - UNIX-CONNECT:"$XDG_RUNTIME_DIR/cntrl/daemon.sock"

# Panel / popup
cargo run -p cntrl-cli -- panel
cargo run -p cntrl-cli -- popup
cargo run -p cntrl-cli -- popup --toggle   # twice to spawn / close
```

## Pass criteria (Milestone 3)

- [x] Daemon listens and responds to `ping` / `apply_profile`.
- [x] `cntrl panel` / `popup` / `popup --toggle` / `daemon --start` exist as above.
- [x] Apply from UI uses daemon when available, else local HID.
- [ ] Full device regression: run smoke checklist with daemon on/off (manual).
