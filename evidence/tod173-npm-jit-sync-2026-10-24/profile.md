# Shared JavaScript JIT profile

These counters were collected on the same source and production build as the refreshed package matrix. They are diagnostic attribution, not a performance qualification.

| Workload | Compiles | Native entries | Field read probes / hits | Field store probes / hits | Quench JIT throughput |
|---|---:|---:|---:|---:|---:|
| Zod parse (100k rounds) | 21 / 25 attempts | 7,223,265 | 8,554,219 / 6,865,665 (80.3%) | 1,044,358 / 577,377 (55.3%) | 73,126 ops/s |
| React SSR | 4 / 9 attempts | 20,023 | 327,372 / 278,670 (85.1%) | 23,772 / 7,984 (33.6%) | about 1,335 ops/s |
| Pino JSON logger | 5 / 8 attempts | 349,713 | 758 / 718 (94.7%) | 0 / 0 | about 56,561 ops/s |

The full package matrix still shows Quench far behind peers on these three JIT rows. A prior field-cache candidate that routed probes through a native callback was rejected after matched screens: it did not produce a reliable React or Pino gain and improved Zod by only about 7%. The useful signal is the high hit rate, not that callback implementation.

A broad native `GetField` site-marking trial then let the existing callout skip `Vm::step` on cached hits. It improved Zod by 9.3% over control but regressed React by 1.3% and Pino by 9.3%, so the runtime change was reverted. Raw reports and details are in `fieldsite-trial/`.

## Next shared JIT slice

The next iteration must avoid taxing cold field sites. First collect per-site call counts and cache-hit ratios from the same broad workloads; only then consider an adaptive native guard. A permanent object/cache ABI remains a separate, larger design task. Any field operation must validate changing receivers, inherited properties, accessors, proxies, and exceptions before measuring. Screen React, Zod, and Pino first and rerun the full 22-row matrix only if the shared feature wins across all three.

Source counters: `work/tod173-jit-eval/zod-shared-jit-stats-100k.log`, `work/tod173-jit-eval/react-general-jit-stats.log`, and `work/tod173-jit-eval/pino-general-jit-stats.log`. Current exact package/mode measurements remain in `scorecard.md` and the row JSON reports.
