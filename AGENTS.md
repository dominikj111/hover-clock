# AGENTS.md — HoverClock (AI Contributor Constraints)

This file is the **runtime kernel** for AI contributors. It is always loaded and contains
binding constraints for all work in this repository. `proposal.md` is advisory and is only
loaded under the conditions below.

---

## 0. Document Context Policy

`proposal.md` is **NOT** always loaded.

**It must only be read when:**
- structural architecture changes are requested
- a new subsystem or backend is introduced
- the UX interaction model is modified
- feature expansion beyond the existing widget scope is planned
- system-wide refactoring or migration is required

**It must NOT be read for:**
- local code changes
- UI styling changes
- bug fixes
- minor feature adjustments

**AGENTS.md remains the sole source of runtime constraints.**
`proposal.md` is advisory and may be ignored unless explicitly triggered by the above conditions.

---

## 1. Architecture invariants

- Overlay is a **transient UI layer**, never a persistent desktop component.
- Clock is a **feature, not the system**: architecture must remain widget-extensible.
- Hard separation: **Activation layer, Overlay layer, Widget layer, Config layer, IPC layer**.
- No direct coupling between input detection and rendering logic.
- The daemon is a **single binary, dual-mode** process: daemon mode by default, client mode when a positional command is given.

## 2. JigsawFlow composition rules (singleton-registry)

HoverClock follows the JigsawFlow pattern, supported by the `singleton-registry` crate.

- **Everything is a capability**: components access functionality through **trait contracts** (`Arc<dyn Trait>`) resolved from the flat singleton registry — never through direct component references.
- **Component independence**: no component may depend directly on another component. All cross-component access goes through registry contracts.
- **Offline-first**: every capability must function with no network connectivity. Network protocols are optional enhancements only.
- **Graceful degradation**: optional capabilities are resolved with `try_get`. Absence must **log a warning and continue**, never fail at construction.
- **Facade pattern**: every external dependency (X11, Wayland/layer-shell, GTK specifics, filesystem, environment, sockets) must be wrapped behind a registry facade. Business logic must never touch system APIs directly.
- **Hot-swap**: re-registering a contract atomically replaces the stored `Arc`; existing holders keep a valid reference. Runtime re-registration is limited to documented hot-swap points (configuration reload).
- **Write-once, read-many**: register all core contracts at startup; register all required types during initialization so `get()` never fails at runtime.

## 3. Window behavior invariants (critical)

- Overlay must **NEVER take focus** (input grab forbidden).
- Must **not appear** in task switchers, panels, or window lists.
- Must remain **above fullscreen applications**.
- Must support instant show/hide (< 50 ms target perception).
- Must **not interfere** with the keyboard input of the active application.

## 4. Activation rules

- Two supported triggers only: **hot-corner OR global shortcut** (plus IPC commands in a later milestone).
- Hot-corner detection must be **debounced** (avoid flicker loops).
- Shortcut handling must be global and non-conflicting where possible.
- Activation system must be **backend-abstracted** (X11 vs Wayland split).
- Activation must keep working if IPC is unavailable (degrade to hot-corner/shortcut only).

## 5. Rendering constraints

- Default approach: **GTK widget tree only**.
- Custom drawing (`GtkDrawingArea`) is **prohibited** unless explicitly required for non-text visualization.
- No GPU/canvas engine unless justified by a measurable UI requirement.
- Styling must be **CSS-driven**, not procedural rendering logic.

## 6. Dependency discipline

- **GTK4-rs** is the baseline UI toolkit.
- **`singleton-registry`** is the required composition primitive (JigsawFlow core).
- **Relm4 is forbidden** in the initial implementation (no premature state abstraction layer).
- No Electron, no web rendering stack.
- Optional: `gtk4-layer-shell` only if Wayland support is implemented.
- No async runtime in the UI thread unless bridged to the GTK main loop.

## 7. State management rules

- Overlay state machine must remain **binary or minimal** (`Hidden`/`Visible` at minimum).
- No hidden async side effects in the UI layer.
- Configuration reload must **not** reset runtime overlay state.

## 8. Performance constraints

- Idle CPU usage must remain negligible (**< 0.1%** target).
- Memory footprint must remain minimal (**< 25 MB** target).
- Overlay render path must **avoid allocations per frame**.
- No polling loops at high frequency for input detection.
- IPC must not add latency to the overlay show/hide path.

## 9. Input handling constraints

- Mouse tracking must be **edge-triggered**, not continuous full-screen polling when possible.
- Keyboard shortcut handling must **not block** the system input pipeline.
- **Escape must always dismiss the overlay** if visible.

## 10. Backend abstraction requirement

All system-level interaction must pass through explicit trait boundaries, **registered as registry facades**:

- `ActivationBackend` (X11 / Wayland / future)
- `WindowBackend` (X11 EWMH / layer-shell abstraction)
- `TimeSource` (system clock)
- `IpcServer` (Unix socket listener)

**No direct X11 calls inside UI logic.** Consumers resolve contracts via `try_get` and degrade gracefully when a backend is absent.

## 11. IPC: daemon & client command pattern (workmeshd-inspired)

The daemon is controlled over a Unix socket by an included client module.

- **Single binary, dual mode:** no positional command → daemon mode; positional command (e.g. `hoverclock ping`) → client mode forwarding to the daemon.
- **Unix socket** at `${XDG_RUNTIME_DIR}/hoverclock.sock` (configurable). Socket permissions must be restrictive (0600).
- **Single-instance guard:** on startup, check the socket path — if a live listener exists, refuse to start; if stale, remove and rebind. Maintain a PID file for clean shutdown.
- **Line-based protocol:** request = `command arg1 arg2 ...\n`; response = text lines terminated by EOF/flush. No complex framing in v1.
- **Command registry:** commands implement a shared `Command` trait (`name()`, `execute(args, writer)`) registered in `HashMap<String, Arc<dyn Command>>`. Unknown commands return a **deterministic error line**, never a crash.
- **Client retries:** bounded connection retries with backoff, then a deterministic failure message.
- **GTK main-loop integration:** socket acceptance/reading must run on the GTK main context (glib IO watch, or an async runtime bridged to the GTK main loop). The UI thread must **never block** on socket I/O.
- **Baseline command set:** `ping`, `show`, `hide`, `toggle`, `status`, `widget`, `config reload`, `version`, `commands`, `stop`.
- **Degradation:** IPC failure must never break the overlay (hot-corner/shortcut remain functional).

## 12. Time subsystem constraints

- Time retrieval must be **system-clock based only** (no external API dependencies).
- Formatting must be **locale-safe and deterministic**.
- No time drift correction logic in the application layer.

## 13. Extensibility rules

- New widgets must **not** modify activation logic.
- Widgets must be **pure UI components** with no system side effects.
- Overlay layout must remain composable (vertical stack model preferred).
- New commands must register in the command registry and reuse existing registry contracts (no new direct dependencies).

## 14. Concurrency rules

- UI thread must remain **single-owner of GTK objects**.
- Background threads must **never** directly mutate UI state.
- Cross-thread communication only via **message passing (channel-based)**.
- Socket handlers must never touch GTK objects directly; they submit commands through the registry/command path.

## 15. Failure handling

- Any backend failure (X11/Wayland) must **degrade to a disabled overlay**, not crash.
- Misconfigured activation must **fall back to shortcut only**.
- IPC failure must degrade to a working overlay; socket errors must be logged and the listener must keep serving (or shut down cleanly on `stop`).
- No silent failure in the activation path; must **log deterministically**.

## 16. Prohibited design patterns

- No global mutable state outside the configuration module and the singleton registry.
- No hidden event loops outside the main runtime.
- No UI logic embedded in the activation backend.
- No framework-heavy abstraction layers at the early stage.
- No direct component-to-component coupling bypassing the registry.
- No blocking socket I/O on the GTK UI thread.

## 17. Long-term architectural constraint

- System must remain embeddable as a lightweight **"overlay daemon"**.
- Must **not** evolve into a desktop environment replacement accidentally.
- Any new feature must justify itself as **overlay-summonable information**, not persistent UI.
- The registry + IPC architecture must scale to a widget data plane (notifications, toast, template rendering) without structural change.

---

## 18. Working agreement

- Build and verify with `cargo build` / `cargo test`; keep the crate warning-free where practical.
- Keep this file and `proposal.md` consistent: any change to a binding constraint here must be reflected in the proposal's decision log, and vice versa.
- When in doubt between this file and `proposal.md`, **this file wins**.
