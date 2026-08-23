# Deployment

How to build, install, upgrade, swap between dev and production, and publish HoverClock.
The scripts under `scripts/` are the single source of truth for the lifecycle; this document
explains what they do and how to verify. All paths use the current user's session — the
daemon is a **session** process (X11, never root).

## Requirements

- Linux with an **X11 session or a layer-shell Wayland compositor** (wlroots family /
  KWin ≥ 5.27). The XWayland fallback keeps running on other Wayland sessions with
  degraded stacking.
- **Runtime libraries for release binaries** (built with the default `wayland` feature —
  both are dynamically linked, also on X11): `libgtk-4-1` and `libgtk4-layer-shell-0` on
  Debian / Raspberry Pi OS trixie; `gtk4` + the layer-shell runtime on Fedora/Arch. The
  release installer checks these and installs or reports what is missing.
  - **Debian 12 / Pi OS bookworm:** no layer-shell package exists — release binaries will
    not run there; use the X11-only source build (`cargo build --no-default-features`).
- Builds additionally need the dev headers (`libgtk-4-dev` + `libgtk4-layer-shell-dev` on
  Debian, `gtk4-devel` on Fedora, `pkg-config`) and **Rust ≥ 1.92** (declared MSRV,
  enforced in CI).

## Build

```bash
cargo build                 # debug
cargo build --release       # release (what install.sh uses)
```

Validation gate (same as CI): `cargo build --locked`, `cargo clippy --all-targets -- -D
warnings`, `cargo fmt --check`, `cargo test`.

## Install

### From a GitHub release (recommended — no toolchain)

```bash
curl -fsSL https://raw.githubusercontent.com/dominikj111/hover-clock/main/scripts/install-release.sh | sh
```

`install-release.sh` (POSIX sh — runs under plain `sh`) downloads the binary matching the
architecture (x86_64 / aarch64) from the latest GitHub release, verifies its SHA-256
checksum, installs it to `~/.local/bin`, and registers the daemon — the same init detection
and units as the source install below. It checks the GTK4 runtime libraries first and
installs the missing packages via sudo when run interactively (or with `--yes`). Options:

| Option | Meaning |
| --- | --- |
| `--version X.Y.Z` | Install a specific release (default: latest) |
| `--bin-dir DIR` | Binary directory (default `~/.local/bin`) |
| `--no-service` | Binary only, no daemon registration |
| `--yes` | Install missing system packages without prompting |
| `--help` | Usage |

Upgrading is re-running the installer (always the latest release); uninstalling is one
command — `curl -fsSL https://raw.githubusercontent.com/dominikj111/hover-clock/main/scripts/uninstall.sh | sh`
(or `just uninstall` from a checkout): stops any running instance and removes every trace
(daemon registration, binary, swap stash, audit log — the repo is untouched). Non-systemd
systems get the XDG autostart entry plus a message that crash-restart is systemd-only (and
to file an issue if their init needs support).

### From source (build)

```bash
./scripts/install.sh
```

Builds the release binary, installs it to `~/.local/bin/hover-clock`, registers the daemon,
and starts it. Registration mechanism is auto-detected:

- **systemd** (Debian/Fedora/MX with systemd boot) — a systemd *user* unit at
  `~/.config/systemd/user/hover-clock.service`, `ExecStart=… --start`, wanted by
  `default.target` **and** `graphical-session.target` (some DEs — e.g. xfce on MX Linux —
  never raise the latter; both entries guarantee exactly one start at login). `install.sh`
  **enables + restarts**, so a reinstall always runs the fresh binary.
- **sysvinit / OpenRC / runit** — an XDG autostart entry at
  `~/.config/autostart/hover-clock.desktop` (honored by Xfce/GNOME/KDE regardless of init).
  Trade-off: no crash-restart. There is no unit, so `systemctl --user restart
  hover-clock` does not work here — stop/restart a running daemon with
  `hover-clock --stop` / `hover-clock --restart`, which are init-independent (they drive
  the daemon over its control socket, so they work on every install — including
  systemd).

`install.sh` refuses while dev mode is active (see Swap) — return to production first.

Verify:

```bash
systemctl --user status hover-clock     # active; ExecStart ends with --start
hover-clock                             # client: sends `show` — overlay appears
```

The daemon is **single instance**: a second `hover-clock --start` exits with an explanatory
error (control socket `$XDG_RUNTIME_DIR/hoverclock.sock`; stale sockets from crashes are
reclaimed automatically).

## CLI reference

| Invocation | Effect |
| --- | --- |
| `hover-clock --start` / `-s` | Start the daemon (single instance) |
| `hover-clock` | Client — show the overlay (default command) |
| `hover-clock show` / `hide` / `toggle` | Client — drive overlay state |
| `hover-clock --help` / `--version` | Help / version |

## Delivery options (decision record)

The supported paths are intentionally three: **curl | sh release installer** (default),
**release tarball** (manual), and **source build** (`install.sh` / `just install`, dev and
exotic setups). Everything heavier is deferred — with 0 confirmed users, a second delivery
surface is maintenance, not value:

- **cargo install / crates.io** — deferred until a user asks. It would need a crates.io
  publication and still cannot register the session daemon or ensure the system GTK4
  libraries (the curl installer does both).
- **AppImage / Flatpak / Snap** — rejected as too heavy for a < 25 MB session daemon, and
  a mismatch with the design: the daemon lives in the desktop session (autostart, control
  socket in the runtime dir, X11/Wayland access) that sandboxes make awkward.
- **Distro packages (deb/rpm), AUR, Homebrew, Nix** — per-distro maintenance without users;
  community can add them (an AUR package is a natural first candidate) when demand shows.
- **Single static binary** — GTK4 + layer-shell cannot be meaningfully statically linked;
  the dynamic-libs trade (tiny binary, shared GTK4) is documented in the README.

## Upgrade

```bash
./scripts/upgrade.sh [branch]    # default: current branch; pull, rebuild, restart
```

`upgrade.sh` fetches, checks out the branch, `pull --ff-only`, rebuilds, and **restarts** the
daemon in place (the overlay is transient — a restart between dwells is imperceptible).
Alternatively, install from a release tarball (below) without building.

## Releases & publishing

Versioning is semver; `Cargo.toml` is the single source of truth. Publishing is
**main-only**, tag-gated. One command (bumps `Cargo.toml` + `Cargo.lock`, commits, pushes
main, tags and pushes `vX.Y.Z`; refuses on a dirty tree, a non-main branch, or an existing
tag):

```bash
./scripts/deploy.sh 2.0.0    # or: just deploy 2.0.0
```

Manually, the same steps are:

```bash
# bump Cargo.toml (and Cargo.lock — a stale lock fails CI's --locked builds)
git commit -m "chore: bump to X.Y.Z"
git push origin main
git tag vX.Y.Z              # tag the release commit on main
git push origin vX.Y.Z
```

`.github/workflows/release.yml` then runs: a **guard** job (tag must point at main's history
and match the `Cargo.toml` version, otherwise the run fails with a message), a **notes** job
that generates the release body from the git history since the previous tag
(`scripts/release-notes.sh` — commit subjects grouped by type), then release builds for
**x86_64** and **aarch64**, producing tarballs + SHA-256 checksums uploaded to the GitHub
release page for that tag. The notes are generated once and reused by both build jobs, so
the body is deterministic (no doubled footer).

Install from a release:

```bash
curl -fsSL https://raw.githubusercontent.com/dominikj111/hover-clock/main/scripts/install-release.sh | sh   # recommended
# or manually:
tar xzf hover-clock-vX.Y.Z-x86_64.tar.gz -C ~/.local/bin
# systemd installs: the unit restarts the daemon on the fresh binary.
# Every install (systemd or not): hover-clock --restart works too.
systemctl --user restart hover-clock     # run the new binary (systemd only)
hover-clock --restart                    # init-independent alternative
```

## Swap between dev and production

Development runs from the source tree without losing the production install:

| Script | Effect |
| --- | --- |
| `./scripts/swap-to-dev.sh [args]` | Stops the daemon, **stashes** the installed binary(ies) aside (unlinked, never deleted), hides the daemon registration (service disabled, autostart moved), then runs `cargo run -- --start` by default |
| `./scripts/swap-to-prod.sh` | Relinks the stashed binary to its exact original path and restarts the daemon — instant, offline, no rebuild |
| `./scripts/uninstall.sh` | Removes the production install completely (service/autostart, binaries, swap stash, audit log); the source tree stays untouched |

The stash lives in `~/.local/state/hover-clock/prod-bin/`; the swap state (which binary was
stashed where) in `~/.local/state/hover-clock/state`. Binary discovery is by search
(`~/.local/bin`, `$CARGO_HOME/bin`, every directory on `PATH`, `$HOVERCLOCK_BIN_DIR`
override), so installs from any source (install.sh, `cargo install`, release tarballs) are
restored to their exact path. Restoring never clobbers a newer binary that appeared while in
dev mode. While stashed, `cargo uninstall hover-clock` cannot find the binary — swap back to
production first. A second `swap-to-dev.sh` while dev mode is active is refused (it would
orphan the stash); the message lists the recovery options.

Two caveats, both handled by the scripts:

- **Stale restore.** If a release is published (`./scripts/deploy.sh` → GitHub) while dev mode
  is active, the stash predates it: `swap-to-prod` restores the older binary and prints a
  prominent notice. Bring production to the release with `./scripts/upgrade.sh` (or the
  overlay's orange update button).
- **Start a dev session only via `swap-to-dev.sh`.** A bare `cargo run -- --start` leaves a
  running dev process with no swap state, so `swap-to-prod` cannot restore anything — it
  detects the stray process and prints the recovery steps (`pkill -x hover-clock`, then
  `systemctl --user start hover-clock`).

**Build guard (`build.rs`).** On a machine with the swap machinery installed, a **debug**
build from the repo (`cargo build`, `cargo run`, `cargo test`, clippy) aborts unless the dev
session state exists — the error prints the exact recovery commands. This makes the
`swap-to-dev` ceremony impossible to skip. Bypasses: `--release` builds (`install.sh`,
`upgrade.sh`), CI, and `HOVERCLOCK_BYPASS_DEV_GUARD=1` for scripted builds (`deploy.sh`
sets it around its Cargo.lock sync).

Dev-mode restart loops (unit started while the binary is stashed) are capped by the unit's
`StartLimitIntervalSec`/`StartLimitBurst`; both swap scripts also call
`systemctl --user reset-failed` so a stale restart counter cannot persist.

## Version label

The overlay's bottom label shows the running binary's version (shadow-grey); it turns orange
— `v1.0.0 → v1.1.0` — when a newer release exists on GitHub (checked hourly on a worker
thread; offline/failed checks leave it grey, never an error). See proposal §11.2 and
roadmap S09.

## Logs & audit

- Daemon runtime output: `journalctl --user -u hover-clock --no-pager -n 50` (or `-b`).
- Script audit trail: every install/upgrade/swap/uninstall appends one timestamped line
  (action, version, git sha, outcome) to `~/.local/state/hover-clock/install.log`.

## Troubleshooting

| Symptom | Cause / fix |
| --- | --- |
| `hover-clock: cannot reach the hover-clock daemon…` | No daemon running — start it: `hover-clock --start` |
| `hover-clock: another hover-clock daemon is already running…` | Single-instance guard — the daemon is already up; use the client instead |
| Overlay shows old behavior after install | The daemon wasn't restarted — `systemctl --user restart hover-clock` (install.sh ≥ the fix does this automatically) |
| Build fails: `Package gtk4 was not found` | GTK4 dev headers missing — see Requirements |
| Version label stays grey while a newer release exists | Check passed offline/failed; the hourly refresh retries — or the release was published within the last hour |
