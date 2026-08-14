#!/usr/bin/env bash
# Install HoverClock as a session daemon. Detects the active init:
#   systemd -> systemd *user* service (~/.config/systemd/user), auto-start
#              at login via graphical-session.target
#   other   -> XDG autostart entry (~/.config/autostart), honored by
#              Xfce/GNOME/KDE sessions — the correct mechanism for
#              sysvinit/OpenRC/runit systems (MX with sysvinit boot,
#              Devuan, antiX, Alpine, Void, Gentoo)
# Builds from source, installs the binary to ~/.local/bin. No root needed.
#
# Usage: ./scripts/install.sh
#
# Stop any `cargo run` dev instance first, or use ./scripts/swap-to-dev.sh
# (it handles stopping the daemon and stashing the installed binary).

set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
BIN_DIR="${HOVERCLOCK_BIN_DIR:-$HOME/.local/bin}"
BINARY="$BIN_DIR/hover-clock"

cd "$REPO_ROOT"

echo "==> Building release binary"
cargo build --release --locked

echo "==> Installing binary to $BIN_DIR"
mkdir -p "$BIN_DIR"
install -m 755 target/release/hover-clock "$BINARY"

if [ "$(ps -p 1 -o comm= 2>/dev/null)" = "systemd" ]; then
    UNIT_DIR="$HOME/.config/systemd/user"
    echo "==> Installing systemd user unit"
    mkdir -p "$UNIT_DIR"
    install -m 644 packaging/hover-clock.service "$UNIT_DIR/hover-clock.service"
    systemctl --user daemon-reload
    systemctl --user import-environment DISPLAY XAUTHORITY 2>/dev/null || true
    echo "==> Registering and starting the daemon"
    systemctl --user enable --now hover-clock.service
    echo
    echo "HoverClock daemon installed (systemd):"
    echo "  binary   $BINARY"
    echo "  unit     $UNIT_DIR/hover-clock.service"
    echo "  status   systemctl --user status hover-clock"
    echo "  stop     systemctl --user stop hover-clock"
    echo "  start    systemctl --user start hover-clock"
else
    AUTOSTART_DIR="$HOME/.config/autostart"
    echo "==> Non-systemd init detected; installing XDG autostart entry"
    mkdir -p "$AUTOSTART_DIR"
    sed "s|__BIN_DIR__|$BIN_DIR|" packaging/hover-clock-autostart.desktop \
        > "$AUTOSTART_DIR/hover-clock.desktop"
    echo
    echo "HoverClock daemon installed (XDG autostart):"
    echo "  binary     $BINARY"
    echo "  autostart  $AUTOSTART_DIR/hover-clock.desktop"
    echo "  note       starts at next login; to run now: $BINARY"
fi

echo
if [ "$(ps -p 1 -o comm= 2>/dev/null)" = "systemd" ]; then
    echo "Dev mode: ./scripts/swap-to-dev.sh (stops daemon, stashes the installed binary,"
    echo "then cargo run); return with ./scripts/swap-to-prod.sh."
else
    echo "Dev mode: ./scripts/swap-to-dev.sh (stashes the installed binary, then cargo run);"
    echo "return with ./scripts/swap-to-prod.sh."
fi
