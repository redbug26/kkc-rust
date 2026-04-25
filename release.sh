#!/usr/bin/env bash
# release.sh — Bump version, commit, tag and push to trigger the GitHub release workflow.
# Usage:
#   ./release.sh           # auto-increment patch  (0.1.14 → 0.1.15)
#   ./release.sh minor     # auto-increment minor  (0.1.14 → 0.2.0)
#   ./release.sh major     # auto-increment major  (0.1.14 → 1.0.0)
#   ./release.sh 0.2.3     # explicit version

set -euo pipefail

CARGO_TOML="Cargo.toml"

# ── Read current version ───────────────────────────────────────────────────
current=$(grep '^version' "$CARGO_TOML" | head -1 | sed 's/version = "\(.*\)"/\1/')
IFS='.' read -r major minor patch <<< "$current"

# ── Compute new version ────────────────────────────────────────────────────
arg="${1:-patch}"
case "$arg" in
  major)
    new_version="$((major + 1)).0.0" ;;
  minor)
    new_version="${major}.$((minor + 1)).0" ;;
  patch)
    new_version="${major}.${minor}.$((patch + 1))" ;;
  [0-9]*.[0-9]*.[0-9]*)
    new_version="$arg" ;;
  *)
    echo "Usage: $0 [major|minor|patch|X.Y.Z]"
    exit 1 ;;
esac

echo "Current version : $current"
echo "New version     : $new_version"
echo ""

# ── Confirm ────────────────────────────────────────────────────────────────
read -r -p "Proceed? [y/N] " confirm
[[ "$confirm" =~ ^[yY]$ ]] || { echo "Aborted."; exit 0; }

# ── Commit any pending changes first ─────────────────────────────────────
if [[ -n "$(git status --porcelain)" ]]; then
  echo ""
  echo "Pending changes detected — committing before bump:"
  git status --short
  git add -A
  git commit -m "chore: pre-release"
  git push origin main
  echo "✓ Pre-release commit pushed"
fi

# ── Bump version in Cargo.toml ────────────────────────────────────────────
sed -i '' "s/^version = \"${current}\"/version = \"${new_version}\"/" "$CARGO_TOML"
echo "✓ Cargo.toml updated"

# ── Verify build ──────────────────────────────────────────────────────────
echo ""
echo "Building (debug) to verify…"
cargo build 2>&1 | tail -5
echo "✓ Build OK"

# ── Commit ────────────────────────────────────────────────────────────────
git add "$CARGO_TOML"
git commit -m "chore: bump version to v${new_version}"
git push origin main
echo "✓ Commit pushed"

# ── Tag & push ────────────────────────────────────────────────────────────
git tag "v${new_version}"
git push origin "v${new_version}"
echo "✓ Tag v${new_version} pushed"

echo ""
echo "🚀 Release v${new_version} triggered!"
echo "   Follow progress at: https://github.com/redbug26/kkc-rust/actions"
