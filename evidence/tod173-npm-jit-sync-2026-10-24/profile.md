# Shared JavaScript JIT profile

These counters were collected on the same source and production build as the refreshed package matrix. They are diagnostic attribution, not a performance qualification.

| Workload | Compiles | Native entries | Field read probes / hits | Field store probes / hits | Quench JIT throughput |
|---|---:|---:|---:|---:|---:|
| Zod parse (100k rounds) | 21 / 25 attempts | 7,223,265 | 8,554,219 / 6,865,665 (80.3%) | 1,044,358 / 577,377 (55.3%) | 73,126 ops/s |
| React SSR | 4 / 9 attempts | 20,023 | 327,372 / 278,670 (85.1%) | 23,772 / 7,984 (33.6%) | about 1,335 ops/s |
| Pino JSON logger | 5 / 8 attempts | 349,713 | 758 / 718 (94.7%) | 0 / 0 | about 56,561 ops/s |

The full package matrix still shows Quench far behind peers on these three JIT rows. A prior field-cache candidate that routed probes through a native callback was rejected after matched screens: it did not produce a reliable React or Pino gain and improved Zod by only about 7%. The useful signal is the high hit rate, not that callback implementation.

## Next shared JIT slice

Compile ordinary cached data-field hits into the native dispatch body. At each field site, guard the current receiver against the existing field-cache entry and read the cached slot only on a proven hit. Proxies, accessors, shape changes, unsupported storage, and cache misses must leave at the exact `GetField` PC and use the existing VM semantics. Use one stable field-cache ABI and keep receiver identity dynamic across iterations. Avoid a branch in every VM callback; only the native field site should pay its guard cost.

The first implementation must carry the heap and cache layout invariants explicitly and must preserve write barriers for stores (stores are a later slice). Validate changing receivers, inherited properties, accessors, proxies, and exceptions before measuring. Run the focused React, Zod, and Pino production screens first; keep the feature only if the gains are repeatable and no-JIT behavior is unchanged. Then rerun the full 22-row matrix.

Source counters: `work/tod173-jit-eval/zod-shared-jit-stats-100k.log`, `work/tod173-jit-eval/react-general-jit-stats.log`, and `work/tod173-jit-eval/pino-general-jit-stats.log`. Current exact package/mode measurements remain in `scorecard.md` and the row JSON reports.
