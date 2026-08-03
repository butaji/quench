#!/usr/bin/env sh
set -eu

root=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
jobs=${QUENCH_NODE_STAGE_JOBS:-${JOBS:-$(getconf _NPROCESSORS_ONLN 2>/dev/null || echo 2)}}
failures=$(mktemp)
trap 'rm -f "$failures"' EXIT HUP INT TERM
export root failures

find "$root/tests/node-compat" -mindepth 1 -maxdepth 1 -type d -name 'stage-*' \
  -exec basename {} \; | sed 's/stage-//' | sort -n | while IFS= read -r stage; do
  printf '%s\n' "$stage"
done | xargs -n 1 -P "$jobs" sh -c '
  stage=$0
  flags=""
  if [ "$stage" -ge 169 ] && [ "$stage" -le 174 ]; then
    flags="--experimental-stream-iter"
  fi
  if ! cargo run -q --manifest-path "$root/Cargo.toml" -p quench-node -- $flags --stage "$stage" >/dev/null 2>&1; then
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
