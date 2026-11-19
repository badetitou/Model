#!/usr/bin/env bash
# Publish every crate of the workspace to crates.io, in dependency order.
# Requires a configured Cargo registry token (`cargo login` or
# CARGO_REGISTRY_TOKEN). Run from anywhere; --allow-dirty keeps this usable
# with uncommitted release changes. With an argument, checks that it matches
# the workspace version before publishing.
set -euo pipefail

cd "$(dirname "$0")/.."
source "$(dirname "$0")/crates.sh"

VERSION="$(grep -m1 '^version = "' Cargo.toml | cut -d'"' -f2)"
if [ $# -gt 0 ] && [ "$1" != "$VERSION" ]; then
  echo "version mismatch: tag '$1' vs Cargo.toml '$VERSION'" >&2
  exit 1
fi

# A newly published crate takes a few seconds to reach the sparse index that
# `cargo publish --dry-run` uses to verify the next crate's dependencies.
wait_until_verifiable() {
  local crate="$1"
  for _ in $(seq 1 24); do
    if cargo publish --dry-run --locked --allow-dirty -p "$crate" >/dev/null 2>&1; then
      return 0
    fi
    sleep 5
  done
  echo "timed out waiting for $crate's dependencies to reach crates.io" >&2
  return 1
}

for crate in "${CRATES[@]}"; do
  echo "== $crate $VERSION: verifying"
  wait_until_verifiable "$crate"
  echo "== $crate $VERSION: publishing"
  cargo publish --locked --allow-dirty -p "$crate"
done

echo "Published ${#CRATES[@]} crates at $VERSION."
