#!/usr/bin/env sh
set -eu

root=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
if [ "${1:-}" ]; then
  report=$1
else
  report="$root/target/compat/differential-current.json"
  if [ -f "$root/target/compat/differential-parallel.json" ]; then
    report="$root/target/compat/differential-parallel.json"
  fi
fi
fixtures=${2:-"$root/tests/node/test/parallel"}

if [ ! -f "$report" ]; then
  echo "report does not exist: $report" >&2
  exit 2
fi
if [ ! -d "$fixtures" ] && [ ! -f "$fixtures" ]; then
  echo "fixture path does not exist: $fixtures" >&2
  exit 2
fi

QUENCH_NODE_BIN="${QUENCH_NODE_BIN:-$root/target/debug/quench-node}" \
  node "$root/tools/compat-report-status.cjs" "$root" "$report" "$fixtures"

node "$root/tools/audit-platform-coverage.cjs" \
  "$root" "$report" "$root/tools/compat-ownership.json" "$fixtures"
