# HoverClock — Design Proposal

**Status:** Early-stage architecture · Core focus: overlay behavior and input activation system
**Scope:** Lightweight Linux overlay daemon (X11 first, Wayland planned)
**Version:** 0.3 (draft)

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
| `hover-clock` / `hover-clock show\|hide\|toggle` (client) | Drive overlay state over the socket (`show` is the no-arg default)

All triggers are **edge-triggered** and debounced to avoid flicker loops.

## 6. Design Constraints

- Overlay must **never steal focus** (no input grab, no focus request).
- Must remain **visible above fullscreen applications**.
- Must **not appear** in task switchers, panels, or window lists.
- Must maintain a **minimal CPU and memory footprint**.
- Show/hide must feel instant (< 50 ms target perception for trigger-to-visible latency).
  The M3 fade in/out is a short opacity transition (~100–150 ms) layered on top — the window
  is visible first, then fades — so perceived latency stays under budget.
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

- **Single binary, dual mode:** `--start` (alias `-s`; `--daemon` kept as a hidden
  compatibility alias for already-installed units) → daemon mode; any other
  invocation → client mode. No command defaults to `show` — a manual run renders the overlay;
  positional commands (`show`, `hide`, `toggle`, later `ping`, …) forward to the daemon over
  the socket.
- **Unix socket** at `${XDG_RUNTIME_DIR}/hoverclock.sock` (configurable via TOML).
- **Single-instance guard:** the daemon binds the control socket before GTK initializes — a
  second `--start` exits immediately with an explanatory error (never two daemons); a
  stale socket from a crashed daemon is reclaimed. Clean exit unlinks the socket.
- **Line-based protocol:** request = `command arg1 arg2 ...\n`; response = text lines, terminated by EOF/flush. No framing beyond newlines for v1.
- **Command registry:** commands implement a shared `Command` trait (`name()`, `async execute(args, writer)`) and are registered in a `HashMap<String, Arc<dyn Command>>`. Unknown commands return a deterministic error line — never a crash.
- **Client retries:** bounded connection retries with backoff (workmeshd `max_retries` / `retry_delay_ms`), then a deterministic failure message.
- **GTK main-loop integration:** socket acceptance/reading runs on the GTK main context via
gio `SocketService` + async futures — the UI thread never blocks on socket I/O (no worker
threads). The dispatch closure touches widgets directly.
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

**CLI-first (engineering guideline).** The command line is the primary surface: the daemon is
controllable and demonstrable without a GUI. The transport is a boundary, not a detail — v1
is a Unix socket; later transports (HTTP/TCP/UDP, hosted by workmeshd) reuse the same command
contract and make the control plane portable to Windows, which has no Unix sockets. Future
daemon flags (e.g. `--settings` opening a settings window once config exists, M5) land in
`src/main.rs`; they are orthogonal to the positional client commands.

## 8. Rendering Strategy

**Do not think in terms of a game-style canvas.** GTK provides a retained-mode scene graph; for a digital clock, custom drawing is unnecessary complexity.

### 8.1 Widget tree

```text
OverlayWindow
└─ GtkOverlay
   └─ GtkBox                          (layout: vertical stack)
      ├─ ClockWidget                  (compound: stack + labels, M3)
      │    ├─ TimeLabel
      │    ├─ DayLabel
      │    └─ DateLabel
      └─ CalendarWidget               (compound: month grid, M4)
           ├─ WeekdayHeader
           ├─ DayCell 1 … DayCell N   (today's cell: .calendar-today)
           └─ …
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

Fade in/out (M3) is the show/hide transition: overlay opacity animates via `set_opacity`
(or CSS `opacity` + GTK transitions). Fade-out completes before `set_visible(false)`;
fade-in starts immediately after showing, keeping trigger-to-visible latency within the §6
budget.

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

Widgets are **pure UI components** with no system side effects; new widgets must **not** modify
activation logic. Overlay layout remains composable — vertical stack model preferred. Widgets
follow the **state/render split** (JigsawFlow rendering facade): a widget owns its state and
produces its widget tree via a `view()` step; the renderer is a swappable adapter behind a
contract (§11.4).

### 11.1 Composite widget model (Weaver Desktop fabric)

The widget model follows Weaver Desktop's UI-fabric design: **widgets may contain widgets** —
an overlay widget is a tree, not a flat list. A widget is either a **layout** (container that
arranges child widgets), a **leaf** (content), or a **compound** widget (a named tree of
layouts and leaves). Layouts carry declarative placement properties (`direction`, `spacing`,
`padding`, `align`); interaction surfaces as **semantic events, never raw input** (e.g. a
`click` leaf emits `activated`).

| Widget kind | Role | Examples |
| --- | --- | --- |
| `layout` | Container — arranges child widgets | vertical/horizontal stack, month grid |
| `label` | Leaf — static text display | time, day, date, calendar day cells |
| `click` | Leaf — clickable, button semantics | version-notification "upgrade" action |
| compound | Named tree of layouts + leaves | `clock` (stack + labels), `calendar` (grid + day cells) |

### 11.2 Clock widget

First widget (M3): time/day/date labels in a vertical stack — a `layout` containing `label`
leaves (§8.1). A small **version label** sits at the bottom: the running binary's version,
coloured dirty white, turning orange (with the repository version appended) when a newer
version exists in a local `Cargo.toml` — an interim check until the GitHub release check
lands (see `hover-clock/src/version.rs`).

### 11.3 Calendar widget

Second widget (M4): a **minimal month calendar** whose only purpose is to recognise what day
of the month today is — no selection, no navigation, no date picking. It is a compound widget:
a `layout` month grid (weekday header + day-cell `label`s) with today's cell highlighted via a
CSS class (`.calendar-today`). Non-interactive (no pointer handlers).

### 11.4 Render contract (swappable renderer)

GTK is the current renderer, not the only one. Following the JigsawFlow rendering-facade
principle (§6.1 of the JigsawFlow guidelines): widgets keep state/logic separate from
rendering — a widget updates its state and produces a declarative widget tree; a
`RenderBackend` contract describes what the app needs from any renderer, and each toolkit
implements it as an adapter (`GtkRenderBackend` today; `QtRenderBackend`, `EguiRenderBackend`
possible later). Swapping the engine then means writing an adapter — application flow, state,
and business logic stay untouched.

**Acknowledged trade-off:** a uniform render contract flattens toolkit-specific idioms (GTK is
retained-mode, egui immediate-mode). Accepted deliberately: renderer replaceability outranks
idiom fidelity. GTK specifics stay at the rendering boundary (§8); the composite model (§11.1)
is renderer-neutral by construction, so the widget tree itself does not change when the engine
does.

### 11.5 Data-plane widget contract

Later, widgets may be driven over the socket (data plane) — a widget contract
(`WidgetProvider`) will be added when that lands.

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
| **M3 — Presentation** | Full clock widget (time/day/date), CSS styling, fade in/out show/hide transitions, auto-hide timer; daemon/client CLI split (§7.4): `--start`/`-s` starts the single-instance daemon, no-arg client sends `show` over the Unix control socket; version label with interim local-Cargo.toml check (§11.2). |
| **M4 — Calendar widget** | Minimal month calendar highlighting today; composite widget model — widgets containing widgets (layout + leaf kinds, Weaver Desktop fabric, §11.1). |
| **M5 — Registry & config** | Adopt `singleton-registry`: flat capability registry, facade contracts (`WindowBackend`, `ActivationBackend`, `TimeSource`, `Config`), TOML config with live hot-swap reload. |
| **M6 — IPC (daemon/client)** | Completes the control plane started in M3 (§7.4): full `Command` registry, client retries, `ping`, `status`, `version`, `commands`, `stop`, `widget`/`config reload` (later). |
| **M7 — Wayland** | Layer-shell backend behind `WindowBackend` / `ActivationBackend` contracts. |
| **Later (private exploration)** | Notifications, toast messaging, template-driven widgets, socket data-plane API, overlay-shell direction. |

## 15. Open Questions

- Hot-corner geometry on multi-monitor setups: all monitors or primary only?
- Wayland hot-corner detection strategy (pointer constraints / global position APIs).
- Debounce, auto-hide, and fade in/out timing values (defaults + configurability).
- Overlay placement: fixed corner vs. follows pointer corner?
- Socket path/ownership: `$XDG_RUNTIME_DIR` vs `/tmp`; socket permissions (0600?).
- Socket protocol evolution: newline framing vs JSON-RPC once the data plane lands (JigsawFlow's Phase-1 transport is TCP/localhost + JSON-RPC; workmeshd uses newline framing — v1 stays simple).
- Transport evolution: Unix socket (v1) → HTTP/TCP/UDP hosted by workmeshd, which also makes the control plane portable to Windows (no Unix sockets) — when does the transport boundary become explicit?
- Async runtime choice for IPC: tokio bridged to the GTK main loop vs glib IO watch only.
- Whether `Esc` alone is sufficient vs. `Esc` + left-click dismissal semantics on Wayland.
- Layer-shell anchor/layer choice (`OVERLAY` vs. `TOP`) for stacking above fullscreen.

## 16. Decision Log

| Date | Decision | Rationale |
| --- | --- | --- |
| Early stage | Pure GTK4-rs; **no Relm4** | Not enough UI complexity; avoid premature state abstraction layer. |
| Early stage | Widget-tree rendering + CSS, no custom canvas | Text-based overlay; GTK handles layout, HiDPI, GPU. |
| Early stage | Abstract `ActivationBackend` / `WindowBackend` | X11-first, but Wayland layer-shell ready without rewrite. |
| Early stage | X11 EWMH hints for overlay semantics | `_ABOVE`, `_SKIP_TASKBAR`, `_SKIP_PAGER`, no focus request. |
| Early stage | **Adopt JigsawFlow pattern + `singleton-registry`** | Flat capability registry, trait contracts, offline-first, graceful degradation, facade-wrapped dependencies — matches the overlay-daemon shape and keeps it extensible. |
| Early stage | **Daemon/client command pattern (workmeshd-inspired)** | Single binary, dual mode; Unix socket control plane with `Command` trait registry; proven pattern for controlling a long-lived daemon. |
| 2026-08 | **State/render split** — widgets own state and produce a widget tree via `view()`; the renderer is a swappable adapter behind a `RenderBackend` contract (JigsawFlow rendering facade §6.1) | GTK stays replaceable (Qt/egui) without touching application flow; matches the shell-family pattern (Weaver egui shell, iced-shell) |

## 17. Compatibility & Portability

**Stance:** Linux-first design. The compatibility record below makes porting and deployment
decisions explicit; macOS/Windows are *ports behind the facade contracts* (§10), not goals.

### 17.1 Linux distributions

- The X11 stack is pure-Rust (`x11rb` — no libX11 dev dependency) plus GTK4 as the only
  system library. Build deps: `pkg-config` + GTK4 dev headers (`libgtk-4-dev` on
  Debian/Pi OS, `gtk4-devel` on Fedora) + Rust toolchain. ARM (Raspberry Pi) builds
  natively; the GL renderer via Mesa fits the §13 footprint targets.
- **GTK version floor:** `gtk4-rs 0.11` builds against GTK ≥ 4.0; the crate enables no
  version-gated feature and uses only base APIs (CSS via `load_from_data`, the 4.0-era
  equivalent of the 4.12-only `load_from_string`). Debian 12 / Raspberry Pi OS bookworm
  (GTK 4.8) are expected to build — tested on GTK 4.18 (Debian 13) and CI. Debian 13 and
  current Fedora ship 4.18+.
- **Deprecated seam:** `gdk_x11_surface_set_skip_taskbar_hint` / `_skip_pager_hint`
  (M1/M2 taskbar-flash fix) are deprecated since GTK 4.18 and present through 4.20;
  removal breaks the link loudly. They only suppress a transient taskbar flash — the
  post-map EWMH state re-application already implemented is the functional fallback.

### 17.2 X11 window managers

| WM / DE | Status | Notes |
| --- | --- | --- |
| GNOME (Xorg session) | ✅ | EWMH hints, `_NET_CURRENT_DESKTOP`, `Super + T` grab all honored. |
| KDE Plasma (X11) | ✅ | KWin honors NOTIFICATION type, ABOVE, skip-taskbar/pager. |
| xfwm4 (test env) | ✅ | Verified live. |
| i3 | ⚠️ mostly | i3 ignores `_NET_WM_STATE_ABOVE`; overlay must float — add `for_window [window_type="notification"] floating enable` to the i3 config. A fullscreen window may cover the overlay (verify). |

Workspace tracking (§17.3, X11) degrades to a logged warning on WMs that do not
advertise `_NET_CURRENT_DESKTOP` — by design, never a crash.

**Session autostart (systemd user unit) — DE behavior differs at login:**
GNOME (`gnome-session`) and KDE Plasma raise `graphical-session.target` when the
session is ready; **xfce/XFCE-style sessions (MX Linux, Xfce, likely LXQt/MATE)
never raise it** — the user session goes straight to `default.target` (verified on
MX Linux 23/xfce; `journalctl --user -u graphical-session.target` stays empty while
the manager reaches `default.target` at login). The unit is therefore
`WantedBy=default.target graphical-session.target` (with
`After=default.target graphical-session.target`, `PartOf=graphical-session.target`):
the first raised target pulls the daemon in, the second finds it already active —
exactly one start on every DE. `DISPLAY`/`XAUTHORITY` reach the user manager at
login via `pam_systemd` (GDM/lightdm/SDDM all do this), so the daemon connects at
`default.target` without hardcoding a display. See
`roadmap/handoffs/04-autostart-fix.md`.

### 17.3 Wayland

- **Current build under XWayland (any Wayland session):** activation works (XWayland
  synthesizes XI2 root motion; key grabs pass through for keys the compositor does not
  intercept), but stacking is degraded — the overlay sits inside the XWayland layer and
  is *never* above native Wayland fullscreen surfaces. Workspace tracking is unavailable
  (no `_NET_CURRENT_DESKTOP` under XWayland) and degrades gracefully.
- **M7 layer-shell covers:** wlroots compositors (sway, Hyprland, labwc — including the
  Raspberry Pi OS Wayland preview) and KWin (Plasma ≥ 5.27).
- **Mutter (GNOME) does not implement layer-shell and exposes no public alternative** →
  GNOME Wayland is unreachable for the overlay-above-fullscreen requirement; the Xorg
  session is the supported path there.

### 17.4 macOS (port, not supported)

Already portable: GTK4 Quartz backend, `chrono`, gio main loop, Unix-socket IPC (with
`$TMPDIR` fallback for the missing `XDG_RUNTIME_DIR`). Required work:

1. **cfg-gating** — `gdk4-x11`/`x11rb` deps and the `#[link(name = "gtk-4")]` extern block
   move behind a cargo feature (Linux-only).
2. **Window backend** — a non-focus-stealing overlay is an `NSPanel` (nonactivating,
   floating level, `canJoinAllSpaces`); a GTK `NSWindow` cannot be non-activating.
   **Open decision:** reach into the NSWindow via `objc2-app-kit` (partial semantics) vs.
   a native NSPanel overlay (widget duplication).
3. **Activation backend** — Carbon `RegisterEventHotKey` for `Super + T` / `Esc` (no
   permission needed); `CGEventTap` for the hot corner (requires the Accessibility
   permission). Nonactivating panels can still become key, so `Esc` arrives via `keyDown`.
4. **Packaging** — .app bundle carrying the GTK dylibs.

### 17.5 Windows (port, not supported)

1. **cfg-gating** — same as §17.4.1.
2. **Toolchain** — GTK4 from MSYS2 (`mingw-w64-x86_64-gtk4`, rust-gnu toolchain) or
   gvsbuild (MSVC).
3. **Window backend** — `gdk4-win32` exposes the HWND; then `WS_EX_TOOLWINDOW` (no
   taskbar), `WS_EX_NOACTIVATE` (never takes focus), `WS_EX_TOPMOST` (above fullscreen;
   exclusive-fullscreen games may still win).
4. **Activation backend** — `RegisterHotKey` for `Super + T` / `Esc`-while-visible;
   `SetWindowsHookEx(WH_MOUSE_LL)` for the hot corner.
5. **IPC** — Rust std has no Unix sockets on Windows: named pipe `\\.\pipe\hoverclock`
   or TCP loopback behind the `IpcServer` contract; the §15 framing question becomes the
   transport contract here.
6. **Packaging** — GTK4 is DLL-based; no single binary (NSIS/Inno/MSIX bundling).

### 17.6 Portability architecture notes

- §10 facades + M5 runtime backend selection are the porting seam; `src/main.rs` still
  hard-wires X11 today.
- `WindowBackend::configure` is GTK-typed — constrains the native macOS panel path;
  resolve at port time.
- IPC transport is not yet abstracted (M6); the §15 socket/framing questions double as
  the Windows transport contract.
