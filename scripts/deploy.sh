#!/usr/bin/env bash
# Deploy a new release: bump Cargo.toml (and Cargo.lock), commit, push
# main, tag vX.Y.Z, push the tag — the main-gated release workflow then
# builds the x86_64 + aarch64 tarballs on GitHub.
#
# Usage: ./scripts/deploy.sh <version>     (e.g. 1.2.0 or 2.1.1)
#
# Guards: main branch only, clean working tree, tag not already on origin.

set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$REPO_ROOT"

STATE_DIR="${XDG_STATE_HOME:-$HOME/.local/state}/hover-clock"
LOG_FILE="$STATE_DIR/install.log"

# Audit log: one timestamped line per run (same file as install/swap).
log() {
    mkdir -p "$STATE_DIR"
    printf '%s %s\n' "$(date '+%Y-%m-%d %H:%M:%S')" "$*" >> "$LOG_FILE"
}

VERSION="${1:-}"
VERSION="${VERSION#v}" # tolerate "v1.2.0"
if ! [[ "$VERSION" =~ ^[0-9]+(\.[0-9]+){2}$ ]]; then
    echo "!! usage: ./scripts/deploy.sh <version>  (e.g. 1.2.0 or 2.1.1)" >&2
    exit 1
fi

# --- release guards -------------------------------------------------------
branch="$(git rev-parse --abbrev-ref HEAD)"
if [ "$branch" != "main" ]; then
    echo "!! releases are main-only; current branch: $branch" >&2
    exit 1
fi
if [ -n "$(git status --porcelain)" ]; then
    echo "!! working tree is dirty — commit or stash before releasing" >&2
    exit 1
fi
if git ls-remote --tags origin "v$VERSION" | grep -q "refs/tags/v$VERSION"; then
    echo "!! tag v$VERSION already exists on origin" >&2
    exit 1
fi

# --- bump ------------------------------------------------------------------
echo "==> Bumping Cargo.toml to $VERSION"
sed -i "0,/^version = /s//version = \"$VERSION\"/" Cargo.toml
grep -q "^version = \"$VERSION\"" Cargo.toml || {
    echo "!! failed to update Cargo.toml" >&2
    exit 1
}

# Cargo.lock pins the package version too — a stale lock fails CI's
# `--locked` builds. `cargo build` refreshes it incrementally.
echo "==> Syncing Cargo.lock"
cargo build >/dev/null 2>&1
lock_ver="$(awk '/^name = "hover-clock"$/{f=1} f && /^version = /{print $3; exit}' Cargo.lock | tr -d '"')"
if [ "$lock_ver" != "$VERSION" ]; then
    echo "!! Cargo.lock version is $lock_ver, expected $VERSION" >&2
    exit 1
fi

# --- commit, push, tag -----------------------------------------------------
git add Cargo.toml Cargo.lock
git commit -m "chore: bump to $VERSION"
git push origin main
git tag "v$VERSION"
git push origin "v$VERSION"

log "deploy v$VERSION (sha $(git rev-parse --short HEAD)) -> release workflow"

echo
echo "Released v$VERSION:"
echo "  - main pushed, tag v$VERSION pushed"
echo "  - release workflow builds x86_64 + aarch64 tarballs (github.com/dominikj111/hover-clock/releases)"
echo "  - the installed daemon shows the orange update button once the release is up"
