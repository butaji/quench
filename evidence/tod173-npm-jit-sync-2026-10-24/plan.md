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
- Step 4 complete: all 22 workload/mode reports are present with output parity. Node's no-JIT mode cannot run the Vite and Undici paths because Node 24.19.0 removes the required WebAssembly global under `--jitless`; those rows compare Quench and Bun.
- Step 5 complete diagnostically: 0 of 22 rows pass both throughput gates. The throughput margins range from -3.0% to -99.4%. RSS sampling is explicitly marked diagnostic in every report and cannot qualify the memory gate yet.
- Step 6 is complete: commit `be9bf963f` is pushed to `v2-zod`, and TOD-173 records the unmet gates.
- Step 7 profiling narrows the next shared-tier slice: Zod, React, and Pino field-cache read probes hit 80–95% of the time, but those hits still cross the VM callback. The callback-only field candidate was rejected for lack of a reliable end-to-end gain. Implement a native site-local guard and slot read with exact-PC fallback, then screen React, Zod, and Pino before repeating the full matrix. See `profile.md`.
- The goal remains unmet: all 22 rows fail the throughput gate, and the current RSS method is diagnostic only.

See `scorecard.md` and the per-row JSON files for exact measurements and reproduction inputs.
