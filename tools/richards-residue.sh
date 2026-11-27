#!/bin/bash
# richards-residue.sh BIN [WORKDIR]: fixed-work Ir per runRichards in no-jit, default, the AOT
# executable (`compile -o r$n`, the linked image) and the JIT cache (`r$n.qimg`), and the AOT
# image's residue split next to C -O2's (tools/residue_split.py).
# 20 vs 60 runRichards are differenced, so startup and compile are removed (richards is time-bound
# under its own harness, so its totals are not fixed work).
set -euo pipefail
BIN=$(realpath "$1"); W=$(realpath -m "${2:-$(mktemp -d)}"); mkdir -p "$W"
REPO=$(cd "$(dirname "$0")/.." && pwd); V=$REPO/quench-bench/js-engine-benchmark/v8-v7
[ -f "$V/richards.js" ] || git -C "$REPO" submodule update --init --depth 1 quench-bench/js-engine-benchmark
for n in 20 60; do { cat "$V/base.js" "$V/richards.js"; echo "for (let i = 0; i < $n; i++) runRichards();"; } > "$W/r$n.js"; done
E="env XDG_CACHE_HOME=$W/xdg QUENCH_COMPILE_CACHE= QUENCH_STARTUP_SNAPSHOT=off"
CG="valgrind --tool=callgrind --dump-instr=yes --smc-check=all"
for n in 20 60; do $E "$BIN" compile "$W/r$n.js" -o "$W/r$n" > /dev/null; $E "$BIN" compile "$W/r$n.js" -o "$W/r$n.qimg" > /dev/null; done
ir() { grep -oE 'Collected : [0-9]+' "$1" | awk '{print $3}'; }
for n in 20 60; do
  $E $CG --callgrind-out-file="$W/nojit$n.cg" "$BIN" --no-jit "$W/r$n.js" > /dev/null 2> "$W/nojit$n.err"
  $E $CG --callgrind-out-file="$W/default$n.cg" "$BIN" "$W/r$n.js" > /dev/null 2> "$W/default$n.err"
  $E $CG --callgrind-out-file="$W/aot$n.cg" "$W/r$n" > /dev/null 2> "$W/aot$n.err"
  $E $CG --callgrind-out-file="$W/jitcache$n.cg" "$BIN" --no-jit "$W/r$n.qimg" > /dev/null 2> "$W/jitcache$n.err"
done
for m in nojit default aot jitcache; do echo "$m Ir per runRichards: $(( ($(ir "$W/${m}60.err") - $(ir "$W/${m}20.err")) / 40 ))"; done
# Image code addresses: dump under valgrind so they match callgrind's.
rm -rf "$W/dump"; mkdir "$W/dump"
$E QUENCH_TRACE=jit-dump="$W/dump" valgrind --tool=none --smc-check=all "$W/r20" > /dev/null 2> "$W/dump.err"
echo "== AOT image"; python3 "$REPO/tools/residue_split.py" image "$W/aot20.cg" "$W/aot60.cg" 40 "$W/dump" "$W/dump.err"
# C analogue: 50 runRichards per iteration, 10 vs 30 iterations differenced.
gcc -O2 -o "$W/c-O2" "$REPO/quench-bench/c-analogues/richards.c"
for n in 10 30; do $CG --callgrind-out-file="$W/c$n.cg" "$W/c-O2" $n > /dev/null 2>&1; done
echo "== C -O2"; python3 "$REPO/tools/residue_split.py" c "$W/c10.cg" "$W/c30.cg" 1000 "$W/c-O2"
echo "files in $W"
