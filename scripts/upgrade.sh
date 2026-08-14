#!/usr/bin/env bash
# Pull the latest source, rebuild, and restart the installed daemon in
# place. The overlay is transient, so a brief restart between dwells is
# imperceptible — the desktop session is never disturbed.
#
# Usage: ./scripts/upgrade.sh [branch]   (default: current branch)
#
# On non-systemd systems the daemon is an autostart process, so it cannot
# be restarted cleanly yet (no control channel until M5) — the new binary
# is installed and the old process stopped; it starts again at next login.

set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
BRANCH="${1:-$(git -C "$REPO_ROOT" symbolic-ref --short HEAD 2>/dev/null || echo main)}"
BIN_DIR="${HOVERCLOCK_BIN_DIR:-$HOME/.local/bin}"
BINARY="$BIN_DIR/hover-clock"

cd "$REPO_ROOT"

echo "==> Pulling latest from origin/$BRANCH"
git fetch origin
git checkout "$BRANCH"
git pull --ff-only origin "$BRANCH"

echo "==> Building release binary"
cargo build --release --locked

echo "==> Installing binary to $BIN_DIR"
mkdir -p "$BIN_DIR"
install -m 755 target/release/hover-clock "$BINARY"

if systemctl --user is-active --quiet hover-clock.service 2>/dev/null; then
    echo "==> Restarting the daemon"
    systemctl --user restart hover-clock.service
elif pgrep -x hover-clock >/dev/null 2>&1; then
    echo "==> Restarting the daemon (autostart instance)"
    pkill -x hover-clock
    echo "    starts again at next login; run it now with: $BINARY"
else
    echo "==> Daemon is not running; start it with: systemctl --user start hover-clock"
fi

echo "Upgrade complete."
