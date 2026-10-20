# JetStream 3 no-JIT, canonical runner, 2026-10-20 (second run)

One round per test, `v2-cloud-jet` at 7c0e356a, production profile, against
Bun `BUN_JSC_useJIT=0` and Node v26.10.0 `--jitless` on the same container (Linux x86_64,
4 vCPU). The host ran slower than the first run of the day (Bun's geomean 18.46 -> 16.17), so
compare ratios: Quench's geomean over the 33 tests both completed is 0.491 of Bun's
(0.476 in the first run). Quench leads on 6 tests (the Wasm ones) and on max RSS in 28.
