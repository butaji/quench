#!/usr/bin/env sh
set -eu

root=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)

if command -v cargo-nextest >/dev/null 2>&1; then
  cargo nextest run --manifest-path "$root/Cargo.toml" --workspace
else
  echo "cargo-nextest is not installed; install it to run Rust tests in parallel" >&2
  cargo test --manifest-path "$root/Cargo.toml" --workspace
fi

"$root/tools/check-focused-stages-parallel.sh"
