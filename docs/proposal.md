# HoverClock — Design Proposal

**Status:** Early-stage architecture · Core focus: overlay behavior and input activation system
**Scope:** Lightweight Linux overlay daemon (X11 first, Wayland planned)
**Version:** 0.3 (draft)

## 1. Purpose

HoverClock is a lightweight Linux overlay daemon that surfaces information — starting with a digital clock — **on demand**, via two activation methods:

- **Hot-area trigger** (mouse enters the full-width strip along the top screen edge)
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
| Move mouse to the top edge of the screen (full-width hot strip) | Overlay appears (debounced), centred above the screen's middle |
| Press `Super + T` | Toggle overlay |
| Press `Esc` | Hide overlay |
| Move mouse away from the overlay/hot strip | Auto-hide (debounced) |
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

[gtk4-layer-shell-rs](https://github.com/wmww/gtk4-layer-shell) is the implementation (M7,
landed): the overlay is a `zwlr_layer_shell_v1` surface in the **OVERLAY layer** (the
protocol's topmost layer — designed for OSDs/notifications), keyboard mode NONE (never
takes focus), exclusive zone 0 (never reserves workspace). Placement is layer-shell
anchor + margins relative to the target output (the X11 `move_to` coordinate hack is not
needed); the surface is switched to the triggered output before each show. Stacking above
fullscreen apps is compositor-guaranteed — no EWMH hints exist on Wayland. GNOME/Mutter
does not implement layer-shell (§17.3).

## 10. Backend Abstraction

All system-level interaction passes through explicit trait boundaries, **registered as registry facades** (JigsawFlow facade pattern). **No direct X11 calls inside UI logic.**

| Contract (trait) | X11 implementation | Wayland implementation |
| --- | --- | --- |
| `ActivationBackend` | Hot-corner + global shortcut (X11) | Hot-corner strips (top-edge layer surfaces, M7); global shortcut unavailable — see §16 |
| `WindowBackend` | EWMH window hints | Layer-shell surface (M7) |
| `TimeSource` | System clock (`chrono::Local`) | System clock (shared) |
| `IpcServer` | Unix socket listener (shared) | Unix socket listener (shared) |

Consumers resolve these via the registry; absence of a backend degrades to a disabled overlay with a logged warning (never a crash). Runtime selection landed at M7: `backend::build_backends()` (session-level — the registry formalisation is M5).

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
leaves (§8.1). A small **version label** sits at the bottom: the running binary's version in
shadow-grey, turning orange (with the newer version appended) when a newer release exists on
GitHub. The check hits the GitHub Releases API on a worker thread (bounded timeouts) and
refreshes hourly; failures (offline, rate-limited, malformed) degrade to the current colour —
never an error (roadmap S09; `src/version.rs`).

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
| **M3 — Presentation** | Full clock widget (time/day/date), CSS styling, fade in/out show/hide transitions, auto-hide timer; daemon/client CLI split (§7.4): `--start`/`-s` starts the single-instance daemon, no-arg client sends `show` over the Unix control socket; version label checking the GitHub releases API, offline → current colour (§11.2). |
| **M4 — Calendar widget** | Minimal month calendar highlighting today; composite widget model — widgets containing widgets (layout + leaf kinds, Weaver Desktop fabric, §11.1). |
| **M5 — Registry & config** | Adopt `singleton-registry`: flat capability registry, facade contracts (`WindowBackend`, `ActivationBackend`, `TimeSource`, `Config`), TOML config with live hot-swap reload. |
| **M6 — IPC (daemon/client)** | Completes the control plane started in M3 (§7.4): full `Command` registry, client retries, `ping`, `status`, `version`, `commands`, `stop`, `widget`/`config reload` (later). |
| **M7 — Wayland** | Layer-shell backend behind `WindowBackend` / `ActivationBackend` contracts. |
| **Later (private exploration)** | Notifications, toast messaging, template-driven widgets, socket data-plane API, overlay-shell direction. |

## 15. Open Questions

Resolved at M7 are marked **→ decided (§16)**.

- Hot-corner geometry on multi-monitor setups: all monitors or primary only?
  **→ decided (§16)**: one strip per output, like X11.
- Wayland hot-corner detection strategy (pointer constraints / global position APIs).
  **→ decided (§16)**: input-region top strips (no global pointer API exists).
- Debounce, auto-hide, and fade in/out timing values (defaults + configurability).
- Overlay placement: fixed corner vs. follows pointer corner?
- Socket path/ownership: `$XDG_RUNTIME_DIR` vs `/tmp`; socket permissions (0600?).
- Socket protocol evolution: newline framing vs JSON-RPC once the data plane lands (JigsawFlow's Phase-1 transport is TCP/localhost + JSON-RPC; workmeshd uses newline framing — v1 stays simple).
- Transport evolution: Unix socket (v1) → HTTP/TCP/UDP hosted by workmeshd, which also makes the control plane portable to Windows (no Unix sockets) — when does the transport boundary become explicit?
- Async runtime choice for IPC: tokio bridged to the GTK main loop vs glib IO watch only.
- Whether `Esc` alone is sufficient vs. `Esc` + left-click dismissal semantics on Wayland.
- Layer-shell anchor/layer choice (`OVERLAY` vs. `TOP`) for stacking above fullscreen.
  **→ decided (§16)**: `OVERLAY` for the overlay, `TOP` for the sensor strips.

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
| 2026-08 (M7) | **Wayland hot corner = input-region top strips** — one thin solid-dark 2 px top-edge layer surface per output; enter/leave crossing events are the corner trigger | Wayland has no global pointer-position API (§15); the strip *is* its own input region. Three hard-won mechanisms (docs/wayland-layer-shell-findings.md): (1) the strip needs an **exclusive zone** — without it wlroots places it in the layer's *free* area below reserved chrome (the PIXEL bar); with it, at the monitor's true top edge, parity with X11. (2) the strip must **commit a buffer** to be mapped — an empty transparent window commits without attaching, so a DrawingArea forces one. (3) labwc composites layer surfaces **opaque with baked alpha** — true invisibility is impossible on this stack, so the strip is a visible 2 px dark band (user's thinness choice). Trade-off: the strip captures clicks in its band (X11 keeps the passive corner) |
| 2026-08 (M7) | **Layer choice: `OVERLAY` for both overlay and sensor strips** | Overlay must float above fullscreen apps (OVERLAY is the protocol's topmost layer). Strips also use OVERLAY: with an exclusive zone the strip lands in the layer's exclusive area at the output's true top edge (above any bar, above fullscreen — parity with the X11 monitor-absolute corner). Without exclusivity wlroots drops it into the free area below reserved chrome, which proved unusable (handoff 07) |
| 2026-08 (M7) | **Wayland global shortcut: compositor keybind, not app-side** | No portable app-side global-shortcut API exists: `ext_global_shortcuts_v1` is an unmerged upstream MR; the GlobalShortcuts portal has no wlroots backend; XWayland key grabs only see keys when an *X11* app is focused (native Wayland apps get the key directly — verified live, handoff 07). The compositor owns global keys: labwc binds `W-T`/`Escape` in rc.xml to the `hover-clock` client (`toggle`/`hide` over the control socket) — works over any app, never steals focus. Per-compositor config; the client command is the portable interface |
| 2026-08 (M7) | **Placement = layer-shell anchor + margins; `move_to` reinterprets absolute coords per output** | Layer-shell has no absolute positioning; the surface follows the triggered output via `set_monitor` before each map, offset expressed as margins. GDK logical pixels throughout the Wayland path (X11 reports physical — the paths never mix) |
| 2026-08 (M7) | **One binary, one artifact per arch — no X11/Wayland split** | Measured: the max droppable backend code is 112 KB (`x11rb`) of a 4.08 MB binary — the runtime is GTK-dominated (11 MB, both backends internal, not strippable per-app). A split would double the release matrix, force install-time session selection (wrong for machines that boot both sessions — e.g. this Pi) and version lockstep, for ~2.7% binary savings. The `wayland` feature gate already gives X11-only builders the lean build. If a Wayland-only target ever needs it, add an `x11` feature (default-on) instead of splitting releases |

## 17. Compatibility & Portability

**Stance:** Linux-first design. The compatibility record below makes porting and deployment
decisions explicit; macOS/Windows are *ports behind the facade contracts* (§10), not goals.

### 17.1 Linux distributions

- The X11 stack is pure-Rust (`x11rb` — no libX11 dev dependency) plus GTK4 as the only
  system library. Build deps: `pkg-config` + GTK4 dev headers (`libgtk-4-dev` on
  Debian/Pi OS, `gtk4-devel` on Fedora) + Rust toolchain. ARM (Raspberry Pi) builds
  natively; the GL renderer via Mesa fits the §13 footprint targets.
- **M7 adds the layer-shell system library:** the `wayland` cargo feature (default-on)
  links `gtk4-layer-shell` (pkg-config `gtk4-layer-shell-0`) — `libgtk4-layer-shell-dev`
  on Debian/Pi OS trixie (unavailable on bookworm; use `--no-default-features` there).
  X11-only builds can drop the feature: `cargo build --no-default-features`.
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

**Workspace following (§5) — WM behavior differs.** xfwm4 does not relocate an
already-mapped overlay when the workspace changes: the overlay re-maps (unmap+map), which
costs a one-frame flicker — accepted on the test env. WMs that honor `_NET_WM_DESKTOP` and
sticky for *mapped* windows (KWin, Mutter, Openbox, Fluxbox) can keep the overlay sticky
(`_NET_WM_DESKTOP = 0xFFFFFFFF`) instead — no re-map, no flicker; the X11 backend could set
it best-effort on those. Layer-shell (M7, §17.3) is workspace-independent by construction, so
both the re-map and the flicker disappear there.

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

- **M7 (layer-shell) landed:** on compositors with `zwlr_layer_shell_v1` (wlroots family —
  labwc, sway, Hyprland — and KWin ≥ 5.27) the overlay runs native: OVERLAY layer (above
  fullscreen by construction), placement via anchor + margins, hot corner via a thin 2 px
  solid strip at the true top edge. Super+T/Esc degrade (no portable global-shortcut protocol, §16).
  Verified on labwc 0.9.8 (handoff 07).
- **XWayland fallback (unchanged):** when layer-shell is unavailable (GNOME Wayland,
  XWayland sessions) the factory falls back to the X11 backends — activation works,
  stacking degraded, warnings logged (§10 degradation doctrine).
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

- §10 facades + M5 runtime backend selection are the porting seam; `src/main.rs` now
  selects the session backend through `backend::build_backends()` (M7) instead of
  hard-wiring X11.
- `WindowBackend::configure` is GTK-typed — constrains the native macOS panel path;
  resolve at port time.
- IPC transport is not yet abstracted (M6); the §15 socket/framing questions double as
  the Windows transport contract.
