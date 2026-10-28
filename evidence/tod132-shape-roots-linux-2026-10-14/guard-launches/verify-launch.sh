#!/usr/bin/env bash
set -euo pipefail
log=$(mktemp /workspace/quench/work/guard-launch-XXXXXX)
if ! /workspace/quench/work/valgrind/usr/bin/valgrind "$@" > "$log.stdout" 2> "$log.stderr"; then
  cat "$log.stdout" "$log.stderr" >&2
  exit 1
fi
if ! rg -q '^done ' "$log.stdout"; then
  cat "$log.stdout" >&2
  echo 'guard launch did not emit its completion marker' >&2
  exit 1
fi
cat "$log.stdout"
cat "$log.stderr" >&2
