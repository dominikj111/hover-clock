# HoverClock — Roadmap

Derived from [docs/proposal.md](../docs/proposal.md) §14. One story in progress at a time; a
story is done only when confirmed implemented (acceptance met + verification per
`../AGENTS.md`).

## Current state

The daemon runs on X11 (xfwm4-verified): overlay is hidden until triggered, shows on top-edge
dwell (200 ms debounced) or `Super + T`, dismisses on `Esc`, never takes focus, never
flashes in the taskbar (GDK skip-taskbar hints, fix on M1/M2), stays invisible to task
switchers, and auto-hides (debounced) on hot-area leave. **S04 (M3 — Presentation)** in progress:
clock widget (time/day/date, §11), static single style (rounded corners, translucent black,
§8.3), `decorated(false)`, corner-leave auto-hide, and the daemon/client CLI split (§7.4) —
`--start`/`-s` starts the single-instance daemon, no-arg invocations are clients
sending `show` over the Unix control socket — landed, plus the version row (GitHub release
check + click-to-update button, §11.2/S09) and the M3 presentation finish: fade in/out
transitions and placement (centred above the triggered monitor's middle) — **S04 and S09
delivered** (hand-offs
`05-m3-presentation.md`, `06-s09-version-update.md`); release flow live (v1.0.0–v1.2.0,
`just deploy`). **S08 (M7 Wayland) delivered** — layer-shell overlay + hot-corner strips,
verified native on labwc 0.9.8 (hand-off `07-m7-wayland.md`). **Next: S05 (M4 Calendar
widget)**. Packaging: daemon autostart fixed for DEs that never raise
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

### S04 — M3 Presentation ✅ 2026-08-17

- **Status:** ✅ 2026-08-17 — all deliverables landed and live-verified: fade in/out
  transitions, placement (centred above the triggered monitor's middle), daemon/client CLI
  split (§7.4); hand-off:
  [handoffs/05-m3-presentation.md](handoffs/05-m3-presentation.md).
- **Goal:** The clock becomes a widget: time/day/date, CSS styling, fade in/out show/hide
  transitions, auto-hide timer (mouse leaves overlay/corner, debounced).
- **Deliverables:** full clock widget (§11 layout), CSS theming, fade in/out show/hide
  transitions, placement centred above the triggered monitor's middle, auto-hide wired to
  `CornerLeft`; daemon/client CLI split (§7.4): `--start`/`-s` starts the single-instance
  daemon, no-arg client sends `show` over the Unix control socket (`show`/`hide`/`toggle`,
  single-instance guard, gio async serving on the main loop); version label with interim
  local-Cargo.toml check (§11.2, `src/version.rs`).
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

### S08 — M7 Wayland ✅ 2026-08-22

- **Status:** ✅ 2026-08-22 — layer-shell `WindowBackend` + hot-corner strip
  `ActivationBackend` landed behind the existing contracts, verified live on labwc 0.9.8
  (native Wayland, the labwc session on this Pi) with a physical pointer: the corner is
  the **top 2 px of the monitor's true top edge** — the strip carries an exclusive zone
  (beats wlroots' free-area placement below the PIXEL bar's reserved band), a
  DrawingArea-forced buffer (an empty transparent window never maps), and a solid dark
  fill (labwc composites layer surfaces opaque — no true transparency; see
  docs/wayland-layer-shell-findings.md), overlay in OVERLAY layer, placement anchor+margin. `Super+T`/`Esc`
  work via labwc rc.xml keybinds → `hover-clock` client (compositor-native; app-side
  global shortcuts don't exist on Wayland — §16). Single binary, one
  artifact per arch (no X11/Wayland split, §16). Hand-off:
  [handoffs/07-m7-wayland.md](handoffs/07-m7-wayland.md).
- **Goal:** Wayland layer-shell backend behind the existing `WindowBackend` /
  `ActivationBackend` contracts.
- **Deliverables:** layer-shell `WindowBackend`; pointer/shortcut `ActivationBackend`.
- **Acceptance:** same behavior contract on a Wayland compositor; X11 path unchanged.
- **Note:** layer-shell surfaces are compositor-managed and **not workspace-bound** — a
  workspace switch never touches the overlay, so the xfwm4 re-map flicker (S04 hand-off)
  disappears by construction; layer-shell **anchoring** (top-right + margins) also replaces
  the X11 placement hack natively. GNOME/Mutter does not implement layer-shell (§17.3).
- **Design refs:** §10, §15 (layer-shell anchor/layer choice)
- **Hand-off:** [07-m7-wayland.md](handoffs/07-m7-wayland.md)

### S09 — Version notification widget ✅ 2026-08-17

- **Status:** ✅ 2026-08-17 — check + click-to-update landed and verified through two real
  release cycles (self-updated 1.0.0 → 1.1.0 → 1.2.0) and the dev-mode button loop; hand-off:
  [handoffs/06-s09-version-update.md](handoffs/06-s09-version-update.md).
- **Goal:** When a newer release exists, the overlay shows a small non-intrusive widget
  (e.g. under the clock) — click to upgrade.
- **Deliverables:** ~~version check against the GitHub releases API with offline-first
  degradation~~ (done); ~~click-to-upgrade action (downloads the release tarball, verifies
  the checksum, replaces the binary, restarts the daemon)~~ (done); refresh policy (hourly,
  landed).
- **Acceptance:** widget appears only when a newer version is known; offline shows nothing;
  click upgrades and restarts the daemon without disturbing the session.
- **Design refs:** §2 (widget growth path), §7.4 (data plane, later), §13 (footprint —
  low-frequency check, no polling)
- **Hand-off:** pending

### S10 — Versioning & release (1.0.0) ✅ 2026-08-17

- **Status:** ✅ 2026-08-17 — `v1.0.0` tagged on main, release published (x86_64 +
  aarch64 tarballs + SHA-256), pipeline verified end-to-end. dev merged into main; the
  project now works directly on main (stale dev branch to be deleted).
- **Goal:** Turn the dev-branch work into a numbered, published release: versioning
  convention, main-gated release pipeline live, `v1.0.0` tagged and published, main in sync
  with dev.
- **Context:** main is still the initial 0.1.0 clock example; all M1–M3 + control-plane work
  lives on dev (which also carries `release.yml`, currently dev-only). No tags or releases
  exist yet. The interim version label (`src/version.rs`) checks a local `Cargo.toml`;
  S09 replaces it with the GitHub-releases check.
- **Deliverables:** ~~merge dev → main~~ (done); semver policy (Cargo.toml as single source
  of truth; bumped to 1.0.0); ~~tag `v1.0.0` on main → GitHub release~~ (done);
  ~~end-to-end upgrade check~~ (done).
  History-leak decision: **accepted, left as is** (early AGENTS.md commits reference
  `../llm_profiles/...`; already pushed, low practical risk — no rewrite).
- **Acceptance:** `v1.0.0` tag on main produces a release; installed daemon shows `v1.0.0`
  (white); main reflects dev's feature set; upgrade path verified end-to-end.
- **Design refs:** README Releases section, `.github/workflows/release.yml`, S09
- **Hand-off:** pending
- **Hand-off:** pending

## Later (private exploration)

Notifications, toast messaging, template-driven widgets, socket data-plane API, overlay-shell
direction. Out of scope for this public repository.
