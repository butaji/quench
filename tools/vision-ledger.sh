#!/usr/bin/env bash
# Run the vision ledger over every owner's pushed branch tip and v2-merged.
#   tools/vision-ledger.sh <clone> <work-dir> <out-dir> [gate ir-budget.txt]
# <clone> is any clone of the repository (blobless is fine); worktrees go in <work-dir>.
# Writes <out-dir>/ledger.md and delta.md; keeps state in <out-dir>/state.json.
set -euo pipefail
here="$(cd "$(dirname "$0")" && pwd)"
clone="$1"; work="$2"; out="$3"; gatefile="${4:-}"
mkdir -p "$work" "$out"
# owner|branch (owner names match OWNERS in vision-ledger.mjs)
map='Derive|claude/project-thread-klqkmr
Replace|claude/project-thread-r2lrq5
b-jit|claude/project-thread-grxu0e
u-jit|claude/project-thread-4qar6s
Wasm|claude/project-thread-9m05xy
Builtins|claude/project-thread-hk47c4
Front end|claude/project-thread-j9mtn6
Calls and frames|claude/project-thread-bvz0p4
Fact store|claude/project-thread-wyc2t0
Env, GC and heap memory|claude/project-thread-404a6x
No-jit|claude/project-thread-p34pk3
Compile pipeline|claude/project-thread-cktdr4
Node host|claude/project-thread-cqybmh
unowned|v2-merged'
scans=()
while IFS='|' read -r owner branch; do
  git -C "$clone" fetch -q origin "$branch" 2>/dev/null || { echo "skip $branch (fetch failed)"; continue; }
  wt="$work/$(echo "$branch" | tr '/' '_')"
  if [ -d "$wt" ]; then git -C "$wt" checkout -q --detach "origin/$branch"; else git -C "$clone" worktree add -q --detach "$wt" "origin/$branch"; fi
  j="$work/scan-$(echo "$owner" | tr ' ,' '__').json"
  node "$here/vision-ledger.mjs" scan "$wt" "$branch@$(git -C "$wt" rev-parse --short=10 HEAD)" "$j" --owner "$owner"
  scans+=("$j")
done <<< "$map"
if [ -n "$gatefile" ] && [ -f "$gatefile" ]; then
  node "$here/vision-ledger.mjs" gate "$gatefile" "$work/scan-gate.json"; scans+=("$work/scan-gate.json")
fi
inv="$work/$(echo v2-merged | tr '/' '_')/crates/quench-tier-corpus/tier_inversions.txt"
if [ -f "$inv" ]; then node "$here/vision-ledger.mjs" inversions "$inv" "$work/scan-inversions.json"; scans+=("$work/scan-inversions.json"); fi
node "$here/vision-ledger.mjs" combine "$out" "$out/state.json" "${scans[@]}"
