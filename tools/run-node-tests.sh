#!/usr/bin/env sh
set -eu

root=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
timeout_seconds=${QUENCH_NODE_TEST_TIMEOUT_SECONDS:-10}
binary="$root/target/debug/quench-node"
needs_build=0
if [ ! -x "$binary" ]; then
  needs_build=1
elif find "$root/crates/quench-node" "$root/tests/node-compat" \
    "$root/tests/node/test/common" -type f -newer "$binary" \
    -print -quit | grep -q .; then
  needs_build=1
fi
if [ "$needs_build" -eq 1 ]; then
  cargo build --quiet --manifest-path "$root/Cargo.toml" -p quench-node
fi
if [ "${1:-}" = "--stage" ]; then
  mode_args="--stage ${2:-0}"
else
  fixture=${1:-$root/tests/node/test/parallel}
  fixture_flags=""
  if [ -f "$fixture" ]; then
    fixture_flags=$(sed -nE 's/^[[:space:]]*\/\/[[:space:]]*Flags:[[:space:]]*(.*)$/\1/p' "$fixture" | head -n 1)
  fi
  if [ -f "$fixture" ]; then
    mode_args="${fixture_flags} --test-dir ${fixture}"
  else
    mode_args="${fixture_flags} --test-dir ${fixture}"
  fi
fi
if command -v timeout >/dev/null 2>&1; then
  exec timeout "$timeout_seconds" "$binary" $mode_args
elif command -v gtimeout >/dev/null 2>&1; then
  exec gtimeout "$timeout_seconds" "$binary" $mode_args
else
  exec perl -e 'alarm shift; exec @ARGV' "$timeout_seconds" "$binary" $mode_args
fi
