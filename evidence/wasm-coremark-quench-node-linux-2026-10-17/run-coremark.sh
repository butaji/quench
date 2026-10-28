#!/bin/bash
# Interleaved CoreMark campaign through quench-node: ROUNDS rounds, every engine once per round,
# order rotated per round. Each run is its own process under the static fork/exec/wait4 launcher.
# Usage: run-coremark.sh SCRATCH ROUNDS OUT.csv
# SCRATCH holds cm/ (wasm-coremark-rs with target/release/bm, built with
# `-F pulley,wamr,wasmtime,winch`), bin/quench-node (the measured static production
# quench-node) and node26/ (official Node v26.12.0 linux-x64). ONLY=regex selects engines. Output CSV: round,engine,score,maxrss_kb,wall_ms,status
set -u
S=$1; ROUNDS=${2:-11}; OUT=$3
D=$(dirname "$0"); W=$S/cm/coremark-minimal-mvp.wasm; L=$S/maxrss; BM=$S/cm/target/release/bm
declare -A CMD=(
  [quench-node:nojit]="$S/bin/quench-node $D/run-js.mjs $W"
  [wasmi:nojit]="$BM wasmi" [stitch:nojit]="$BM stitch" [wasm3:nojit]="$BM wasm3"
  [tinywasm:nojit]="$BM tinywasm" [wamr:nojit]="$BM wamr" [pulley:nojit]="$BM pulley"
  [bun:nojit]="env -u BUN_OPTIONS BUN_JSC_useJIT=0 bun $D/run-js.mjs $W"
  [quench-node:jit]="$S/bin/quench-node --jit $D/run-js.mjs $W"
  [wasmtime:jit]="$BM wasmtime" [winch:jit]="$BM winch"
  [node26:jit]="$S/node26/bin/node $D/run-js.mjs $W" [bun:jit]="env -u BUN_OPTIONS bun $D/run-js.mjs $W"
)
names=($(printf "%s\n" "${!CMD[@]}" | sort | grep -E "${ONLY:-.}")); n=${#names[@]}
echo "round,engine,score,maxrss_kb,wall_ms,status" > "$OUT"
for ((r=0; r<ROUNDS; r++)); do
  for ((i=0; i<n; i++)); do
    e=${names[$(( (i + r * 3) % n ))]}
    res=$($L ${CMD[$e]} 2>&1)
    score=$(grep -oE 'Score: [0-9.]+' <<<"$res" | tail -1 | cut -d' ' -f2)
    m=$(grep -oE 'maxrss_kb=[0-9]+ wall_ms=[0-9]+ status=[0-9]+' <<<"$res" | tail -1 | sed 's/[a-z_]*=//g' | tr ' ' ',')
    echo "$r,$e,${score:-NA},$m" >> "$OUT"
  done
done
