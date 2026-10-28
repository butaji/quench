#!/bin/bash
# Interleaved CoreMark campaign: ROUNDS rounds, every engine once per round, order rotated per round.
# Each run is its own process under the static fork/exec/wait4 launcher; output CSV: round,engine,score,maxrss_kb,wall_ms,status
set -u
S=$1; ROUNDS=${2:-11}; OUT=$3
W=$S/cm/coremark-minimal-mvp.wasm; L=$(dirname "$0")/maxrss; BM=$S/cm/target/release/bm
declare -A CMD=(
  [quench-static-retrained:nojit]="$S/bin/q-static-retrained -no-jit $W"
  [quench-static-retrained:jit]="$S/bin/q-static-retrained -jit $W"
  [quench-dyn-order:nojit]="$S/bin/q-dyn-order -no-jit $W"
  [quench-dyn-order:jit]="$S/bin/q-dyn-order -jit $W"
  [quench-static-order:nojit]="$S/bin/q-static-order -no-jit $W"
  [quench-static-order:jit]="$S/bin/q-static-order -jit $W"
  [quench-static-noorder:nojit]="$S/bin/q-static-noorder -no-jit $W"
  [quench-static-noorder:jit]="$S/bin/q-static-noorder -jit $W"
  [wasmi:nojit]="$BM wasmi" [wasmi-v1:nojit]="$BM wasmi-v1" [stitch:nojit]="$BM stitch"
  [wasm3:nojit]="$BM wasm3" [tinywasm:nojit]="$BM tinywasm" [wamr:nojit]="$BM wamr"
  [pulley:nojit]="$BM pulley"
  [bun:nojit]="env -u BUN_OPTIONS BUN_JSC_useJIT=0 bun $S/run-js.mjs $W"
  [wasmtime:jit]="$BM wasmtime" [winch:jit]="$BM winch"
  [node26:jit]="$S/node26/bin/node $S/run-js.mjs $W" [bun:jit]="env -u BUN_OPTIONS bun $S/run-js.mjs $W"
)
names=($(printf "%s\n" "${!CMD[@]}" | sort | grep -E "${ONLY:-.}")); n=${#names[@]}
echo "round,engine,score,maxrss_kb,wall_ms,status" > "$OUT"
for ((r=0; r<ROUNDS; r++)); do
  for ((i=0; i<n; i++)); do
    e=${names[$(( (i + r * 7) % n ))]}
    res=$($L ${CMD[$e]} 2>&1)
    score=$(grep -oE 'Score: [0-9.]+' <<<"$res" | tail -1 | cut -d' ' -f2)
    m=$(grep -oE 'maxrss_kb=[0-9]+ wall_ms=[0-9]+ status=[0-9]+' <<<"$res" | tail -1 | sed 's/[a-z_]*=//g' | tr ' ' ',')
    echo "$r,$e,${score:-NA},$m" >> "$OUT"
  done
done
