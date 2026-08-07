#!/usr/bin/env bash
# Enforce the repository-wide 500 physical-line limit for tracked source files.
set -euo pipefail

root="$(cd "$(dirname "$0")/.." && pwd)"
cd "$root"

status=0
while IFS= read -r -d '' file; do
  lines=$(wc -l < "$file")
  if (( lines > 500 )); then
    printf 'ERROR: %s has %d lines (maximum is 500)\n' "$file" "$lines" >&2
    status=1
  fi
done < <(git ls-files -z -- '*.rs' '*.ts' '*.tsx' '*.js' '*.mjs')

exit "$status"
