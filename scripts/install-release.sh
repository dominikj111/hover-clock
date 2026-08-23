#!/bin/sh
# HoverClock release installer — the `curl | sh` entry point.
#
# Downloads the binary matching this machine's architecture (x86_64 or
# aarch64) from the latest GitHub release (override with --version),
# verifies its SHA-256 checksum, installs it to ~/.local/bin, and
# registers the daemon:
#   systemd     -> user service (~/.config/systemd/user), auto-start at
#                  login, restart on crash
#   non-systemd -> XDG autostart entry (~/.config/autostart) plus a note
#                  that crash-restart is not available — systemd is the
#                  primary supported path (see README)
# No root needed. Works on X11 and Wayland (layer-shell) sessions alike.
# POSIX sh (dash-compatible), so plain `curl | sh` works everywhere.
#
# Usage:
#   curl -fsSL https://raw.githubusercontent.com/dominikj111/hover-clock/main/scripts/install-release.sh | sh
#   # with options:
#   curl -fsSL https://raw.githubusercontent.com/dominikj111/hover-clock/main/scripts/install-release.sh | sh -s -- --version v2.0.0
#
# Options:
#   --version X.Y.Z    install a specific release (default: latest)
#   --bin-dir DIR      binary directory (default: ~/.local/bin)
#   --no-service       install the binary only — no daemon registration
#   --yes              install missing system packages without prompting
#   --help             this help
#
# Runtime libraries the release binary links (checked before download):
#   libgtk-4.so.1            (Debian/Pi OS: libgtk-4-1)
#   libgtk4-layer-shell.so.0 (Debian/Pi OS: libgtk4-layer-shell-0)
#   Debian 12 / Pi OS bookworm has no layer-shell package — build from
#   source with `cargo build --no-default-features` there (README).

set -eu

REPO="dominikj111/hover-clock"
RAW_BASE="https://raw.githubusercontent.com/$REPO"
API_BASE="https://api.github.com/repos/$REPO/releases"
DL_BASE="https://github.com/$REPO/releases/download"
ISSUES_URL="https://github.com/$REPO/issues"
STATE_DIR="${XDG_STATE_HOME:-$HOME/.local/state}/hover-clock"
LOG_FILE="$STATE_DIR/install.log"

VERSION=""
BIN_DIR="${HOVERCLOCK_BIN_DIR:-$HOME/.local/bin}"
DO_SERVICE=1
ASSUME_YES=0

usage() {
    cat <<'EOF'
HoverClock release installer.

Usage:
  curl -fsSL https://raw.githubusercontent.com/dominikj111/hover-clock/main/scripts/install-release.sh | sh -s -- [options]

Options:
  --version X.Y.Z   Install a specific release (default: latest)
  --bin-dir DIR     Binary directory (default: ~/.local/bin)
  --no-service      Install the binary only — no daemon registration
  --yes             Install missing system packages without prompting
  --help            Show this help

Requires: curl, tar, sha256sum. No root needed.
EOF
}

log() {
    mkdir -p "$STATE_DIR"
    printf '%s %s\n' "$(date '+%Y-%m-%d %H:%M:%S')" "$*" >>"$LOG_FILE"
}

# --- serialize against install.sh / upgrade.sh / swap / uninstall ----------
# Same lock file as the other lifecycle scripts: install/upgrade/swap/
# uninstall racing on the binary or the swap state would corrupt both.
if command -v flock >/dev/null 2>&1; then
    mkdir -p "$STATE_DIR"
    exec 9>"$STATE_DIR/lock"
    if ! flock -n 9; then
        echo "!! Another hover-clock script is running (install/upgrade/swap/uninstall)." >&2
        echo "   Wait for it to finish, then retry." >&2
        exit 1
    fi
fi

# --- refuse to install over an active dev instance --------------------------
if [ -f "$STATE_DIR/state" ]; then
    echo "!! Dev mode is active: the production binary is stashed by swap-to-dev.sh." >&2
    echo "   From the repository run ./scripts/swap-to-prod.sh, then re-run this installer." >&2
    exit 1
fi
installed="$(readlink -f "$BIN_DIR/hover-clock" 2>/dev/null || echo "$BIN_DIR/hover-clock")"
for pid in $(pgrep -x hover-clock 2>/dev/null || true); do
    exe="$(readlink -f "/proc/$pid/exe" 2>/dev/null || true)"
    if [ -n "$exe" ] && [ "$exe" != "$installed" ]; then
        echo "!! A dev instance of hover-clock is running (PID $pid — not the installed daemon)." >&2
        echo "   Stop it first (Ctrl+C in the terminal that started it, or 'pkill -x hover-clock')," >&2
        echo "   then re-run this installer." >&2
        exit 1
    fi
done

# --- args --------------------------------------------------------------------
while [ $# -gt 0 ]; do
    case "$1" in
        --version)
            [ $# -ge 2 ] || { echo "!! --version needs a value (e.g. 2.0.0)" >&2; exit 1; }
            VERSION="${2#v}"; shift 2 ;;
        --version=*) VERSION="${1#*=}"; VERSION="${VERSION#v}"; shift ;;
        --bin-dir)
            [ $# -ge 2 ] || { echo "!! --bin-dir needs a value" >&2; exit 1; }
            BIN_DIR="$2"; shift 2 ;;
        --bin-dir=*) BIN_DIR="${1#*=}"; shift ;;
        --no-service) DO_SERVICE=0; shift ;;
        --yes|-y) ASSUME_YES=1; shift ;;
        --help|-h) usage; exit 0 ;;
        *)
            echo "!! Unknown option: $1" >&2
            usage >&2
            exit 1
            ;;
    esac
done
case "$VERSION" in
    *[!0-9.]*) echo "!! Invalid version: $VERSION (expected e.g. 2.0.0)" >&2; exit 1 ;;
esac

# --- architecture ----------------------------------------------------------------
ARCH="$(uname -m)"
case "$ARCH" in
    x86_64 | amd64) ARCH=x86_64 ;;
    aarch64 | arm64) ARCH=aarch64 ;;
    *)
        echo "!! Unsupported architecture: $ARCH (release binaries are x86_64 and aarch64)." >&2
        echo "   Build from source instead: git clone https://github.com/$REPO && ./scripts/install.sh" >&2
        exit 1
        ;;
esac

# --- runtime libraries --------------------------------------------------------------
LDCONFIG="$(command -v ldconfig 2>/dev/null || echo /sbin/ldconfig)"
missing=""
if [ ! -x "$LDCONFIG" ]; then
    echo "!! ldconfig not found — cannot verify the GTK4 runtime libraries." >&2
    echo "   Install the GTK4 runtime for your distribution and re-run." >&2
    exit 1
fi
if ! "$LDCONFIG" -p 2>/dev/null | grep -q 'libgtk-4\.so\.1'; then
    missing="libgtk-4-1 (GTK4 runtime)"
fi
if ! "$LDCONFIG" -p 2>/dev/null | grep -q 'libgtk4-layer-shell\.so\.0'; then
    missing="${missing:+$missing }libgtk4-layer-shell-0 (layer-shell runtime)"
fi

install_deps() {
    if command -v apt-get >/dev/null 2>&1; then
        if ! sudo apt-get update -qq; then
            echo "!! 'sudo apt-get update' failed." >&2
            exit 1
        fi
        if ! sudo apt-get install -y libgtk-4-1 libgtk4-layer-shell-0; then
            echo "!! apt could not install the runtime libraries." >&2
            echo "   On Debian 12 / Pi OS bookworm the layer-shell library is not packaged —" >&2
            echo "   build from source with 'cargo build --no-default-features' instead (README Requirements)." >&2
            exit 1
        fi
    elif command -v dnf >/dev/null 2>&1; then
        sudo dnf install -y gtk4 gtk4-layer-shell
    elif command -v pacman >/dev/null 2>&1; then
        sudo pacman -S --noconfirm gtk4 gtk4-layer-shell
    else
        echo "!! No supported package manager found." >&2
        echo "   Install the GTK4 runtime and the gtk4-layer-shell shared library for your" >&2
        echo "   distribution, then re-run this installer." >&2
        exit 1
    fi
}

if [ -n "$missing" ]; then
    echo "Missing runtime libraries: $missing"
    if [ "$ASSUME_YES" -eq 1 ]; then
        install_deps
    elif [ -t 0 ] && command -v sudo >/dev/null 2>&1; then
        printf 'Install the missing packages with sudo? [y/N] '
        read ans
        case "$ans" in
            y | Y | yes) install_deps ;;
            *) echo "Install the packages manually, then re-run this installer." >&2; exit 1 ;;
        esac
    else
        echo "Install them manually, then re-run this installer. On Debian / Pi OS trixie:" >&2
        echo "  sudo apt-get install -y libgtk-4-1 libgtk4-layer-shell-0" >&2
        exit 1
    fi
    # Re-verify after installing (package names can differ across distros).
    if ! "$LDCONFIG" -p 2>/dev/null | grep -q 'libgtk-4\.so\.1' || \
       ! "$LDCONFIG" -p 2>/dev/null | grep -q 'libgtk4-layer-shell\.so\.0'; then
        echo "!! Libraries still not found after install — the package names may differ on your" >&2
        echo "   distribution; install the gtk4 and gtk4-layer-shell runtimes manually." >&2
        exit 1
    fi
fi

# --- resolve version ----------------------------------------------------------------
if [ -z "$VERSION" ]; then
    echo "==> Resolving the latest release"
    VERSION="$(curl -fsSL "$API_BASE/latest" | grep -o '"tag_name": *"[^"]*"' | head -n1 | cut -d'"' -f4)"
    VERSION="${VERSION#v}"
    if [ -z "$VERSION" ]; then
        echo "!! Could not determine the latest release from GitHub (network or API issue)." >&2
        echo "   Retry, or pin a version explicitly: ... | sh -s -- --version v2.0.0" >&2
        exit 1
    fi
fi
echo "==> HoverClock v$VERSION ($ARCH)"

# --- download + verify ---------------------------------------------------------------
NAME="hover-clock-v$VERSION-$ARCH"
TARBALL_URL="$DL_BASE/v$VERSION/$NAME.tar.gz"
TMP="$(mktemp -d)"
trap 'rm -rf "$TMP"' EXIT INT TERM

echo "==> Downloading $TARBALL_URL"
curl -fsSL "$TARBALL_URL" -o "$TMP/$NAME.tar.gz"
curl -fsSL "$TARBALL_URL.sha256" -o "$TMP/$NAME.tar.gz.sha256"

expected="$(awk '{print $1}' "$TMP/$NAME.tar.gz.sha256")"
actual="$(sha256sum "$TMP/$NAME.tar.gz" | awk '{print $1}')"
if [ -z "$expected" ] || [ "$expected" != "$actual" ]; then
    echo "!! Checksum mismatch — the download is corrupt or tampered with." >&2
    exit 1
fi
echo "==> Checksum OK"

tar -xzf "$TMP/$NAME.tar.gz" -C "$TMP"

# --- install binary ------------------------------------------------------------------
echo "==> Installing to $BIN_DIR"
mkdir -p "$BIN_DIR"
install -m 755 "$TMP/hover-clock" "$BIN_DIR/hover-clock"

# --- register daemon -------------------------------------------------------------------
MECH=none
if [ "$DO_SERVICE" -eq 1 ]; then
    if [ "$(ps -p 1 -o comm= 2>/dev/null)" = "systemd" ]; then
        MECH=systemd
        UNIT_DIR="$HOME/.config/systemd/user"
        mkdir -p "$UNIT_DIR"
        # Unit comes from the same release tag as the binary (single source of
        # truth: packaging/hover-clock.service); embedded copy as fallback.
        if ! curl -fsSL "$RAW_BASE/v$VERSION/packaging/hover-clock.service" -o "$TMP/hover-clock.service" 2>/dev/null; then
            cat >"$TMP/hover-clock.service" <<'EOF'
[Unit]
Description=HoverClock — transient overlay daemon (clock on demand)
After=default.target graphical-session.target
PartOf=graphical-session.target
StartLimitIntervalSec=60
StartLimitBurst=5

[Service]
Type=simple
ExecStart=%h/.local/bin/hover-clock --start
Restart=on-failure
RestartSec=3
# Only needed if DISPLAY/XAUTHORITY/WAYLAND_DISPLAY are not in the user
# manager environment at login (console login + manual startx, or a
# native Wayland session whose env was not imported):
# Environment=DISPLAY=:0 XAUTHORITY=%h/.Xauthority WAYLAND_DISPLAY=wayland-0

[Install]
WantedBy=default.target graphical-session.target
EOF
        fi
        sed "s|%h/.local/bin|$BIN_DIR|" "$TMP/hover-clock.service" >"$UNIT_DIR/hover-clock.service"
        systemctl --user daemon-reload
        systemctl --user import-environment DISPLAY XAUTHORITY WAYLAND_DISPLAY 2>/dev/null || true
        systemctl --user enable hover-clock.service
        # `restart`, not `enable --now`: --now only starts a *stopped* unit —
        # on a reinstall the old daemon would keep running the replaced binary.
        systemctl --user restart hover-clock.service
    else
        MECH=xdg-autostart
        AUTOSTART_DIR="$HOME/.config/autostart"
        mkdir -p "$AUTOSTART_DIR"
        if ! curl -fsSL "$RAW_BASE/v$VERSION/packaging/hover-clock-autostart.desktop" -o "$TMP/hover-clock.desktop" 2>/dev/null; then
            cat >"$TMP/hover-clock.desktop" <<'EOF'
[Desktop Entry]
Type=Application
Name=HoverClock
Comment=Transient overlay daemon — clock on demand (hot corner / Super+T)
Exec=__BIN_DIR__/hover-clock --start
Terminal=false
EOF
        fi
        sed "s|__BIN_DIR__|$BIN_DIR|" "$TMP/hover-clock.desktop" >"$AUTOSTART_DIR/hover-clock.desktop"
    fi
fi

log "install-release v$VERSION ($ARCH) -> $BIN_DIR/hover-clock [$MECH]"

echo
echo "HoverClock v$VERSION installed:"
echo "  binary   $BIN_DIR/hover-clock"
case "$MECH" in
    systemd)
        echo "  service  systemd user unit — auto-start at login, restart on crash"
        echo "  status   systemctl --user status hover-clock"
        ;;
    xdg-autostart)
        echo "  service  XDG autostart entry — starts at next login (no crash-restart)"
        echo "           systemd is the primary supported path; if your init needs"
        echo "           proper support, file an issue: $ISSUES_URL"
        echo "  run now  $BIN_DIR/hover-clock"
        ;;
    none)
        echo "  service  not registered (--no-service)"
        ;;
esac
echo "  test     $BIN_DIR/hover-clock           # client: shows the overlay"
echo "  update   re-run this installer (always installs the latest release)"
echo "  remove   curl -fsSL https://raw.githubusercontent.com/$REPO/main/scripts/uninstall.sh | sh"
echo
echo "On Wayland, bind the global shortcuts in your compositor config (labwc rc.xml,"
echo "sway, Hyprland) — see the README 'Wayland status' section."
echo "Audit log: $LOG_FILE"
