# HoverClock

[![CI](https://github.com/dominikj111/hover-clock/actions/workflows/ci.yml/badge.svg)](https://github.com/dominikj111/hover-clock/actions/workflows/ci.yml)
[![MSRV](https://img.shields.io/badge/MSRV-1.92-purple)](https://github.com/dominikj111/hover-clock/blob/main/Cargo.toml)
[![GTK](https://img.shields.io/badge/GTK-%3E%3D4.12-green)](https://github.com/dominikj111/hover-clock/blob/main/Cargo.toml)
[![Platform](https://img.shields.io/badge/platform-Linux%20(X11)-blue)](#tested-environments)
[![Wayland](https://img.shields.io/badge/Wayland-planned-yellow)](#wayland-status)
[![License](https://img.shields.io/badge/License-BSD--3--Clause-blue.svg)](LICENSE)

A transient Linux overlay daemon that surfaces widgets on demand — starting with a digital
clock — via hot-corner or global shortcut, above fullscreen applications, without ever
taking focus.

- **Non-focus-stealing** — invisible to task switchers, never takes input focus
- **Above fullscreen apps** — EWMH overlay semantics (`NOTIFICATION` type, `ABOVE`,
  `SKIP_TASKBAR`/`SKIP_PAGER`)
- **Event-driven, no polling** — XI2 pointer motion, passive key grabs, edge-triggered and
  debounced triggers
- **Single GTK4 binary** — offline-first, minimal footprint (< 25 MB, < 0.1% idle CPU
  targets)
- **X11 first, Wayland planned** — layer-shell backend behind the same trait facades

## Status

Current milestone **M3 (presentation)** — the clock widget (time/day/date, CSS-styled,
transient) is in progress. See [`roadmap/ROADMAP.md`](roadmap/ROADMAP.md) for the current
story and hand-offs, and [`docs/proposal.md`](docs/proposal.md) §14 for the full milestone
plan.

## Requirements

- Linux with an **X11 session** (Wayland planned — see [Wayland status](#wayland-status))
- **GTK4 ≥ 4.12** development libraries (`libgtk-4-dev` on Debian/Raspberry Pi OS,
  `gtk4-devel` on Fedora) + `pkg-config`
- **Rust ≥ 1.92** (stable; edition 2024)

> Debian 12 / Raspberry Pi OS bookworm ship GTK 4.8 — drop the `v4_12` cargo feature there
> (see `docs/proposal.md` §17.1).

## Build & run

```bash
cargo build
cargo run
```

The daemon starts with the overlay hidden. Triggers:

| Action | Result |
| --- | --- |
| Move the mouse to a screen's top-right corner (dwell ~200 ms) | Overlay appears |
| `Super + T` | Toggle overlay |
| `Esc` | Hide overlay |
| Leave the corner | Auto-hide (debounced) |

The overlay never appears in the taskbar, never shows in Alt-Tab, and never takes focus.

## Install as a daemon

The overlay is designed to run as a **session daemon**: start automatically at login,
restart on crash, upgrade in place without touching the desktop session.

| Command | Effect |
| --- | --- |
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

Getting the *newest* version is `./scripts/upgrade.sh` while in production mode, or
downloading the latest GitHub release (see [Releases](#releases)).

## Releases

Versioning is semver (`0.x` for now), with `Cargo.toml` as the single source of truth.
Publishing is **main-only**: create a tag `vX.Y.Z` on `main` (matching the `Cargo.toml`
version), push it, and the release pipeline — guarded so tags pointing elsewhere or with a
version mismatch are rejected — builds release binaries for **x86_64** and **aarch64**
(Raspberry Pi) and uploads tarballs + SHA-256 checksums to the GitHub release page for that
tag. Extract a tarball into `~/.local/bin` (or run `./scripts/install.sh` to build from
source).

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
  restart on crash, clean stop/start.
- **sysvinit / OpenRC / runit** (MX with sysvinit boot, Devuan, antiX, Alpine, Void,
  Gentoo) — `install.sh` detects a non-systemd init and installs an **XDG autostart**
  entry (`~/.config/autostart/hover-clock.desktop`), honored by Xfce/GNOME/KDE sessions
  regardless of init. Trade-offs: no crash-restart, and upgrading a *running* daemon
  needs a session restart until the socket control plane (M5) lands.

### Dev workflow

`cargo run` starts a second instance, which would fight the daemon over the same X grabs
(duplicate overlays). The swap scripts keep daemon and dev mode cleanly separated without
ever losing the production install:

```bash
./scripts/swap-to-dev.sh                # stop daemon, stash binary, cargo run
./scripts/swap-to-dev.sh -- --some-arg  # with extra arguments
./scripts/swap-to-prod.sh               # relink binary, daemon back up
```

During dev the daemon registration is hidden (service disabled, autostart entry moved
aside), so a login while in dev mode cannot start a daemon with a missing binary.
`./scripts/uninstall.sh` removes the production install completely — the source tree stays
untouched, so the repo is always ready for `cargo run`.

## Tested environments

| Environment | Status |
| --- | --- |
| Debian GNU/Linux 13 (trixie), Xfce 4.20 (xfce4-session 4.20.2), libgtk-4-1 4.18.6+ds-2 | ✅ |
| MX Linux (trixie-based), Xfce 4.20 | ✅ |
| Raspberry Pi 4 Model B Rev 1.4 (aarch64) | ✅ |

CI additionally builds and lints on `ubuntu-latest` with the stable toolchain and the
declared MSRV (1.92). The compatibility record for other distributions, window managers
(GNOME, KDE, i3), and macOS/Windows ports lives in `docs/proposal.md` §17.

## Wayland status

The current X11 build runs under XWayland on Wayland sessions: activation works, but
stacking is degraded — the overlay cannot float above *native* Wayland fullscreen windows.
A layer-shell backend (M6) is planned; note that Mutter/GNOME does not implement
layer-shell, so GNOME Wayland is out of reach — use the Xorg session there
(`docs/proposal.md` §17.3).

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
