#!/usr/bin/env sh
set -eu

root=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
report=${1:-"$root/target/compat/differential-current.json"}
ownership=${2:-"$root/tools/compat-ownership.json"}
fixtures=${3:-"$root/tests/node/test/parallel"}

if [ "${QUENCH_COMPAT_ALLOW_STALE:-0}" != "1" ]; then
  "$root/tools/compat-report-status.sh" "$report" "$fixtures"
  node "$root/tools/audit-platform-coverage.cjs" "$root" "$report" "$ownership" "$fixtures"
else
  node "$root/tools/audit-platform-coverage.cjs" "$root" "" "$ownership" "$fixtures"
fi
