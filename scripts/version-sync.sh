#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"

CARGO_TOML="$REPO_ROOT/Cargo.toml"
PACKAGE_JSON="$REPO_ROOT/package.json"

if [[ ! -f "$CARGO_TOML" ]]; then
  echo "error: $CARGO_TOML not found" >&2
  exit 1
fi

if [[ ! -f "$PACKAGE_JSON" ]]; then
  echo "error: $PACKAGE_JSON not found" >&2
  exit 1
fi

CARGO_VERSION="$(grep '^version' "$CARGO_TOML" | head -1 | sed 's/.*"\(.*\)"/\1/')"
PKG_VERSION="$(grep '"version"' "$PACKAGE_JSON" | head -1 | sed 's/.*"version": *"\(.*\)".*/\1/')"

if [[ -z "$CARGO_VERSION" ]]; then
  echo "error: could not read version from $CARGO_TOML" >&2
  exit 1
fi

if [[ -z "$PKG_VERSION" ]]; then
  echo "error: could not read version from $PACKAGE_JSON" >&2
  exit 1
fi

WRITE=false
for arg in "$@"; do
  if [[ "$arg" == "--write" ]]; then
    WRITE=true
  fi
done

if [[ "$CARGO_VERSION" == "$PKG_VERSION" ]]; then
  echo "versions in sync: $CARGO_VERSION"
  exit 0
fi

if [[ "$WRITE" == true ]]; then
  sed "s/\"version\": \".*\"/\"version\": \"${CARGO_VERSION}\"/" "$PACKAGE_JSON" > "$PACKAGE_JSON.tmp" \
    && mv "$PACKAGE_JSON.tmp" "$PACKAGE_JSON"
  echo "updated package.json version: $PKG_VERSION -> $CARGO_VERSION"
  exit 0
fi

echo "error: version mismatch — Cargo.toml=$CARGO_VERSION, package.json=$PKG_VERSION" >&2
echo "run with --write to update package.json" >&2
exit 1
