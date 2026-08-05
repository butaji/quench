#!/usr/bin/env sh
set -eu

root=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
dir=${1:-"$root/tests/node/test/parallel"}
timeout_seconds=${QUENCH_NODE_TEST_TIMEOUT_SECONDS:-10}
binary=${QUENCH_NODE_BIN:-"$root/target/debug/quench-node"}
if [ ! -x "$binary" ]; then
  cargo build --quiet --manifest-path "$root/Cargo.toml" -p quench-node
fi

run_fixture() {
  if command -v timeout >/dev/null 2>&1; then
    timeout "$timeout_seconds" "$binary" "$1"
  elif command -v gtimeout >/dev/null 2>&1; then
    gtimeout "$timeout_seconds" "$binary" "$1"
  else
    perl -e 'alarm shift; exec @ARGV' "$timeout_seconds" "$binary" "$1"
  fi
}

prefixes=$(find "$dir" -type f \( -name '*.js' -o -name '*.mjs' -o -name '*.cjs' \) \
  | sed 's#.*/##; s#^test-##; s#-.*##' | sort -u)
for prefix in $prefixes; do
  total=0
  passed=0
  while IFS= read -r fixture; do
    total=$((total + 1))
    run_fixture "$fixture" >/dev/null 2>&1 && passed=$((passed + 1)) || true
  done <<EOF
$(find "$dir" -type f \( -name "test-${prefix}-*.js" -o -name "test-${prefix}-*.mjs" -o -name "test-${prefix}-*.cjs" \) | sort)
EOF
  percent=$(awk -v passed="$passed" -v total="$total" \
    'BEGIN { if (total == 0) print "0.00"; else printf "%.2f", passed * 100 / total }')
  printf '%s: %s/%s (%s%%)\n' "$prefix" "$passed" "$total" "$percent"
done
