# HoverClock

HoverClock is a lightweight Linux overlay daemon that exposes time (and future widgets) via two activation methods:

- **Hot-corner trigger** (mouse enters the top-right screen corner)
- **Global keyboard shortcut** (`Super + T` toggles the overlay)

It is designed for fullscreen-first workflows where permanent panels and desktop widgets are undesirable.

The overlay is **transient**, **non-focus-stealing**, and appears **above fullscreen applications**. It never shows up in the taskbar, task switchers, or pager.

> **Status:** Early-stage architecture. Core focus is correct overlay behavior and the input activation system. The widget system is intentionally minimal and extensible.

---

## Key Features

- Hot-corner activation (mouse-driven reveal)
- Global shortcut toggle (`Super + T`)
- `Esc` / mouse-away auto-hide
- Fullscreen-safe overlay (no focus capture, not in task switchers)
- Minimal digital clock UI (expandable to widgets)
- X11-first architecture (Wayland layer-shell abstraction planned)
- GTK4-based rendering with CSS singleton-registrystyling
- **JigsawFlow-aligned composition** via the `singleton-registry` crate (flat capability registry, graceful degradation, facade-wrapped dependencies)
- **Daemon + client command pattern** (workmeshd-inspired): control the overlay over a Unix socket (`ping`, `show`, `hide`, `toggle`, `status`, `stop`, ...)

## Interaction Model

| Action | Result |
| --- | --- |
| Move mouse to top-right corner | Overlay appears |
| Press `Super + T` | Toggle overlay |
| Press `Esc` | Hide overlay |
| Move mouse away | Auto-hide |
| `Esc` + left-click | Hide overlay |
| `hoverclock show` / `hide` / `toggle` | Drive overlay state via socket |

## Architecture Overview

HoverClock is structured into four core subsystems, composed through a **flat singleton registry** (JigsawFlow pattern):

- **Activation backend** — hot-corner + keyboard input handling
- **Overlay manager** — window lifecycle (show / hide / state)
- **Widget layer** — clock + future modules (calendar, system info)
- **Config system** — TOML-based runtime configuration
- **IPC** — Unix socket server + built-in client module (daemon/client pattern)

Rendering is handled via the GTK4 widget tree (no custom canvas required for the initial implementation):

```text
hoverclock                        (single binary, dual-mode)
 ├─ CLI (clap)                    — daemon mode vs client mode
 ├─ Singleton registry            — flat capability registry (singleton-registry)
 ├─ Activation service            — hot-corner + global shortcut
 ├─ Config service                — TOML runtime configuration
 ├─ IPC server                    — Unix socket listener (control plane)
 ├─ Overlay window
 │    ├─ Clock widget
 │    └─ Calendar widget
 └─ Client module                 — sends commands to the daemon socket
```

All system-level interaction passes through explicit trait contracts (`ActivationBackend`, `WindowBackend`, `TimeSource`, `IpcServer`) registered in the singleton registry, so Wayland support can be added without a rewrite and optional capabilities degrade gracefully when absent.

## Controlling the Daemon

The daemon listens on a Unix socket (`${XDG_RUNTIME_DIR}/hoverclock.sock` by default). The same binary acts as a client when given a command:

```bash
hoverclock ping          # liveness check
hoverclock status        # overlay state, backends, widgets
hoverclock show          # show overlay
hoverclock hide          # hide overlay
hoverclock toggle        # toggle overlay
hoverclock version       # daemon version
hoverclock stop          # graceful shutdown
```

## Requirements

- Linux (X11 required for the initial version)
- GTK4
- Rust toolchain (stable)

Crates:

- `gtk4` (GTK4-rs)
- `singleton-registry` (JigsawFlow composition primitive)
- `chrono` (system-clock time formatting)

Optional:

- `gtk4-layer-shell` (future Wayland support)

## Build

```bash
git clone https://github.com/<your-org>/hoverclock.git
cd hoverclock
cargo build --release
```

## Run

```bash
cargo run --release          # daemon mode
cargo run --release -- ping  # client mode
```

The current milestone (M0) is a basic GTK4 clock window. Overlay behavior (EWMH hints), hot-corner detection, the global shortcut, and the IPC control plane land in M1–M5 — see `proposal.md` for the roadmap.

## Configuration

TOML-based runtime configuration is planned (M4): hot corner, shortcut, debounce, auto-hide timing, style overrides, and socket path. Configuration reload must not reset the runtime overlay state; the config singleton is hot-swapped in the registry.

## Project Status & Roadmap

| Milestone | Scope |
| --- | --- |
| M0 — Current | GTK4-rs scaffold, clock label, classic window |
| M1 | Overlay behavior: EWMH hints, non-focusable window |
| M2 | Activation: hot-corner + global shortcut + `Esc` |
| M3 | Full clock widget, CSS styling, auto-hide |
| M4 | Registry & config: singleton-registry composition, facade contracts, TOML live reload |
| M5 | IPC: dual-mode binary, Unix socket, command registry, client module |
| M6 | Wayland layer-shell backend |

Later exploration (notifications, toast messaging, socket data-plane API, overlay-shell direction) is out of scope for the public repository and will be developed privately.

## Documentation

- [`AGENTS.md`](./AGENTS.md) — binding constraints for AI contributors (always loaded; runtime kernel)
- [`proposal.md`](./proposal.md) — design planner / offline spec (loaded selectively for structural changes)

## License

TBD — this is an initial open-source exploration project.
