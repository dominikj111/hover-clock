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
# Discovers the production binary at the known install locations
# (~/.local/bin — install.sh / GitHub release tarball; ~/.cargo/bin —
# cargo install) plus anywhere else it is found on PATH.

set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
STATE_DIR="${XDG_STATE_HOME:-$HOME/.local/state}/hover-clock"
STASH_DIR="$STATE_DIR/prod-bin"
STATE_FILE="$STATE_DIR/state"
UNIT_DIR="$HOME/.config/systemd/user"
AUTOSTART_DIR="$HOME/.config/autostart"

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

# Known install locations, plus any other hover-clock on PATH.
candidates=()
for p in "$HOME/.local/bin/hover-clock" "$HOME/.cargo/bin/hover-clock"; do
    [ -f "$p" ] && candidates+=("$p")
done
other="$(command -v hover-clock 2>/dev/null || true)"
if [ -n "$other" ]; then
    case " ${candidates[*]} " in
        *" $other "*) ;;
        *) candidates+=("$other") ;;
    esac
fi

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

echo
echo "Dev mode. Running from source:"
exec cargo run -- "$@"
