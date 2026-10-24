# TOD-173 execution plan

## Acceptance gates

For each of the 10 npm workloads, in both no-JIT and JIT modes, Quench must be at least 1.20x the fastest peer's throughput and use at most 0.80x the largest peer RSS. A package/mode row passes only when both gates pass. Do not train PGO on the scored npm workloads.

## Work sequence

1. Sync the generic shared JIT work from `v2-jit` into `v2-zod`, keeping workload-specific behavior out of the runtime.
2. Run the runtime library suite and fix any compatibility regressions introduced by the sync.
3. Produce a clean production build with PGO trained only on the repository's general runtime curriculum.
4. Run 11 matched rounds for all 10 workloads in no-JIT and JIT modes, check output parity, and record commands, versions, hashes, throughput, and RSS.
5. Calculate each row against its fastest throughput peer and lowest RSS peer. Treat RSS as unqualified until a platform-qualified process-tree sampler is available.
6. Commit and push the source, regression tests, and evidence to `v2-zod`; keep TOD-173 In Progress until every scored row passes.
7. Use the largest measured shared interpreter/JIT bottleneck to choose the next generic optimization, then repeat steps 2–6.

## Current execution

- Steps 1–3 complete. Ten generic JIT commits are synced. The runtime suite passed 942 tests. The clean production build used only general runtime training.
- Step 4 complete: the corrected final recheck has all 22 workload/mode reports with output parity, using the same production binary. Node's no-JIT mode cannot run Vite and Undici because Node 24.19.0 removes the required WebAssembly global under `--jitless`; those two rows compare Quench and Bun.
- Step 5 complete diagnostically: 0 of 22 rows pass the throughput gate. Throughput margins range from -0.8% to -99.3%. RSS sampling is explicitly diagnostic in every report and cannot qualify the memory gate. The latest scorecard and reports are in `final-pgo-recheck/`.
- Step 6 has source and prior evidence synced to `v2-zod`; the refreshed scorecard is being committed with this update. TOD-173 remains In Progress.
- Step 7 profiling found high field-cache hit rates, but a broad `GetField` native-site trial regressed React by 1.3% and Pino by 9.3% while improving Zod by 9.3%; that runtime change was reverted. Per-cache-site attribution is committed. Two adaptive field-site windows were also rejected. A direct `GetIterator` callout fast path and a dedicated compiled-lane handler were screened and reverted: neither produced consistent throughput gains on React, Zod, and Pino. See `profile.md`, `fieldsite-trial/`, `adaptive-native-site-trial/`, `getiterator-trial/`, and `getiterator-lane-trial/`.
- A fourfold global reduction to the general-JIT compile price was also rejected: React fell 37.4%, Zod 23.3%, and Pino 2.0% against the matched control. Current next step: profile actual compile attempts, native entries, and interpreted work per function to make tier-admission decisions selective and accurately priced. Do not lower the global gate from opcode totals alone.
- The goal remains unmet. Every workload/mode row misses the throughput gate, and RSS remains unqualified.

The authoritative current scorecard is [`final-pgo-recheck/scorecard.md`](final-pgo-recheck/scorecard.md), with the per-row reports beside it. The root `scorecard.md` records the earlier baseline.
