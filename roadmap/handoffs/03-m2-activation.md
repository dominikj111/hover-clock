# Hand-off — S03 M2 Activation

- **Story status:** ✅ 2026-08-10 (confirmed done)
- **Next story:** S04 M3 Presentation

## Delivered

- `src/backend/mod.rs` — `ActivationBackend` facade contract (proposal §10): `start()`,
  `set_overlay_visible()`; `ActivationEvent` (`CornerEntered`/`CornerLeft`/`Toggle`/`Dismiss`)
  and `Monitor` value types. Edge-triggered; debounce is policy, lives in the consumer.
- `src/backend/x11.rs` — `X11ActivationBackend`:
  - XI2 pointer motion (`XISelectEvents`, `XI_Motion`, all devices) on the root window —
    per-client selection, event-driven, no polling (§13).
  - Hot corner: top-right 4 px region of every monitor (RandR 1.5 monitors, fallback to root
    geometry), edge-triggered enter/leave state machine.
  - `Super + T`: core passive grab for the daemon's lifetime, all four NumLock/CapsLock
    lock-state combinations. `Esc`: grabbed only while the overlay is visible, released on hide.
  - `install_event_source()`: glib source watching a dup of the X connection fd (`gio::Socket`),
    events arrive on the main context with zero polling, zero extra threads.
- `src/main.rs` — glue: overlay starts **hidden**; corner entry starts a 200 ms dwell timer
  (canceled on corner leave) before showing; `Super + T` toggles; `Esc` dismisses. Activation
  failures degrade to S02 behavior (overlay always visible) with a logged warning.
- `Cargo.toml` — `gio` (fd source), `x11rb` features `randr` + `xinput`.

## Decisions & deviations

1. **No root-window core event mask.** The first attempt selected `POINTER_MOTION|KEY_PRESS` on
   the root via `ChangeWindowAttributes` (read-mask-then-OR, to preserve the WM's bits) — the
   server answered `BadAccess`: the WM owns `SUBSTRUCTURE_REDIRECT`, and the mask replacement
   would have clobbered it. Switched to **XI2 `XISelectEvents`**, whose selection is per-client
   and independent of the core mask. Keyboard stays on core `XGrabKey` (grabbed keys deliver
   without any event selection), so XI2 selects `XI_Motion` only — selecting XI key events too
   would double-deliver keys that the core grab already routes.
2. **`gio::Socket` `create_source`, not `DatagramBased`.** `g_datagram_based_create_source` on
   a `GSocket` returns `NULL` (GLib-CRITICAL, process died). `g_socket_create_source`
   (`SocketExtManual`) is the correct GSocket source. The fd is dup'd because `Socket::from_fd`
   takes ownership.
3. **Dwell debounce lives in the glue (main.rs), not the backend** — the backend reports
   edge-triggered enter/leave; the 200 ms dwell timer is show/hide policy. This keeps the
   facade free of timing policy and lets S04's auto-hide timer reuse the same `CornerLeft` event.
4. **`poll()` was dropped from the trait** during implementation (dead code): the X11 event
   source is the delivery mechanism for S03; a poll-based contract adds nothing yet. Revisit
   when a second backend or tests need it.
5. **Unspecified defaults** (per the workspace uncertainty rule): corner = top-right of every
   monitor, 4 px region, 200 ms dwell. Overlay placement was deferred to S04.
6. **GTK `Application` handoff trap** (verification): a stale first instance holds the
   `application_id`; new instances register to it and exit silently, so the tested binary was
   never the running one. Kill stale instances by exact name (`pkill -x hover-clock`) before
   iterating.

## Known issues / follow-ups

- **Overlay placement**: xfwm4 places the notification window near the screen center (verified
  `+564+344`, `+568+380` on 1366x768), not at the trigger corner. S04 needs a placement
  mechanism: GTK4 has no `move()`; `present_at()` steals focus and is banned, so placement
  should go through the X11 `WindowBackend` or a dedicated backend call. `CornerEntered` already
  carries the `Monitor` for placement.
- **Corner re-entry while visible** is a harmless no-op (`set_visible(true)` on a visible
  window); re-entry after `CornerLeft` re-fires. S04's auto-hide interacts here: leaving the
  corner should start the auto-hide timer, not dismiss instantly (§5: "move mouse away →
  auto-hide (debounced)").
- `keysym::T` scans the keyboard mapping for `XK_T`/`XK_t`; exotic layouts where the T key has
  neither are unsupported (grab silently skipped — hot corner still works).
- `application_id` is still the S01 leftover (`com.github.gtk-rs.examples.clock`); revisit with
  the binary's real identity (S07/M6).

## Hand-off to S04

Start **S04 — M3 Presentation**: full clock widget (time/day/date labels per §11), CSS styling,
show/hide transitions, auto-hide timer wired to `CornerLeft` (debounced per §5), and placement
at the triggered monitor's top-right. Consult proposal §11/§13 first. The `Monitor` in
`CornerEntered` is already in the event contract for placement; the gio fd-source pattern in
`install_event_source` is the model for any further event-driven backend work.
