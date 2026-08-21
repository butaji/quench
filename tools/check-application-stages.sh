#!/usr/bin/env sh
set -eu
root=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
stages=${QUENCH_APPLICATION_STAGES:-"2047 2069 2080 2081 2104 2251"}
runner=${QUENCH_NODE_BIN:-"$root/target/debug/quench-node"}
metrics=${QUENCH_APPLICATION_METRICS_FILE:-"$root/target/compat/application-gates.jsonl"}
[ -x "$runner" ] || cargo build --quiet --manifest-path "$root/Cargo.toml" -p quench-node
mkdir -p "$(dirname -- "$metrics")"
: >"$metrics"
failed=0
for stage in $stages; do
  echo "application_stage=$stage"
  set +e
  "$runner" --stage "$stage"
  status=$?
  set -e
  printf '{"stage":%s,"status":%s}\n' "$stage" "$status" >>"$metrics"
  [ "$status" -eq 0 ] || failed=1
done
echo "application_metrics=$metrics"
[ "$failed" -eq 0 ]
