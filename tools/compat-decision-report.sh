#!/usr/bin/env sh
set -eu

root=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
report=${1:-"$root/target/compat/differential-current.json"}
previous=${2:-}
metrics=${3:-"$root/target/compat/focused-stage-metrics.jsonl"}
output=${4:-"$root/target/compat/compat-decision.json"}

node "$root/tools/compat-decision-report.cjs" "$root" "$report" \
  "$root/tools/compat-ownership.json" "$previous" "$metrics" "$output"
