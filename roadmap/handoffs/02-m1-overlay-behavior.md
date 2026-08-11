# Hand-off — S02 M1 Overlay Behavior

- **Story status:** ✅ 2026-08-07 (confirmed done)
- **Next story:** S03 M2 Activation

## Delivered

- `src/backend/mod.rs` — `WindowBackend` facade contract (proposal §10).
- `src/backend/x11.rs` — `X11WindowBackend`: dedicated x11rb connection applying
  `_NET_WM_WINDOW_TYPE_NOTIFICATION`, `_NET_WM_STATE` (ABOVE/SKIP_TASKBAR/SKIP_PAGER), ICCCM
  `WM_HINTS input=False`.
- `src/main.rs` — backend configured at realize time (pre-map, so the WM reads hints at
  manage); window shown via `set_visible(true)` instead of `present()`.
- `Cargo.toml` — added `gdk4-x11` (X11 surface XID), `x11rb` (pure-Rust X11 connection).

## Decisions & deviations

1. **`present()` replaced by `set_visible(true)`** — *not* in the M1 plan. GTK4's `present()`
   unconditionally calls `gdk_toplevel_focus` → sends `_NET_ACTIVE_WINDOW` → the WM focuses the
   window (verified in xfwm4 source: `clientActivate` honors it when `click_to_focus` is set).
   This single change made the overlay stop stealing focus. Keep it: never call `present()` for
   the overlay (§6 deviation, absorbed into AGENTS.md rules).
2. **`WM_HINTS` must be re-applied post-map.** GTK rewrites `WM_HINTS` when it shows the surface
   (`gdk_x11_surface_show` → `set_initial_hints`), clobbering any pre-map write. The backend now
   re-writes `input=False` on every map + one 300 ms retry; xfwm4 re-reads `WM_HINTS` on
   PropertyNotify, so future focus requests are refused.
3. **`_NET_WM_STATE` via EWMH client message, not direct write.** The WM owns the property after
   manage (direct writes get clobbered). xfwm4 additionally reads the property at manage time
   (`clientGetNetState`), so the pre-map direct write *and* the post-map client message are both
   kept — each covers a different WM behavior.
4. **Cross-connection races.** Our x11rb connection is separate from GTK's X connection; any
   write that must beat the map request is inherently racy. Realize-time writes (window type)
   worked; GTK-rewritten properties (WM_HINTS) could not be won pre-map — hence the post-map
   strategy. Do not reintroduce pre-map WM_HINTS as the primary mechanism.
5. **Timing of `mapped_notify` is client-side.** GDK sets `is_mapped` during `present()` before
   the server maps the window — so it is *not* a post-manage signal. The 300 ms retry covers the
   manage race. If M3 (show/hide) needs a reliable post-manage hook, select PropertyChange on the
   window and watch for the WM's `_NET_WM_STATE` write instead of adding more timers.

## Known issues / follow-ups

- **Stacking above fullscreen** was verified only from source (xfwm4 puts NOTIFICATION windows in
  `WIN_LAYER_NOTIFICATION`, above fullscreen). No fullscreen app was running to confirm live.
- **Alt-tab / taskbar invisibility** follows from the EWMH contract; not exercised live (no
  interactive session during testing).
- **Follow-up (taskbar flash fixed):** live use showed the overlay's icon briefly in the tasklist
  on every show. Root cause: GDK's show path (`set_initial_hints` in `gdksurface-x11.c`) rebuilds
  `_NET_WM_STATE` from GDK's own toplevel state and *deletes* the property when that state is
  empty — a direct pre-map write never reaches the WM's manage read; the tasklist (libwnck)
  only excludes `_NET_WM_STATE_SKIP_TASKBAR`, not the NOTIFICATION type. Fix: set GDK's X11
  skip hints (`gdk_x11_surface_set_skip_taskbar_hint`/`_skip_pager_hint`, exported, deprecated
  since 4.18) at realize, so GDK writes the atoms on its own connection before the map request.
  Verified: state present at MapNotify, no tasklist entry across 8 show/hide cycles, `Esc`/
  `Super + T`/corner unaffected.
- M1 did **not** adopt `singleton-registry` (that is S05/M4 by roadmap) — `WindowBackend` is a
  plain trait used directly; the registry will register it later.
- `application_id` still says `com.github.gtk-rs.examples.clock` (S01 leftover) — revisit when
  the binary gets its real identity (S06/M5).

## Hand-off to S03

Start **S03 — M2 Activation**: `ActivationBackend` contract (hot-corner detection debounced and
edge-triggered, global `Super + T`, `Esc` dismiss). Consult proposal §5/§10/§15 first — open
questions (hot-corner geometry on multi-monitor, debounce values, overlay placement) are
unanswered (per the workspace uncertainty rule, default them to common practice). Note: Esc
handling on X11 needs a keyboard grab or pass-through key handling; the overlay must never take
focus, so `Esc` cannot rely on window focus (this story's `input=False` guarantees that).
