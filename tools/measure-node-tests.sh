#!/usr/bin/env sh
set -eu

root=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
dir=${1:-"$root/tests/node/test"}
binary=${QUENCH_NODE_BIN:-"$root/target/debug/quench-node"}
timeout_seconds=${QUENCH_NODE_TEST_TIMEOUT_SECONDS:-10}

if [ ! -x "$binary" ]; then
  cargo build --quiet --manifest-path "$root/Cargo.toml" -p quench-node
fi

total=0
passed=0
failed=0
skipped=0
started=$(date +%s)

run_fixture() {
  file=$1
  if command -v timeout >/dev/null 2>&1; then
    timeout "$timeout_seconds" "$binary" "$file"
  elif command -v gtimeout >/dev/null 2>&1; then
    gtimeout "$timeout_seconds" "$binary" "$file"
  else
    perl -e 'alarm shift; exec @ARGV' "$timeout_seconds" "$binary" "$file"
  fi
}

while IFS= read -r file; do
  total=$((total + 1))
  if run_fixture "$file" >/dev/null 2>&1; then
    passed=$((passed + 1))
  else
    failed=$((failed + 1))
  fi
  if [ $((total % 100)) -eq 0 ]; then
    echo "progress tested=$total passed=$passed failed=$failed" >&2
  fi
done <<EOF
$(find "$dir" -type f \( -name '*.js' -o -name '*.mjs' -o -name '*.cjs' \) | sort)
EOF

percent=$(awk -v passed="$passed" -v total="$total" 'BEGIN { if (total == 0) print "0.00"; else printf "%.2f", passed * 100 / total }')
echo "test_files=$total"
echo "passed_files=$passed"
echo "failed_files=$failed"
echo "skipped_files=$skipped"
echo "file_pass_rate=${percent}%"
echo "elapsed_seconds=$(( $(date +%s) - started ))"
