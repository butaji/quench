#!/usr/bin/env sh
set -eu

root=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
pass=0
fail=0
failed=""

for stage in $(find "$root/tests/node-compat" -mindepth 1 -maxdepth 1 -type d -name 'stage-*' -exec basename {} \; | sed 's/stage-//' | sort -n); do
  flags=""
  if [ "$stage" -ge 169 ] && [ "$stage" -le 174 ]; then
    flags="--experimental-stream-iter"
  fi
  if cargo run -q --manifest-path "$root/Cargo.toml" -p quench-node -- $flags --stage "$stage" >/tmp/quench-node-stage.out 2>&1; then
    pass=$((pass + 1))
  else
    fail=$((fail + 1))
    failed="$failed $stage"
  fi
done

echo "focused_stage_pass=$pass"
echo "focused_stage_fail=$fail"
echo "failed_stages=${failed# }"
