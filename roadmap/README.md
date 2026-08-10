# HoverClock — Roadmap

Derived from [proposal.md](../proposal.md) §14. Each milestone links to its detailed tracking file when active.

## Milestones

| Milestone | Scope | Status |
|-----------|-------|--------|
| **M0** | GTK4-rs project scaffold; single clock label; classic window (repo baseline) | Done |
| **M1** | EWMH hints (`_ABOVE`, `_SKIP_TASKBAR`, `_SKIP_PAGER`); non-focusable window; X11 `WindowBackend` | Done |
| **M2** | X11 `ActivationBackend`: hot-corner detection (debounced, edge-triggered) + global `Super + T` + `Esc` dismiss | Done |
| **M3** | Full clock widget (time/day/date), CSS styling, show/hide transitions, auto-hide timer | Current |
| **M4** | Adopt `singleton-registry`: flat capability registry, facade contracts, TOML config with live hot-swap reload | Pending |
| **M4** | Adopt `singleton-registry`: flat capability registry, facade contracts, TOML config with live hot-swap reload | Pending |
| **M5** | Dual-mode binary, Unix socket listener, `Command` registry, client module with retries | Pending |
| **M6** | Wayland layer-shell backend behind `WindowBackend` / `ActivationBackend` contracts | Pending |

## Later (private exploration)

Notifications, toast messaging, template-driven widgets, socket data-plane API, overlay-shell direction. Out of scope for this public repository.
