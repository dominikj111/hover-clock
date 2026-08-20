# AGENTS.md — HoverClock

## Purpose

HoverClock is a transient Linux overlay daemon (X11 first, Wayland planned): widgets —
starting with a clock — surface on demand via hot-corner or global shortcut, above fullscreen
apps, invisible to task switchers, never taking focus. Rust + GTK4-rs, single binary,
dual-mode (daemon + client), offline-first. Success: proposal §14 milestones confirmed
implemented in order, one at a time.

## Navigation

| Need | Read |
|------|------|
| Understand the system / intended behavior | `docs/proposal.md` — always read the § cited on the story card before starting |
| Check current status / what's next | `roadmap/ROADMAP.md` — current story + hand-offs |
| Set up / use the Wayland test session | `docs/WAYLAND_TESTING.md` — labwc session, install, caveats |
| Change the design | `docs/proposal.md`, amending `docs/index.md` in the same change |
| Change window / activation backends | `src/backend/` — facade contracts in `src/backend/mod.rs` |
| Add a widget | proposal §11 (widget contract); composition in `src/main.rs` |
| Hand-off contract / iteration loop | ICM/MWP guideline (§5 accept → process → handoff) |

## Rules

- `docs/proposal.md` is authoritative when implementation conflicts with the current design.
- The overlay is transient — never a persistent desktop component; the clock is a feature, not
  the system (architecture stays widget-extensible).
- No direct coupling between input detection and rendering; system APIs live behind trait
  facades (JigsawFlow facade contract) and degrade to logged warnings, never crashes.
- The daemon is a single binary, dual-mode process: `--start`/`-s` runs the daemon
  (single instance, §7.4 single-instance guard), any other invocation is a client sending a
  command over the Unix control socket (no args → `show`).
- Escape always dismisses the overlay if visible — it never has focus, so dismissal cannot
  rely on window focus.
- Never call `present()` on the overlay (it requests focus); show/hide via `set_visible()`.

## Workflow

Read the current story card + its hand-off → the proposal §N cited on the card → trace the
existing code before writing → smallest coherent change → validate (below) → mark the story
done and write the hand-off.

## Dev/prod swap discipline

Any dev or live-testing session goes through the swap scripts — never a bare `cargo run`:

1. **Before** any build/test/live-smoke work: `./scripts/swap-to-dev.sh` — stops the
daemon, stashes the installed binary aside, runs from source. Never start a dev daemon
(`cargo run -- --start`, `target/debug/hover-clock`) directly.
2. **After** the session: `./scripts/swap-to-prod.sh` — restores the installed binary and
restarts the daemon.

The guard is enforced mechanically: `build.rs` aborts any **debug** build (`cargo build`,
`cargo run`, `cargo test`, clippy) on this machine when the swap state is absent, with the
recovery commands in the error. Release builds, CI, and `HOVERCLOCK_BYPASS_DEV_GUARD=1`
(scripted builds such as `deploy.sh`) bypass it.

If a `hover-clock` process is running but `~/.local/state/hover-clock/state` is absent, a dev
session was started outside the swap scripts: stop it (`pkill -x hover-clock`), reset the
unit (`systemctl --user reset-failed hover-clock`), and verify the installed daemon
(`systemctl --user status hover-clock`) before continuing. If a release was published
(`./scripts/deploy.sh`) during the dev session, `swap-to-prod` restores the stale stash and
warns — run `./scripts/upgrade.sh` to bring production to the released version.

## Validation

- `cargo build` + `cargo clippy` clean (zero warnings); `cargo test` green.
- Formatting contract is rustfmt of the MSRV toolchain: `cargo +1.92.0 fmt` (CI pins the
  Format check to 1.92.0 — rustfmt style drifts between versions).
- Live smoke on the X session (xfwm4): top-edge dwell shows, `Esc` hides, `Super + T` toggles,
  the active window never becomes the overlay.

## Context

Instantiated from the workspace profile by topic name: ICM/MWP guideline (iteration loop),
JigsawFlow guideline (registry facades, degradation), GTK frontend guideline (widget
composition), Rust development guideline (idioms, minimal deps), Project structure guideline
(this file's shape). Do not duplicate the profile's conventions here.
