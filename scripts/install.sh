#!/usr/bin/env bash
# Install CNTRL to /usr/local/bin on Arch/Linux.
#
# Usage:
#   ./scripts/install.sh              # build + install binary + udev + desktop
#                                     # (a release archive ships the binary: no build)
#   INSTALL_SYSTEMD=1 ./scripts/install.sh  # also install the daemon, boot/login restore + resume hook
#   INSTALL_IT87=1 ./scripts/install.sh     # also load the it87 fan driver at boot (experimental)
#   PREFIX=/usr ./scripts/install.sh  # install to /usr/bin instead
#
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
PREFIX="${PREFIX:-/usr/local}"
BINDIR="${PREFIX}/bin"
INSTALL_SYSTEMD="${INSTALL_SYSTEMD:-0}"
INSTALL_IT87="${INSTALL_IT87:-0}"

# A release archive has the binary next to scripts/; a checkout builds it.
if [[ -x "$ROOT/cntrl" ]]; then
    BIN="$ROOT/cntrl"
else
    echo "==> Building release..."
    cargo build --release --manifest-path "$ROOT/Cargo.toml" -p cntrl-cli
    BIN="$ROOT/target/release/cntrl"
fi

echo "==> Installing binary: $BINDIR/cntrl"
sudo install -Dm755 "$BIN" "$BINDIR/cntrl"

# An older copy earlier on PATH would keep running old code from your shell
# while the services run the new one. Point it at the installed binary.
STALE="$HOME/.local/bin/cntrl"
if [[ -f "$STALE" && ! -L "$STALE" && "$STALE" != "$BINDIR/cntrl" ]]; then
    echo "==> Replacing stale copy $STALE with a link to $BINDIR/cntrl"
    ln -sf "$BINDIR/cntrl" "$STALE"
fi

# A running daemon keeps executing the old binary until restarted.
systemctl --user try-restart cntrl-daemon.service 2>/dev/null || true

# Units ship with /usr/local/bin; rewrite to wherever the binary really went.
ME="${SUDO_USER:-$USER}"
unit() {
    sed -e "s|/usr/local/bin/cntrl|$BINDIR/cntrl|" \
        -e "s|@USER@|$ME|" -e "s|@HOME@|$(getent passwd "$ME" | cut -d: -f6)|" \
        "$ROOT/packaging/systemd/$1"
}

echo "==> Installing udev rules..."
sudo install -Dm644 \
    "$ROOT/packaging/udev/99-mchose-ace68.rules" \
    /etc/udev/rules.d/99-mchose-ace68.rules
sudo install -Dm644 \
    "$ROOT/packaging/udev/99-cntrl-hwmon.rules" \
    /etc/udev/rules.d/99-cntrl-hwmon.rules
sudo udevadm control --reload-rules
sudo udevadm trigger

echo "==> Installing desktop entry..."
sudo install -Dm644 \
    "$ROOT/packaging/desktop/cntrl.desktop" \
    /usr/share/applications/cntrl.desktop

if [[ "$INSTALL_SYSTEMD" == "1" ]]; then
    echo "==> Installing systemd RGB probe service..."
    unit cntrl-rgb-probe.service | sudo tee /etc/systemd/system/cntrl-rgb-probe.service >/dev/null
    sudo systemctl daemon-reload
    sudo systemctl enable --now cntrl-rgb-probe.service
    echo "    systemd service enabled."

    echo "==> Installing resume hook (re-applies RGB after suspend/hibernate)..."
    unit "cntrl-resume@.service" | sudo tee "/etc/systemd/system/cntrl-resume@.service" >/dev/null
    sudo systemctl daemon-reload
    sudo systemctl enable "cntrl-resume@${SUDO_USER:-$USER}.service"
    echo "    resume hook enabled for ${SUDO_USER:-$USER}."

    echo "==> Installing user services (daemon + restore lighting on login)..."
    mkdir -p "$HOME/.config/systemd/user"
    unit cntrl-daemon.service > "$HOME/.config/systemd/user/cntrl-daemon.service"
    unit cntrl-restore.service > "$HOME/.config/systemd/user/cntrl-restore.service"
    systemctl --user daemon-reload
    systemctl --user enable cntrl-restore.service
    systemctl --user enable cntrl-daemon.service
    systemctl --user restart cntrl-daemon.service
    echo "    daemon restarted on the new binary."
fi

if [[ "$INSTALL_IT87" == "1" ]]; then
    echo "==> Loading the it87 motherboard fan driver at boot (experimental)..."
    echo it87 | sudo tee /etc/modules-load.d/cntrl-it87.conf >/dev/null
    sudo modprobe it87 || echo "    modprobe it87 failed — see README 'Motherboard headers'."
    sudo udevadm trigger --subsystem-match=hwmon
    if ! grep -q O /sys/module/it87/taint 2>/dev/null; then
        echo "    WARNING: this is the kernel's built-in it87. On recent Gigabyte boards it"
        echo "    cannot set fan speed. Install it87-dkms-git and your kernel's headers."
    fi
fi

echo ""
echo "Done."
echo "  Start the panel:     cntrl panel"
echo "  Start the popup:     cntrl popup"
echo "  Re-probe RGB devices: cntrl probe-rgb --force"
if [[ "$INSTALL_SYSTEMD" != "1" ]]; then
    echo ""
    echo "  To install the daemon, login restore + suspend/hibernate resume hook:"
    echo "    INSTALL_SYSTEMD=1 $0"
fi
