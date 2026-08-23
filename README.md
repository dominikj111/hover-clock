# HoverClock

[![CI](https://github.com/dominikj111/hover-clock/actions/workflows/ci.yml/badge.svg)](https://github.com/dominikj111/hover-clock/actions/workflows/ci.yml)
[![MSRV](https://img.shields.io/badge/MSRV-1.92-purple)](https://github.com/dominikj111/hover-clock/blob/main/Cargo.toml)
[![GTK](https://img.shields.io/badge/GTK-4.0%2B-green)](https://github.com/dominikj111/hover-clock/blob/main/Cargo.toml)
[![Platform](https://img.shields.io/badge/platform-Linux%20(X11%20%2B%20Wayland)-blue)](#tested-environments)
[![Wayland](https://img.shields.io/badge/Wayland-native-green)](#wayland-status)
[![License](https://img.shields.io/badge/License-BSD--3--Clause-blue.svg)](LICENSE)

A transient Linux overlay daemon that surfaces widgets on demand — starting with a digital
clock — via hot-corner or global shortcut, above fullscreen applications, without ever
taking focus.

- **Non-focus-stealing** — invisible to task switchers, never takes input focus
- **Above fullscreen apps** — X11: EWMH overlay semantics (`NOTIFICATION` type, `ABOVE`, `SKIP_TASKBAR`/`SKIP_PAGER`); Wayland: layer-shell OVERLAY layer
- **Event-driven, no polling** — XI2 pointer motion (X11) / top-edge sensor strips (Wayland), passive key grabs, edge-triggered and
  debounced triggers
- **Single GTK4 binary** — offline-first, minimal footprint (< 25 MB, < 0.1% idle CPU
  targets)
- **X11 + native Wayland** — layer-shell backend behind the same trait facades

## Status

**v2.0.0** (current release) — **M7 (Wayland) delivered and merged to main**: the native
layer-shell backend works on X11 and Wayland from the same binary, hence the major bump.
Next milestone: **M4 (calendar widget)**. See [`roadmap/ROADMAP.md`](roadmap/ROADMAP.md) for
the current story and hand-offs, and [`docs/proposal.md`](docs/proposal.md) §14 for the full
milestone plan.

## Install

**Recommended — one command, no toolchain:** downloads the release binary for your
architecture (x86_64 / aarch64), verifies its SHA-256 checksum, installs it to
`~/.local/bin`, and registers the daemon (systemd user service — auto-start at login,
restart on crash; XDG autostart fallback on non-systemd systems):

```bash
curl -fsSL https://raw.githubusercontent.com/dominikj111/hover-clock/main/scripts/install-release.sh | sh
```

The script first checks the GTK4 runtime libraries (`libgtk-4-1` + `libgtk4-layer-shell-0`
on Debian / Raspberry Pi OS) and installs — or clearly reports — what is missing; it never
needs root. Options: `--version X.Y.Z`, `--bin-dir DIR`, `--no-service`, `--yes`; see
[`docs/DEPLOYMENT.md`](docs/DEPLOYMENT.md) for details.

| Path | When to use |
| --- | --- |
| **`curl \| sh`** (above) | End users — binary from GitHub Releases, daemon registered, no Rust toolchain needed |
| **Release tarball** | Manual install — extract `hover-clock-vX.Y.Z-<arch>.tar.gz` into `~/.local/bin` and run `hover-clock --start` |
| **Build from source** | Development, or when no release binary fits (other arch, distro without the layer-shell library) — clone, `cargo build`, then `./scripts/install.sh` (or `just install`) |
| **`cargo install`** | Not offered yet — see note below |

> **Why no `cargo install`?** It would need a crates.io publication, and — unlike the curl
> installer — it cannot register the session daemon or ensure the system GTK4 libraries.
> The release-tarball path covers the same ground with less maintenance while the project
> has 0 confirmed users; when someone asks for it, publishing is a small step.

## Requirements

- Linux with an **X11 session, or a Wayland session on a layer-shell compositor**
  (wlroots family — labwc, sway, Hyprland — or KWin ≥ 5.27; see [Wayland status](#wayland-status))
- **Rust ≥ 1.92** (stable; edition 2024)
- **GTK4 development libraries** — install per distribution, then verify:

| Distribution | Command |
| --- | --- |
| Debian 13 / Raspberry Pi OS (trixie) | `sudo apt install -y libgtk-4-dev libgtk4-layer-shell-dev pkg-config` |
| Debian 12 / Pi OS bookworm (no layer-shell pkg) | `sudo apt install -y libgtk-4-dev pkg-config && cargo build --no-default-features` |
| Fedora | `sudo dnf install -y gtk4-devel pkgconf-pkg-config` |
| Arch | `sudo pacman -S gtk4 pkg-config` |

> The `wayland` cargo feature (default-on) links the system `libgtk4-layer-shell`;
> `--no-default-features` drops it for X11-only builds.

Verify with `pkg-config --modversion gtk4` before building.

> **Build fails with “glib-2.0.pc was not found” or “Package gtk4 was not found”?** The GTK4
> dev libraries are missing — run the install command for your distribution above, then
> retry.
>
> **GTK version floor:** the crate enables no version-gated feature and uses only base GTK4
> APIs, so it builds against any GTK 4.x — tested on GTK 4.18 (Debian 13) and CI
> (Ubuntu 24.04+). Debian 12 / Raspberry Pi OS bookworm's GTK 4.8 is expected to work
> (untested). See `docs/proposal.md` §17.1.

## Build & run

```bash
cargo build
cargo run -- --start         # start the daemon (single instance; one terminal)
cargo run                   # client — tell the daemon to show the overlay
```

The daemon starts with the overlay hidden. Triggers:

| Action | Result |
| --- | --- |
| Move the mouse to a screen's top-right corner (dwell ~200 ms) | Overlay appears |
| `Super + T` | Toggle overlay |
| `Esc` | Hide overlay |
| Leave the corner | Auto-hide (debounced) |
| `hover-clock` (any terminal) | Show the overlay — same as a corner dwell |

The overlay never appears in the taskbar, never shows in Alt-Tab, and never takes focus.

A small **version label** sits at the bottom of the clock: the running binary's version in
shadow-grey, turning orange — `v2.0.0 → v2.0.1` — when a newer release exists on GitHub.
The check queries the GitHub Releases API hourly (worker thread, bounded timeouts; offline or
failed checks leave the label in its current colour, never an error).

### Command-line

The CLI is the primary surface: one binary, two roles, speaking over a Unix control socket
(`$XDG_RUNTIME_DIR/hoverclock.sock`). The daemon is a **single instance** — a second
`--start` while one is live exits with an explanatory error, never coexists silently.
Socket-driven commands (`show`/`hide`/`toggle`) land over the socket; the transport later
extends to TCP/IP for Windows portability (`docs/proposal.md` §7.4).

| Invocation | Effect |
| --- | --- |
| `hover-clock --start` (alias `-s`) | Start the daemon (single instance) |
| `hover-clock` | Client — show the overlay (default command) |
| `hover-clock show` | Same as above, explicit |
| `hover-clock hide` / `hover-clock toggle` | Client — hide / toggle the overlay |
| `hover-clock --stop` / `hover-clock stop` | Stop the running daemon — clean exit, control socket released |
| `hover-clock --restart` / `hover-clock restart` | Restart the running daemon **in place** (same process id — systemd/autostart keep tracking it) |

## Lifecycle scripts (source installs)

The overlay is designed to run as a **session daemon**: start automatically at login,
restart on crash, upgrade in place without touching the desktop session. The full lifecycle
— install, upgrade, dev/prod swap, publishing, troubleshooting — is documented in
[`docs/DEPLOYMENT.md`](docs/DEPLOYMENT.md); the quick reference is below. If [`just`](https://github.com/casey/just)
is installed, the common operations are one word: `just install`, `just install-release`,
`just swap-to-dev`, `just check`, `just deploy 2.0.0` (run `just` to list all recipes).

| Command | Effect |
| --- | --- |
| `./scripts/install-release.sh` | Download the latest release binary (no build) — install + register the daemon; the `curl \| sh` path |
| `./scripts/install.sh` | Build release, install to `~/.local/bin`, register + start the daemon (mechanism per init, below) |
| `./scripts/upgrade.sh [branch]` | Pull latest, rebuild, restart the running daemon seamlessly (the overlay is transient — a restart between dwells is imperceptible) |
| `./scripts/swap-to-dev.sh` | Stop the daemon, **stash the installed binary aside** (unlink), then `cargo run` from source |
| `./scripts/swap-to-prod.sh` | **Relink** the stashed binary back and restart the daemon — instant, offline, no rebuild/re-download |
| `./scripts/uninstall.sh` | Permanently remove: service/autostart entry, binaries, and swap stash |

Production installs are **unlinked, never deleted** when you switch to dev, and relinked on
return — the swap is instant and offline. The scripts **discover** the production binary
rather than presuming a path: `~/.local/bin` (install.sh or a GitHub release tarball),
`$CARGO_HOME/bin` (cargo install), and every directory on `PATH` are scanned, and the
binary is restored to its exact original path. Restoring **never clobbers**: if a newer
binary appeared at the original path while you were in dev mode (re-install,
`cargo install --force`, a fresh release tarball), the existing one is kept and the stale
stash is discarded with a notice.

> **cargo install note:** while the binary is stashed (dev mode), `cargo uninstall
> hover-clock` cannot find it — swap back to production
> (`./scripts/swap-to-prod.sh`) first.

Getting the *newest* version is `./scripts/upgrade.sh` while in production mode,
re-running the curl installer, or downloading the latest GitHub release (see
[Releases](#releases)).

## Releases

Versioning is semver, with `Cargo.toml` as the single source of truth — **v2.0.0** is
current (major bump: native Wayland support, M7, merged to main). Publishing is
**main-only**: create a tag `vX.Y.Z` on `main` (matching the `Cargo.toml` version), push it,
and the release pipeline — guarded so tags pointing elsewhere or with a version mismatch are
rejected — builds release binaries for **x86_64** and **aarch64** (Raspberry Pi) and uploads
tarballs + SHA-256 checksums to the GitHub release page for that tag. Install from a release
with the curl installer above, by extracting a tarball into `~/.local/bin`, or via
`./scripts/install.sh` (build from source).

Manual alternatives: `cargo install --path .` (installs to `~/.cargo/bin`) or download the
release tarball from GitHub Releases and run the binary directly.

> **Dependencies:** the binary links dynamically to system GTK4 (≥ 4.12) — release
> tarballs do not bundle it. Install `libgtk-4-1` (Debian/Raspberry Pi OS) or `gtk4`
> (Fedora) first. The footprint stays small because GTK4 is shared with the desktop.

### Init systems

The daemon is a *session* process (it needs the X session, never root), so it hooks the
desktop session, not the init:

- **systemd** (Debian/Fedora/MX with systemd boot) — `install.sh` installs a systemd
  *user* service (`~/.config/systemd/user/hover-clock.service`): auto-start at login,
  restart on crash, clean stop/start. The unit is managed directly with
  `systemctl --user stop|start|restart hover-clock` (e.g. `systemctl --user restart
  hover-clock`) — **valid only on systemd installs**, where the user unit exists.
- **sysvinit / OpenRC / runit** (MX with sysvinit boot, Devuan, antiX, Alpine, Void,
  Gentoo) — `install.sh` detects a non-systemd init and installs an **XDG autostart**
  entry (`~/.config/autostart/hover-clock.desktop`), honored by Xfce/GNOME/KDE sessions
  regardless of init. Trade-off: no crash-restart. There is no unit, so
  `systemctl --user restart hover-clock` **does not work** here — stop/restart a running
  daemon with `hover-clock --stop` / `hover-clock --restart` instead, which are
  init-independent (they drive the daemon over its control socket, so they work on
  every install — including systemd).

### Logs

- **Script audit trail** — every `install.sh` / `upgrade.sh` / swap / uninstall run appends
  a timestamped line (action, version, git sha, outcome) to
  `~/.local/state/hover-clock/install.log` (removed by uninstall).
- **Daemon runtime output** — `journalctl --user -u hover-clock --no-pager -n 50` (or
  `-b` for the last boot).

### Dev workflow

`cargo run` is the **client** — it sends `show` to the running daemon and never touches X
grabs, so dev runs and the installed daemon cannot fight:

```bash
./scripts/swap-to-dev.sh                # stop daemon, stash binary, run from source (daemon)
cargo run                              # client: show the overlay
./scripts/swap-to-dev.sh -- --help      # any args pass through to the binary
./scripts/swap-to-prod.sh              # relink binary, daemon back up
```

During dev the daemon registration is hidden (service disabled, autostart entry moved
aside), so a login while in dev mode cannot start a daemon with a missing binary.
`./scripts/uninstall.sh` removes the production install completely — the source tree stays
untouched, so the repo is always ready for `cargo run`.

## Tested environments

| Environment | Status |
| --- | --- |
| Debian GNU/Linux 13 (trixie), Xfce 4.20 (xfce4-session 4.20.2), systemd, X11 (libgtk-4-1 4.18.6+ds-2) | ✅ |
| MX Linux (trixie-based), Xfce 4.20, systemd boot, X11 | ✅ |
| Raspberry Pi 4 Model B Rev 1.4 (aarch64), trixie-based Pi OS, PIXEL desktop, systemd | ✅ X11 (xfwm4) · ✅ Wayland (labwc 0.9.8) — native layer-shell overlay + hot corner verified (M7, handoff 07); Super+T/Esc via labwc rc.xml keybinds → `hover-clock` client (compositor-native); caveats in [Wayland status](#wayland-status) |

CI additionally builds and lints on `ubuntu-latest` with the stable toolchain and the
declared MSRV (1.92). The compatibility record for other distributions, window managers
(GNOME, KDE, i3), and macOS/Windows ports lives in `docs/proposal.md` §17.

## Wayland status

**Native layer-shell (M7) is live** on compositors with `zwlr_layer_shell_v1`
(wlroots family and KWin ≥ 5.27): the overlay runs in the OVERLAY layer (stacking above
fullscreen by construction), placement is anchor + margins, and the hot corner is a set
of a thin solid-dark 2 px top-edge sensor strip at the monitor's true top edge (an exclusive
zone beats wlroots' free-area placement — the corner is the top 2 px, over the bar, parity
with the X11 gesture; it is visible because this compositor composites layer surfaces
opaque, see docs/wayland-layer-shell-findings.md §3). Verified on labwc 0.9.8 (handoff 07).

**No multi-desktop (workspace) dependency.** Layer-shell surfaces are not workspace-bound —
a workspace switch never touches the overlay (by construction; the X11 xfwm4 re-map
flicker disappears). The missing multi-desktop is a **Raspberry Pi OS characteristic** — its
labwc session ships without workspaces — not a Wayland or labwc limitation (labwc supports
workspaces on other systems). Verified on the two extremes: Pi OS (no workspaces at all) and
MX Linux / Xfce (workspaces) — the same build behaves identically on both.

Limits:
- **Global shortcuts are compositor-configured on Wayland** — no app-side global-shortcut
  API exists (`ext_global_shortcuts_v1` unmerged; portal without wlroots backend; XWayland
  grabs only see X-focused apps). Bind them in the compositor config to the client:
  labwc rc.xml `W-T`→`hover-clock toggle`, `Escape`→`hover-clock hide` (see
  `docs/WAYLAND_TESTING.md`); sway/Hyprland equivalents. Verified on labwc 0.9.8.
- **GNOME/Mutter does not implement layer-shell** — the Xorg session is the supported
  path there (XWayland fallback keeps running, stacking degraded).
- **Sensor strip captures its 2 px band** — clicks in the very top 2 px do not pass
  through (§16 trade-off; X11 keeps the same passive corner semantics).

See [`docs/WAYLAND_TESTING.md`](docs/WAYLAND_TESTING.md) for the labwc verification
surface and [`docs/proposal.md`](docs/proposal.md) §17.3.

## Third-party examples

- **GNOME Shell hot corner** — corner-triggered overview in the GNOME desktop
- **KDE Plasma screen edges** — edge/corner triggers for desktop actions
- **xfce4-hotcorner-plugin** — hot-corner actions for Xfce
- **Conky** — persistent X11 desktop overlay widgets (non-transient counterpoint)

## Contributing

- [`CONTRIBUTING.md`](CONTRIBUTING.md) — workflow, validation, and conventions
- `docs/proposal.md` — design contract; read the § cited on the story card before each task
- `roadmap/ROADMAP.md` — current story, status, and hand-offs
- `AGENTS.md` — operating file for AI contributors

## License

[BSD 3-Clause](LICENSE) — free to use, modify, and distribute (including commercially),
provided the copyright notice (© 2026, dominikj111) and this license text are retained in
redistributions of the source and reproduced in distributed binaries — so a project that
uses HoverClock is asked to keep the attribution. See [`CONTRIBUTING.md`](CONTRIBUTING.md)
for the contribution terms.
