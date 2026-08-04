#!/usr/bin/env sh
set -eu

root=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
jobs=${QUENCH_NODE_STAGE_JOBS:-${JOBS:-4}}
timeout_seconds=${QUENCH_NODE_TEST_TIMEOUT_SECONDS:-30}
failures=$(mktemp)
trap 'rm -f "$failures"' EXIT HUP INT TERM
export root failures
export timeout_seconds

cargo build -q --manifest-path "$root/Cargo.toml" -p quench-node
runner="$root/target/debug/quench-node"
export runner

find "$root/tests/node-compat" -mindepth 1 -maxdepth 1 -type d -name 'stage-*' \
  -exec basename {} \; | sed 's/stage-//' | sort -n | while IFS= read -r stage; do
  printf '%s\n' "$stage"
done | xargs -n 1 -P "$jobs" sh -c '
  stage=$0
  flags=""
  if [ "$stage" -ge 169 ] && [ "$stage" -le 174 ]; then
    flags="--experimental-stream-iter"
  fi
  run_stage() {
    if command -v timeout >/dev/null 2>&1; then
      timeout "$timeout_seconds" "$runner" $flags --stage "$stage"
    elif command -v gtimeout >/dev/null 2>&1; then
      gtimeout "$timeout_seconds" "$runner" $flags --stage "$stage"
    else
      perl -e "alarm shift; exec \\@ARGV" "$timeout_seconds" "$runner" $flags --stage "$stage"
    fi
  }
  if ! run_stage >/dev/null 2>&1; then
    printf "%s\\n" "$stage" >>"$failures"
  fi
'

fail=$(wc -l <"$failures" | tr -d ' ')
pass=$(find "$root/tests/node-compat" -mindepth 1 -maxdepth 1 -type d -name 'stage-*' | wc -l | tr -d ' ')
pass=$((pass - fail))
echo "focused_stage_pass=$pass"
echo "focused_stage_fail=$fail"
echo "failed_stages=$(sort -n "$failures" | tr "\n" " " | sed "s/[[:space:]]*$//")"
[ "$fail" -eq 0 ]
