#!/usr/bin/env bash
# Reports how many symbols of a trained link order still name functions in a binary.
# The order names symbols without crate hashes (`Cs_`), as tools/ordered-ld/ld.lld resolves
# them at link time. Fails below the minimum share (percent, default 90).
#
#   tools/check-link-order.sh target/release/quench crates/quench/link/x86_64-linux.order
set -euo pipefail
binary=$1
order=$2
minimum=${3:-90}
present=$(comm -12 <(sort -u "$order") <(nm "$binary" | awk '{print $NF}' | sed -E 's/Cs[0-9A-Za-z]+_/Cs_/g; s/\.llvm\.[0-9]+$//' | sort -u) | wc -l)
total=$(sort -u "$order" | wc -l)
share=$((present * 100 / total))
echo "$order: $present of $total symbols present in $binary ($share%)"
if [ "$share" -lt "$minimum" ]; then
  echo "link order is stale (below $minimum%): retrain it" >&2
  exit 1
fi
