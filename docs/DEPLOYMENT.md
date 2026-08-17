# Deployment

How to build, install, upgrade, swap between dev and production, and publish HoverClock.
The scripts under `scripts/` are the single source of truth for the lifecycle; this document
explains what they do and how to verify. All paths use the current user's session — the
daemon is a **session** process (X11, never root).

## Requirements

- Linux with an **X11 session** (Wayland planned; the X11 build runs under XWayland with
  degraded stacking on Wayland sessions).
- GTK4 runtime: `libgtk-4-1` (Debian / Raspberry Pi OS), `gtk4` (Fedora/Arch). Builds need
  the dev headers (`libgtk-4-dev`, `gtk4-devel`, `pkg-config`).
- Rust ≥ 1.92 to build from source (declared MSRV, enforced in CI).

## Build

```bash
cargo build                 # debug
cargo build --release       # release (what install.sh uses)
```

Validation gate (same as CI): `cargo build --locked`, `cargo clippy --all-targets -- -D
warnings`, `cargo fmt --check`, `cargo test`.

## Install (production daemon)

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
  Trade-off: no crash-restart, and upgrading a *running* daemon needs a session restart
  until the control plane gains a `stop` command (M6).

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
./scripts/deploy.sh 1.2.0    # or: just deploy 1.2.0
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
and match the `Cargo.toml` version, otherwise the run fails with a message), then release
builds for **x86_64** and **aarch64**, producing tarballs + SHA-256 checksums uploaded to the
GitHub release page for that tag.

Install from a release:

```bash
tar xzf hover-clock-vX.Y.Z-x86_64.tar.gz -C ~/.local/bin
systemctl --user restart hover-clock     # run the new binary
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
