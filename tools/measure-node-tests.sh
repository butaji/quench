#!/usr/bin/env sh
set -eu

root=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
dir=${1:-"$root/tests/node/test"}
binary=${QUENCH_NODE_BIN:-"$root/target/debug/quench-node"}

if [ ! -x "$binary" ]; then
  cargo build --quiet --manifest-path "$root/Cargo.toml" -p quench-node
fi

total=0
passed=0
failed=0
skipped=0

while IFS= read -r file; do
  total=$((total + 1))
  if "$binary" "$file" >/dev/null 2>&1; then
    passed=$((passed + 1))
  else
    failed=$((failed + 1))
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
