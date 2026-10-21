# JetStream 3 lane fixtures, Quench no-JIT vs Bun default (BUN_JSC_useJIT=0)

Host: Linux x86_64, 4 vCPU, 16 GB (cloud container), 2026-10-21.
Quench: v2-cloud-jet at 8825f66a (release build, lane GC pacing).
Bun: /root/.bun/bin/bun 1.4.2, BUN_JSC_useJIT=0, **BUN_OPTIONS removed**.

The container's shell exports `BUN_OPTIONS=--smol`; earlier comparisons in this lane ran Bun in
that memory-saving mode, which lowered its scores (sha256, aes, js-tokens and lazy-collections
read as Quench wins there and are not). This run removes it.

One run per fixture and engine (`JetStreamDriver.js --no-prefetch --test=NAME` with the lane
prelude), max RSS from wait4. Score wins 7/34 (six Wasm fixtures; aes 1.01x is within noise),
max RSS wins 31/34 (losses: doxbee-promise, doxbee-async, babel-wtb), geomean Score ratio 0.524.
