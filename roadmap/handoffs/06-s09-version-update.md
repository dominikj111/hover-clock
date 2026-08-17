# Hand-off — S09 Version notification / self-update

- **Story status:** ✅ 2026-08-17 (confirmed done — button tested through two real release
  cycles: v1.0.0 → v1.1.0 and v1.1.0 → v1.2.0 self-updates, plus the dev-mode button loop)
- **Next story:** S05 M4 Calendar widget

## Delivered

- `src/version.rs` — GitHub Releases API check:
  - `latest_release()` → `Release { version, tarball_url, sha256_url }` (arch-matched assets,
    derived from `Cargo.toml`'s `repository` URL); lightweight JSON parse (`tag_name` +
    `browser_download_url`), no serde dep; bounded timeouts (5 s); every failure degrades to
    "no newer version known", never an error.
  - `running_version()` / `label_text()` — pure, unit-tested label logic.
- `src/main.rs` — version row UI: plain label (shadow-grey, user-tuned colour) normally; a
  **click-to-update button** (orange pill matching the overlay — 10 px radius, translucent
  orange tint/border, hover brightening, dimmed while updating) when a newer release exists.
  Check runs at startup + hourly (`timeout_add_seconds_local(3600)`); worker thread + 
  `MainContext::invoke` into a thread-local `VersionUi` (GTK widgets are not Send).
- `src/update.rs` — self-update: download the arch tarball (ureq), verify the SHA-256
  (`sha2`), extract via system `tar`, stage + atomic `rename` over the running binary
  (all-or-nothing — any failure before the replace leaves the install untouched, button
  restores for retry), restart:
  - systemd: `systemctl --user restart` when the unit exists **and is enabled**;
  - otherwise (dev mode / autostart): detached `sh -c "sleep 1; exec … --start"` re-exec —
    the new process reclaims the stale socket (single-instance guard).
- `Cargo.toml` — `ureq` (tls only), `sha2`.
- `release.yml` — explicit release body instead of `generate_release_notes` (the two matrix
  jobs regenerated/appended notes, doubling the "Full Changelog" footer on v1.1.0/v1.2.0).
- Tooling: `justfile` (install/upgrade/swap/check/deploy…), `scripts/deploy.sh <version>`
  (one-command release: bump Cargo.toml + lock, commit, push, tag; guards: main-only, clean
  tree, tag not on origin).

## Decisions & deviations

1. **Dev-mode self-update works** — `restart()` checks `systemctl --user is-enabled`, not just
   the unit file's existence: dev mode disables the unit (its binary is stashed), so systemd
   restart would fail; the re-exec path takes over. Verified: dev button → downloads v1.2.0 →
   replaces `target/debug/hover-clock` → re-execs → daemon runs the release build.
2. **GTK has no CSS `cursor` property** — the declaration was dropped (theme parser warning).
3. **No confirmation dialog on click** — clicking is the confirmation (transient overlay; S09
   acceptance: "without disturbing the session").
4. **Version-lowering for dev testing is a local-only, uncommitted hack** — committing a
   downgrade on main would ride along with the next `just deploy` (the dirty-tree guard only
   catches uncommitted changes) and poison the release flow. Revert with
   `git checkout -- Cargo.toml Cargo.lock`.
5. **GitHub infra hiccups**: release-build jobs occasionally fail with "No server is currently
   available" (their 503s, even on `gh` calls). Fix: `gh run rerun <id> --failed` once GitHub
   settles.

## Verification

- `cargo build --locked`, clippy `-D warnings`, `cargo +1.92.0 fmt --check`, `cargo test`
  (12 tests incl. release/asset parsing) — green.
- Live: two production self-updates (1.0.0 → 1.1.0 → 1.2.0), dev-mode button test, release
  pages clean (single changelog link).
