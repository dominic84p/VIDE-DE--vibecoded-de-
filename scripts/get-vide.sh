#!/usr/bin/env bash
# Install (or update to) the newest prebuilt VIDE-DE from GitHub Releases, no compiling.
# The bundle is built on Ubuntu 24.04 (scripts/make-bundle.sh), so it runs there, on Linux Mint 22,
# and on anything newer, Arch included.
#
# Usage: scripts/get-vide.sh            or, with nothing checked out:
#   curl -fsSL https://github.com/dominic84p/vide-de/releases/latest/download/get-vide.sh | bash
set -euo pipefail

# All in a function so that with `curl | bash` the whole script is read before any of it runs.
main() {
  REPO=dominic84p/vide-de
  URL="https://github.com/$REPO/releases/latest/download/vide-de-x86_64.tar.gz"

  if [ "$(uname -m)" != x86_64 ]; then
    echo "The prebuilt VIDE-DE is x86_64 only; build it from source on $(uname -m)." >&2
    exit 1
  fi
  for tool in curl tar; do
    command -v "$tool" >/dev/null || { echo "Needs $tool installed." >&2; exit 1; }
  done

  # Piped into bash, stdin is the script; give the installer (sudo, apt) the terminal instead.
  [ -t 0 ] || [ ! -r /dev/tty ] || exec </dev/tty

  TMP="$(mktemp -d)"
  trap 'rm -rf "$TMP"' EXIT

  echo "Downloading the latest VIDE-DE release..."
  curl -fL --progress-bar "$URL" -o "$TMP/vide-de.tar.gz"
  tar -C "$TMP" -xzf "$TMP/vide-de.tar.gz"
  cat "$TMP/vide-de/VERSION"

  # The bundle's own installer asks for sudo and does the rest.
  "$TMP/vide-de/install.sh"
}

main "$@"
