# General JIT tier work attribution

The opt-in `QUENCH_JS_JIT_STATS=1` record now includes `general_entry_work`, `general_dispatch_backedge_work`, and `general_compile_work_required`. This separates entry-based pricing from measured loop work. It does not change runtime behavior or the scoring harness. The diagnostic binary is a non-PGO debug build, SHA-256 `397f4876a1674512133d9a1b0d58ae6d4858ee64b6575c06d2684e6233d9eb34`.

Commands used `NODE_ENV=production`, `QUENCH_JS_JIT_GENERAL_DISPATCH=1`, `QUENCH_JS_JIT_STATS=1`, `quench-node --jit`, and the package workload directly. React used 2,500 rounds and 100 warmups; Zod used 1,000 rounds and 50 warmups; Pino used 5,000 rounds and 250 warmups. `.out` records workload results; `.err` contains the per-function stats. These are attribution runs, not performance comparisons.

Representative functions:

| Package / function | Status | Dynamic general-dispatch opcodes | Entry work / compile work required | Backedge work | Compile attempts / success |
|---|---|---:|---:|---:|---:|
| React `pushStartGenericElement` | general-promotion-pending | 504,539 | 35,945 / 148,480 | 0 | 0 / 0 |
| React `flushCompletedQueues` | compiled | 10,417 | 2,124,200 / 1,077,760 | 0 | 1 / 1 |
| Zod `operations` | general-promotion-pending | 7,354 | 2,822 / 138,240 | 0 | 1 / 0 |
| Zod anonymous closure | compiled | 8,785 | 121,765 / 122,880 | 0 | 1 / 1 |
| Pino `getValueIfExists` | general-promotion-pending | 26,249 | 1,708 / 110,080 | 0 | 0 / 0 |
| Pino `_asJson` | compiled | 3,898 | 372,027 / 376,320 | 0 | 1 / 1 |

The pending React and Pino functions show that the current pricing proxy can be far below observed bytecode activity when calls or iterator work repeat inside a function. The general quarter-cost trial still regressed both packages badly, so this evidence does not justify a lower global price. The next candidate must use a bounded, low-overhead estimate of actual repeated work and must not trigger broad compilation of cold functions.
