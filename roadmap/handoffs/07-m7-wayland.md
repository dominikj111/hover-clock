# Handoff 07 — S08 (M7 Wayland layer-shell backend)

**Task:** S08 — M7 Wayland. Roadmap status: ⬜ backlog → ✅ delivered (2026-08-22).
The roadmap's official next story is S05 (M4 Calendar); S08 was pulled forward at the
user's direction.

## What was done

- `src/backend/wayland.rs` (new) — `WaylandWindowBackend` (layer-shell overlay: OVERLAY
  layer, keyboard mode NONE, exclusive zone 0, placement via `set_monitor` + anchor
  margins computed from the controller's absolute coords) and `WaylandActivationBackend`
  (one transparent 4 px top-edge sensor strip per output; enter/leave crossing events →
  `CornerEntered`/`CornerLeft`; `set_overlay_visible` is a documented no-op — no
  dismissal-key grab exists on Wayland). The strip needs two non-obvious mechanisms to
  work (see "What was done differently" §2–3): an **exclusive zone** for true-edge
  placement and a **DrawingArea-forced buffer** so the compositor maps it.
- `src/backend/mod.rs` — `wayland` module (feature-gated), `Backends` type alias,
  `build_backends()` factory: `gtk4_layer_shell::is_supported()` → Wayland, else X11
  (XWayland/GNOME fallback unchanged). `ActivationBackend` trait gained
  `install_event_source` (now `Result<(), String>`); `WindowBackend` gained a defaulted
  `prepare()` (layer-shell must init pre-realize — the library hooks realize internally).
- `src/backend/x11.rs` — `install_event_source` moved from inherent method into the trait
  impl; behaviour byte-identical.
- `src/main.rs` — factory + `Rc<dyn ActivationBackend>`/`Option<Rc<dyn WindowBackend>>`
  in `OverlayController` (consumers depend on contracts, JigsawFlow); `prepare()` +
  realize-hook wiring for both backends.
- `Cargo.toml` — `wayland` cargo feature (default-on) gating `gtk4-layer-shell` (system
  lib `libgtk4-layer-shell`, pkg-config `gtk4-layer-shell-0`); `--no-default-features` =
  X11-only build (bookworm etc.).
- Docs in the same change: proposal §9.2, §10, §15 (Wayland questions resolved), §16
  (six M7 decisions incl. the single-binary no-split decision), §17.1 (build deps),
  §17.3, §17.6; WAYLAND_TESTING.md (M7 smoke + caveats); DEPLOYMENT.md; README (badges,
  requirements, Wayland status, tested-environments row); ci.yml + release.yml
  (`libgtk4-layer-shell-dev`, X11-only gate job).

## What was done differently

1. **No global shortcut — degraded, not implemented.** `ext_global_shortcuts_v1` is an
   **unmerged upstream MR** (verified: absent from wayland-protocols staging and stable;
   absent from labwc 0.9.8). The GlobalShortcuts portal has no wlroots backend.
   `Super+T`/`Esc` degrade to a logged warning; corner + IPC drive the overlay (§16).
   The plan promised ext_global_shortcuts via wayland-client — the protocol target
   vanished; per minimalism + verified-references, the shortcut was dropped rather than
   vendoring an unstable MR protocol. Revisit when it lands upstream.
2. **Strip placement discovery (the corner bug).** wlroots places a non-exclusive layer
   surface in the layer's *free* area: with the Pi OS PIXEL bar (`wf-panel-pi`,
   ~36 px exclusive band at the top) a plain top-anchored strip landed at y36, not y0 —
   in both TOP and OVERLAY layers. The fix is an **exclusive zone** on the strip (4 px):
   it then lands in the layer's *exclusive* area at the output's true top edge — y0–3,
   over the bar, the same gesture as X11. (Earlier I wrongly concluded y36 was the
   correct Wayland semantic and made the strip 12 px tall below the bar; the user
   pushed for the true top edge and the exclusive zone delivered it.)
3. **The transparent strip must commit a buffer.** An empty GtkWindow with transparent
   CSS does `wl_surface.commit()` *without* `attach` — no buffer, so wlroots never maps
   the surface and it receives no input (this is why the red probe strips worked and
   the transparent one didn't — the user's hunch "you cannot have invisible layer/area"
   was mechanistically right). Fix: a `DrawingArea` with a no-op draw func forces GTK to
   render a transparent ARGB buffer every frame → mapped → input works, still invisible.
4. **Single binary, one artifact per arch — no X11/Wayland split** (user question,
   recorded §16). Measured: max droppable = 112 KB (`x11rb`) of a 4.08 MB binary;
   runtime is GTK-dominated (11 MB, dual backends internal). Split would double the
   release matrix + install-time session selection for ~2.7% binary savings.
5. **Pointer injection impossible on labwc** — XWayland XWarpPointer/XTEST move the X
   pointer (verified) but wlroots does not forward to the Wayland seat (zero
   `wl_pointer` events observed). `/dev/uinput` is root-only. Corner verification
   required the user's physical mouse.
6. **Process footgun (mine, twice):** a `cargo build --no-default-features` gate run
   overwrote `target/debug/hover-clock` with the X11-only binary; the daemon then ran
   X11 backends in the Wayland session — no strips (corner dead) and the overlay mapped
   as a plain window (taskbar icon). The X11-only check must always be followed by a
   default-features rebuild before running anything.
7. **X11 path not live-verified in this session** — the machine only runs the labwc
   session. The X11 factory arm is code-moved (same logic); unit tests + X11-only build
   green, but a live xfwm4 smoke is still owed (see Open questions).

## Verification

- `cargo +1.92.0 fmt --check`, clippy `--all-targets -D warnings` (0), `cargo test`
  (12/12), `cargo build --locked` (default + `--no-default-features`) — green.
- Live on labwc 0.9.8 (Wayland): overlay maps as a layer surface — OVERLAY layer,
  namespace `hover-clock`, 324×161 at margins (478, 179) = centred above monitor middle,
  −100 px (protocol-log + pixel evidence). IPC `show`/`hide` clean. Degradation warning
  fires once. Strip: valid 1280×12 layer surface, configured + acked.
- **Corner verified with a physical mouse** (user): dwell in the top 4 px at the true
  top edge (over the PIXEL bar) → clock fades in; auto-hide on leave confirmed. The
  red-probe strips (24 px, 12 px) and the final invisible exclusive-zone strip all
  fired; the transparent strip without a buffer did not (see §3 above).
- Auto-hide on leaving the band: exercised via the same crossing path (leave → debounced
  hide), same dispatch code as the verified show.

## Open questions

- **X11 live regression smoke** (xfwm4): hot corner, `Super+T`, `Esc`, taskbar
  invisibility — run on the X11 session before the next release to confirm the code-move
  didn't regress anything.
- Multi-monitor: single output here; `set_monitor` switching pre-map and the per-output
  strips are implemented but untested with a second monitor.
- **Clock placement vs X11:** layer-shell margins are interpreted relative to the
  layer's free area (not the output edge), so on desktops with a reserved top band the
  clock sits ~36 px lower than the X11 formula (monitor-centre −100 px). Cosmetic;
  revisit with S05 config if it matters.
- Strip height/placement tuning is a §15 open value (config comes with S05).
- GNOME Wayland remains unsupported by design (no layer-shell, §17.3).

## Next step

Per roadmap: S05 (M4 Calendar widget) — month grid layout, today highlighted, composed
next to the clock. Or the user's direction (e.g., the X11 regression smoke above, or a
release).
