#!/usr/bin/env bash
# Install HoverClock as a session daemon. Detects the active init:
#   systemd -> systemd *user* service (~/.config/systemd/user), auto-start
#              at login via default.target (every systemd user session)
#              and graphical-session.target (GNOME/KDE); xfce-style
#              sessions never raise the latter — default.target covers them
#   other   -> XDG autostart entry (~/.config/autostart), honored by
#              Xfce/GNOME/KDE sessions — the correct mechanism for
#              sysvinit/OpenRC/runit systems (MX with sysvinit boot,
#              Devuan, antiX, Alpine, Void, Gentoo)
# Builds from source, installs the binary to ~/.local/bin. No root needed.
#
# Usage: ./scripts/install.sh
#
# Refuses to run while a dev instance is active: a `cargo run` process, or
# a swap-to-dev stash (production binary set aside). Return to production
# mode with ./scripts/swap-to-prod.sh first (or stop the dev instance).

set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
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
# Refuse when another hover-clock script holds the lock (install/upgrade/
# swap/uninstall racing on the binary and the swap state would corrupt
# both). Non-blocking: a clear message beats a silent wait.
LOCK_FILE="$STATE_DIR/lock"
mkdir -p "$STATE_DIR"
exec 9>"$LOCK_FILE"
if ! flock -n 9; then
    echo "!! Another hover-clock script is running (install/upgrade/swap/uninstall)." >&2
    echo "   Wait for it to finish, then retry." >&2
    exit 1
fi

# --- refuse to install over an active dev instance ------------------------
# Install is an operation on production mode. A dev `cargo run` process
# executes a binary that is not the install target; swap-to-dev stashes the
# production binary and records the stash state. Either way, installing now
# would leave the dev process running a stale binary and the swap state
# dangling.
if [ -f "$STATE_DIR/state" ]; then
    echo "!! Dev mode is active: the production binary is stashed by swap-to-dev.sh." >&2
    echo "   Run ./scripts/swap-to-prod.sh first (restores the installed binary),">&2
    echo "   then re-run this install." >&2
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

echo "==> Building release binary"
cargo build --release --locked

echo "==> Installing binary to $BIN_DIR"
mkdir -p "$BIN_DIR"
install -m 755 target/release/hover-clock "$BINARY"

if [ "$(ps -p 1 -o comm= 2>/dev/null)" = "systemd" ]; then
    MECH=systemd
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
    MECH=xdg-autostart
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

version="$(grep -m1 '^version' Cargo.toml | sed 's/.*= *"\(.*\)"/\1/')"
sha="$(git rev-parse --short HEAD 2>/dev/null || echo unknown)"
log "install v$version ($sha) -> $BINARY [$MECH]"

echo
if [ "$(ps -p 1 -o comm= 2>/dev/null)" = "systemd" ]; then
    echo "Dev mode: ./scripts/swap-to-dev.sh (stops daemon, stashes the installed binary,"
    echo "then cargo run); return with ./scripts/swap-to-prod.sh."
else
    echo "Dev mode: ./scripts/swap-to-dev.sh (stashes the installed binary, then cargo run);"
    echo "return with ./scripts/swap-to-prod.sh."
fi

echo "Audit log: $LOG_FILE (one line per install/upgrade/swap run)"
