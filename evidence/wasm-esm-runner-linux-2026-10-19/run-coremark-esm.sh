#!/bin/bash
# CoreMark with `quench-node` as the Wasm runner: `run-esm.mjs` imports `run` from the Wasm
# module itself (Node's Wasm ESM integration), whose `env` import resolves to a package
# exporting `clock_ms`, the wasm-coremark-rs host contract. Rows as
# `../wasm-coremark-rss-order-lazy-fs-linux-2026-10-19/run-coremark-evicted.sh` (every binary
# evicted before its run, one engine per process under `maxrss.c`, order rotated per round),
# with the `run-js.mjs` rows of the same binaries for comparison.
# Usage: run-coremark-esm.sh SCRATCH ROUNDS OUT.csv; SCRATCH as there. ONLY=regex selects.
set -u
S=$1; ROUNDS=${2:-11}; OUT=$3
D=$(cd "$(dirname "$0")/../wasm-coremark-quench-node-linux-2026-10-17" && pwd)
W=$S/cm/coremark-minimal-mvp.wasm; L=$S/maxrss; BM=$S/cm/target/release/bm
E=$S/esm
mkdir -p "$E/node_modules/env"
cp "$W" "$E/coremark.wasm"
cat > "$E/run-esm.mjs" <<'JS'
import { run } from "./coremark.wasm";
console.log(` - Score: ${run()}`);
JS
cat > "$E/node_modules/env/package.json" <<'JSON'
{ "name": "env", "type": "module", "exports": "./index.js" }
JSON
cat > "$E/node_modules/env/index.js" <<'JS'
let start;
export function clock_ms() { const now = performance.now(); start ??= now; return (now - start) | 0; }
JS
declare -A CMD=(
  [quench-node-esm:nojit]="$S/bin/quench-node $E/run-esm.mjs"
  [quench-node-esm:jit]="$S/bin/quench-node --jit $E/run-esm.mjs"
  [node26-esm:jit]="$S/node26/bin/node $E/run-esm.mjs"
  [quench-node:nojit]="$S/bin/quench-node $D/run-js.mjs $W"
  [quench-node:jit]="$S/bin/quench-node --jit $D/run-js.mjs $W"
  [node26:jit]="$S/node26/bin/node $D/run-js.mjs $W"
  [wasmi:nojit]="$BM wasmi" [stitch:nojit]="$BM stitch" [wasm3:nojit]="$BM wasm3"
  [wamr:nojit]="$BM wamr" [wasmtime:jit]="$BM wasmtime" [winch:jit]="$BM winch"
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
