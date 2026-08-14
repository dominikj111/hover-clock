#!/usr/bin/env bash
# Permanently remove the HoverClock production install: stops the daemon,
# removes the systemd user service / XDG autostart entry, the installed
# binary(ies), and any swap stash. The source tree stays untouched.
#
# To merely switch to dev mode while keeping the production install for a
# fast return, use ./scripts/swap-to-dev.sh / ./scripts/swap-to-prod.sh.
#
# Usage: ./scripts/uninstall.sh
# Note: stops any running hover-clock instance, including a dev one.

set -euo pipefail

BIN_DIR="${HOVERCLOCK_BIN_DIR:-$HOME/.local/bin}"
UNIT_DIR="$HOME/.config/systemd/user"
AUTOSTART_DIR="$HOME/.config/autostart"
STATE_DIR="${XDG_STATE_HOME:-$HOME/.local/state}/hover-clock"
LOG_FILE="$STATE_DIR/install.log"

# Audit log: one timestamped line per action (removed by uninstall).
log() {
    mkdir -p "$STATE_DIR"
    printf '%s %s\n' "$(date '+%Y-%m-%d %H:%M:%S')" "$*" >> "$LOG_FILE"
}

echo "==> Stopping and disabling the daemon"
systemctl --user disable --now hover-clock.service 2>/dev/null || true
pkill -x hover-clock 2>/dev/null || true

echo "==> Removing registration and binaries"
rm -f "$UNIT_DIR/hover-clock.service"
rm -f "$AUTOSTART_DIR/hover-clock.desktop"
rm -f "$BIN_DIR/hover-clock"
rm -f "${CARGO_HOME:-$HOME/.cargo}/bin/hover-clock"

# Any other hover-clock on PATH that is user-writable (e.g. a GitHub
# release tarball extracted elsewhere). Discovery, not presumption.
IFS=':' read -ra dirs <<< "$PATH"
for dir in "${dirs[@]}"; do
    [ -n "$dir" ] || continue
    p="$dir/hover-clock"
    [ -f "$p" ] || continue
    case "$p" in
        "$BIN_DIR/hover-clock" | "${CARGO_HOME:-$HOME/.cargo}/bin/hover-clock") continue ;;
    esac
    if [ -w "$dir" ]; then
        rm -f "$p"
    else
        echo "!! not removed: $p (directory not writable — remove manually)"
    fi
done

log "uninstall: removed unit/autostart entries, binaries, swap stash"
rm -rf "$STATE_DIR"
systemctl --user daemon-reload 2>/dev/null || true

echo "HoverClock uninstalled. Source tree untouched."
