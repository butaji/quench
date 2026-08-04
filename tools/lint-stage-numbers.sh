#!/usr/bin/env bash
set -euo pipefail

root="$(cd "$(dirname "$0")/.." && pwd)"
stage_root="$root/tests/node-compat"
expected=0
stage_count=0
while IFS= read -r stage; do
  if [[ "$stage" != "$expected" ]]; then
    echo "stage numbering gap: expected $expected, found $stage" >&2
    exit 1
  fi
  expected=$((expected + 1))
  stage_count=$((stage_count + 1))
done < <(
  find "$stage_root" -maxdepth 1 -type d -name 'stage-*' -print \
    | sed 's#^.*/stage-##' \
    | sort -n
)

echo "OK: $stage_count contiguous stages"
