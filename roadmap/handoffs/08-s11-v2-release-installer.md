# Handoff 08 — S11 (v2.0.0 release & installer)

**Task:** S11 — v2.0.0 release & installer. Roadmap status: ⬜ prep done — tag `v2.0.0`
pending (`./scripts/deploy.sh 2.0.0`).

## What was done

- `scripts/install-release.sh` (new) — the `curl | sh` installer, **POSIX sh** (dash-clean,
  verified `sh -n`/`dash -n`) so plain `curl … | sh` works everywhere:
  - arch detection (`x86_64` / `aarch64`, `amd64`/`arm64` aliases; other arches → message
    pointing at the source build)
  - runtime-library check via `ldconfig` (`libgtk-4.so.1`, `libgtk4-layer-shell.so.0`);
    interactive `sudo` install prompt (or `--yes`), package map apt/dnf/pacman, bookworm
    caveat message, re-verify after install
  - version resolution: `--version X.Y.Z` or default = latest GitHub release (API)
  - download tarball + `.sha256` from the release tag, checksum verified (not `sha256sum -c`
    — the checksum file carries a `dist/` path prefix, so hash compare by field instead)
  - install to `~/.local/bin` (default), `install -m 755`
  - daemon registration: systemd user unit fetched from the **same release tag**
    (single source of truth: `packaging/hover-clock.service`, embedded fallback kept in
    sync) with `%h/.local/bin` → `$BIN_DIR` sed; `import-environment` in both installers
    now carries `WAYLAND_DISPLAY` (native Wayland sessions don't export DISPLAY — the unit
    needs the Wayland env to find the compositor); non-systemd → XDG
    autostart entry + message that crash-restart is systemd-only and to file an issue
  - refuses over an active dev instance (state file + pgrep/exe check, same contract as
    install.sh); shares the `flock` lock file with the other lifecycle scripts; audit-log
    line in the same `install.log`
  - flags: `--version`, `--bin-dir`, `--no-service`, `--yes`, `--help`
- README — Status rewritten (M7 merged, v2.0.0); new **Install** section leading with the
  one-liner + path table + "why no cargo install" note; "Install as a daemon" → "Lifecycle
  scripts"; Releases semver line fixed (`0.x` was stale); version-label example bumped;
  **Wayland status gained the no-multi-desktop paragraph** (layer-shell surfaces are not
  workspace-bound — RPi OS/labwc has no workspaces, MX/Xfce does, same build on both; the
  observation the user asked to record).
- docs/DEPLOYMENT.md — Requirements split into runtime-libs-for-release-binaries vs
  build-deps (bookworm: release binaries won't run — X11-only source build); **Install**
  now leads with the release installer + options table; new **Delivery options (decision
  record)** (curl|sh + tarball + source now; cargo-install/AppImage/Flatpak/Snap/distro
  packages deferred with reasons); deploy example bumped to 2.0.0.
- AGENTS.md — purpose: "(X11 first, Wayland planned)" → "(X11 + native Wayland, M7
  merged)"; navigation row for install/delivery; Rules: release binaries link system libs
  (bookworm note), install-release.sh embedded units must stay in sync with
  `packaging/`; Validation gained the Wayland live-smoke line.
- docs/proposal.md — §Scope line updated (X11 + native Wayland, M7 merged).
- justfile — `just install-release` recipe.
- scripts/release-notes.sh — footer now carries the curl|sh one-liner on release pages.
- ROADMAP — Current state rewritten; S11 card added.

## What was done differently

- The installer's checksum step compares the hash field directly instead of
  `sha256sum -c`: the release checksum file contains `dist/hover-clock-vX.Y.Z-….tar.gz`
  (path prefix from the workflow's `sha256sum dist/…`), so `-c` would need the matching
  layout.
- The systemd unit and autostart entry are **fetched from the release tag**, not embedded
  as the primary source — the packaging files stay the single source of truth and the unit
  matches the exact release being installed. Embedded copies are only the fallback (their
  sync is now an AGENTS.md rule).

## Verification

- `sh -n` + `dash -n` clean (POSIX), bash scripts `bash -n` clean; `git diff --check` clean.
- Smoke-tested for real against the **v1.3.1 release assets** with `HOME` pointed at a
  temp dir and `--no-service` (dev mode is active on this machine; the installer correctly
  refuses over a live dev instance — that refusal was observed first): latest-resolution
  path (GitHub API) and pinned `--version` path both downloaded, checksum-verified, and
  installed; the installed binary runs (`hover-clock 1.3.1`); audit-log line written.
- Unit/autostart fetch + `$BIN_DIR` sed verified against the v1.3.1 tag.
- The systemd enable/restart branch itself was not executed end-to-end (would touch the
  live user manager); the logic mirrors install.sh's proven path.

## Known gaps / next steps

- `./scripts/deploy.sh 2.0.0` → tag → release workflow builds the v2.0.0 tarballs; then
  the curl|sh one-liner works for real users. Until the tag exists, `install-release.sh`
  resolves v1.3.1 (latest).
- The engineering profile (`llm_profiles/engineering/projects/gtk-overlay-desktop.md`)
  was synced (M7 merged, v2.0.0, multi-desktop note); no repo impact.
