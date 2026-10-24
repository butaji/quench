# Per cache site JIT attribution

These 11-round reports use an optimized, non-PGO diagnostics binary built from the instrumented source. `QUENCH_JS_JIT_STATS=1` and `QUENCH_JS_JIT_GENERAL_DISPATCH=1` were enabled. The reports verify output parity and record the diagnostics binary hash. They are attribution evidence, not performance qualification; use `final-pgo-recheck/` for the scored PGO binary results.

The new `field_cache_sites` diagnostics list records the cache site, probe count, hit count, and integer hit rate per compiled function. The aggregate field cache counters remain available. Representative results:

| Workload | Probes | Hits | Hit rate | Site distribution |
|---|---:|---:|---:|---|
| React SSR | 3,601,114 | 3,065,381 | 85.1% | 93 sites. `flushCompletedQueues` sites 2849–2853 and 2857 onward each had 43,417 probes and 100% hits; adjacent site 2854 had 43,417 probes and 0 hits. |
| Zod parse | 690,305 | 597,839 | 86.6% | 31 sites. `core.cjs` `init` site 37 had 75,647 probes and 100.0% hits; adjacent `init` site 9 had 30,778 probes and 0 hits. In `core.cjs` function `_`, sites 47, 57, and 58 had 30,767 probes and 100% hits; sites 49 and 59 had 30,767 probes and 0 hits. |
| Pino JSON logger | 539 | 517 | 95.9% | 19 sites. `cloneSelectively` sites 174–180 each had 55 probes and 100% hits; site 155 had 22 probes and 0 hits. |

The broad native field-site experiment remains rejected: aggregate hit rates alone do not identify which field PCs justify the added native path. The next trial should gate field lowering at individual site PCs using runtime-local evidence, preserve exact `Vm::step` fallback on every guard miss or effectful case, and compare the candidate against its control on React, Zod, and Pino before expanding the matrix.
