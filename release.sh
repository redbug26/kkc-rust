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
echo ""

# ── Wait for GitHub Actions to publish the Homebrew formula ──────────────
FORMULA_URL="https://raw.githubusercontent.com/redbug26/kkc-rust/refs/heads/main/Formula/kkc.rb"
HOMEBREW_TAP_DIR="/Users/miguelvanhove/Dropbox/Sources/homebrew-tap"
echo "⏳ Waiting for GitHub release workflow to complete…"
echo "   Polling ${FORMULA_URL}"
echo "   (checking every 30 seconds)"

while true; do
  remote_version=$(curl -sf "$FORMULA_URL" \
    | grep -Eo 'version "[^"]+"' \
    | head -1 \
    | grep -Eo '[0-9]+\.[0-9]+\.[0-9]+' \
    || true)

  if [[ "$remote_version" == "$new_version" ]]; then
    echo "✓ Formula updated to v${new_version}"
    break
  fi

  echo "   Remote formula version: ${remote_version:-<not found>} (waiting for ${new_version})…"
  sleep 30
done

# ── Pull latest changes (formula was committed by CI) ────────────────────
echo ""
echo "Pulling latest changes…"
git pull origin main
echo "✓ git pull done"

# ── Copy formula to homebrew-tap and push ────────────────────────────────
echo ""
echo "Copying Formula/kkc.rb to ${HOMEBREW_TAP_DIR}/Formula/"
mkdir -p "${HOMEBREW_TAP_DIR}/Formula"
cp Formula/kkc.rb "${HOMEBREW_TAP_DIR}/Formula/kkc.rb"

pushd "${HOMEBREW_TAP_DIR}" > /dev/null
git add Formula/kkc.rb
if git diff --cached --quiet; then
  echo "ℹ️  No changes to commit in homebrew-tap (formula already up to date)."
else
  git commit -m "chore: update kkc to v${new_version}"
  git push
  echo "✓ homebrew-tap pushed"
fi
popd > /dev/null

echo ""
echo "✅ All done! kkc v${new_version} is released and the tap is updated."
