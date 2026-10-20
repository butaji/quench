#!/bin/bash
# The 2026-10-17 campaign (`../wasm-coremark-quench-node-linux-2026-10-17/run-coremark.sh`) with
# every engine's binary evicted from the page cache before each of its runs: a binary written
# by a large sequential write (llvm-bolt, a copy) sits in 2 MB folios that a fault maps whole.
# Usage: run-coremark-evicted.sh SCRATCH ROUNDS OUT.csv; SCRATCH as there. ONLY=regex selects.
set -u
S=$1; ROUNDS=${2:-11}; OUT=$3
D=$(cd "$(dirname "$0")/../wasm-coremark-quench-node-linux-2026-10-17" && pwd)
W=$S/cm/coremark-minimal-mvp.wasm; L=$S/maxrss; BM=$S/cm/target/release/bm
declare -A CMD=(
  [quench-node:nojit]="$S/bin/quench-node $D/run-js.mjs $W"
  [wasmi:nojit]="$BM wasmi" [stitch:nojit]="$BM stitch" [wasm3:nojit]="$BM wasm3"
  [wamr:nojit]="$BM wamr" [quench-node:jit]="$S/bin/quench-node --jit $D/run-js.mjs $W"
  [wasmtime:jit]="$BM wasmtime" [winch:jit]="$BM winch" [node26:jit]="$S/node26/bin/node $D/run-js.mjs $W"
)
evict() { python3 -c 'import os,sys
fd=os.open(sys.argv[1],os.O_RDONLY); os.posix_fadvise(fd,0,0,os.POSIX_FADV_DONTNEED); os.close(fd)' "$1"; }
# Pages still dirty from the write cannot be evicted until written back.
sync
names=($(printf "%s\n" "${!CMD[@]}" | sort | grep -E "${ONLY:-.}")); n=${#names[@]}
echo "round,engine,score,maxrss_kb,wall_ms,status" > "$OUT"
for ((r=0; r<ROUNDS; r++)); do
  for ((i=0; i<n; i++)); do
    e=${names[$(( (i + r * 3) % n ))]}
    evict "$(readlink -f ${CMD[$e]%% *})"
    res=$($L ${CMD[$e]} 2>&1)
    score=$(grep -oE 'Score: [0-9.]+' <<<"$res" | tail -1 | cut -d' ' -f2)
    m=$(grep -oE 'maxrss_kb=[0-9]+ wall_ms=[0-9]+ status=[0-9]+' <<<"$res" | tail -1 | sed 's/[a-z_]*=//g' | tr ' ' ',')
    echo "$r,$e,${score:-NA},$m" >> "$OUT"
  done
done
