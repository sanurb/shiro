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

# Guard: npm package name must be scoped
EXPECTED_PKG_NAME="@sanurb/shiro-cli"
ACTUAL_PKG_NAME="$(grep '"name"' "$PACKAGE_JSON" | head -1 | sed 's/.*"name": *"\(.*\)".*/\1/')"
if [[ "$ACTUAL_PKG_NAME" != "$EXPECTED_PKG_NAME" ]]; then
  echo "error: package.json name is '$ACTUAL_PKG_NAME', expected '$EXPECTED_PKG_NAME'" >&2
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
  # Changesets bumps package.json — propagate that version to Cargo.toml (source of truth for Rust builds).
  # Also sync Cargo.toml → package.json in case Cargo.toml was bumped manually.
  if [[ "$CARGO_VERSION" != "$PKG_VERSION" ]]; then
    # Prefer package.json as the authority when called from changesets (it was just bumped).
    NEW_VERSION="$PKG_VERSION"
    sed "s/^version = \"${CARGO_VERSION}\"/version = \"${NEW_VERSION}\"/" "$CARGO_TOML" > "$CARGO_TOML.tmp" \
      && mv "$CARGO_TOML.tmp" "$CARGO_TOML"
    echo "updated Cargo.toml workspace version: $CARGO_VERSION -> $NEW_VERSION"
  fi
  exit 0
fi

echo "error: version mismatch — Cargo.toml=$CARGO_VERSION, package.json=$PKG_VERSION" >&2
echo "run with --write to update package.json" >&2
exit 1
