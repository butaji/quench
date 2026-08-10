#!/usr/bin/env sh
set -eu
root=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
exec node "$root/tools/compat-goal-audit.cjs" "$root" "${1:-}"
runner=${QUENCH_NODE_BIN:-"$root/target/debug/quench-node"}
[ -x "$runner" ] || cargo build --quiet --manifest-path "$root/Cargo.toml" -p quench-node
for stage in $stages; do
  echo "application_stage=$stage"
  "$runner" --stage "$stage"
done
