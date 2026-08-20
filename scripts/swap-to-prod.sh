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

# --- stale-restore guard ----------------------------------------------------
# A release published while dev mode was active (`deploy.sh` → GitHub) makes
# the stash stale: swap-to-prod restores it (offline contract), but must say
# so loudly — otherwise production silently drops back to an old build.

# Numeric x.y.z comparison: $1 > $2.
version_gt() {
    IFS='.' read -r -a a <<< "$1"
    IFS='.' read -r -a b <<< "$2"
    for i in 0 1 2; do
        if (( ${a[$i]:-0} > ${b[$i]:-0} )); then return 0; fi
        if (( ${a[$i]:-0} < ${b[$i]:-0} )); then return 1; fi
    done
    return 1
}

# Compare a restored binary against the latest published release; print a
# prominent notice when the restored version is older. Offline / no curl /
# unparseable output degrade to silence (same spirit as the daemon's hourly
# check — never an error).
check_stale_restore() {
    local bin="$1" version tag
    [ -x "$bin" ] || return 0
    version="$("$bin" --version 2>/dev/null | awk '{print $2}')"
    [[ "$version" =~ ^[0-9]+\.[0-9]+\.[0-9]+$ ]] || return 0
    command -v curl >/dev/null 2>&1 || return 0
    tag="$(curl -fsS --max-time 5 "https://api.github.com/repos/dominikj111/hover-clock/releases/latest" 2>/dev/null \
        | sed -n 's/.*"tag_name"[[:space:]]*:[[:space:]]*"v\([0-9.]*\)".*/\1/p' | head -1)"
    [[ "$tag" =~ ^[0-9]+\.[0-9]+\.[0-9]+$ ]] || return 0
    if version_gt "$tag" "$version"; then
        echo
        echo "!! Production binary restored at v$version, but v$tag is the latest release"
        echo "   (published while dev mode was active — the stash predates it)."
        echo "   Bring production to v$tag with:  ./scripts/upgrade.sh"
        echo "   (or click the orange update button in the overlay)."
        echo
    fi
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
    # A dev session started without swap-to-dev leaves a running dev process
    # and no state to restore — say how to recover instead of leaving a dead
    # service and a stray process.
    installed="$(readlink -f "${HOVERCLOCK_BIN_DIR:-$HOME/.local/bin}/hover-clock" 2>/dev/null || true)"
    dev_pids=""
    for pid in $(pgrep -x hover-clock 2>/dev/null || true); do
        exe="$(readlink -f "/proc/$pid/exe" 2>/dev/null || true)"
        if [ -n "$exe" ] && [ "$exe" != "$installed" ]; then
            dev_pids="$dev_pids $pid"
        fi
    done
    if [ -n "$dev_pids" ]; then
        echo "   NOTE: dev instance(s) running without swap state (started outside swap-to-dev.sh):"
        for pid in $dev_pids; do
            echo "     PID $pid: $(tr '\0' ' ' < "/proc/$pid/cmdline" 2>/dev/null)"
        done
        echo "   Stop them with 'pkill -x hover-clock', then start the installed daemon with"
        echo "   'systemctl --user start hover-clock'."
    fi
    echo "Install production with ./scripts/install.sh or download the latest"
    echo "GitHub release tarball, then swap-to-dev / swap-to-prod as usual."
    exit 1
fi

echo "==> Stopping dev instances"
pkill -x hover-clock 2>/dev/null || true

service=0
autostart=0
restored=0
restored_origs=()
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
                    restored_origs+=("$orig")
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
    # Clear any accumulated restart counter from a dev-mode loop (unit
    # started while the binary was stashed: Restart=on-failure + missing
    # ExecStart would otherwise have been retrying for hours).
    systemctl --user reset-failed hover-clock.service 2>/dev/null || true
    echo "==> Starting the daemon"
    systemctl --user enable hover-clock.service
    # `restart`, not `enable --now`: --now only starts a *stopped* unit —
    # the daemon must run the restored (production) binary, not a stale
    # process mapped from before the stash.
    systemctl --user restart hover-clock.service
elif [ "$autostart" = "1" ]; then
    echo "==> Autostart entry restored — the daemon starts at next login"
else
    echo "==> No daemon registration found; run ./scripts/install.sh to register"
fi

log "swap-to-prod: restored $restored production binary(ies) [service=$service autostart=$autostart]"
if [ "${#restored_origs[@]}" -gt 0 ]; then
    check_stale_restore "${restored_origs[0]}"
fi
echo "Back to production."
