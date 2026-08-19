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
| HoverClock current X11 build | XWayland — runs with *degraded* stacking (§17.3) |

GTK4 apps run native Wayland by default; force explicitly with
`GDK_BACKEND=wayland`.

## Baseline smoke of the current build (before M7)

In the labwc session:

```bash
cargo run -- -s
```

Expected per §17.3: activation works (XWayland synthesizes root motion, key
grabs pass through), stacking degraded (overlay never above native Wayland
fullscreen surfaces), workspace tracking unavailable and degrading
gracefully. That is the XWayland baseline M7 replaces.

## Caveats

- **No desktop chrome**: no panel, tray, icons, or app menu. Launch via
  `Super+Return` (foot) or fuzzel.
- **Screenshots**: use `grim` + `slurp` — XFCE screenshot tools do not work
  on Wayland.
- **No X11 global shortcuts**: the M7 `ActivationBackend` uses the native
  global-shortcuts protocol, not X key grabs.
- If GTK theming looks wrong: `sudo apt install xdg-desktop-portal-gtk
  xdg-desktop-portal-wlr`.
- X11 remains the daily driver and the primary smoke-test surface (xfwm4);
  the labwc session is a *testing/embedded* surface, not a second desktop.
