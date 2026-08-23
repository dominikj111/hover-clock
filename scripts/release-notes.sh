#!/usr/bin/env bash
# Generate GitHub release notes from the git history between the previous
# tag and the current one: commit subjects, grouped by type, so every
# release page gets real per-release notes instead of the same boilerplate
# body.
#
# Used by .github/workflows/release.yml: the `notes` job generates the body
# ONCE and both matrix build jobs consume the same artifact, keeping the
# body deterministic — re-generating in each job is what doubled the footer
# before (see commit b408829 and the comment in release.yml).
#
# Usage: ./scripts/release-notes.sh <tag> [previous_tag]
#   tag          current release tag (vX.Y.Z)
#   previous_tag optional — defaults to the nearest older tag; the first
#                release has none (full history up to <tag> is used).

set -euo pipefail

TAG="${1:-}"
if [ -z "$TAG" ]; then
    echo "usage: $0 <tag> [previous_tag]" >&2
    exit 1
fi
PREV="${2:-}"

if [ -z "$PREV" ]; then
    # Nearest older tag: the line right after <tag> in the version-
    # descending tag list (v1.3.1, v1.3.0, ...). The getline return check
    # matters: at EOF (newest or only tag) getline leaves $0 untouched,
    # which would otherwise echo the tag itself back.
    PREV="$(git tag --sort=-version:refname | awk -v t="$TAG" '$0==t {if ((getline line) > 0) print line; exit}')"
fi

if [ -n "$PREV" ]; then
    RANGE="$PREV..$TAG"
else
    RANGE="$TAG"
fi

# Theme for the "## vX.Y.Z — <theme>" header, taken from the bump
# commit's message (deploy.sh's optional theme argument →
# "chore: bump to X.Y.Z — <theme>"). Empty → plain "## vX.Y.Z".
# Guarded: only bump commits carry a theme; any other tag keeps the
# plain header.
theme=""
subject="$(git log -1 --format='%s' "$TAG" 2>/dev/null || true)"
case "$subject" in
    "chore: bump to "*) theme="$(printf '%s' "$subject" | sed -E 's/^chore: bump to [0-9.]+( — )?//')" ;;
esac

feat=()
fix=()
other=()
while IFS= read -r subject; do
    case "${subject%%:*}" in
        feat*) feat+=("$subject") ;;
        fix*) fix+=("$subject") ;;
        *) other+=("$subject") ;;
    esac
done < <(git log --no-merges --format='%s' "$RANGE" | grep -vE '^chore: bump to ')

# Strip the "type (scope): " prefix so bullets read as statements.
# Handles both styles in the history: "fix (release): ..." and the
# more common no-space "fix(ci): ..." — the space-only pattern left
# the prefix in place on every scoped commit.
strip() {
    printf '%s\n' "$1" | sed -E 's/^[a-z][a-z-]*( \([^)]*\)|\([^)]*\))?: //'
}

emit() {
    local section="$1"
    shift
    [ $# -gt 0 ] || return 0
    echo "### $section"
    for s in "$@"; do
        echo "- $(strip "$s")"
    done
    echo
}

echo "## $TAG${theme:+ — $theme}"
echo
if [ -n "$PREV" ]; then
    echo "Changes since $PREV:"
else
    echo "First release."
fi
echo
emit Features "${feat[@]}"
emit Fixes "${fix[@]}"
emit Other "${other[@]}"
echo "Full changelog: https://github.com/dominikj111/hover-clock/commits/$TAG"
echo
echo "Install: \`curl -fsSL https://raw.githubusercontent.com/dominikj111/hover-clock/main/scripts/install-release.sh | sh\`"
echo "Usage: see the [README](https://github.com/dominikj111/hover-clock#readme)."
