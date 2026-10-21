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

## Re-sweep at 0028c07f (same host and Bun reference)

Quench at 0028c07f (`scores-0028c07f.txt`), one run per fixture, against the Bun rows above.
Some runs overlapped a single-threaded build on this 4-vCPU host, which can only lower
Quench's scores. Geomean Score ratio 0.524 -> 0.656; Score wins 7 -> 11 (the six Wasm
fixtures, plus stanford-crypto-pbkdf2 1.04, hash-map 1.04, js-tokens 1.05, aes 1.08,
sha256 1.09, the smallest of them within run-to-run noise); max RSS wins 32/34 (losses:
doxbee-promise 50 vs 42 MB, babel-wtb 94 vs 93 MB), RSS geomean ratio 0.674. The lowest
Score ratios: argon2-wasm 0.11, source-map-wtb 0.16, web-ssr 0.22, doxbee-async 0.24.
