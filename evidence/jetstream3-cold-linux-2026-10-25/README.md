# JetStream 3, cold cohort (no cached input), Linux x86_64

**Cohort: cold.** Each run is a fresh process with nothing cached by an earlier run:
quench `QUENCH_COMPILE_CACHE=` (disabled; no cache file opened) and `QUENCH_STARTUP_SNAPSHOT`
unset; Bun `BUN_RUNTIME_TRANSPILER_CACHE_PATH=0`; Node `NODE_COMPILE_CACHE` unset,
`NODE_DISABLE_COMPILE_CACHE=1` (`tools/rsscmp.py`). Do not mix these rows with the warm
rows of earlier evidence directories (TOD-189).

Host: 4 vCPU cloud container, 6.18.44-fc-v153, Intel(R) Xeon(R) Processor @ 2.10GHz.
quench-node at 7d1d6adfa (release); Bun 1.4.2 with `BUN_JSC_useJIT=0` for no-JIT.
Score: JetStream's per-test Score; RSS: child `ru_maxrss` (wait4). Runner: `cli.js --no-prefetch --test=X`.

- `nojit-vs-bun-7d1d6adfa.txt`: one paired run per test, quench then Bun (`tools/psweep.sh`);
  ratios in `-ratios.txt`: geomean Score 0.876x, RSS 1.53x; 9 of 34 tests >=1.2x on both.
- `paired-medians-7d1d6adfa.txt`: 5-run interleaved medians (`tools/medcmp.py`) for hash-map,
  sha256 and pbkdf2 against Bun and against 336c186e9.
- `jit-vs-bun-jit-7d1d6adfa.txt`: quench `--jit` against Bun with its JIT (`tools/jsweep.sh`); ratios in
  `-ratios.txt`: geomean Score 0.149x, RSS 2.88x; 0 of 34 tests >=1.2x on Score, 34 of 34 on RSS.
- `nojit-vs-node26-jitless-7d1d6adfa.txt`: quench against Node v26.12.0 `--jitless` (official
  linux-x64 build, SHA256 verified). Node `--jitless` has no `WebAssembly`, so the 8 tests that
  need it (the 7 Wasm tests and source-map-wtb) fail there; Bun is their only no-JIT rival.
  Geomean Score 0.620x, RSS 3.47x over the 26 that run; 3 >=1.2x on both.
- `nojit-strongest-rival-7d1d6adfa.txt`: per test, the lower ratio of the two rivals for Score and
  for RSS: **8 of 34 tests >=1.2x on both** (the 6 Wasm tests other than argon2, hash-map, pbkdf2).
