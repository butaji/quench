#!/usr/bin/env sh
set -eu

root=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
if [ "${1:-}" = "--stage" ]; then
  exec cargo run --quiet --manifest-path "$root/Cargo.toml" -p quench-node -- --stage "${2:-0}"
fi
dir=${1:-"$root/tests/node/test/parallel"}
exec cargo run --quiet --manifest-path "$root/Cargo.toml" -p quench-node -- --test-dir "$dir"
