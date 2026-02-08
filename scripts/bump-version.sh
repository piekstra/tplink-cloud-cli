#!/usr/bin/env bash
set -euo pipefail

if [ $# -ne 1 ]; then
  echo "Usage: $0 <version>"
  echo "Example: $0 0.1.42"
  exit 1
fi

VERSION="$1"

if ! echo "$VERSION" | grep -qE '^[0-9]+\.[0-9]+\.[0-9]+$'; then
  echo "Error: Version must be in semver format (e.g., 0.1.42)"
  exit 1
fi

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
ROOT="$(dirname "$SCRIPT_DIR")"

# Cargo.toml (line 3 — package version, not dependency versions)
sed -i '' '3s/version = "[^"]*"/version = "'"$VERSION"'"/' "$ROOT/Cargo.toml"
echo "Cargo.toml -> $VERSION"

echo ""
echo "Version bumped to $VERSION"
