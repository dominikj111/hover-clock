# AGENTS.md — HoverClock

Repository guide for contributors and AI agents. The source of truth for design, architecture,
constraints, and roadmap is **[proposal.md](./proposal.md)** — always consult it for decisions.

## Project at a Glance

HoverClock is a transient overlay daemon for Linux (X11 first, Wayland planned).
It surfaces widgets — starting with a clock — via hot-corner or global shortcut.
The overlay never steals focus, stays above fullscreen apps, and is invisible to task switchers.

- **Stack:** Rust, GTK4-rs, `singleton-registry` (JigsawFlow composition)
- **Daemon + client:** single binary, dual-mode; Unix socket control plane
- **Widget-extensible:** clock is the first widget, not the final product
- **Offline-first:** no network dependency; system clock only

## Key Documents

| File | Role |
|------|------|
| [`proposal.md`](./proposal.md) | Source of truth — full design, architecture, constraints, roadmap |
| [`roadmap/`](./roadmap/) | Milestone tracking and progress |
| [`handover/`](./handover/) | Iteration handover logs — what was done, what was learned, what's next |

## Development Style

- **Surgical changes only.** Touch only what the task requires. No full-file rewrites, no "improving" adjacent code, no refactoring things that aren't broken.
- **No speculative features.** Minimum code that solves the problem. No abstractions for single-use code, no "flexibility" that wasn't requested.
- **Match existing style** even if you'd do it differently. Every changed line should trace directly to the task.
- **Verify with `cargo build` / `cargo test`** after changes; keep the crate warning-free.

## Generic Tautologies

- The overlay is transient; never a persistent desktop component.
- The clock is a feature, not the system — architecture stays widget-extensible.
- The daemon is a single binary, dual-mode process.
- No direct coupling between input detection and rendering.
- System backends are abstracted behind trait facades; business logic never touches system APIs directly.
- Components resolve dependencies through the singleton registry, never through direct references.
- Escape always dismisses the overlay if visible.

## Referenced Guidelines

These external documents inform HoverClock's development patterns. They live outside this repo and apply project-agnostically:

| Guideline | Applies to |
|-----------|------------|
| [`llm_profiles/jigsawflow_guidelines.md`](../llm_profiles/jigsawflow_guidelines.md) | JigsawFlow composition, singleton-registry usage, facade pattern, testing |
| [`llm_profiles/software_development_style_guides.md`](../llm_profiles/software_development_style_guides.md) | AI contributor behavior, chain-of-thought, simplicity, surgical edits |
| [`llm_profiles/gtk_frontend_guidelines.md`](../llm_profiles/gtk_frontend_guidelines.md) | GTK widget composition, overlay/view layering, CSS-driven theming |
| [`llm_profiles/icm_mwp_guidelines.md`](../llm_profiles/icm_mwp_guidelines.md) | ICM/MWP methodology: context cascade, intent pipeline, roadmap → task → handover iteration flow, handover log contract |
