#!/usr/bin/env sh
set -eu

root=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
timeout_seconds=${QUENCH_NODE_TEST_TIMEOUT_SECONDS:-30}
pass=0
fail=0
failed=""

for stage in $(find "$root/tests/node-compat" -mindepth 1 -maxdepth 1 -type d -name 'stage-*' -exec basename {} \; | sed 's/stage-//' | sort -n); do
  flags=""
  if [ "$stage" -ge 169 ] && [ "$stage" -le 174 ]; then
    flags="--experimental-stream-iter"
  fi
  if command -v timeout >/dev/null 2>&1; then
    run="timeout $timeout_seconds cargo run -q --manifest-path $root/Cargo.toml -p quench-node -- $flags --stage $stage"
  elif command -v gtimeout >/dev/null 2>&1; then
    run="gtimeout $timeout_seconds cargo run -q --manifest-path $root/Cargo.toml -p quench-node -- $flags --stage $stage"
  else
    run="perl -e \"alarm shift; exec \\@ARGV\" $timeout_seconds cargo run -q --manifest-path $root/Cargo.toml -p quench-node -- $flags --stage $stage"
  fi
  if eval "$run" >/tmp/quench-node-stage.out 2>&1; then
    pass=$((pass + 1))
  else
    fail=$((fail + 1))
    failed="$failed $stage"
  fi
done

echo "focused_stage_pass=$pass"
echo "focused_stage_fail=$fail"
echo "failed_stages=${failed# }"
