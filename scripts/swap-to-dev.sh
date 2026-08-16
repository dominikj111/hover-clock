#!/usr/bin/env bash
# Swap from installed-daemon mode to dev mode — without losing the
# production install:
#
#   1. stops the installed daemon (systemd user service / autostart process)
#   2. stashes the installed production binary(ies) aside and hides the
#      daemon registration (service disabled, autostart entry moved) so a
#      login while in dev mode cannot start a daemon with a missing binary
#   3. runs `cargo run` from the source tree
#
# The production binary is NOT deleted — it is unlinked and restored by
# ./scripts/swap-to-prod.sh, so returning to production is instant and
# offline (no rebuild, no re-download).
#
# Usage: ./scripts/swap-to-dev.sh [cargo run args...]
#
# Discovers the production binary rather than presuming a path: the known
# install locations (~/.local/bin — install.sh / GitHub release tarball;
# $CARGO_HOME/bin — cargo install; HOVERCLOCK_BIN_DIR override) plus every
# directory on PATH. Each found binary is stashed and restored to its
# exact original path by ./scripts/swap-to-prod.sh.
#
# cargo note: while the binary is stashed, `cargo uninstall hover-clock`
# cannot find it — swap back to production first.

set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
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

# --- refuse a second swap-to-dev while dev mode is already active ---------
# The state file records which production binary is stashed where; rerunning
# would overwrite it and orphan the stash (swap-to-prod could not restore
# it). Refuse instead.
if [ -f "$STATE_FILE" ]; then
    echo "!! Dev mode is already active (swap state exists)." >&2
    echo "   Return to production with ./scripts/swap-to-prod.sh first," >&2
    echo "   or stop the running dev session (Ctrl+C) before swapping again." >&2
    exit 1
fi

cd "$REPO_ROOT"

# --- 1. stop the installed daemon -----------------------------------------
service=0
if [ -f "$UNIT_DIR/hover-clock.service" ]; then
    service=1
    echo "==> Stopping systemd daemon"
    systemctl --user disable --now hover-clock.service 2>/dev/null || true
fi
pkill -x hover-clock 2>/dev/null || true

# --- 2. hide daemon registration ------------------------------------------
autostart=0
if [ -f "$AUTOSTART_DIR/hover-clock.desktop" ]; then
    autostart=1
    mkdir -p "$STATE_DIR/autostart"
    mv "$AUTOSTART_DIR/hover-clock.desktop" "$STATE_DIR/autostart/hover-clock.desktop"
fi

# --- 3. stash production binary(ies) ---------------------------------------
mkdir -p "$STASH_DIR"
printf 'service=%s\nautostart=%s\n' "$service" "$autostart" > "$STATE_FILE"

# Discovery, not presumption: known locations + HOVERCLOCK_BIN_DIR override
# + every directory on PATH.
candidates=()
add_candidate() {
    for c in "${candidates[@]}"; do
        [ "$c" = "$1" ] && return
    done
    candidates+=("$1")
}
for p in "${HOVERCLOCK_BIN_DIR:-$HOME/.local/bin}/hover-clock" "${CARGO_HOME:-$HOME/.cargo}/bin/hover-clock"; do
    [ -f "$p" ] && add_candidate "$p"
done
IFS=':' read -ra dirs <<< "$PATH"
for dir in "${dirs[@]}"; do
    [ -n "$dir" ] || continue
    [ -f "$dir/hover-clock" ] && add_candidate "$dir/hover-clock"
done

i=0
for bin in "${candidates[@]}"; do
    if [ ! -w "$(dirname "$bin")" ]; then
        echo "!! not stashing $bin (directory not writable — remove manually)"
        continue
    fi
    stash="$STASH_DIR/hover-clock.$i"
    echo "==> Stashing $bin"
    mv "$bin" "$stash"
    printf 'bin %s %s\n' "$stash" "$bin" >> "$STATE_FILE"
    i=$((i + 1))
done

log "swap-to-dev: stashed $i production binary(ies) [service=$service autostart=$autostart]; dev run"

echo
# Release the state lock before handing control to the dev session, so other
# scripts (e.g. uninstall) can still run while dev is active.
flock -u 9
echo "Dev mode. Running from source:"
exec cargo run -- "$@"
