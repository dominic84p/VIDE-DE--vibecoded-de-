#!/usr/bin/env bash
# Build the prebuilt bundle (compositor + every app) and publish it as the latest GitHub release,
# which is what scripts/get-vide.sh and install-vide-de.desktop download.
#
# Usage: scripts/release.sh            (needs podman, and gh logged in)
# Tags are build-YYYYMMDD-<commit>: there are no version numbers to bump, just builds of main.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

if [ -n "$(git status --porcelain)" ]; then
  echo "Uncommitted changes: commit them first so the release matches a commit on GitHub." >&2
  exit 1
fi
SHA="$(git rev-parse HEAD)"
git fetch -q --tags origin
if ! git merge-base --is-ancestor "$SHA" origin/main; then
  echo "HEAD isn't pushed to origin/main yet: push first." >&2
  exit 1
fi

SHORT="$(git rev-parse --short HEAD)"
TAG="build-$(date -u +%Y%m%d)-$SHORT"
if gh release view "$TAG" >/dev/null 2>&1; then
  echo "$TAG is already released." >&2
  exit 1
fi

"$ROOT/scripts/make-bundle.sh"

# What changed since the last build, for the release notes.
PREV="$(gh release list --limit 1 --json tagName --jq '.[0].tagName // empty')"
NOTES="$(mktemp)"
trap 'rm -f "$NOTES"' EXIT
{
  echo "Prebuilt VIDE-DE for x86_64: Ubuntu 24.04, Linux Mint 22, Arch and anything newer."
  echo
  echo "**Install or update:** download \`install-vide-de.desktop\` and double-click it, or run"
  echo
  echo '```bash'
  echo "curl -fsSL https://github.com/dominic84p/vide-de/releases/latest/download/get-vide.sh | bash"
  echo '```'
  echo
  echo "Or unpack \`vide-de-x86_64.tar.gz\` and double-click **Install VIDE-DE** inside it."
  if [ -n "$PREV" ] && git rev-parse -q --verify "$PREV^{commit}" >/dev/null; then
    echo
    echo "### Changes since $PREV"
    echo
    git log --no-merges --format='- %s (%h)' "$PREV..$SHA"
  fi
} > "$NOTES"

B="$ROOT/target/bundle"
gh release create "$TAG" --target "$SHA" --latest \
  --title "VIDE-DE $(date -u +%Y-%m-%d) ($SHORT)" --notes-file "$NOTES" \
  "$B/vide-de-x86_64.tar.gz" "$B/vide-de-x86_64.zip" \
  "$ROOT/scripts/get-vide.sh" "$ROOT/scripts/install-vide-de.desktop"
