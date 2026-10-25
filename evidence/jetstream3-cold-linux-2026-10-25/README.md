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
