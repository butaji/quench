#!/bin/bash
# Runs every invalidation case in the two modes: each prints `ok <case>` or throws.
# Usage: tests/invalidation/run.sh [quench binary]
bin=${1:-target/experiment/quench}; dir=$(dirname "$0"); fail=0
for f in "$dir"/*.js; do for m in --no-jit ""; do
  out=$(XDG_CACHE_HOME=/dev/null timeout 120 "$bin" $m "$f" 2>&1)
  [ "$out" = "ok $(basename "$f" .js)" ] || { echo "FAIL ${m:-default} $f: $(echo "$out" | head -3)"; fail=1; }
done; done
[ $fail = 0 ] && echo "all invalidation cases pass"; exit $fail
