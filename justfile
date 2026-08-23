# HoverClock — command shortcuts (just).
#
# Wraps the lifecycle scripts and the cargo/clap surface so the common
# operations are one word: `just install`, `just swap-to-dev`,
# `just deploy 1.2.0`. All recipes run from the repository root.

default:
    @just --list

# --- build & quality -------------------------------------------------------

build:
    cargo build

build-release:
    cargo build --release

# CI-equivalent gate: clippy, formatting (MSRV rustfmt — the CI
# contract), and tests.
check:
    cargo clippy --all-targets -- -D warnings
    cargo +1.92.0 fmt --check
    cargo test

fmt:
    cargo +1.92.0 fmt

test:
    cargo test

# --- daemon & client -------------------------------------------------------

# Start the daemon from source (single instance).
daemon:
    cargo run -- --start

# Run the binary with arbitrary args (defaults to the `show` client).
run *args="":
    cargo run -q -- {{args}}

show:
    cargo run -q

hide:
    cargo run -q -- hide

toggle:
    cargo run -q -- toggle

# --- lifecycle (delegates to scripts/) --------------------------------------

install:
    ./scripts/install.sh

# Download the latest release binary (curl|sh path) — no build.
install-release:
    ./scripts/install-release.sh

upgrade *args="":
    ./scripts/upgrade.sh {{args}}

swap-to-dev *args="":
    ./scripts/swap-to-dev.sh {{args}}

swap-to-prod:
    ./scripts/swap-to-prod.sh

uninstall:
    ./scripts/uninstall.sh

# --- release ----------------------------------------------------------------

# Deploy a new release: bumps Cargo.toml/Cargo.lock, commits, pushes main
# and tags v<version> (main-gated release workflow builds the tarballs).
# An optional theme (second arg) becomes the "## vX.Y.Z — <theme>"
# heading on the release page.
deploy version theme="":
    ./scripts/deploy.sh "{{version}}" "{{theme}}"
