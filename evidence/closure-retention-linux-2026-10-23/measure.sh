#!/bin/bash
# Runs every measurement of this evidence with one engine binary, writing <name>-<label>.txt.
# usage: measure.sh ENGINE LABEL   (ENGINE: quench-node or node; both take --expose-gc)
set -euo pipefail
binary="$1"
label="$2"
cd "$(dirname "$0")"
"$binary" --expose-gc probe.cjs > "probe-$label.txt"
"$binary" fill_semantics.cjs > "fill-$label.txt"
: > "rss-$label.txt"
: > "gc-cost-$label.txt"
for run in 1 2 3; do
  echo "# run $run" >> "rss-$label.txt"
  "$binary" --expose-gc rss.cjs >> "rss-$label.txt"
  "$binary" --expose-gc gc_cost.cjs >> "gc-cost-$label.txt"
done
echo "probe mismatches: $(grep -c MISMATCH "probe-$label.txt" || true)"
grep -h "heavyTask\|residual" "rss-$label.txt"
cat "gc-cost-$label.txt"
