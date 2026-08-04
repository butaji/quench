#!/usr/bin/env bash
# Emit a coverage table by test-name prefix from running quench-node over a directory.
# Usage: tools/coverage-by-prefix.sh [directory]
set -euo pipefail

root="$(cd "$(dirname "$0")/.." && pwd)"
dir="${1:-$root/tests/node/test/parallel}"

if [[ ! -d "$dir" ]]; then
  echo "Error: directory $dir does not exist" >&2
  exit 1
fi

echo "$@" | head -200 > /tmp/qcov-output.txt
cargo run -q --manifest-path "$root/Cargo.toml" -p quench-node \
  -- --test-dir "$dir" > /tmp/qcov-output.txt 2>&1 || true

awk '
/^(ok|not_ok) / {
  n = split($2, parts, "/")
  base = parts[n]
  sub(/^test-/, "", base)
  idx = index(base, "-")
  if (idx > 0) prefix = substr(base, 1, idx - 1)
  else prefix = base
  total[prefix]++
  if ($1 == "ok") pass[prefix]++
}
END {
  grand_total = 0
  grand_pass = 0
  for (p in total) {
    t = total[p]
    s = (p in pass) ? pass[p] : 0
    f = t - s
    r = (t > 0) ? int(s * 100 / t) : 0
    printf "%-22s %8d %8d %8d %5d%%\n", p, t, s, f, r
    grand_total += t
    grand_pass += s
  }
  r = (grand_total > 0) ? int(grand_pass * 100 / grand_total) : 0
  printf "\n%-22s %8d %8d %8s %5d%%\n", "OVERALL", grand_total, grand_pass, "", r
}' /tmp/qcov-output.txt
