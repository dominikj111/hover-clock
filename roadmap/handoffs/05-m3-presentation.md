# Hand-off — S04 M3 Presentation

- **Story status:** ✅ 2026-08-17 (confirmed done — live smoke on the X session: corner dwell
  shows at the triggered monitor's top-right with fade-in, corner-leave/Esc fade out, no
  Gtk-CRITICAL noise)
- **Next story:** S05 M4 Calendar widget

## Delivered

- `src/main.rs` — clock widget (proposal §11): time/day/date labels + version row in a
  vertical stack, wrapped in the `.clock-frame`/`.clock-widget` styled boxes; CSS via
  `load_css()` (bundled `src/style.css`, §8.3); `decorated(false)`.
- `OverlayController` — owns show/hide behaviour for all trigger paths (corner, Super+T,
  Esc, workspace switch, IPC):
  - **Trigger-corner placement** (§5): the window realizes once at setup
    (`gtk_widget_realize`, not `gtk_native_realize` — the direct call crashes GTK with
    frame-clock/double-realize assertions), then `WindowBackend::move_to` (new facade method,
    X11 `configure_window`) positions it at the triggered monitor's top-right before mapping
    — no flash at GTK's default location. Monitor comes from `CornerEntered`; `Toggle` uses
    the last known. HiDPI: `Monitor` is in physical pixels vs GDK logical — exact placement
    assumes scale 1 (noted).
  - **Fade in/out** (§5): 10 × 15 ms opacity animation layered on the instant map (latency
    stays under budget); a new fade replaces any in-flight one. Workspace re-map stays
    instant (no fade).
- Daemon/client CLI split (§7.4): `--start`/`-s` (hidden `--daemon` alias) runs the
  single-instance daemon (control socket bound before GTK init; stale sockets reclaimed);
  no-arg invocations are clients sending `show` (default) / `hide` / `toggle` over the Unix
  socket. gio `SocketService` + async futures serve the plane on the main loop (no worker
  threads, UI never blocks). See hand-off `06-s09-version-update.md` for the version row
  (S09).

## Decisions & deviations

1. **`gtk_native_realize` must not be called directly.** First placement implementation
   called it per-show — GTK4 asserts on a missing frame clock (first call) and on
   double-realize (every show). Fixed: `gtk_widget_realize` once at setup (realize ≠ map; the
   overlay stays hidden until triggered), placement only configures the position.
2. **Corner placement lives behind the `WindowBackend` facade** (`move_to`, default no-op) —
   GTK4/GDK4 removed programmatic window positioning, so it's an X11 `configure_window`.
   A WM that ignores the requested position degrades to GTK default placement (acceptable;
   xfwm4 honors it).
3. **Workspace re-map: instant hide, fade-in re-show** — the unmap+map batch stays
   imperceptible on the hide side; the re-shown overlay fades in (~150 ms, same as corner/
   Esc/toggle), which reads much smoother than an instant pop. (Originally instant on both
   sides; the fade-in landed after the story was verified.)
4. **Widget size for placement** is measured at setup (`widget.measure`, natural size) with
   the allocated size preferred once the window has mapped; version-label text changes can
   shift the width slightly (margin absorbs it).
5. **rustfmt contract**: CI's Format check is pinned to the MSRV toolchain (1.92.0) — format
   with `cargo +1.92.0 fmt` (local 1.97 rustfmt drifted and failed CI).

## Verification

- `cargo build --locked`, `cargo clippy --all-targets -- -D warnings`, `cargo +1.92.0 fmt
  --check`, `cargo test` (12 tests) — green on both 1.92.0 and local stable.
- Live: corner dwell → overlay at the monitor's top-right, fade-in; Esc/corner-leave →
  fade-out; Super+T → fade; workspace switch → instant; no Gtk-CRITICAL spam.
