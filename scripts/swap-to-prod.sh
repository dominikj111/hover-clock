#!/usr/bin/env bash
# Swap from dev mode back to production: restores the stashed production
# binary(ies) to their original locations (unlink/relink — instant,
# offline, no rebuild, no re-download) and restarts the daemon.
#
# Usage: ./scripts/swap-to-prod.sh
#
# To get the *newest* version instead of the stashed one, run
# ./scripts/upgrade.sh (source build) or download the latest GitHub
# release tarball while in production mode.

set -euo pipefail

STATE_DIR="${XDG_STATE_HOME:-$HOME/.local/state}/hover-clock"
STASH_DIR="$STATE_DIR/prod-bin"
STATE_FILE="$STATE_DIR/state"
LOG_FILE="$STATE_DIR/install.log"
UNIT_DIR="$HOME/.config/systemd/user"
AUTOSTART_DIR="$HOME/.config/autostart"

# Audit log: one timestamped line per action (removed by uninstall).
log() {
    mkdir -p "$STATE_DIR"
    printf '%s %s\n' "$(date '+%Y-%m-%d %H:%M:%S')" "$*" >> "$LOG_FILE"
}

# --- serialize state-changing runs ----------------------------------------
LOCK_FILE="$STATE_DIR/lock"
mkdir -p "$STATE_DIR"
exec 9>"$LOCK_FILE"
if ! flock -n 9; then
    echo "!! Another hover-clock script is running (install/upgrade/swap/uninstall)." >&2
    echo "   Wait for it to finish, then retry." >&2
    exit 1
fi

if [ ! -f "$STATE_FILE" ]; then
    echo "No saved production install found (swap state is empty)."
    echo "Install production with ./scripts/install.sh or download the latest"
    echo "GitHub release tarball, then swap-to-dev / swap-to-prod as usual."
    exit 1
fi

echo "==> Stopping dev instances"
pkill -x hover-clock 2>/dev/null || true

service=0
autostart=0
restored=0
while IFS= read -r line; do
    case "$line" in
        service=*)
            service="${line#service=}"
            ;;
        autostart=*)
            autostart="${line#autostart=}"
            ;;
        bin\ *)
            rest="${line#bin }"
            stash="${rest%% *}"
            orig="${rest#* }"
            if [ -f "$stash" ]; then
                if [ -e "$orig" ]; then
                    # Never clobber: if a newer binary appeared at the original
                    # path during dev (re-install, cargo install --force, new
                    # GitHub tarball), keep it and drop the stale stash.
                    if cmp -s "$orig" "$stash"; then
                        echo "==> $orig already in place (identical) — dropping stash"
                        rm -f "$stash"
                    else
                        echo "!! $orig exists and differs from the stashed binary"
                        echo "   (newer version installed during dev?) — keeping the existing one,"
                        echo "   discarding the stale stash."
                        rm -f "$stash"
                    fi
                elif [ -w "$(dirname "$orig")" ]; then
                    echo "==> Restoring $orig"
                    mkdir -p "$(dirname "$orig")"
                    mv -f "$stash" "$orig"
                    restored=$((restored + 1))
                else
                    echo "!! cannot restore $orig (directory not writable — restore manually)"
                fi
            else
                echo "!! stash missing for $orig — skipping"
            fi
            ;;
    esac
done < "$STATE_FILE"
rm -f "$STATE_FILE"
rmdir "$STASH_DIR" 2>/dev/null || true

if [ "$autostart" = "1" ] && [ -f "$STATE_DIR/autostart/hover-clock.desktop" ]; then
    echo "==> Restoring autostart entry"
    mkdir -p "$AUTOSTART_DIR"
    mv "$STATE_DIR/autostart/hover-clock.desktop" "$AUTOSTART_DIR/hover-clock.desktop"
    rmdir "$STATE_DIR/autostart" 2>/dev/null || true
fi

if [ "$service" = "1" ] && [ -f "$UNIT_DIR/hover-clock.service" ]; then
    systemctl --user daemon-reload
    echo "==> Starting the daemon"
    systemctl --user enable --now hover-clock.service
elif [ "$autostart" = "1" ]; then
    echo "==> Autostart entry restored — the daemon starts at next login"
else
    echo "==> No daemon registration found; run ./scripts/install.sh to register"
fi

log "swap-to-prod: restored $restored production binary(ies) [service=$service autostart=$autostart]"
echo "Back to production."
