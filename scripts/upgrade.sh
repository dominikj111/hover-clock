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
STATE_DIR="${XDG_STATE_HOME:-$HOME/.local/state}/hover-clock"
LOG_FILE="$STATE_DIR/install.log"

# Audit log: one timestamped line per action (removed by uninstall).
log() {
    mkdir -p "$STATE_DIR"
    printf '%s %s\n' "$(date '+%Y-%m-%d %H:%M:%S')" "$*" >> "$LOG_FILE"
}

# --- serialize state-changing runs ----------------------------------------
# Refuse when another hover-clock script holds the lock.
LOCK_FILE="$STATE_DIR/lock"
mkdir -p "$STATE_DIR"
exec 9>"$LOCK_FILE"
if ! flock -n 9; then
    echo "!! Another hover-clock script is running (install/upgrade/swap/uninstall)." >&2
    echo "   Wait for it to finish, then retry." >&2
    exit 1
fi

# --- refuse to upgrade over an active dev instance ------------------------
# Same contract as install.sh: upgrade is an operation on production mode.
if [ -f "$STATE_DIR/state" ]; then
    echo "!! Dev mode is active: the production binary is stashed by swap-to-dev.sh." >&2
    echo "   Run ./scripts/swap-to-prod.sh first (restores the installed binary)," >&2
    echo "   then re-run this upgrade." >&2
    exit 1
fi
dev_pids=""
installed="$(readlink -f "$BINARY" 2>/dev/null || echo "$BINARY")"
for pid in $(pgrep -x hover-clock 2>/dev/null || true); do
    exe="$(readlink -f "/proc/$pid/exe" 2>/dev/null || true)"
    if [ -n "$exe" ] && [ "$exe" != "$installed" ]; then
        dev_pids="$dev_pids $pid"
    fi
done
if [ -n "$dev_pids" ]; then
    echo "!! A dev instance of hover-clock is running (not the installed daemon):" >&2
    for pid in $dev_pids; do
        echo "   PID $pid: $(tr '\0' ' ' < "/proc/$pid/cmdline" 2>/dev/null)" >&2
    done
    echo "   Stop it first (Ctrl+C in the terminal that started it, or 'pkill -x hover-clock')," >&2
    echo "   or return to production mode with ./scripts/swap-to-prod.sh." >&2
    exit 1
fi

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

outcome="daemon not running"
if systemctl --user is-active --quiet hover-clock.service 2>/dev/null; then
    echo "==> Restarting the daemon"
    systemctl --user restart hover-clock.service
    outcome="daemon restarted (systemd)"
elif pgrep -x hover-clock >/dev/null 2>&1; then
    echo "==> Restarting the daemon (autostart instance)"
    pkill -x hover-clock
    echo "    starts again at next login; run it now with: $BINARY"
    outcome="autostart instance stopped (starts at next login)"
else
    echo "==> Daemon is not running; start it with: systemctl --user start hover-clock"
fi

version="$(grep -m1 '^version' Cargo.toml | sed 's/.*= *"\(.*\)"/\1/')"
sha="$(git rev-parse --short HEAD 2>/dev/null || echo unknown)"
log "upgrade v$version ($sha) -> $BINARY: $outcome"

echo "Upgrade complete."
