#!/usr/bin/env bash
set -euo pipefail

# Read version from Cargo.toml (single source of truth)
VERSION=$(grep '^version' Cargo.toml | head -1 | sed 's/.*"\(.*\)"/\1/')
TAG="v${VERSION}"

echo "Preparing release ${TAG}..."

# Guard: dirty working tree
if [[ -n "$(git status --porcelain)" ]]; then
  echo "ERROR: Working tree is dirty. Commit or stash changes before tagging." >&2
  exit 1
fi

# Guard: version sync (package.json must match Cargo.toml)
echo "Checking version sync..."
if ! bash scripts/version-sync.sh; then
  echo "ERROR: Version sync failed. Run 'bash scripts/version-sync.sh' to fix." >&2
  exit 1
fi

# Guard: formatting
echo "Checking formatting..."
if ! cargo fmt --all -- --check; then
  echo "ERROR: cargo fmt check failed. Run 'cargo fmt --all' to fix." >&2
  exit 1
fi

# Guard: lints
echo "Running clippy..."
if ! cargo clippy --workspace --all-targets -- -D warnings; then
  echo "ERROR: clippy found warnings. Fix them before tagging." >&2
  exit 1
fi

# Idempotent: skip if tag already exists
if git rev-parse "${TAG}" >/dev/null 2>&1; then
  echo "Tag ${TAG} already exists. Skipping."
  exit 0
fi

# Create and push tag
echo "Creating tag ${TAG}..."
git tag "${TAG}"
git push origin "${TAG}"
echo "Released ${TAG}."
