#!/usr/bin/env bash
# Publish all workspace crates to crates.io in dependency order.
# Requires: cargo login (run once to save API token)
#
# Usage: bash scripts/publish-crates.sh [--dry-run]
set -euo pipefail

DRY_RUN=""
if [[ "${1:-}" == "--dry-run" ]]; then
  DRY_RUN="--dry-run"
  echo "DRY RUN — no actual publishing"
fi

# Dependency order: leaf crates first, CLI last
CRATES=(
  shiro-core
  shiro-store
  shiro-index
  shiro-parse
  shiro-embed
  shiro-docling
  shiro-sdk
  shiro-cli
)

# Delay between publishes to allow crates.io index to update
DELAY=30

for crate in "${CRATES[@]}"; do
  echo "=== Publishing ${crate} ==="
  cargo publish -p "${crate}" ${DRY_RUN}

  if [[ -z "${DRY_RUN}" && "${crate}" != "${CRATES[-1]}" ]]; then
    echo "Waiting ${DELAY}s for crates.io index update..."
    sleep "${DELAY}"
  fi
done

echo "Done."
