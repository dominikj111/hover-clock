# HoverClock

A transient Linux overlay daemon that surfaces widgets on demand — starting with a digital
clock — via hot-corner or global shortcut, above fullscreen applications, without ever
taking focus.

## Requirements

- Linux with an X11 session (Wayland planned)
- GTK4 development libraries (e.g. `libgtk-4-dev` on Debian)
- Rust stable toolchain (edition 2024)

## Run dev

```bash
cargo build
cargo run
```

The daemon starts with the overlay hidden. Move the pointer to the top-right corner (dwell
~200 ms) or press `Super + T` to show the clock; `Esc` hides it. The window never appears in
the taskbar and never takes focus.

## Third-party examples

- **GNOME Shell hot corner** — corner-triggered overview in the GNOME desktop
- **KDE Plasma screen edges** — edge/corner triggers for desktop actions
- **xfce4-hotcorner-plugin** — hot-corner actions for Xfce
- **Conky** — persistent X11 desktop overlay widgets (non-transient counterpoint)

## For contributors

- `docs/proposal.md` — design contract; read the § cited on the story card before each task
- `roadmap/ROADMAP.md` — current story, status, and hand-offs
- `AGENTS.md` — operating file for AI contributors

## License

TBD
