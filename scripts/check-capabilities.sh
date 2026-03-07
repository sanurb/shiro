#!/usr/bin/env bash
set -euo pipefail

# Build once, query once
cargo build --quiet
CAPS=$(./target/debug/shiro capabilities --home /tmp/shiro-cap-check 2>/dev/null)

# Extract schemaVersion (top-level hardcoded field, not DB schema_version)
SCHEMA_VER=$(echo "$CAPS" | jq -r '.result.schemaVersion')

if [ "$SCHEMA_VER" -lt 2 ]; then
  echo "ERROR: schemaVersion is $SCHEMA_VER, expected >= 2"
  exit 1
fi

# Check that every feature claimed is not "stub", "planned", or "not_implemented"
FEATURES=$(echo "$CAPS" | jq -r '.result.features | to_entries[] | "\(.key)=\(.value)"')

while IFS='=' read -r feat status; do
  if [[ "$status" == "stub" || "$status" == "planned" || "$status" == "not_implemented" ]]; then
    echo "ERROR: Feature '$feat' has status '$status' — Rule 0 violation"
    exit 1
  fi
done <<< "$FEATURES"

echo "OK: All features are implemented (schemaVersion=$SCHEMA_VER)"
