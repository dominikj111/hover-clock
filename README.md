# HoverClock

A lightweight Linux overlay daemon that surfaces information on demand — starting with a digital clock. Triggered by hot-corner or global shortcut, it appears above fullscreen applications without stealing focus.

> Full design, architecture, constraints, and roadmap are in **[proposal.md](./proposal.md)**.

## Quick Start

```bash
cargo build --release
cargo run --release          # daemon mode
cargo run --release -- ping  # client mode
```

## Requirements

- Linux (X11; Wayland planned)
- GTK4
- Rust stable

## Documentation

| File | Purpose |
|------|---------|
| [`proposal.md`](./proposal.md) | Source of truth — design, architecture, decisions, roadmap |
| [`AGENTS.md`](./AGENTS.md) | Repository guide for contributors and AI agents |
| [`roadmap/`](./roadmap/) | Milestone tracking and progress |

## License

TBD
