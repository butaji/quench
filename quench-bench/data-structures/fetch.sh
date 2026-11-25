#!/usr/bin/env bash
# Downloads the "Gemini 4 Neon" data-structure suite (https://ben-chmarks.com/gemini-neon/)
# into suite/ (not committed: upstream publishes no license). For each structure:
# PROMPT.md, the canonical Node and C drivers with their bindings, and the gemini-4-neon_high
# solutions for node, bun and c. Quench runs the node solution, the same source Node runs.
set -euo pipefail
cd "$(dirname "$0")"
B=https://ben-chmarks.com/benchmarks
S=solutions/gemini-4-neon_high
declare -A FILE=([btree]=btree [dlist]=dlist [hashmap]=hashmap [heap]=heap [radix]=radix
  [rbtree]=rbtree [rtree]=rtree [skiplist]=skiplist [unionfind]=unionfind)
for d in "${!FILE[@]}"; do
  f=${FILE[$d]}
  mkdir -p suite/$d/node suite/$d/bun suite/$d/c
  curl -fsS $B/$d/PROMPT.md -o suite/$d/PROMPT.md
  for l in node c; do curl -fsS $B/$d/lang/$l/BINDING.md -o suite/$d/$l/BINDING.md; done
  curl -fsS $B/$d/lang/node/driver.mjs -o suite/$d/node/driver.mjs
  cp suite/$d/node/driver.mjs suite/$d/bun/driver.mjs
  curl -fsS $B/$d/lang/c/driver.c -o suite/$d/c/driver.c
  curl -fsS $B/$d/$S/node/$f.mjs -o suite/$d/node/$f.mjs
  curl -fsS $B/$d/$S/bun/$f.mjs -o suite/$d/bun/$f.mjs
  curl -fsS $B/$d/$S/c/$f.c -o suite/$d/c/$f.c
  curl -fsS $B/$d/$S/c/$f.h -o suite/$d/c/$f.h
done
echo "fetched into $(pwd)/suite"
