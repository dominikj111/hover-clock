# Wayland layer-shell findings — HoverClock M7 (empirical)

Field notes from building a transient GTK4 overlay's Wayland backend. Stack: Raspberry Pi 4
(aarch64), Pi OS trixie, **labwc 0.9.8** (+xwayland), gtk4-rs 0.11.4, gtk4-layer-shell 0.8.1
(Rust) / libgtk4-layer-shell 1.0.4 (C), wlroots compositor. Everything below was verified
live with a physical pointer and a `WAYLAND_DEBUG=1` protocol trace; nothing is assumed.

Context: the app is a transient overlay daemon (clock). It needs (a) a hidden-until-trigger
overlay window and (b) an activation trigger at the monitor's top edge (hot corner). No
global pointer-position API exists on Wayland, so the trigger is a dedicated layer-shell
surface. Section order = the order the findings bit us.

---

## 1. Architecture that works: trigger surface and clock surface are separate

```
OUTPUT TOP EDGE (y0)
┌───────────────────────────────────────────┐
│██ trigger strip (2–4 px, layer-shell)     │ ← input-capturing band, always mapped
└───────────────────────────────────────────┘
                     │ pointer dwell (debounced)
                     ▼
        ┌─────────────────────┐
        │  clock overlay       │ ← separate layer-shell surface, OVERLAY layer
        └─────────────────────┘
```

- Both are `zwlr_layer_shell_v1` surfaces via gtk4-layer-shell (not xdg-toplevels).
- The trigger strip is always mapped and **is its own input region**; enter/leave crossing
  events (`gtk::EventControllerMotion` on the strip window) drive the show/hide state
  machine. The clock surface maps only while visible.
- Layer choice: **OVERLAY** for both (the protocol's topmost layer; stacking above
  fullscreen is compositor-guaranteed — no EWMH-style hints exist on Wayland).
- `keyboard_interactivity = NONE` on both: the overlay never takes focus.

## 2. A layer surface gets input only when it is **mapped** — and mapping requires a **buffer**

The single biggest gotcha. `wl_surface` input delivery requires the compositor to consider
the surface mapped. A wlroots compositor maps a layer surface only after the client
attaches a **buffer** and commits.

An empty `GtkWindow` with transparent CSS never renders anything, so GTK does
`wl_surface.commit()` **without `wl_surface.attach()`** — no buffer — and the compositor
never maps the surface. Result: **no input, ever**, no matter the input region. The surface
appears "dead".

Evidence (WAYLAND_DEBUG): the failing strip showed `-> wl_surface#44.commit()` with no
`attach` line; the working one showed
`-> zwp_linux_buffer_params_v1#59.create_immed(wl_buffer#60, 1280, 4, <AR24>, 0)` followed
by `wl_surface#44.attach(wl_buffer#60, 0, 0)`.

Fix: force GTK to produce a buffer every frame — a `gtk::DrawingArea` with a draw func:

```rust
let holder = gtk::DrawingArea::new();
holder.set_size_request(1, HOT_STRIP_HEIGHT);
holder.set_draw_func(|_, cr, w, h| {
    cr.set_source_rgb(0.05, 0.05, 0.05);
    cr.rectangle(0.0, 0.0, w as f64, h as f64);
    cr.fill();
});
strip.set_child(Some(&holder));
```

Caveat: `gtk_window_set_opacity(0.0)` on the strip makes GTK skip rendering entirely →
back to commit-without-attach → unmapped → no input. Do not try opacity-0 "invisibility".

## 3. Layer surfaces composite **opaque with baked alpha** on this stack — true invisibility is not achievable

The buffer carries alpha (we observed an AR24/ARGB2101010 dma-buf), but the composited
result ignores blending. Evidence: a 50%-alpha green fill renders as **solid (0,128,0)** —
exactly "green pre-darkened by 0.5", with zero contribution from the panel beneath. A
fully-transparent fill (alpha 0) renders as solid black. The same is true of the clock
card's `rgba(0,0,0,0.8)`: it looks correct only because a black card on a dark desktop is
indistinguishable from an opaque one.

Consequence: **an invisible input-only layer surface is not achievable via GTK4 + this
compositor today.** The trigger surface must carry *visible* content to map (and any
content composites opaque). Design accordingly: a thin solid band in the theme's color
reads as an intentional bezel/edge line. (Whether this is labwc-specific or GDK choosing
an opaque EGL config is unresolved — see Open items.)

## 4. Placement: exclusive zone vs free area — getting to the *true* output edge

A layer surface's position is decided by the compositor. On wlroots:

- **Non-exclusive** surfaces are placed in the layer's **free area** — i.e. *below* any
  reserved chrome. With the Pi OS PIXEL bar (`wf-panel-pi`) reserving ~36 px at the top, a
  plain top-anchored strip landed at **y36**, not y0 — in both TOP and OVERLAY layers.
- **Exclusive** surfaces (nonzero `exclusive_zone`) are placed in the layer's exclusive
  area, at the output's actual edge.

Fix: give the strip an exclusive zone **equal to its height** (2 px on a 2 px strip): it
lands at **y0–1**, over the bar, matching the X11 corner's "push to the top bezel"
gesture. Keep zone == height: a larger zone shifts everything below it down by the
difference and leaves a visible desktop gap between the strip and the next surface.

Related: layer-shell **margins are interpreted relative to the free area**, not the output
edge. With a reserved top band, a clock placed at `margin_top = monitor_cy − …` sits ~36 px
lower than the same formula on X11. Cosmetic; worth knowing when comparing placements
across sessions.

## 5. Input region: default is the whole surface

`wl_surface.set_input_region` defaults to **infinite** (whole surface). GTK only narrows it
if you set an input shape. So a mapped strip receives pointer events over its entire area
with no extra calls — and **clicks in the band do not pass through** to whatever is beneath
(they hit the strip). This is the Wayland cost of a corner trigger: the X11 corner is
passive motion-watching; the Wayland strip necessarily captures its band. Keep the band
thin.

## 6. Global shortcuts: nothing portable exists yet

- `ext_global_shortcuts_v1` (KDE's proposal) is an **unmerged upstream MR** — absent from
  wayland-protocols staging and stable, absent from labwc 0.9.8 (verified by strings on the
  binary and the repo tree).
- The `org.freedesktop.portal.GlobalShortcuts` portal has **no wlroots backend**;
  xdg-desktop-portal-wlr implements Screenshot/ScreenCast only.

So `Super+T` / `Esc` have no implementation target on this compositor. They degrade to a
logged warning; corner + IPC drive the overlay. Revisit when the protocol or a portal
backend lands.

## 7. Testing reality: XWayland pointer injection does not reach the Wayland seat

`XWarpPointer` and XTEST (via XWayland) move the *X* pointer (verified with
`XQueryPointer`) but wlroots does not forward that to the Wayland seat — zero
`wl_pointer` events observed on any surface. `/dev/uinput` is root-only. **Physical
pointer required for trigger testing** on this compositor; there is no software
injection path without root.

## 8. Runtime backend selection & the "is_supported() lies" trap

The factory picks the backend with `gtk4_layer_shell::is_supported()` (true on Wayland
with layer-shell, false otherwise) → native Wayland, else the X11 backend (XWayland/GNOME
fallback). One real trap: if the binary was built **without** the `wayland` cargo feature,
`is_supported()` is compiled out and the app silently runs the X11 backends *inside* a
Wayland session — no strips, and the overlay maps as a plain xdg-toplevel (taskbar icon,
wrong stacking). A `--no-default-features` gate build can clobber `target/debug` and
produce exactly this. **Diagnose with `readelf -d <binary> | grep NEEDED`** — the
layer-shell library must be present (feature mistake looks identical to a shim/link
problem, and it usually isn't one).

## 9. Size: one binary, runtime selection — no X11/Wayland split

Measured on aarch64 release: binary 4.08 MB, of which the droppable X11 code (`x11rb`) is
112 KB (2.7%). Runtime is GTK-dominated (libgtk-4 ~11 MB, both backends compiled in,
not strippable per-app). Splitting releases would double the artifact matrix and force
install-time session selection (wrong for machines that boot both sessions) for ~2.7%
binary savings. Keep one artifact per arch; a `wayland` cargo feature (default-on) already
serves X11-only builders (`--no-default-features`).

## 10. The final working recipe (HoverClock)

- `WaylandWindowBackend`: overlay window → `init_layer_shell()`, namespace `hover-clock`,
  layer OVERLAY, keyboard NONE, exclusive zone 0, anchor top+left, placement via
  `set_monitor` + margins computed from the controller's absolute coords (converted to
  margins relative to the target output).
- `WaylandActivationBackend`: one strip window per output — `init_layer_shell()`, layer
  OVERLAY, anchor top+left+right, **exclusive zone = strip height** (2 px), keyboard NONE,
  DrawingArea with a
  solid fill (visible, per §3), `EventControllerMotion` enter/leave → semantic
  `CornerEntered`/`CornerLeft` events → the same debounced show/auto-hide glue as X11.
- Never focus; fade in/out via window opacity; Escape/Super+T absent on Wayland (§6).

## Open items

- Whether the opaque-with-baked-alpha compositing (§3) is labwc-specific or GDK choosing
  an opaque EGL config for the layer surface — test on sway/Hyprland/KWin when available.
- `set_monitor` switching pre-map and per-output strips: implemented, untested with a
  second monitor.
- Global-shortcut path: revisit when `ext_global_shortcuts_v1` lands upstream or a
  GlobalShortcuts portal backend ships.
