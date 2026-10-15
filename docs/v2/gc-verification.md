# Garbage collector verification and improvement

GC efficiency is a workload-dependent frontier of execution time, collection
pauses and peak resident memory. A lower collection count is not an acceptance
criterion: it can simply mean retaining more garbage. A smaller live heap does
not prove lower max RSS. Keep these quantities separate in every report.

## Current contracts

`heap.rs` owns allocation pressure, mark state, weak processing and sweep.
`vm/gc.rs` supplies VM roots and owner-attached edges, traces shape symbols and
accessors, and prunes VM metadata after collection. Objects and arrays may live
in direct record spaces rather than legacy slots. `heap.get` only sees legacy
slots; collector changes must visit both representations.

`Heap::collection_is_full` currently forces a full trace when direct records
exist. Legacy minor collection tests do not establish record-heap generational
correctness. `ParallelShapeTrace` reads immutable transition metadata and atomic
shape marks; it must not read the interior-mutable shape lookup caches.

Allocation pressure is currently a cell count weighted by flat string bytes.
Array element growth and other external payload growth are not a complete byte
budget. Measure this gap separately from the cost of marking the live heap.

## Experiment loop

1. Sync `v2-gc` with `origin/v2-merged`. Record the exact baseline, toolchain,
   profile, target, CPU quota, marker count, linker and link-order inputs.
   Preserve baseline and candidate executables separately and hash both.
   If code placement contributes to an RSS result, requalify each build with
   the same allowed, disjoint link-order training corpus; keep that experiment
   separate from a comparison holding existing order files fixed.
2. Write one hypothesis and its decision rule in the existing Linear experiment
   catalog before changing code. Identify the phase it can improve and the
   component it removes from the memory peak. Profile builds locate costs;
   matched production builds decide performance. Do not tune to fixture identity.
3. Establish correctness before timing. Exercise direct records and legacy cells,
   every owner-attached edge, live and overwritten accessors, symbol keys,
   prototype edges, sparse elements, continuations and host roots. Check that
   weak references clear, finalizers run once with live held values, and
   ephemerons reach a fixed point. Dead historical shape transitions must not
   retain values. Compare any optimized trace with an independent full trace.
4. Use the pinned CommonJS benchmark materialization under `quench-node`.
   Measure setup-only and fixed work separately. Run at least 11 interleaved
   baseline/candidate pairs with alternating order and warm executable pages.
   Keep outputs, exit status, elapsed time and `wait4` peak RSS for every launch.
   Report paired 95% intervals and invalid runs; do not select the best sample.
5. Attribute the result: roots, marked records/bytes, remembered owners,
   ephemeron iterations, swept records/bytes, phase times and allocated bytes.
   Distinguish process floor, live data, garbage headroom and fragmentation.
   Releasing memory after the high-water mark cannot improve max RSS.
6. Run the all-eight instruction-count guard, including A/A repeatability on
   this host. Count fixed work minus setup, check successful semantic output,
   and flag regressions above 1% beyond the measured A/A band. Linux shared-host
   timing alone cannot establish a cross-benchmark win. Keep no-JIT mode.
7. Retain a candidate only when the predeclared time/RSS criteria pass. An
   interval crossing zero is inconclusive. Store rejected patches and results
   in git evidence rather than leaving unqualified production changes enabled.
   Run workspace checks and the runtime suite, publish through `v2-merged`,
   and update the existing Linear issue/catalog with evidence and next action.
   Apple M4 production qualification remains a separate gate.

The existing paired runner supports:

```sh
cargo build --profile iteration -p quench-bench --bin quench-bench
target/iteration/quench-bench splay.js --fixed-work \
  --quench /absolute/baseline/quench-node \
  --quench-peer /absolute/candidate/quench-node --runs 11 \
  --out evidence/<experiment>/splay-pairs.json
```

Use `evidence/tod123-rss-linux-2026-10-14/callgrind-guard.sh` for the existing
Linux guard. Its raw counts require successful program execution; do not accept
a partial report from a missing fixture, timeout or failed collector run.

## Order of larger collector work

* Reduce measured tracing/sweep work without adding persistent per-object
  metadata or changing policy. Shape tracing is one independently testable step.
* Account for payload allocation and growth (Linear TOD-125). Include arrays,
  buffers, strings and program ownership; distinguish shared backing storage,
  churn and live bytes. Avoid a whole-heap census at every safepoint. Check both
  large transient payloads and long-lived payload growth. The trunk owns the
  production headroom factor; compare proposed factors as separate experiments.
* Implement direct-record young collection (TOD-124) only with a complete
  mutation-barrier audit and explicit generation lifecycle. Verify every minor
  young live set against a full trace with old objects pinned, including old
  weak maps, ephemeron chains, promoted objects and old-to-young metadata edges.
  Measure remembered-set maintenance as part of total execution cost.
* Evaluate lazy bitmap sweep and marker counts only after phase attribution
  shows a material opportunity. Sweep before heap growth, preserve finalization
  ordering, and include worker stack/floor memory in the comparison.

Throughput, RSS, and pause-tail results need separate evidence. Aggregate GC
time does not establish a p99 or maximum pause. Add per-collection samples to a
diagnostic build before making a latency claim, and confirm any retained policy
on short-lived churn, long-lived graphs, payload-heavy and weak-reference cases
as well as the eight benchmark workloads.

On Linux without hardware counters, the standard runner can report valid
execution with zero clean timing rounds. Keep that classification: bootstrap
intervals over raw wall time are diagnostics, not qualification. A repeatable
instruction reduction still needs the time/RSS and M4 gates.

Count first-use host initialization in the process peak, including output
formatting. A phase-isolated micro can explain a cost, but changing a fixture
after a memory regression does not erase the regression. Confirm small RSS
deltas with the existing minimal `posix_spawn`/`wait4` helper; a large measurement
parent can inflate the child's apparent floor. Sampled `/proc` resident pages
help attribution but may miss brief peaks and cannot replace `wait4` max RSS.
