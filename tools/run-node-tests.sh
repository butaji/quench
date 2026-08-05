#!/usr/bin/env sh
set -eu

root=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
timeout_seconds=${QUENCH_NODE_TEST_TIMEOUT_SECONDS:-10}
binary="$root/target/debug/quench-node"
if [ ! -x "$binary" ]; then
  cargo build --quiet --manifest-path "$root/Cargo.toml" -p quench-node
fi
if [ "${1:-}" = "--stage" ]; then
  mode_args="--stage ${2:-0}"
else
  mode_args="--test-dir ${1:-$root/tests/node/test/parallel}"
fi
if command -v timeout >/dev/null 2>&1; then
  exec timeout "$timeout_seconds" "$binary" $mode_args
elif command -v gtimeout >/dev/null 2>&1; then
  exec gtimeout "$timeout_seconds" "$binary" $mode_args
else
  exec perl -e 'alarm shift; exec @ARGV' "$timeout_seconds" "$binary" $mode_args
fi
