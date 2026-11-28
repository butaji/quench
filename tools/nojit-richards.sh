#!/bin/bash
# nojit-richards.sh BIN [WORKDIR]: the no-jit lane's richards measurement for a build.
#  1. Fixed-work Ir per runRichards (20 vs 60 runs differenced, as richards-residue.sh does), with the
#     last measured figure (16.19M, 2026-11-27 20:48) beside it.
#  2. Where those Ir go: the top functions of the differenced run (callgrind_annotate), so a row the
#     facts should have chosen at load but did not shows up as general-step or miss time.
#  3. Score and peak RSS (quench-bench/v8-v7-rss.cjs, richards only, 3 rounds).
set -euo pipefail
BIN=$(realpath "$1"); W=$(realpath -m "${2:-$(mktemp -d)}"); mkdir -p "$W"
REPO=$(cd "$(dirname "$0")/.." && pwd); V=$REPO/quench-bench/js-engine-benchmark/v8-v7
[ -f "$V/richards.js" ] || git -C "$REPO" submodule update --init --depth 1 quench-bench/js-engine-benchmark
{ cat "$V/base.js" "$V/richards.js"; echo "const n = +process.argv[process.argv.length - 1]; for (let i = 0; i < n; i++) runRichards();"; } > "$W/r.js"
E="env XDG_CACHE_HOME=$W/xdg QUENCH_COMPILE_CACHE= QUENCH_STARTUP_SNAPSHOT=off"
CG="valgrind --tool=callgrind --dump-instr=no --smc-check=all"
ir() { grep -oE 'Collected : [0-9]+' "$1" | awk '{print $3}'; }
for n in 20 60; do
  $E $CG --callgrind-out-file="$W/nojit$n.cg" "$BIN" --no-jit "$W/r.js" $n > /dev/null 2> "$W/nojit$n.err"
done
per=$(( ($(ir "$W/nojit60.err") - $(ir "$W/nojit20.err")) / 40 ))
echo "no-jit Ir per runRichards: $per (last measured 16190000; C -O2 320000)"
callgrind_annotate --threshold=99 "$W/nojit60.cg" 2>/dev/null | sed -n '1,/^$/d;p' | head -40 > "$W/top60.txt" || true
callgrind_annotate --threshold=99 "$W/nojit20.cg" 2>/dev/null | sed -n '1,/^$/d;p' | head -40 > "$W/top20.txt" || true
echo "== top functions, 60 runs (see $W/top20.txt for the 20-run baseline)"; head -30 "$W/top60.txt"
# Score and peak RSS in both Quench modes beside Node and Bun, by the board's own runner (stock base.js +
# richards.js + reporting runner, rss-exec peaks); engines that are not installed are skipped by it.
"$BIN" "$REPO/quench-bench/v8-v7-rss.cjs" --quench "$BIN" --only richards --runs 3 --out "$W/board.json" | tee "$W/board.txt" || true
echo "files in $W"
