# Shared JavaScript JIT profile

These counters were collected on the same source and production build as the refreshed package matrix. They are diagnostic attribution, not a performance qualification.

| Workload | Compiles | Native entries | Field read probes / hits | Field store probes / hits | Quench JIT throughput |
|---|---:|---:|---:|---:|---:|
| Zod parse (100k rounds) | 21 / 25 attempts | 7,223,265 | 8,554,219 / 6,865,665 (80.3%) | 1,044,358 / 577,377 (55.3%) | 73,126 ops/s |
| React SSR | 4 / 9 attempts | 20,023 | 327,372 / 278,670 (85.1%) | 23,772 / 7,984 (33.6%) | about 1,335 ops/s |
| Pino JSON logger | 5 / 8 attempts | 349,713 | 758 / 718 (94.7%) | 0 / 0 | about 56,561 ops/s |

The full package matrix still shows Quench far behind peers on these three JIT rows. A prior field-cache candidate that routed probes through a native callback was rejected after matched screens: it did not produce a reliable React or Pino gain and improved Zod by only about 7%. The useful signal is the high hit rate, not that callback implementation.

A broad native `GetField` site-marking trial then let the existing callout skip `Vm::step` on cached hits. It improved Zod by 9.3% over control but regressed React by 1.3% and Pino by 9.3%, so the runtime change was reverted. Raw reports and details are in `fieldsite-trial/`.

## Per-site attribution and next shared JIT slice

Opt-in per-site counters are now implemented in `program_store.rs` and emitted under the existing `QUENCH_JS_JIT_STATS=1` flag. The 11-round results in `per-site-attribution/README.md` show hot sites beside zero-hit sites inside the same functions. Aggregate function hit rates are insufficient to mark every GetField PC native.

Two adaptive native-PC field-site windows (8 probes with 7 hits; 32 probes with 29 hits) were screened against matched PGO controls. Both were rejected: the short window regressed React and Zod; the longer window regressed React in all 11 paired rounds, with less than 1% Zod gain and flat Pino. See `adaptive-native-site-trial/`.

Two shared `GetIterator` fast-path candidates were screened against matched PGO controls. Bypassing the compiled callout's general opcode dispatch regressed React by 0.8%, Zod by 2.2%, and Pino by 4.0%. A dedicated compiled-lane handler, using the same guarded `Vm::get_iterator`, regressed React by 2.0% and Pino by 1.0%, with only a 0.9% Zod gain. Both candidates were reverted; reports are in `getiterator-trial/` and `getiterator-lane-trial/`. Neither path closes the large package gaps in the final scorecard.

A fourfold lower general-JIT compilation price was also rejected after matched screens: React throughput fell 37.4%, Zod fell 23.3%, and Pino fell 2.0%. Earlier tier admission therefore needs to be priced from measured native and interpreted work more selectively; simply lowering the global cost gate compiles too eagerly. The reports are in `tierprice-quarter-trial/`. The final scorecard still fails every package/mode throughput gate.

Opt-in tier-work attribution now records estimated entry work, loop-backedge work, and the priced compile threshold per function. The production-mode JIT profile shows React `pushStartGenericElement` with 504,539 general-dispatch opcodes but 35,945 priced entry-work units versus 148,480 required; Pino `getValueIfExists` has 26,249 opcodes but 1,708 entry-work units versus 110,080 required. See `tier-work-attribution/`. This identifies an estimation gap, not proof that compiling these functions will pay back its cost.

A per-frame sampled-work estimator was then added for frames already admitted to the targeted general tier. Its exact-source PGO screen showed only small changes against control: React -0.6%, Zod +1.8%, and Pino flat. Expanding it to every hot legacy candidate did not materially increase the dominant functions' observed work, regressed Undici, Vite, WebSocket, and route matching, and was reverted. The ten JIT rows and stale-profile warning for that rejected expansion are recorded in `tier-hotcandidate-trial/README.md`.

The next trial should move to native lowering of the most frequent general-dispatch operations in hot functions. Preserve interpreter callouts for coercion, accessors, proxy effects, and exceptions; screen each lowering against paired package workloads before expanding it. The goal remains unmet: the current shared JIT candidates are far below the peer throughput threshold.

A sequential-callout continuation trial removed the indirect PC dispatch when a VM step returned the statically next instruction. A fresh general-runtime-only PGO build and 11-round paired JIT screens measured React +1.95% (9/11 paired wins), Zod -2.99% (1/11), and Pino -1.01% (3/11). It was reverted; see `sequential-fallthrough-trial/`. The attribution profiles show React `pushStartGenericElement` executing 504,368 `GetIterator` general-dispatch operations, while Pino `getValueIfExists` executes 10,499; global `GetIterator` fast-path trials already regressed React and Pino. Next, add diagnostic per-bytecode-site counts for the hot operations, then lower only sites with repeated evidence and safe guards. Do not deploy another global fast path from aggregate opcode counts.

An adaptive per-function, per-PC `GetIterator` callout then waited for 256 hits, routed that site through `Vm::get_iterator`, and cached the hot-site bit in the general-tier bitmap. On an exact-source PGO build, 11 paired rounds measured React +1.91% (9/11), Zod -1.19% (5/11), and Pino +1.01% (6/11). The candidate remains roughly 89%–97% slower than the fastest peer on these workloads, so it was reverted. Exact reports are in `adaptive-iterator-site-trial/`. The next work must optimize the larger interpreted region around repeated package code, rather than the iterator callout alone.

Source counters: `work/tod173-jit-eval/zod-shared-jit-stats-100k.log`, `work/tod173-jit-eval/react-general-jit-stats.log`, and `work/tod173-jit-eval/pino-general-jit-stats.log`. Current exact package/mode measurements remain in `scorecard.md` and the row JSON reports.
