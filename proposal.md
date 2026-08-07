# HoverClock — Design Proposal

**Status:** Early-stage architecture · Core focus: overlay behavior and input activation system
**Scope:** Lightweight Linux overlay daemon (X11 first, Wayland planned)
**Version:** 0.2 (draft)

> **Reading policy:** This document is the *design planner / offline spec*. It is **not** read on every task.
> AGENTS.md is the runtime kernel and the sole source of binding constraints.
> See §15 *Document Loading Policy* for exactly when this file is consulted.

---

## 1. Purpose

HoverClock is a lightweight Linux overlay daemon that surfaces information — starting with a digital clock — **on demand**, via two activation methods:

- **Hot-corner trigger** (mouse enters the top-right screen corner)
- **Global keyboard shortcut** (`Super + T` toggles the overlay)

It is designed for fullscreen-first workflows where permanent panels and desktop widgets are undesirable. The overlay is **transient**, **non-focus-stealing**, and renders **above fullscreen applications**.

## 2. Vision

The clock is the **first widget**, not the final product. The widget layer is deliberately minimal and extensible. Intended growth path:

1. **Clock widget** — initial deliverable (time, day, date).
2. **Template-driven widgets** — simple application rendering following provided templates.
3. **Notifications & toast messaging** — time-aware transient messages.
4. **Long-term exploration** — a unified "overlay shell" that complements or gradually replaces classic Linux desktop panels and controls, providing a consistent experience.

The daemon is **controllable over a Unix socket** by a built-in client module (workmeshd-style daemon/client pattern). The socket first carries the *control plane* (show/hide/toggle/status), later the *data plane* (widget rendering, notifications, toast messages) for external processes.

> **Important scoping note:** The *current public repository* is intentionally just a GTK overlay clock tool. The broader OS-shell evolution is an open-source **exploration**; later-stage work and additional widgets will become **private** and are out of scope for this repository.

## 3. Goals

- Correct, robust overlay behavior on X11 (focus, stacking, visibility).
- Reliable input activation: hot-corner + global shortcut, edge-triggered.
- Architecture that stays widget-extensible without premature abstraction.
- Composition aligned with the **JigsawFlow pattern** via the `singleton-registry` crate (flat capability registry, graceful degradation, facade-wrapped external dependencies).
- A **daemon + client command pattern** (inspired by workmeshd) over a Unix socket for controlling the overlay and, later, driving widgets.
- Minimal CPU and memory footprint.
- A clean backend boundary so Wayland (layer-shell) can be added without a rewrite.
- X11 **and** Wayland support as a target for the platform layer.

## 4. Non-Goals (explicit)

- **Not** a desktop environment or window manager.
- **Not** a persistent panel replacement (overlay is summonable, never persistent).
- No app launcher / dock / taskbar functionality in scope.
- No web stack, no Electron, no web rendering.
- No custom canvas rendering for text-based widgets.
- No external/remote time sources; system clock only.
- No multi-process widget host in the initial phase.
- No distributed/network registry (JigsawFlow *Singleton Network* is explicitly out of scope).

## 5. Interaction Model

| Action | Result |
| --- | --- |
| Move mouse to top-right corner | Overlay appears (debounced) |
| Press `Super + T` | Toggle overlay |
| Press `Esc` | Hide overlay |
| Move mouse away from the overlay/corner | Auto-hide (debounced) |
| `Esc` + mouse left-click | Hide overlay (dismissal affordance) |
| `hoverclock show` / `hide` / `toggle` (client) | Drive overlay state over the socket |

All triggers are **edge-triggered** and debounced to avoid flicker loops.

## 6. Design Constraints

- Overlay must **never steal focus** (no input grab, no focus request).
- Must remain **visible above fullscreen applications**.
- Must **not appear** in task switchers, panels, or window lists.
- Must maintain a **minimal CPU and memory footprint**.
- Show/hide must feel instant (< 50 ms target perception).
- **Offline-first:** the overlay, clock, activation, and control plane must work with no network at all.

## 7. System Architecture

### 7.1 Core daemon structure

The first version is intentionally thin. Pure GTK4-rs — no Relm4, no additional framework layer. The daemon is a **single binary with two modes** (workmeshd pattern): run as daemon by default, or act as a client when a positional command is given.

```text
hoverclock                        (single binary, dual-mode)
 ├─ CLI (clap)                    — daemon mode vs client mode
 ├─ Singleton registry            — flat capability registry (singleton-registry)
 │    ├─ Config service
 │    ├─ Activation service
 │    ├─ Overlay service
 │    └─ IPC service
 ├─ Activation service            — hot-corner + global shortcut
 ├─ Config service                — TOML runtime configuration
 ├─ IPC server                    — Unix socket listener (control plane)
 ├─ Overlay window
 │    ├─ Clock widget
 │    └─ Calendar widget
 └─ Client module                 — sends commands to the daemon socket
```

There is not enough UI complexity yet to justify a state-management framework (Relm4 is deferred until component hierarchy and message-passing genuinely demand it).

### 7.2 Core / Overlay split (forward-looking)

```text
Core
 ├─ Activation
 ├─ Config
 ├─ Widgets
 ├─ IPC
 └─ Overlay
      ├─ X11 Window Backend
      └─ Layer-Shell Backend (Wayland)
```

Even though the first release targets X11, the overlay is designed around an **abstract backend** now, preventing a painful rewrite for Wayland later.

### 7.3 JigsawFlow composition (singleton-registry)

HoverClock follows the **JigsawFlow pattern**, supported by the `singleton-registry` crate as its core primitive. This means:

1. **Everything is a capability.** The application is composed through a flat, type-safe singleton registry. Components do not form hierarchies and do not call each other directly — they look up **trait contracts** (`Arc<dyn Trait>`) from the registry.

2. **Three binding requirements** (from JigsawFlow):
   - **Offline-First** — every capability must function with no network connectivity; network protocols are optional enhancements only.
   - **Component Independence** — components must not depend directly on other components; access is always via trait contracts through the registry. When a required capability is unavailable, the component **logs a warning and degrades gracefully** instead of failing (the crate's `try_get` is the canonical degradation path).
   - **Facade Pattern** — every external dependency (X11 calls, Wayland/layer-shell, GTK specifics, filesystem, environment, sockets) is wrapped behind a singleton-registry facade. Business logic never touches system APIs directly.

3. **Hot-swappable singletons.** Re-registering a contract atomically replaces the stored `Arc`; existing holders keep a valid reference (e.g., config reload replaces the config singleton without restart).

4. **Write-once, read-many.** All core contracts are registered at startup; runtime re-registration is limited to documented hot-swap points (configuration reload).

Example shape:

```rust
define_registry!(core);

// Contracts are traits; implementations are registered as trait objects.
core::register(Arc::new(X11WindowBackend::new(...)) as Arc<dyn WindowBackend>);
core::register(Arc::new(X11ActivationBackend::new(...)) as Arc<dyn ActivationBackend>);

// Consumers degrade gracefully when a capability is absent.
let backend = core::try_get::<Arc<dyn WindowBackend>>()
    .unwrap_or_else(|| { warn!("no window backend; overlay disabled"); /* fallback */ });
```

### 7.4 Daemon / client command pattern (workmeshd-inspired)

The daemon is controlled through a Unix socket by an included client module — the same pattern as workmeshd (`src/daemon.rs` listener + `src/client.rs` sender + `src/command` registry).

- **Single binary, dual mode:** no positional command → daemon mode; positional command (e.g. `hoverclock ping`) → client mode that forwards the command to the daemon over the socket.
- **Unix socket** at `${XDG_RUNTIME_DIR}/hoverclock.sock` (configurable via TOML).
- **Single-instance guard:** on startup, check the socket path — if a live listener exists, refuse to start; if the socket is stale, remove it and bind. PID file for clean shutdown bookkeeping (workmeshd pattern).
- **Line-based protocol:** request = `command arg1 arg2 ...\n`; response = text lines, terminated by EOF/flush. No framing beyond newlines for v1.
- **Command registry:** commands implement a shared `Command` trait (`name()`, `async execute(args, writer)`) and are registered in a `HashMap<String, Arc<dyn Command>>`. Unknown commands return a deterministic error line — never a crash.
- **Client retries:** bounded connection retries with backoff (workmeshd `max_retries` / `retry_delay_ms`), then a deterministic failure message.
- **GTK main-loop integration:** socket acceptance/reading must run on the GTK main context (glib IO watch, or an async runtime bridged to the GTK main loop). The UI thread must never block on socket I/O.
- **Baseline command set (control plane):**

  | Command | Effect |
  | --- | --- |
  | `ping` | Liveness check |
  | `show` / `hide` / `toggle` | Drive overlay state |
  | `status` | Report overlay state, backends, widget list |
  | `widget <name> [on|off]` | Enable/disable a widget (later) |
  | `config reload` | Reload TOML without resetting overlay state |
  | `version` | Daemon version |
  | `commands` | List registered commands |
  | `stop` | Graceful shutdown |

- **Degradation:** if IPC fails (socket busy, permission denied), the overlay itself must remain fully functional via hot-corner and shortcut.

## 8. Rendering Strategy

**Do not think in terms of a game-style canvas.** GTK provides a retained-mode scene graph; for a digital clock, custom drawing is unnecessary complexity.

### 8.1 Widget tree

```text
OverlayWindow
└─ GtkOverlay
   └─ GtkBox
      ├─ TimeLabel
      ├─ DayLabel
      └─ DateLabel
```

### 8.2 Render pipeline (managed by GTK)

```text
Widget Tree
    ↓
Snapshot
    ↓
GSK Scene Graph
    ↓
OpenGL / Vulkan / Cairo backend
```

GTK handles text layout, HiDPI, anti-aliasing, and GPU acceleration. Example clock label:

```rust
let time = gtk::Label::new(Some("08:14"));
time.add_css_class("clock");
```

```css
.clock {
    font-size: 48pt;
    font-weight: bold;
}
```

### 8.3 Styling

CSS controls the visual layer: rounded corners, blur-like appearance, padding, transparency, drop shadow — without writing rendering code.

### 8.4 When custom drawing is justified

Only for non-text visualization: analog clock, circular timer, graphs, waveforms, custom effects. That uses `gtk::DrawingArea` with `snapshot.append_color(...)` / `snapshot.append_texture(...)` or Cairo. **Out of scope for the initial clock.**

## 9. Overlay Window Behavior — Focus & Stacking

Rendering is the easy part. The difficult part is making the overlay behave correctly:

- Visible above fullscreen apps
- Not focusable
- Not in taskbar / alt-tab / pager
- No keyboard grab
- No interference with the active application's input

### 9.1 X11 (EWMH hints)

```text
_NET_WM_STATE_ABOVE
_NET_WM_STATE_SKIP_TASKBAR
_NET_WM_STATE_SKIP_PAGER
```

plus: do not request focus; use a non-focusable window type (`_NET_WM_WINDOW_TYPE_NOTIFICATION` / `_NET_WM_WINDOW_TYPE_UTILITY`). This part will consume far more engineering effort than drawing the clock.

### 9.2 Wayland (layer-shell)

[gtk4-layer-shell-rs](https://github.com/wmww/gtk4-layer-shell) is the intended target. Layer-shell was created precisely for panels, launchers, overlays, OSD widgets, and notification systems — exactly HoverClock's use case.

## 10. Backend Abstraction

All system-level interaction passes through explicit trait boundaries, **registered as registry facades** (JigsawFlow facade pattern). **No direct X11 calls inside UI logic.**

| Contract (trait) | X11 implementation | Wayland implementation |
| --- | --- | --- |
| `ActivationBackend` | Hot-corner + global shortcut (X11) | Pointer/global shortcut (Wayland) |
| `WindowBackend` | EWMH window hints | Layer-shell surface |
| `TimeSource` | System clock (`chrono::Local`) | System clock (shared) |
| `IpcServer` | Unix socket listener (shared) | Unix socket listener (shared) |

Consumers resolve these via the registry; absence of a backend degrades to a disabled overlay with a logged warning (never a crash).

## 11. Widget System

- **Clock widget** is the first and only initial widget (time, day, date labels in a vertical stack).
- Widgets are **pure UI components** with no system side effects.
- New widgets must **not** modify activation logic.
- Overlay layout remains composable — vertical stack model preferred.
- Calendar widget is the planned second widget (listed in the daemon sketch above).
- Later, widgets may be driven over the socket (data plane) — a widget contract (`WidgetProvider`) will be added when that lands.

## 12. Configuration

- TOML-based runtime configuration, registered as a registry singleton (`Config`).
- Configuration reload must **not** reset runtime overlay state.
- Misconfigured activation falls back to the shortcut-only mode.
- Socket path, retry policy, and widget enablement are configurable.

## 13. Performance Targets

- Idle CPU usage negligible (< 0.1% target).
- Memory footprint minimal (< 25 MB target).
- Overlay render path avoids per-frame allocations.
- No high-frequency polling loops for input detection (edge-triggered).
- IPC path must not add latency to the overlay show/hide path.

## 14. Milestones / Roadmap

| Milestone | Scope |
| --- | --- |
| **M0 — Current** | GTK4-rs project scaffold; single clock label; classic window (repo baseline). |
| **M1 — Overlay behavior** | EWMH hints (`_ABOVE`, `_SKIP_TASKBAR`, `_SKIP_PAGER`); non-focusable window; X11 `WindowBackend`. |
| **M2 — Activation** | X11 `ActivationBackend`: hot-corner detection (debounced, edge-triggered) + global `Super + T` + `Esc` dismiss. |
| **M3 — Presentation** | Full clock widget (time/day/date), CSS styling, show/hide transitions, auto-hide timer. |
| **M4 — Registry & config** | Adopt `singleton-registry`: flat capability registry, facade contracts (`WindowBackend`, `ActivationBackend`, `TimeSource`, `Config`), TOML config with live hot-swap reload. |
| **M5 — IPC (daemon/client)** | Dual-mode binary, Unix socket listener, `Command` registry, client module with retries; `ping`, `show`, `hide`, `toggle`, `status`, `version`, `commands`, `stop`. |
| **M6 — Wayland** | Layer-shell backend behind `WindowBackend` / `ActivationBackend` contracts. |
| **Later (private exploration)** | Notifications, toast messaging, template-driven widgets, socket data-plane API, overlay-shell direction. |

## 15. Document Loading Policy

If both files were always read, you'd create implicit duplication of constraints and a non-deterministic system under evolution. Therefore:

| File | Role | Loaded |
| --- | --- | --- |
| `AGENTS.md` | Runtime kernel — binding constraints | **Always** |
| `proposal.md` | Design planner / offline spec | **Selectively** |

**Read proposal.md only when:**
- structural architecture changes are requested
- a new subsystem or backend is introduced
- the UX interaction model is modified
- feature expansion beyond the existing widget scope is planned
- system-wide refactoring or migration is required

**Do NOT read proposal.md for:**
- local code changes
- UI styling changes
- bug fixes
- minor feature adjustments

AGENTS.md remains the sole source of runtime constraints; proposal.md is advisory and may be ignored unless explicitly triggered by the conditions above.

## 16. Open Questions

- Hot-corner geometry on multi-monitor setups: all monitors or primary only?
- Wayland hot-corner detection strategy (pointer constraints / global position APIs).
- Debounce and auto-hide timing values (defaults + configurability).
- Overlay placement: fixed corner vs. follows pointer corner?
- Socket path/ownership: `$XDG_RUNTIME_DIR` vs `/tmp`; socket permissions (0600?).
- Socket protocol evolution: newline framing vs JSON-RPC once the data plane lands (JigsawFlow's Phase-1 transport is TCP/localhost + JSON-RPC; workmeshd uses newline framing — v1 stays simple).
- Async runtime choice for IPC: tokio bridged to the GTK main loop vs glib IO watch only.
- Whether `Esc` alone is sufficient vs. `Esc` + left-click dismissal semantics on Wayland.
- Layer-shell anchor/layer choice (`OVERLAY` vs. `TOP`) for stacking above fullscreen.

## 17. Decision Log

| Date | Decision | Rationale |
| --- | --- | --- |
| Early stage | Pure GTK4-rs; **no Relm4** | Not enough UI complexity; avoid premature state abstraction layer. |
| Early stage | Widget-tree rendering + CSS, no custom canvas | Text-based overlay; GTK handles layout, HiDPI, GPU. |
| Early stage | Abstract `ActivationBackend` / `WindowBackend` | X11-first, but Wayland layer-shell ready without rewrite. |
| Early stage | X11 EWMH hints for overlay semantics | `_ABOVE`, `_SKIP_TASKBAR`, `_SKIP_PAGER`, no focus request. |
| Early stage | **Adopt JigsawFlow pattern + `singleton-registry`** | Flat capability registry, trait contracts, offline-first, graceful degradation, facade-wrapped dependencies — matches the overlay-daemon shape and keeps it extensible. |
| Early stage | **Daemon/client command pattern (workmeshd-inspired)** | Single binary, dual mode; Unix socket control plane with `Command` trait registry; proven pattern for controlling a long-lived daemon. |
| Early stage | proposal.md selectively loaded | Avoids constraint duplication; AGENTS.md stays the runtime kernel. |
