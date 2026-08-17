# HoverClock — Roadmap

Derived from [docs/proposal.md](../docs/proposal.md) §14. One story in progress at a time; a
story is done only when confirmed implemented (acceptance met + verification per
`../AGENTS.md`).

## Current state

The daemon runs on X11 (xfwm4-verified): overlay is hidden until triggered, shows on top-right
hot-corner dwell (200 ms debounced) or `Super + T`, dismisses on `Esc`, never takes focus, never
flashes in the taskbar (GDK skip-taskbar hints, fix on M1/M2), stays invisible to task
switchers, and auto-hides (debounced) on corner leave. **S04 (M3 — Presentation)** in progress:
clock widget (time/day/date, §11), static single style (rounded corners, translucent black,
§8.3), `decorated(false)`, corner-leave auto-hide, and the daemon/client CLI split (§7.4) —
`--start`/`-s` starts the single-instance daemon, no-arg invocations are clients
sending `show` over the Unix control socket — landed; fade in/out transitions and
trigger-corner placement remain. Packaging: daemon autostart fixed for DEs that never raise
`graphical-session.target` (xfce on MX Linux — unit now wanted by `default.target` too); see
[handoffs/04-autostart-fix.md](handoffs/04-autostart-fix.md).

## Story cards

### S01 — M0 Project scaffold ✅ 2026-08-06

- **Status:** ✅ 2026-08-06
- **Goal:** Repository baseline: a GTK4-rs clock that builds and runs.
- **Deliverables:** `Cargo.toml`/`Cargo.lock`, `src/main.rs` clock label, README/proposal/AGENTS.
- **Acceptance:** `cargo build` green; classic window shows a ticking clock.
- **Design refs:** §7
- **Hand-off:** None — repo baseline, no hand-off written.

### S02 — M1 Overlay behavior ✅ 2026-08-07

- **Status:** ✅ 2026-08-07
- **Goal:** The window behaves as an overlay: above fullscreen apps, invisible to task
  switchers, never focusable.
- **Deliverables:** `WindowBackend` facade + `X11WindowBackend` (EWMH hints, ICCCM
  `input=False`, `set_visible` instead of `present`).
- **Acceptance:** `_NET_WM_WINDOW_TYPE=NOTIFICATION`, `_NET_WM_STATE=ABOVE|SKIP_TASKBAR|
  SKIP_PAGER`, focus never the overlay — verified live on xfwm4.
- **Design refs:** §6, §9, §10
- **Hand-off:** [02-m1-overlay-behavior.md](handoffs/02-m1-overlay-behavior.md)

### S03 — M2 Activation ✅ 2026-08-10

- **Status:** ✅ 2026-08-10
- **Goal:** Surface the overlay on demand: hot-corner detection (debounced, edge-triggered),
  global `Super + T`, `Esc` dismiss.
- **Deliverables:** `ActivationBackend` facade + `X11ActivationBackend` (XI2 pointer motion,
  core key grabs, glib fd source), glue in `src/main.rs` (dwell debounce, toggle, dismiss).
- **Acceptance:** overlay hidden at startup; corner dwell shows, quick pass stays hidden;
  `Super + T` toggles; `Esc` hides; active window never becomes the overlay.
- **Design refs:** §5, §10, §13
- **Hand-off:** [03-m2-activation.md](handoffs/03-m2-activation.md)

### S04 — M3 Presentation 🔄

- **Status:** 🔄 in progress
- **Goal:** The clock becomes a widget: time/day/date, CSS styling, fade in/out show/hide
  transitions, auto-hide timer (mouse leaves overlay/corner, debounced).
- **Deliverables:** full clock widget (§11 layout), CSS theming, fade in/out show/hide
  transitions, placement at the triggered monitor's top-right, auto-hide wired to
  `CornerLeft`; daemon/client CLI split (§7.4): `--start`/`-s` starts the single-instance
  daemon, no-arg client sends `show` over the Unix control socket (`show`/`hide`/`toggle`,
  single-instance guard, gio async serving on the main loop).
- **Acceptance:** widget layout + styling visible; show fades in, hide fades out (no hard
  pop); auto-hide dismisses after debounce; overlay appears at the triggered monitor's
  corner; performance targets hold (§13).
- **Design refs:** §5, §11, §13
- **Hand-off:** pending

### S05 — M4 Calendar widget ⬜

- **Status:** ⬜ backlog
- **Goal:** A minimal month calendar whose only job is to show which day of the month today
  is; grows the widget model to **widgets containing widgets** (layout containers + leaf
  widgets, Weaver Desktop fabric model, §11.1).
- **Deliverables:** calendar compound widget — a grid `layout` of day-cell `label`s with a
  weekday header; today's cell highlighted via a CSS class; composed into the overlay next to
  the clock (vertical stack, §11); non-interactive (no selection/navigation).
- **Acceptance:** the overlay shows the current month with today's day visually distinct; the
  composite model is exercised (a `layout` containing leaf widgets); no pointer handlers on
  the calendar, no activation changes.
- **Design refs:** §8.1, §11
- **Hand-off:** pending

### S06 — M5 Registry & config ⬜

- **Status:** ⬜ backlog
- **Goal:** Adopt `singleton-registry`: flat capability registry, facade contracts, TOML
  config with live hot-swap reload.
- **Deliverables:** registry wiring for `WindowBackend`/`ActivationBackend`/`TimeSource`/
  `Config`; TOML config; reload without resetting overlay state; shortcut-only fallback on
  misconfiguration.
- **Acceptance:** components resolve via the registry; config hot-swap verified; degraded
  activation still works.
- **Design refs:** §10, §12
- **Hand-off:** pending

### S07 — M6 IPC (daemon/client) ⬜

- **Status:** ⬜ backlog
- **Goal:** Complete the control plane started in S04: full command registry and lifecycle
  commands over the existing Unix socket (daemon/client split + `show`/`hide`/`toggle` already
  landed there, §7.4).
- **Deliverables:** `Command` registry with `name()`/`execute(args, writer)`, client retries
  with backoff; commands `ping`, `status`, `version`, `commands`, `stop` (`widget`/`config
  reload` later).
- **Acceptance:** client drives overlay state over the socket; overlay stays functional without
  IPC (degradation, §12).
- **Design refs:** §7, §12, §15 (socket path/ownership, framing, transport evolution)
- **Hand-off:** pending

### S08 — M7 Wayland ⬜

- **Status:** ⬜ backlog
- **Goal:** Wayland layer-shell backend behind the existing `WindowBackend` /
  `ActivationBackend` contracts.
- **Deliverables:** layer-shell `WindowBackend`; pointer/shortcut `ActivationBackend`.
- **Acceptance:** same behavior contract on a Wayland compositor; X11 path unchanged.
- **Design refs:** §10, §15 (layer-shell anchor/layer choice)
- **Hand-off:** pending

### S09 — Version notification widget ⬜

- **Status:** ⬜ backlog
- **Goal:** When a newer release exists, the overlay shows a small non-intrusive widget
  (e.g. under the clock) — click to upgrade.
- **Deliverables:** version check against the GitHub releases API with **offline-first
  degradation** (no network → no widget, no errors, no polling spam); click-to-upgrade
  action (runs the swap/upgrade flow and restarts the daemon seamlessly); refresh policy.
- **Acceptance:** widget appears only when a newer version is known; offline shows nothing;
  click upgrades and restarts the daemon without disturbing the session.
- **Design refs:** §2 (widget growth path), §7.4 (data plane, later), §13 (footprint —
  low-frequency check, no polling)
- **Hand-off:** pending

## Later (private exploration)

Notifications, toast messaging, template-driven widgets, socket data-plane API, overlay-shell
direction. Out of scope for this public repository.
