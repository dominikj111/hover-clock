# Wayland testing environment

How to stand up a Wayland session for the **M7 layer-shell backend** work
(proposal §17.3). Read this before starting M7 — it is the baseline against
which the native Wayland backend will be verified.

## Session setup (Debian 13 / MX 25, lightdm)

```bash
sudo apt install labwc foot fuzzel waybar xwayland
```

- **labwc** — wlroots compositor, explicitly an M7 target (§17.3).
- **xwayland** — required, and often *not* installed by default; without it
  X11-only apps (Thunar, GTK3) will not start at all under Wayland.
- **foot** — terminal; labwc's default `Super+Return` keybind expects it.
- **fuzzel** — app launcher (optional but handy, no panel by default).
- **waybar** — panel + system tray (optional).

Switch sessions at the greeter (MX greeter = lightdm-gtk-greeter): log out →
**Session** dropdown → **Labwc** → log in. Available Wayland sessions live in
`/usr/share/wayland-sessions/`.

Verify you are really on Wayland:

```bash
echo $XDG_SESSION_TYPE   # → wayland
echo $WAYLAND_DISPLAY    # → wayland-0
```

## What runs how

| App | Mode |
|-----|------|
| kitty, Firefox (≥ 121), recent Chrome | native Wayland |
| Thunar, other GTK3 / X11-only apps | XWayland (labwc auto-starts it) |
| HoverClock (post-M7) | native Wayland — layer-shell overlay (stacking above fullscreen), hot-corner strips; Super+T/Esc degraded (§16). The XWayland fallback path is kept |
| HoverClock current X11 build (pre-M7) | XWayland — runs with *degraded* stacking (§17.3) |

GTK4 apps run native Wayland by default; force explicitly with
`GDK_BACKEND=wayland`.

## Smoke after M7 (native layer-shell on labwc)

In the labwc session, from a terminal (`Super+Return` → foot):

```bash
cd <repo> && cargo run -- -s
```

Expected (verified on labwc 0.9.8, handoff 07):

- The daemon logs the §16 degradation warning once — `Super+T`/`Esc` have no portable
  global-shortcut protocol on Wayland and are unavailable; the hot corner is the
  trigger and corner-leave auto-hide the dismissal (IPC `show`/`hide`/`toggle` also
  drive the overlay).
- Dwell the pointer on the **top 2 px of the screen** (the strip sits at the monitor's
  true top edge, above any bar, via its exclusive zone; it is a thin dark line — see
  docs/wayland-layer-shell-findings.md §3 for why it cannot be invisible): the clock fades
  in centred above the monitor's middle (OVERLAY layer, above everything). Move away: it
  auto-hides after ~250 ms. The strip band captures clicks there (documented, §16).
- The overlay never takes focus (keyboard mode NONE) and never reserves workspace
  (no exclusive zone); stacking above fullscreen apps is compositor-guaranteed.

Baseline check of the *X11* path (unchanged): run the same binary in an X11 session
(xfwm4) — hot corner, `Super+T`, `Esc`, taskbar invisibility as before.

## Caveats

- **No desktop chrome**: no panel, tray, icons, or app menu. Launch via
  `Super+Return` (foot) or fuzzel.
- **Screenshots**: use `grim` + `slurp` — XFCE screenshot tools do not work
  on Wayland.
- **No global shortcuts on labwc**: the M7 `ActivationBackend` has no shortcut source —
  `ext_global_shortcuts_v1` is unmerged upstream and labwc does not advertise it
  (proposal §16). `Super+T`/`Esc` degrade with a logged warning; corner + IPC only.
  XWayland key grabs would work for keys labwc does not bind, but are the X11
  mechanism — deliberately not used (WAYLAND_TESTING.md commitment).
- **Pointer injection for testing does not work**: labwc/wlroots does not forward
  XWayland pointer warps (XTEST/XWarpPointer) to the Wayland seat — verify corner
  behaviour with a physical pointer.
- If GTK theming looks wrong: `sudo apt install xdg-desktop-portal-gtk
  xdg-desktop-portal-wlr`.
- X11 remains the daily driver and the primary smoke-test surface (xfwm4);
  the labwc session is a *testing/embedded* surface, not a second desktop.
