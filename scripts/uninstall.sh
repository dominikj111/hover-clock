#!/bin/sh
# HoverClock uninstaller — the `curl | sh` uninstall entry point.
#
# Removes every trace of HoverClock from this machine, no matter how it
# was installed (release binary, source build, cargo install, dev or
# prod mode):
#   - stops any running instance (daemon or dev)
#   - disables and removes the systemd user unit / XDG autostart entry
#   - deletes the binary from ~/.local/bin, $CARGO_HOME/bin, and every
#     user-writable directory on PATH
#   - removes the control socket (runtime dir and temp dir fallback)
#   - removes the swap stash, audit log, and the whole state directory
# The repository itself is never touched — after an uninstall, a
# checked-out repo builds and runs freely again (the dev guard's swap
# state is removed with the rest).
#
# Usage:
#   curl -fsSL https://raw.githubusercontent.com/dominikj111/hover-clock/main/scripts/uninstall.sh | sh
#   # or from the repo:
#   ./scripts/uninstall.sh          # also: just uninstall
#
# Note: stops any running hover-clock instance, including a dev one.

set -eu

BIN_DIR="${HOVERCLOCK_BIN_DIR:-$HOME/.local/bin}"
CARGO_BIN="${CARGO_HOME:-$HOME/.cargo}/bin"
UNIT_DIR="$HOME/.config/systemd/user"
AUTOSTART_DIR="$HOME/.config/autostart"
STATE_DIR="${XDG_STATE_HOME:-$HOME/.local/state}/hover-clock"
LOG_FILE="$STATE_DIR/install.log"

log() {
    mkdir -p "$STATE_DIR"
    printf '%s %s\n' "$(date '+%Y-%m-%d %H:%M:%S')" "$*" >>"$LOG_FILE"
}

# --- serialize against the other lifecycle scripts -------------------------
# Same lock file as install/upgrade/swap: an uninstall racing with them
# on the binary or the swap state would corrupt both.
if command -v flock >/dev/null 2>&1; then
    mkdir -p "$STATE_DIR"
    exec 9>"$STATE_DIR/lock"
    if ! flock -n 9; then
        echo "!! Another hover-clock script is running (install/upgrade/swap/uninstall)." >&2
        echo "   Wait for it to finish, then retry." >&2
        exit 1
    fi
fi

removed=""
note_rm() {
    removed="${removed}  - $1
"
}

echo "==> Stopping any running hover-clock instance"
systemctl --user disable --now hover-clock.service 2>/dev/null || true
systemctl --user reset-failed hover-clock.service 2>/dev/null || true
pkill -x hover-clock 2>/dev/null || true

echo "==> Removing daemon registration"
if [ -f "$UNIT_DIR/hover-clock.service" ]; then
    rm -f "$UNIT_DIR/hover-clock.service"
    note_rm "systemd user unit: $UNIT_DIR/hover-clock.service"
    systemctl --user daemon-reload 2>/dev/null || true
fi
if [ -f "$AUTOSTART_DIR/hover-clock.desktop" ]; then
    rm -f "$AUTOSTART_DIR/hover-clock.desktop"
    note_rm "XDG autostart entry: $AUTOSTART_DIR/hover-clock.desktop"
fi

echo "==> Removing binaries"
for p in "$BIN_DIR/hover-clock" "$CARGO_BIN/hover-clock"; do
    if [ -f "$p" ] || [ -L "$p" ]; then
        rm -f "$p"
        note_rm "binary: $p"
    fi
done
IFS=':'
for dir in $PATH; do
    [ -n "$dir" ] || continue
    p="$dir/hover-clock"
    [ -f "$p" ] || [ -L "$p" ] || continue
    case "$p" in
        "$BIN_DIR/hover-clock" | "$CARGO_BIN/hover-clock") continue ;;
    esac
    if [ -w "$dir" ]; then
        rm -f "$p"
        note_rm "binary: $p"
    else
        echo "!! not removed: $p (directory not writable — remove manually)"
    fi
done
unset IFS

echo "==> Removing control socket"
if [ -n "${XDG_RUNTIME_DIR:-}" ] && { [ -f "$XDG_RUNTIME_DIR/hoverclock.sock" ] || [ -S "$XDG_RUNTIME_DIR/hoverclock.sock" ]; }; then
    rm -f "$XDG_RUNTIME_DIR/hoverclock.sock"
    note_rm "control socket: $XDG_RUNTIME_DIR/hoverclock.sock"
fi
if [ -n "${TMPDIR:-}" ] && { [ -f "$TMPDIR/hoverclock.sock" ] || [ -S "$TMPDIR/hoverclock.sock" ]; }; then
    rm -f "$TMPDIR/hoverclock.sock"
    note_rm "control socket: $TMPDIR/hoverclock.sock"
fi
if [ -S /tmp/hoverclock.sock ]; then
    rm -f /tmp/hoverclock.sock
    note_rm "control socket: /tmp/hoverclock.sock"
fi

echo "==> Removing state (swap stash, audit log, lock)"
state_existed=0
[ -d "$STATE_DIR" ] && state_existed=1
log "uninstall: removed unit/autostart entries, binaries, swap stash"
rm -rf "$STATE_DIR"
if [ "$state_existed" -eq 1 ]; then
    note_rm "state directory: $STATE_DIR"
fi

if [ -z "$removed" ]; then
    echo
    echo "Nothing to remove — HoverClock was not installed on this system."
else
    echo
    echo "Removed:"
    printf '%s' "$removed"
fi
echo
echo "HoverClock uninstalled. The source repository (if you have it) is untouched and"
echo "builds/runs freely again — no daemon registration, no leftover state."
