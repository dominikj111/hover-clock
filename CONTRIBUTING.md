# Contributing to HoverClock

Thanks for considering a contribution. This file describes how the project is organized and
what a contribution looks like — from a one-line fix to a new milestone. It is a shorter,
human-facing companion to [`AGENTS.md`](./AGENTS.md) (the operating file used by AI
contributors) and the design contract in [`docs/proposal.md`](./docs/proposal.md).

## Project shape

- HoverClock is a **transient Linux overlay daemon** (X11 first, Wayland planned) — a clock
  widget surfaces on demand via hot-corner or `Super + T`, above fullscreen apps, never
  taking focus.
- The public repository is **intentionally just the GTK overlay clock**; the broader
  overlay-shell direction is private exploration (proposal §2).
- All behavior is governed by the **design contract** (`docs/proposal.md`) — it is
  authoritative when implementation conflicts with the current design. `docs/index.md` maps
  topics to sections; keep it in sync whenever the proposal changes.

## Quick start

```bash
cargo build        # compile
cargo run          # run: overlay hidden until triggered
cargo clippy       # zero warnings expected
cargo test         # unit tests (currently a placeholder suite)
```

Minimum supported Rust version: **1.92** (declared in `Cargo.toml`; enforced in CI).

Requirements: a Linux system with an X11 session, GTK4 dev headers
(`libgtk-4-dev` on Debian/Raspberry Pi OS, `gtk4-devel` on Fedora), `pkg-config`, and a Rust
toolchain ≥ 1.92. Wayland is planned (M6, layer-shell); on a Wayland session the X11 build
runs under XWayland with degraded stacking — see proposal §17.

## Development workflow (one story at a time)

Work proceeds one story at a time off [`roadmap/ROADMAP.md`](./roadmap/ROADMAP.md):

1. Pick the **current story card** and read its hand-off (`roadmap/handoffs/`).
2. Read the proposal **§ cited on the card** (use `docs/index.md` to navigate).
3. **Trace the existing code** before writing anything — backends live in `src/backend/`
   behind trait facades (`src/backend/mod.rs`).
4. Make the **smallest coherent change** that meets the card's acceptance criteria.
5. **Validate** (below), then mark the story done and write the hand-off.

## Validation

- `cargo build` and `cargo clippy` clean (zero warnings); `cargo test` green.
- Live smoke test on an X session: corner dwell shows the overlay, `Esc` hides it,
  `Super + T` toggles, and the active window never becomes the overlay.

## Code conventions

- **Facades, not direct system calls.** Business logic never touches X11, sockets, or the
  environment directly — everything goes through registry facades (`src/backend/`).
- **Degrade, never crash.** When a capability is unavailable (no X11, WM without EWMH
  properties), log a warning and continue; the overlay must keep working via remaining
  triggers.
- **Transient overlay rules.** Never call `present()` on the overlay (it requests focus) —
  show/hide via `set_visible()`. `Esc` always dismisses; dismissal must not rely on focus.
- **Minimal dependencies.** Prefer the idiomatic one-line fix over the elaborate one; no new
  dependency without justification in the change.
- Formatting follows `cargo fmt`; doc comments explain *why*, not what.

## Reporting issues

Include: distribution and version, desktop environment / window manager and version, session
type (X11 vs Wayland), GTK version (`pkg-config --modversion gtk4`), and any log output
(GTK warnings are prefixed with `hover-clock`).

## Licensing

By contributing, you agree that your contributions are licensed under the project's license
(see [`LICENSE`](./LICENSE) and the `license` field in `Cargo.toml`).
