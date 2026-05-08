#!/usr/bin/env bash
#
# Local helper: build VS Code extension `.vsix` against locally built server
# binaries (or downloaded ones via `gh release download`).
#
# Usage:
#   scripts/package-extension.sh              # uses target/release/js-sem-highlight
#   scripts/package-extension.sh --release v0.1.0   # downloads from a tag
#
# Output: client/js-sem-highlight-<version>.vsix

set -euo pipefail

MODE="local"
RELEASE_TAG=""

while [[ $# -gt 0 ]]; do
  case "$1" in
    --release)
      MODE="release"
      RELEASE_TAG="${2:?--release requires a tag}"
      shift 2
      ;;
    -h|--help)
      cat <<USAGE
Usage:
  $0                       Bundle local target/release/js-sem-highlight
  $0 --release <tag>       Download all platform binaries from GitHub release
  $0 --help                Show this help
USAGE
      exit 0
      ;;
    *)
      echo "Unknown argument: $1" >&2
      exit 1
      ;;
  esac
done

PROJECT_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$PROJECT_ROOT/client"

if [[ ! -d node_modules ]]; then
  echo "==> Installing client dependencies"
  npm ci
fi

echo "==> Compiling TypeScript"
npx tsc -p .

mkdir -p server

if [[ "$MODE" = "local" ]]; then
  HOST_OS="$(uname -s)"
  HOST_ARCH="$(uname -m)"
  case "$HOST_OS-$HOST_ARCH" in
    Darwin-arm64) SLUG="darwin-arm64";;
    Darwin-x86_64) SLUG="darwin-x64";;
    Linux-x86_64) SLUG="linux-x64";;
    Linux-aarch64) SLUG="linux-arm64";;
    *)
      echo "Unsupported host: $HOST_OS-$HOST_ARCH" >&2
      exit 1
      ;;
  esac
  echo "==> Bundling local binary into server/$SLUG/"
  mkdir -p "server/$SLUG"
  cp "$PROJECT_ROOT/target/release/js-sem-highlight" "server/$SLUG/"
  chmod +x "server/$SLUG/js-sem-highlight"
else
  echo "==> Downloading binaries from release $RELEASE_TAG"
  for slug in darwin-arm64 darwin-x64 linux-x64 linux-arm64 win32-x64; do
    mkdir -p "server/$slug"
    if [[ "$slug" = "win32-x64" ]]; then
      archive="js-sem-highlight-${slug}.zip"
      gh release download "$RELEASE_TAG" -p "$archive" -D /tmp
      unzip -o "/tmp/$archive" -d /tmp/extract-${slug}
      cp "/tmp/extract-${slug}/js-sem-highlight-${slug}/js-sem-highlight.exe" "server/$slug/"
    else
      archive="js-sem-highlight-${slug}.tar.gz"
      gh release download "$RELEASE_TAG" -p "$archive" -D /tmp
      tar -xzf "/tmp/$archive" -C /tmp
      cp "/tmp/js-sem-highlight-${slug}/js-sem-highlight" "server/$slug/"
      chmod +x "server/$slug/js-sem-highlight"
    fi
  done
fi

echo "==> Packaging .vsix"
npx vsce package

echo "==> Done. Look for the .vsix in $PROJECT_ROOT/client/"
ls -la "$PROJECT_ROOT/client/"*.vsix
