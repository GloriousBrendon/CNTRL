#!/usr/bin/env bash
# Install CNTRL to /usr/local/bin on Arch/Linux.
#
# Usage:
#   ./scripts/install.sh              # build + install binary + udev + desktop
#   INSTALL_SYSTEMD=1 ./scripts/install.sh  # also enable boot RGB probe service
#   PREFIX=/usr ./scripts/install.sh  # install to /usr/bin instead
#
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
PREFIX="${PREFIX:-/usr/local}"
BINDIR="${PREFIX}/bin"
INSTALL_SYSTEMD="${INSTALL_SYSTEMD:-0}"

echo "==> Building release..."
cargo build --release --manifest-path "$ROOT/Cargo.toml" -p cntrl-cli

echo "==> Installing binary: $BINDIR/cntrl"
sudo install -Dm755 "$ROOT/target/release/cntrl" "$BINDIR/cntrl"

echo "==> Installing udev rules..."
sudo install -Dm644 \
    "$ROOT/packaging/udev/99-mchose-ace68.rules" \
    /etc/udev/rules.d/99-mchose-ace68.rules
sudo udevadm control --reload-rules
sudo udevadm trigger

echo "==> Installing desktop entry..."
sudo install -Dm644 \
    "$ROOT/packaging/desktop/cntrl.desktop" \
    /usr/share/applications/cntrl.desktop

if [[ "$INSTALL_SYSTEMD" == "1" ]]; then
    echo "==> Installing systemd RGB probe service..."
    sudo install -Dm644 \
        "$ROOT/packaging/systemd/cntrl-rgb-probe.service" \
        /etc/systemd/system/cntrl-rgb-probe.service
    sudo systemctl daemon-reload
    sudo systemctl enable --now cntrl-rgb-probe.service
    echo "    systemd service enabled."
fi

echo ""
echo "Done."
echo "  Start the panel:     cntrl panel"
echo "  Start the popup:     cntrl popup"
echo "  Re-probe RGB devices: cntrl probe-rgb --force"
if [[ "$INSTALL_SYSTEMD" != "1" ]]; then
    echo ""
    echo "  To enable the boot RGB probe service:"
    echo "    INSTALL_SYSTEMD=1 $0"
fi
