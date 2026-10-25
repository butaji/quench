# TOD-173 call-prefix attribution and package screen

Candidate source commit: `0194517b2` on `v2-zod` (integration chain: `078701ab8` on
`v2-cloud-wasm`, `95dcb6a80` on `v2-merged`). The prefix was reverted on
`origin/v2-merged` by `d2327ad5e`. Current local `v2-zod` is `3d385509f`, a
merge of that reverted checkpoint; remote `origin/v2-zod` is still at
`0194517b2`. Production candidate binary SHA-256:
`295e6bc1998ed260917db2b4be5d95d2046d0a185dea830dbf5fb056623120c3`.
The matched control SHA-256 is
`3010c9f68840ad8c451a8c487754d099de3ba2ff9d82823258027772f9c997ef`.
The source currently checked out has no prefix-resume implementation. This
document records the rejected candidate and diagnostics only. No direct push to
`v2-merged` was made by this package lane.

## Current source and ownership

The current task record in Linear supersedes the older experimental screen in
this directory: clean-PGO Linux, 11 matched rounds, 0/22 workload×mode rows
passed the numerical gates (Quench binary SHA-256
`0fd1ddc4dee59d52040f8abeaf4c14bb337c663bdffd72be265da28349348616`). RSS on
Linux is diagnostic, and this is not an Apple M4 qualification.

The rejected shared JIT slice changed only
`crates/quench-runtime/src/vm/compiled/osr_loop.rs`: it admits the supported
loop prefix before the first dynamic call, then resumes at the call through the
existing exact-PC writeback. The separate lowered end preserves the loop's real
backedge. This implementation is offered to the shared JIT task for review;
the package lane makes no further shared compiler edits or integration pushes
while the sync owner gates the tree.

The package lane's focused regression fixture is preserved as a patch at
[`gc-throw-prefix-test.patch`](gc-throw-prefix-test.patch); it is not applied to
the current source because its native-OSR assertion requires the rejected
feature. It runs a hot loop across a dynamic
callback, forces GC in the callback, throws a unique object at iteration 2000,
and checks accumulator, call count, catch identity, interpreter parity, and
native OSR execution. The focused command
`cargo test -p quench-runtime shared_osr_prefix_resumes_at_gc_and_throwing_calls -- --nocapture`
passed (1/1) against the prefix candidate. The current reverted checkpoint has
not been rerun for this candidate-only test; the shared JIT owner reports 972
runtime tests passing, including the three inlining regression guards.

The sync owner has since reported that the shared-JIT regression inventory
found three failures with `078701ab8`:
`shared_osr_inlines_method_calls_in_a_loop`,
`shared_osr_inlined_callee_leaves_through_its_activation`, and
`shared_osr_inlines_calls_on_a_receiver_a_field_read_gives`. The owner reports
all 15 shared OSR tests passing with the revert. Those three existing tests stay
as explicit guards; the package fixture passing does not justify keeping the
candidate. Do not re-land the prefix slice without the shared-JIT owner's review.

## Package-level attribution

The one-round JIT statistics screens show the new call-prefix boundary does not
reach the dominant loop bodies in these workloads:

| Package region | Current activity | First blocker or gap |
| --- | --- | --- |
| Pino `lib/tools.js::_asJson` | 13 lane entries; 5 backward sites admitted; 0 native region runs; 85.7 ms inclusive interpreter time | Its region begins with iterator operations (`GetIterator`, `IteratorClose`, `RequireIteratorResult`) before the first call, so the call-prefix boundary cannot reach the body. Inclusive time is not exclusive CPU. |
| Zod `core/memoizer.cjs::isRecursive` | 8 lane entries; 13 backward sites; 0 admitted and 0 native runs | Large 349-instruction region remains unsupported. |
| Zod `core/util.cjs::mergeDefs` | 6 lane entries; 2 backward sites admitted; 0 native runs | Site admission alone does not show hot execution or native coverage. |

The measurements justify profiling these regions further, not a package-specific
recognizer. Pino needs iterator semantics available before its body is useful;
Zod needs a complete hot-region prerequisite inventory. These are handoff
findings for the shared JIT owner, not new implementation assignments.

## Matched Linux diagnostic screen

Five rounds, 50,000 operations per round, 5,000 warmups; Node 24.19.0 and Bun
1.4.2. The runner checked workload output parity. RSS is the Linux process-tree
`VmRSS` sum sampled every 50 ms, so these files are diagnostic and
`qualificationReady` is false.

| Workload / mode | Quench ops/s / MiB | Node ops/s / MiB | Bun ops/s / MiB |
| --- | ---: | ---: | ---: |
| Zod no-JIT | 105,932 / 30.12 | 246,305 / 188.01 | 185,185 / 47.37 |
| Zod JIT | 106,326 / 24.02 | 1,234,568 / 133.55 | 1,960,784 / 75.39 |
| Pino no-JIT | 61,501 / 32.53 | 135,501 / 29.52 | 106,157 / 54.82 |
| Pino JIT | 59,666 / 18.86 | 480,769 / 42.45 | 666,667 / 66.83 |

Quench loses throughput in all four rows. In the JIT rows it is about 11.6x
and 18.4x slower on Zod, and 8.1x and 11.2x slower on Pino, against Node and
Bun respectively. Some sampled RSS values are lower, but that does not offset
the speed failures or qualify the Linux samples. The original gate remains at
least 20% higher throughput and at least 20% lower maximum RSS than both peers
for all ten packages in both modes. Zero package/mode rows are qualified.

Raw five-round reports and one-round JIT attribution reports are in
[`tests/npm-perf/reports/call-prefix-2026-10-25/`](../../tests/npm-perf/reports/call-prefix-2026-10-25/).

The full representative no-JIT matrix used the default per-workload operation
counts, five rounds, and the same candidate/control binaries in each runner.
Node `--jitless` is explicitly unavailable for Vite's bundled WASM and Undici's
bundled llhttp WASM. Linux RSS is still sampled and several short peer rows miss
the sampling interval.

| Package / workload | Candidate ops/s / MiB | Control ops/s / MiB | Node ops/s / MiB | Bun ops/s / MiB |
| --- | ---: | ---: | ---: | ---: |
| React / react-ssr | 1,385 / 38.14 | 1,374 / 38.01 | 3,207 / 33.95 | 3,115 / 47.45 |
| Next / route-match | 178,571 / 23.05 | 175,439 / 22.99 | 526,316 / 0.75 | 370,370 / 3.18 |
| Vite / transform | 19,380 / 55.50 | 19,608 / 58.35 | unavailable | 24,752 / 76.74 |
| esbuild / transform | 421 / 59.41 | 419 / 59.14 | 1,643 / 93.84 | 1,645 / 78.50 |
| sharp / resize | 699 / 49.38 | 734 / 49.39 | 779 / 165.39 | 765 / 75.60 |
| Fastify / JSON HTTP | 202 / 48.49 | 204 / 48.38 | 4,318 / 50.59 | 3,660 / 60.67 |
| Undici / pool | 4,382 / 49.01 | 4,464 / 49.13 | unavailable | 4,468 / 60.99 |
| ws / echo | 8,403 / 32.96 | 9,091 / 32.90 | 17,857 / 32.75 | 52,632 / 6.08 |
| Pino / JSON logger | 59,524 / 34.59 | 56,180 / 34.53 | 131,579 / 27.51 | 100,000 / 49.45 |
| Zod / parse | 106,952 / 27.40 | 100,503 / 27.12 | 240,964 / 45.02 | 175,438 / 46.60 |

Candidate/control speed differences in no-JIT are not caused by this OSR code
path, which is disabled there; the visible spread is a direct reminder that these
five-round Linux screens are not qualification evidence. The larger JIT gates
remain open for eight packages pending the owner's shared-JIT regression gate.

## Matched candidate/control screen

The candidate is the production PGO binary above. The control was rebuilt from
the same source tree and PGO data with `osr_loop.rs` at `30a4efe62`; the source
change caused LLVM to discard the profile for `analyze_osr_region`, which is a
known confound and makes this a diagnostic comparison. Both binaries were run
in the same five-round, 50,000-operation harness with Node and Bun.

| Workload / mode | Candidate ops/s / MiB | Control ops/s / MiB | Candidate change |
| --- | ---: | ---: | ---: |
| Zod no-JIT | 108,519 / 31.29 | 107,585 / 31.33 | +0.87% speed |
| Zod JIT | 105,541 / 25.64 | 107,066 / 25.15 | -1.42% speed |
| Pino no-JIT | 61,958 / 34.59 | 60,901 / 34.59 | +1.74% speed |
| Pino JIT | 59,172 / 20.61 | 59,737 / 21.23 | -0.95% speed |

These candidate/control changes are small and do not show a reliable package
gain. The reported inlining test failures outweigh the noisy performance deltas.

## Best next falsifiable experiment and blockers

The GC/throw test falsifies a correctness risk for a supported prefix exiting
before a callback. It does not prove package benefit: Pino's hot loop is blocked
before the call, while Zod's measured loops either are not admitted or do not
run natively. The next package-lane experiment is source/PC attribution of
Pino `_asJson` and Zod's hot parser regions under a matched control, with
exclusive CPU ownership, OSR entry/run/exit counts, first unsupported
instruction, and interpreter/native time kept separate. A shared-JIT change
should proceed only after that attribution identifies a complete generic
feature bundle with material predicted coverage.

The full goal remains open. A newer Linear record reports a separate clean-PGO
Linux 11-round matrix at 0/22 package×mode numerical passes; it remains
diagnostic because Linux process-tree RSS is sampled rather than qualified. The
candidate screens documented here are older, five-round experiments with a
known PGO profile mismatch and are also diagnostic. This checkout is Linux
x86_64, not the Apple M4 host required by the campaign; no M4 qualification
result is inferred. The original ≥1.20× fastest-peer throughput and ≤0.80×
lowest-peer max-RSS gates remain unmet.
