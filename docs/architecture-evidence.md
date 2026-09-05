# Architecture and performance evidence

This document records reproducible checks for the fact-generated VM plan,
including the copy-and-patch region-stencil tier described in
[`copy-and-patch-jit.md`](copy-and-patch-jit.md). It is an evidence index, not
a benchmark score and never selects a production path.
The source-based CPython comparison and transferable ARM lessons are recorded
in [`cpython-copy-patch-notes.md`](cpython-copy-patch-notes.md).
Run commands from the repository root; write generated reports under
`target/` so they remain disposable.

## Structural checks

```sh
node tools/architecture-size-report.cjs target/debug/quench-node \
  > target/architecture-size.json
cargo fmt --all -- --check
git diff --check
```

The size report is the task-010 complexity ledger: the current optimized
artifact records 28 generated catalog rows, 466 runtime Rust files, 5.58 MB of
runtime source, and a 14.09 MB `__text` segment. It is descriptive evidence;
it does not authorize a specialization.

The execution-seam inventory is intentionally mechanical: `ir.rs` owns the
catalog and compact encoding, `machine.rs` owns code arenas and site
attachment, `quickening.rs` owns bounded physical cache state, and
`vm_runtime.rs`/`vm_dispatch.rs` own ordinary semantic gateways. The generated
stencil catalog is validated from one build-time `RegionDeclaration` table;
its runtime path only selects, allocates, copies, patches, and executes. The
benchmark crate and `tools/` contain measurement adapters only; they are not
imported by the runtime's execution path.

## Correctness gates

```sh
cargo test -p quench-runtime --lib --no-default-features
cargo test -p quench-runtime --lib --features execution-trace
cargo check -p quench-node
```

Node-oracle and test262 runs remain compatibility evidence; a missing oracle or
timed-out fixture is recorded as unknown rather than converted to a score.

The latest pinned neutral-corpus run (three runs per case, optimized
`target/debug/quench-node`) was:

```sh
node quench-bench/micros/verify.mjs \
  --engine target/debug/quench-node --oracle node \
  --from 1 --to 100 --runs 3 --timeout-ms 30000 \
  --out target/micro-neutral-evidence-3run.json
```

It produced 100/100 exact observable matches. The refreshed task-027 run
used the optimized `bench-throughput` artifact copied to that command path
(the repository's dev profile is intentionally unoptimized), and measured
0.766x wall time, 0.295x peak RSS, 1.043x retired instructions, and 0.745x
cycles (overall index 260.04). This is a reproducibility snapshot, not a
production dispatch rule or a claim about Bun. The earlier 156.04 run remains
useful as historical evidence but is not a profile-matched comparison.

The same run against the locally installed Bun oracle is retained at
`target/micro-neutral-bun-evidence.json`: it also produced 100/100 exact
matches (2.358x wall time, 0.710x peak RSS, 4.466x instructions, and 3.143x
cycles relative to Bun). Node and Bun are measurement oracles only; neither
result enables workload-specific runtime behavior.

Task 028's OSR wiring change was remeasured with the same optimized artifact
and command: 100/100 exact matches, 0.759x wall time, 0.294x peak RSS, 1.040x
retired instructions, 0.740x cycles, overall index 262.71. The unit test
`hot_back_edge_osr_transfers_live_frame_into_baseline` records one actual
interpreter-to-baseline transfer and compares it with a threshold-disabled
cold run; `structured_fori_is_not_an_osr_back_edge` records the intentional
structured-loop exclusion.

Task 029's `enter_slow_path` gateway was validated with the full compact
handler suite (576 passed without execution tracing; 586 passed with it), the
Node host check, and three direct cold-operation transition cases (throw,
labeled break, and continue). An apples-to-apples optimized artifact size
comparison against the pre-task commit recorded 7,237,724 bytes of `__text`
before and after (delta 0), 28 generated catalog rows, and no generated-code
growth. The exact three-run neutral corpus remained 100/100; its fresh overall
index was 248.23 with 0.768x wall-time and 0.321x peak-RSS ratios versus Node
(the prior 262.71 index had a 0.759x wall-time ratio, within run-to-run host
noise). Raw output is `target/micro-neutral-after-029.json`; the before/after
size reports are disposable files under `target/`.

Task 030 Gate 0 profiled the hot dispatch path before changing its state shape.
With the DWARF-enabled profiling artifact, a five-second macOS `sample` run
captured 3,819 `dispatch_callee` samples. ARM64 disassembly counted 259 stack
memory operations in 1,030 `dispatch_callee` instructions (25.15%) and 933 in
5,683 `run_instruction` instructions (16.42%). After introducing the safe Rust
`DispatchState` one-pointer approximation, the same measurement counted 242/1,006
(24.06%) and 939/5,684 (16.52%), respectively. The gate therefore showed a
measurable dispatch-state cost, but not a justification for an unsafe fixed-register
ABI; the implementation keeps ordinary Rust ownership and LLVM allocation.
The optimized three-run neutral corpus remained 100/100 exact matches with an
overall index of 248.13 (0.792x wall time, 0.319x peak RSS, 1.050x instructions,
and 0.752x cycles versus Node). This is within the prior host-noise range and
the bounded-state change adds no per-callsite metadata.

Task 031 Gate 0 is blocked by that outcome: `DispatchState` is a safe
one-pointer grouping, not a fixed physical register holding the NaN-tagging
base. Stable Rust has no portable GHCcc-equivalent ABI, so no tag-register
constant-materialization change was attempted; `TAG_PREFIX`, `TAG_SHIFT`, and
`TAG_MASK` remain semantically identical immediates. This preserves the
paper's correctness requirement and avoids an unsound ABI substitute.

Task 032 Gate 0 used a guard-hit-heavy monomorphic shape/property probe in a
release runtime test: 20 million current side-table probes took 39,162,375 ns,
while the generic rewritten-opcode prototype took 7,859,500 ns (5.0x less
probe-path time). Path A was therefore justified. The implementation adds
three catalog-generated quickened opcode variants and a fixed four-word
per-instruction rewrite cell. A confirmed shape hit overlays the specialized
opcode; a shape/key/descriptor mismatch dequickens and re-enters the complete
generic path. Full runtime/node checks passed, and the optimized neutral corpus
produced 100/100 exact matches with overall index 248.26 (0.785x wall time,
0.320x peak RSS, 1.054x instructions, and 0.746x cycles versus Node). Raw
output is `target/micro-neutral-after-032.json`.

Task 033 adds `IcStubChain`, a fixed-capacity λe chain for rendered sites. Each
distinct key gets one insertion-ordered stub until `MAX_IC_STUBS`; chain miss
and the (N+1)th key return `None`, which is the caller's complete interpreter
fallback. `install` places a fitting stub in the reserved inline slab and
routes an oversized stub to the outlined area. Unit tests cover cold/warm,
polymorphic, exhausted, both placement classes, repeated adversarial keys, and
the fixed peak state; existing call/property differential tests cover the
semantic gateways. The mechanism is disposable metadata and does not alter
the W^X publication boundary.

Task 050 fixed detached console methods in both the Node host console class and
the bootstrap polyfill. The focused `quench-node` regression test exercises
detached logging, diagnostics, grouping, counters, and timers; `var print =
console.log; print("x")` now exits successfully. Host output capture preserves
line boundaries required by the v8-v7 harness.

Task 051's reported Splay mismatch was reproduced and isolated as a harness
comparison error, not a tree-semantics divergence. The old runner compared
timing-derived `Score:` lines (Node `86706` versus Quench `46.5`) as raw
output. The tracked v8-v7 runner compares only suite completion/error markers
and records scores independently; Splay's size, sortedness, and uniqueness
teardown passes on both engines. A small five-shape polymorphic dispatch
regression probe is in `quench-node/src/run.rs`.

Task 052 adds
`quench-bench/js-engine-benchmark/v8-v7/verify.mjs`, which materializes each
upstream fixture with `base.js`, runs it against an engine and an oracle, and
records per-fixture score, completion-marker equality, wall time, and peak RSS.
On the optimized `target/bench-throughput/quench-node` artifact, isolated
Richards completed in 4.11 s (Node 2.03 s) with marker equality, and isolated
DeltaBlue completed in 6.58 s (Node 2.03 s) with marker equality. Earlier
30-second timeouts came from the unoptimized debug artifact and remain
performance evidence, not semantic failures.

Task 061's minimal, general polymorphic-dispatch reproduction uses five
unrelated constructors sharing one method name and distinct instance layouts.
On the same optimized ARM64 artifact, 100k/250k/500k calls took 676/1,567/3,041
ms versus Node's 30/34/35 ms: cost scales linearly with work, with no
quadratic growth or non-terminating path. Execution tracing for the 100k run
recorded 100,000 `GetN`, 100,017 `SetN`, and 99,999 `CallN` operations, with
391,996 named-get layout mismatches and 286,097 descriptor-view allocations.
These counters identify the current bounded single-entry IC/layout-mismatch
path as the material cost; no correctness bug was found, so no property
semantics change was made. A follow-on optimization, if pursued, must target
general bounded polymorphic property/call dispatch and retain complete
fallback semantics.

Task 062 invariant #1 is now a permanent runtime test:
`property_access_does_not_scale_with_historical_shape_count`. It creates K=10
and K=1000 distinct object layouts, then measures equal numbers of property
reads per object. The measured per-access ratios were 0.986x (no tracing) and
0.995x (execution-trace), against a deliberately conservative <8x bound. As a
validation of the bound, a temporary O(K) scan hook in the actual property
gateway produced a 10.31x ratio and failed the test; that hook was reverted.
This confirms the task-061 finding that historical shape count is not itself
the scaling problem.

The remaining four task-062 invariants are now implemented in the same
module. Fresh ARM64 debug-build runs reported ratios of 6.74x (shape
transition with 4 versus 64 existing properties), 0.95x (10 versus 1,000
historical callees), 1.56x (enumeration after 0 versus 2,000 add/delete
mutations), and 0.019x (per-op dispatch cost for 200 versus 20,000
instructions) without tracing; the execution-trace build reported 6.76x,
0.96x, 1.64x, and 0.018x respectively. All five tests pass in both
configurations. The transition bound is deliberately 16x to leave room for
debug-build/setup noise while still rejecting a full linear walk over the
16x property-count range. Invariant #1's temporary O(K) property-gateway
regression produced a 10.31x ratio and failed its <8x bound before being
reverted. A second source-level regression in the transition path was also
introduced with a bounded O(N) scan; it produced a 20.80x ratio and failed
the transition bound before being reverted. These checks validate that the
suite's ratio bounds are sensitive to the claimed failure class rather than
being unconditional timing smoke tests. The historical-callee test was also
fault-injected with a proportional per-callee scan (20.72x, failing its <16x
bound), and the mutation-history enumeration test with a proportional history
scan (28.09x, failing its <16x bound); both hooks were reverted. The
program-length dispatch claim was fault-injected in the real dispatch loop:
a temporary O(N) scan produced a 4.49x ratio, and tightening that test's
temporary bound to <4x caught it. Both the bound and hook were then restored.
This validates all five task-062 ratio gates against representative
complexity regressions.

The enumeration invariant now also includes a live-key scaling companion,
`enumeration_scales_linearly_with_live_key_count` (20 versus 200 live keys,
with a 32x bound for ARM64 debug noise). After amortizing setup and collector
checkpoints with a 100-iteration timed phase, fresh ARM64 runs measured
8.24–10.36x per-enumeration cost and passed in both configurations. As a
deliberate regression check, temporarily removing the canonical-layout
membership fast path caused the same test to run beyond 30 seconds (the old
per-key key-vector rebuild), catching the historical quadratic failure class;
the fast path was restored immediately. The existing history-only probe remains
the mutation-history regression check. The complete architecture-invariant
module now runs 20/20; full runtime gates pass 617 tests without tracing and
627 with `execution-trace`, and `cargo check -p quench-node` is green.

Task 063 extends the module with three collection invariants. Fresh ARM64
debug runs measured 1.53x (packed-array index-0 access at lengths 10 versus
100,000), 0.92x (Map/Set access across 10 versus 1,000 unrelated instances),
and 2.38x (Map/Set/Array iteration after 0 versus 2,000 add/delete cycles)
without tracing; all are below the 16x scaling bound. The collection tests
also pass under `execution-trace`. The packed-array invariant was validated
against a temporary source-level O(N) scan hook (capped at 2,000 probes per
access): it measured a 20.94x ratio and failed its <16x bound before the hook
was reverted. This representative collection regression gate confirms the
ratio is sensitive to a real complexity regression; no test-only hook remains
in production code.
The two cross-instance/history companions were also fault-injected: a
per-instance scan failed at 23.86x and a per-history iteration scan failed at
23.66x; both temporary hooks were reverted.

Task 065 adds closure, recursion-frame, and argument-marshaling invariants.
ARM64 debug ratios were 0.81x/0.75x for closure creation across 10 versus
1,000 closures, 1.12x/1.17x for per-frame recursion cost at depths 10 versus
100, and 0.74x/0.72x for marshaling across 10 versus 1,000 unrelated
functions (untraced/traced). All 14 architecture tests pass in each
configuration. Deliberate faults failed for all three claims before
restoration: a history-scaled closure creation scan measured 20.82x, a
depth-scaled frame loop measured 9.00x against a temporary <8x bound, and a
function-catalog-scaled marshaling scan measured 30.36x.

Task 066 adds raw allocation and release checks. With 200,000 timed
allocations/drops (enough to amortize heap setup), ARM64 ratios were 1.51x
and 1.39x for live heaps of 10 versus 10,000 without tracing, and 1.56x and
1.52x with tracing. The two invariants pass alongside the preceding 16
architecture tests in both configurations. Deliberate live-heap scans failed
the allocation and drop tests at 4,054x and 3,886x respectively before
restoration. No arena-specific invariant is claimed because task 059 is not
implemented.

Task 067 adds bounded stencil-tier checks. Region lookup (10,000 versus
100,000 repeated probes) measured a 1.01x per-probe ratio; a full 16-entry
render memo history produced a 1.20x cache-hit ratio; and arena bump
allocation (1,000 versus 10,000 placements) measured 0.25x after timer/setup
amortization. All three pass the documented <16x bounds. These tests exercise
selection/cache/allocation mechanics without executing native bytes; the
ARM64 executable path remains explicitly opt-in via
`QUENCH_ENABLE_AARCH64_STENCILS`, so no claim of native stencil execution is
made by this invariant suite.
The selector now uses a build-generated direct key match rather than a
runtime linear scan over `REGION_TABLE`; cache capacity remains the fixed
16-entry bound. The architecture test additionally executes the rendered
ARM64 Add stencil under `QUENCH_ENABLE_AARCH64_STENCILS=1` and reports the
native result before timing arena placement. A temporary O(previous-cursor)
scan in `StencilArena::alloc` produced a 4.74x allocation-history ratio and
failed a temporary <4x bound before both hook and bound were restored.

Task 058 audit: quench already hash-conses ordinary-object layout facts in
`value.rs` (`OBJECT_LAYOUTS` plus hash buckets), and the existing
`equal_property_sequences_share_one_layout_fact` test proves two independent
objects reuse one `semantic_layout_id`. The interner is thread-local and its
structural key is the visible property-name sequence; descriptor attributes
and prototype identity are not part of that derived layout fact. This is
therefore Partial rather than a new implementation claim: extending the key
would require a representation/IC audit, and no semantics-changing change is
justified without that evidence.
An additional regression test now builds the same visible layout through two
independent transition paths (`independent_transition_histories_share_one_layout_fact`);
both paths reuse the same interned layout fact. This confirms the existing
runtime-wide (single-thread VM) deduplication behavior without conflating the
derived slot layout with full descriptor/prototype shape identity.

Fresh v8-v7 tracked-runner baseline (ARM64, optimized artifact,
`--runs 1 --timeout-ms 10000`) records both execution score and peak RSS:
Richards 52.1 / 18.2 MiB (output equal), DeltaBlue 46.9 / 82.7 MiB (output
equal), and Splay 326 / 430.7 MiB (output equal). Crypto, RayTrace,
Earley-Boyer, RegExp, and Navier-Stokes exceeded the 10-second engine timeout
and therefore have no score/RSS value; their Node oracle markers completed.
The aggregate over completed engine scores is 92.70 versus Node's 83,617.48
(0.001109x), but it is not a whole-suite score because five fixtures are
incomplete. This snapshot is measurement-only and does not claim an
optimization gain.

Long-timeout v8-v7 rebaseline after task 057 (`--runs 1
--timeout-ms 120000`, ARM64 debug artifact) distinguishes finite slow paths
from timeout-bound fixtures. Richards completed in 22.96 s (Score 5.24,
25.1 MiB RSS, output equal) and DeltaBlue in 47.30 s (Score 4.62, 72.7 MiB,
output equal). Crypto, RayTrace, Earley-Boyer, and Navier-Stokes each reached
the 120 s engine timeout without a completion marker; their Node oracle runs
completed. RegExp finished in 13.83 s but reported `RESULT:RegExp:error`
(semantic/output mismatch, not a timeout). Splay finished in 12.71 s without a
completion marker and peaked at 439.8 MiB; its output remains unverified by the
tracked runner. The aggregate over the two verified fixtures is 4.92 versus
Node 72,590; this is a measurement snapshot and not a whole-suite score.

An optimized-build spot check after task 057 (`target/release/quench-node`,
120-second timeout) completed Richards at Score 56.3 / 15.6 MiB RSS and
DeltaBlue at Score 49.2 / 80.5 MiB RSS, both output-equal. Their combined
geomean is 52.63 versus Node 119,607; this is not comparable to the debug
artifact's 4.92 geomean and is recorded only as the post-collector release
sanity check.

A release-artifact v8-v7 run at the first task-070 escalation tier
(`--timeout-ms 60000`, `target/v8v7-baseline-60s.json`) completed four of
eight fixtures with verified output: Richards 56.3 (3.04 s, 15.3 MiB),
DeltaBlue 49.3 (5.39 s, 80.3 MiB), RayTrace 167 (15.94 s, 18.4 MiB), and
NavierStokes 212 (23.89 s, 18.1 MiB). Crypto and EarleyBoyer remained
timeout-bound at 60 s; RegExp completed in 2.37 s but emitted
`RESULT:RegExp:error`; Splay completed in 1.83 s but emitted no result marker
and peaked at 431.5 MiB, so neither is output-verified. The four verified
engine scores have geomean 99.56 versus Node 73,943.78. This is a new
measurement row, not a replacement for the earlier 10 s and 120 s snapshots.

The next escalation isolated Crypto at `--timeout-ms 300000`:
`target/v8v7-baseline-300s-crypto.json` completed with verified output in
221.08 s, Score 16.2, and 33.5 MiB peak RSS (Node: Score 85,721, 61.8 MiB).
The sustained CPU profile and deep active VM stack distinguish this from a
parked hang; it is a very slow but finite workload. EarleyBoyer remains the
next outstanding 300-second completion probe.
An isolated EarleyBoyer run at the same 300-second tier completed with
verified output in 70.04 s, Score 71.8, and 149.7 MiB peak RSS
(`target/v8v7-baseline-300s-earley.json`; Node Score 144,493 and 126.6 MiB).
Its active CPU execution is therefore very slow but finite rather than a
non-terminating path. The remaining completion gaps are the RegExp error and
Splay missing marker, which are correctness/harness findings rather than
timeout-only unknowns.

A fresh optimized-release run at the 300-second tier after the cycle-collector
root fix (`target/v8v7-baseline-release-300s-rootfix.json`) completed seven of
eight fixtures with verified output. Engine measurements were Richards 56.7
(3.55 s, 15.6 MiB), DeltaBlue 48.3 (6.46 s, 88.3 MiB), Crypto 17.0
(218.49 s, 34.2 MiB), RayTrace 167 (15.95 s, 18.4 MiB), EarleyBoyer 73.7
(68.00 s, 152.2 MiB), Splay 375 (3.68 s, 433.6 MiB), and NavierStokes 210
(24.04 s, 18.2 MiB); all seven have `output_equal:true`. RegExp completed
in 2.36 s with 41.0 MiB RSS but emitted `RESULT:RegExp:error`, so its score is
null and the full-suite aggregate is intentionally not claimed. The seven
verified engine scores have geomean 89.25 versus Node's 86,051.17; this is a
ground-truth completion row, not an optimization claim.

The Splay missing-marker failure was then reduced to a general cycle-collector
rooting bug: during allocation-heavy nested calls, the Rust call driver held
the caller environment outside the JavaScript graph, so trial deletion could
clear a live closure continuation. A synthetic caller/continuation workload
with 2,000 unreachable object cycles now passes with the caller environment
registered as a temporary collector root; removing that one guard reproduced
`TypeError: value is not callable`, validating the regression test rather than
merely timing it. On the rebuilt ARM64 debug artifact, Splay now emits
`RESULT:Splay:ok` and a finite tracked-runner score of 42.6 (21.46 s, 466.6 MiB
RSS), while the cycle-audit probes remain bounded: closure capture peaked at
21,725,184 bytes for N=50,000 (versus 419,495,936 bytes when collection was
deferred wholesale). This is a general root-safety correction, not
benchmark-specific behavior.

Task 056 cycle audit confirms a real leak in the current pure-`Rc` object
graph. The committed measurement probe `tools/cycle-audit.mjs` runs fresh
processes and parses macOS peak RSS. Plain two-object cycles grew from
19,595,264 bytes at N=1,000 to 29,163,520 at N=10,000 and 71,647,232 at
N=50,000. Closure-capture cycles grew from 25,559,040 to 87,146,496 and
359,940,096 bytes at the same sizes. A DeltaBlue-shaped constraint/variable
mutual-reference probe grew from 19,775,488 to 27,246,592 and 60,833,792
bytes. These are process-peak measurements, so they establish monotonic
retention after the cycles become unreachable; task 057 was therefore
justified and its post-fix measurements are recorded below.

Task 064 adds three string invariants. ARM64 debug runs measured append
ratios of 2.55x (500 versus 5,000 appends) without tracing and 3.20x with
tracing, search ratios of 1.72x/1.28x after 10 versus 1,000 unrelated strings,
and equal-length comparison ratios of 68.79x/68.65x for lengths 100 versus
10,000. All remain within their documented near-linear bounds and pass in
both feature configurations. Deliberate faults failed all three claims:
quadratic append (69.23x), unrelated-string search (17.02x), and a temporary
<110x comparison bound with an extra length-scaled scan (112.04x); all hooks
and temporary bounds were reverted.

The task-061 validation runs passed both runtime configurations (`592` tests
without tracing and `602` with `execution-trace`). The optimized ARM64
curriculum sweep completed 33/38 cases with exact observable matches; the five
remaining cases are performance-ceiling findings (OSR case 026 at 8.98x, cases
025/027 at 3.50x/3.01x, closure case 028 at 4.44x, and string case 032 at
4.42x), not correctness failures. The raw run is
`target/task061-curriculum.log`. This is consistent with the linear
polymorphic-dispatch reproduction and does not justify changing property
semantics under task 061.

Task 034 Gate 0 used a DWARF-enabled five-second macOS `sample` of a long
neutral arithmetic loop and searched for rendered-region symbols
(`StencilArena`, `render_selected`, `execute_f64`). It found zero such samples;
the executable stencil path is x86_64-gated and inactive on this ARM64 host.
Therefore icache/branch cost for rendered regions was not a measured cost, and
the task's required outcome is **not a measured cost, no implementation
attempted**. No layout or branch-target bytes changed.

Task 068's call-boundary profile was run on a synthetic arithmetic loop with
the ARM64 stencil opt-in enabled. A five-second DWARF sample captured
`StencilArena::render_or_get` (including `copy_and_patch`/`make_executable`)
and the canonical `run_compact_call_fallback`; the native leaf itself is only
a handful of instructions. The arena already makes `mprotect` idempotent,
so the remaining cost is the Rust-to-rendered-fragment boundary plus per-plan
arena setup, not a missed repeated protection call. Paired curriculum cases
017-019 were 1.24x/2.75x/2.38x wall time with the fallback and 2.01x/3.09x/3.63x
with ARM stencils enabled (debug, execution-trace artifact); all outputs
remained equal, but the stencil path regressed. Since the profile does not
identify a small safe shim change that can amortize a 1-5-instruction leaf,
the ARM default remains off and task 068 is closed with a no-go finding.
Larger region scope (task 048) is the evidence-backed next lever; no
benchmark-specific path or ABI hack was introduced.

Task 072's isolated prototype-method probe used a constructor with a stable
one-level prototype method and 10,000 calls from 100 instances (all names and
constants unrelated to the v8-v7 fixtures). In an execution-trace build it
recorded 19,998 `named_property_hit` results, 0 `named_get_prototype_miss`
events, and 9,998 `GetNQuickened` hits. The stable prototype-chain case is
therefore already admitted by the compact/quickened path; no guard-vocabulary
change was justified. The remaining `GetN` slow volume in the Richards
profile must come from other operations (notably `TraceSite`/`Branch` in the
instrumented run), so task 072 closes with this narrowing finding.

Task 071's broader IC-miss investigation also closes with a no-go finding.
The general two-shape property probe recorded only two layout-mismatch events
and three GetN quickening misses across 10,000 alternating reads; the stable
prototype probe had zero prototype misses. In the full instrumented profile,
the 4.285M slow-lane operations were instead dominated by `TraceSite`
(1.821M), `Branch` (1.714M), `CheckInitialized` (364K), and `Unary` (187K),
while `named_get_layout_mismatch` was zero. This evidence does not justify
changing the correct IC representation under task 071. The broad slow-lane
constant-factor question is recorded as follow-up task 074 rather than being
silently folded into an IC optimization.

## Task 074 slow-lane dispatch survey

The profile-first synthetic clone alternated a branch and numeric unary
operation for 200,000 iterations, then was repeated at 1,000,000 iterations
with different identifiers and constants from every benchmark fixture. A
five-second DWARF `sample` on the ARM64 debug artifact captured 3,702 samples
through the generic `run_instruction` boundary and canonical call/slow-path
chain; the residual fallback accounted for 1,168 samples, while the concrete
`execute_unary` body accounted for only 30. The non-trace sample likewise
showed the dispatch/call boundary rather than one semantic helper as the
dominant stack. The execution-trace run recorded 4,000,549 compact operations,
400,350 slow operations, and 200,003 `Branch` plus 200,004 `Unary` fallback
handlers, confirming the clone exercised the intended ordinary paths.

A generic optimized-build test of the only small candidate (forcing
`run_instruction` inline) was neutral: five warm 1,000,000-iteration runs were
0.42–0.43 s before and after, with identical output, roughly 8.44 billion
retired instructions, and 11.3–11.6 MiB peak RSS. The candidate was reverted.
The remaining cost is distributed dispatch/call machinery (and, in trace
builds, instrumentation), not an isolated handler with a safe compact
replacement. Therefore task 074 closes with a no-go finding: no production
semantic or layout change was justified, and complete ordinary fallback remains
unchanged. Existing neutral (100/100), curriculum (38/38 output-correct), and
v8-v7 correctness gates remain the applicable cross-checks; no score or RSS
gain is claimed from this survey.

## Follow-up: direct-call boundary probe

A symbolized five-second ARM64 `sample` of the real call-heavy workload showed
the dominant general stack as `execute_interpreter` →
`execute_code_frame_completion_with_owner` → `drive_code_completion_with_tier`
→ `dispatch_segment` → `run_compact_call`/`run_compact_call_fallback`, rather
than time in a rendered stencil leaf. This isolates the remaining cost to the
ordinary recursive call/continuation boundary, not to one benchmark-specific
property or arithmetic helper.

Two small boundary experiments were run against different optimized binaries
using the same synthetic recursive program (`f(30)`, 2,692,537 calls). Removing
the nested `stacker::maybe_grow` probe from `execute_direct` was within run-to-run
noise (1.72–1.86 s versus 1.72–1.82 s), and inlining that shim was likewise
neutral. Disabling the direct-call path entirely regressed the same workload to
2.09–2.10 s user time (from 1.72–1.75 s), confirming that the existing guarded
direct path is already the cheaper general mechanism. All temporary changes were
reverted; no unsafe ABI change or new call representation is justified by this
probe.

After the callee-directed continuation rewrite, the same 001–100 corpus still
produced 100/100 exact matches in a fresh one-run smoke pass. The raw report is
`target/micro-neutral-after-cps.json`; it is evidence only and is not read by
the runtime.

## Neutral performance probes

The measurement-only probes exercise reusable operation shapes and validate a
checksum for each result:

```sh
QUENCH_DISPATCH_ITERATIONS=1000000 node tools/dispatch-benchmark.cjs
QUENCH_LAYOUT_ITERATIONS=1000000 node tools/instruction-layout-benchmark.cjs
QUENCH_VALUE_ITERATIONS=1000000 node tools/value-representation-benchmark.cjs
```

For the runtime corpus, use the existing lane harness and retain its raw JSON:

```sh
node tools/bench-ops.cjs --out target/bench-ops.json
```

Reports must include repeated wall time, tail latency, startup/memory when
available, and the artifact identity. Synthetic probe numbers are not claims
about Node, Bun, or a full JavaScript suite. Task 026 targets a CPS-style
dispatch rewrite as a prerequisite for the copy_patch_jit tier (see
[`copy-and-patch-jit.md`](copy-and-patch-jit.md)); outside that gated
exception, no JIT, tail-call codegen, or executable memory is implied.

For task 016/019, a five-second macOS `sample` of a long neutral arithmetic
loop placed `run_code_completion_step_from` on the hot stack (3,659 samples in
the captured run); the outlined fallback symbols were present separately. The
artifact-level `size -m` report records a 14.09 MB text section. Raw profiler
output is disposable and should be regenerated with the same artifact before
making layout or dispatch changes.

The dispatch boundary returns an explicit `DispatchTransition` carrying the
callee-directed continuation target and completion. Normal transitions invoke
the supplied target directly; the frame driver is only an entry/exit shim.
This remains interpreter-only control transfer: it adds no computed-goto,
executable memory, or benchmark-specific path.

## Representation and lookup evidence

`dynamic::JsValue` has a compile-time/runtime size assertion (16 bytes) and
exercises NaN payload, signed-zero, pointer identity, and GC slot behavior in
its unit tests. `dynamic::ShapeTable` tests hash-bucket interning, O(1) derived
slot lookup, and memoized property-add transitions while retaining complete
equality checks and enumeration order. These are semantic invariants; timing
claims require a pinned neutral run of the probes above and are not inferred
from source inspection.

The execute-path `RegisterFile` stores every register and stack slot as the
canonical eight-byte `TaggedValue`; its test checks word copies and IEEE-754
bits without materializing a `Value`. The dynamic adapter remains a documented
16-byte boundary for APIs that need an explicit tag/payload pair, with GC-slot
and identity tests covering the handoff.

The object-heavy neutral subset (cases 031–040, three runs) is retained at
`target/micro-object-evidence.json`; it reports 10/10 exact matches and an
overall index of 151.90. This is reusable property/add/read evidence, not a
fixture-recognition path or a causal before/after claim.

The JavaScript and Wasm lowering adapters share the `SharedBinaryFact`
vocabulary for exact add/subtract/multiply overlap. Their physical instruction
enums remain frontend-specific; Wasm traps, numeric coercions, and other
non-overlapping operators stay explicit rather than being assigned a false
shared fact.

## Task acceptance index

| Tasks | Evidence |
| --- | --- |
| 001–005, 007–008 | `ir.rs` catalog tests and compact encoding tests |
| 002, 010, 012 | `tools/architecture-size-report.cjs` and this document |
| 006, 011 | runtime/node tests plus the 001–100 neutral corpus |
| 009 | `SharedBinaryFact` adapter test and Wasm numeric lowering tests |
| 013–015 | quickening unit tests and the execution-trace profile snapshot |
| 016, 019 | cold-symbol audit, `sample` profile, and `DispatchTransition` tests |
| 017–018 | tagged-value/shape unit tests and `target/micro-object-evidence.json` |
| 021–026 | `docs/copy-and-patch-jit.md` and stencil unit/differential tests |
| 029 | `enter_slow_path` transition tests, cold `#[inline(never)]` gateway, and zero-delta architecture-size comparison |
| 030 | Gate-0 DWARF/sample + ARM64 disassembly before/after counts, full runtime/node gates, and `target/micro-neutral-after-030.json` (100/100) |
| 031 | Gate-0 dependency audit recorded above; blocked without a physical pinned register, so no implementation was attempted |
| 032 | Side-table vs rewritten-opcode Gate-0 timing, opcode rewrite/dequicken differential test, full runtime/node gates, and `target/micro-neutral-after-032.json` (100/100) |
| 033 | `IcStubChain` chain/placement/bounded-state unit tests plus call/property differential gateways |
| 034 | Gate-0 DWARF/sample rendered-region symbol audit: zero samples on ARM64, so no implementation attempted |
| 035 | `docs/deegen-stencil-coverage.md` inventories all 31 catalog opcodes. The generated `DISPATCH` region covers 31/31 (100%) before and after the source-declaration reconciliation; specialized leaves cover 8/31 (25.8%). `dispatch_row_covers_every_compact_opcode` plus `quickened_catalog_entries_use_the_same_cfg_checked_dispatch_region` prove exhaustive admission and CFG-safe fallback. |
| 036 | Normal vs. unreachable `Optimizing` promotion was measured on ARM64 with three-run neutral sweeps: 100/100, Score 253.929 vs 251.828 (+0.83%), within the documented 2–3% noise band. The task's required positive net-win gate is therefore **not met**; the temporary disable multiplier was reverted and no optimizing-JIT claim is made. |
| 037 | Correctness gates passed: 580/580 no-default tests, 590/590 execution-trace tests, and `cargo check -p quench-node`. Fresh three-run neutral sweeps from the reverted, x86_64-gated baseline were all 100/100: interpreter-only Score 250.014 (wall 0.8242, RSS 0.3036, instructions 1.1348, cycles 0.7827), tier-up+OSR Score 253.392 (0.8000, 0.3031, 1.0872, 0.7599), and +Optimizing Score 254.547 (0.7820, 0.3047, 1.0816, 0.7610). These independent runs are monotonic on wall time, but the optimized view is unavailable on ARM64; task 036's paired gate remains the authoritative result and did not establish a net composite-score win. The status-table closure gate is therefore not satisfied. |
| 039 | Native AArch64 stencil backend: build-time const-fn encoders cover the existing eight specialized leaves and their regions. The opt-in clang/objdump encoder check passed. Full gates passed with 585 no-default tests, 595 execution-trace tests, and `cargo check -p quench-node`; the final bounded default neutral corpus was 100/100, Score 252.757 (1.0065 wall, 0.2935 RSS, 1.0563 instructions, 0.7282 cycles versus Node). `QUENCH_ENABLE_AARCH64_STENCILS=1` exercises the real ARM leaves; its paired sweep was 100/100 but scored 221.143, so default execution retains the complete baseline fallback while ARM call overhead is investigated. The extra Optimizing view remains x86_64-only. A direct five-second DWARF sample of curriculum case 025 captured `StencilArena::render_or_get` activity (36 samples) and `StencilLifecycle::observe` (1 sample); `execute_f64` is inlined. The navier-stokes sample remained timeout-bound and produced no rendered-symbol sample. The trace-enabled 38-case curriculum completed with 33/38 cases passing its optional checks; speed 171.9, memory 352.5. ARM-enabled cases 017/018/019 were 0.41x/0.58x/0.50x wall time; 025/026/027 were 3.09x/timeout/2.60x. Existing OSR/string slow-path findings remain. |
| 040.1 | Profile-first gate for arithmetic-loop glue: case 017 trace recorded `LoadLocalChecked` 10011, `Binary` 5006, `LoadConst` 5151, `UpdateLocal` 5000, and `StoreLocal` 5006, versus `Add` 5003. Before task 042, the executor invoked only one numeric f64 leaf and advanced one instruction, so widening `LOOP_OPS` would have been semantically unsound. Task 042 now supplies the bounded sequential executor; item 1 is unblocked, but its widening/measurement remains a separate task-040 change. |
| 042 | Added bounded sequential bridge stencils for the measured multi-op spans. Build-time admission remains single-entry and matches the complete live opcode window before execution; the bridge then invokes every canonical handler in order and returns the final transition. The required 3-block CFG counterexample, two-sequence differential test (including register state and PC), and Unknown-in-the-middle atomic-fallback test pass. Full gates: 588 no-default tests, 598 execution-trace tests, and `cargo check -p quench-node`. Curriculum 017/019 on the current trace build measured 0.40x/0.53x wall versus Node with default ARM fallback; opt-in `QUENCH_ENABLE_AARCH64_STENCILS=1` measured 0.76x/1.17x, so native ARM execution is correct but currently slower and remains opt-in. The full 38-case sweep remained 33/38 correctness/instrumentation passes (the same five performance-ceiling findings: 025–028 and 032). |
| 040.2 | Added bounded `CALL` and `CALL_N` region rows. Each row is a one-operation bridge into the existing canonical call-IC/named-call handler; callable identity and shape facts remain owned by the existing `QuickeningSite`, and any stale/hostile opcode rejects before execution. The named CFG admission test and call-region differential test (canonical-vs-admitted result/register state plus hostile fallback) pass. On this ARM64 build, the current executable optimizing view remains disabled by default, so this catalog expansion does not claim a runtime speedup: case 010 (call-inline-cache) is 14.13x wall versus Node (prior calibration ~15x), and case 029 (closure workload) is 1.06x; both remain correctness-clean. |
| 040.1 | Added the measured five-operation `ARITHMETIC_GLUE` bounded span (`LoadConst` → `LoadLocalChecked` → `Binary` → `UpdateLocal` → `StoreLocal`) using task 042's sequential executor. The new catalog row has its own five-op CFG proof and is covered by the existing admitted-vs-ordinary and whole-span hostile/partial fallback differential tests; all canonical handlers remain authoritative. On ARM64's default fallback build, cases 017/019 remain 0.40x/0.53x wall versus Node (the real stencil path is still opt-in/gated), so this item records coverage and correctness rather than a causal speed claim. |
| 040.3 | Added bounded single-operation bridge rows for `GetProperty` and `SetN`, reusing the generic shape-IC handlers and their existing `QuickeningSite` facts. Catalog CFG tests cover both rows; canonical property differential tests (plain, polymorphic, invalidated, and hostile cases) remain the semantic gate. Cases 011–016 are correctness-clean and measured 1.65x/1.73x/2.07x/2.49x/1.43x/1.75x wall versus Node (single-run ARM64 trace build); this is admission coverage, not a claimed speedup while the executable optimizing view is disabled on ARM64. |
| 040.4 | Added bounded bridge rows for `AGetI`, `ASetI`, and `AGetIInc`, reusing existing array-shape/index handlers with complete fallback. The catalog CFG test covers all three; existing indexed get/set differential tests cover packed, sparse, and non-extensible behavior. Array/object workload case 031 remains correctness-clean at 2.40x wall versus Node (single-run ARM64 trace build). |
| 040.5 | Added a `FOR_I` admission row as a bounded bridge to the canonical structured-loop handler. It does not create an OSR edge: `ForI` remains correctly excluded from OSR because it has no bytecode back-edge. The catalog CFG test covers the row; interpreter-dispatch loop case 003 remains correctness-clean but 5.64x wall versus Node, so no performance claim is made from this bridge-only admission. |
| 041 | Completed the V8-informed scenario survey. Array elements already have canonical packed/holey/sparse classification and complete fallback; the new `array_region_matches_packed_and_holey_fallbacks` differential test proves the `GET_INDEX` bridge cannot read a hole. Global access and comparison/typeof/instanceof remain partial but lack an isolated evidence-backed stencil win (Richards records 3 `LoadCurrentGlobal`, 3,528 `ResolveNameOrUndefined`, 3,409,018 `Binary`, and 186,701 `Unary` events). For-in has no remaining stencil gap after the canonical-layout membership fix. String concatenation is explicitly absent and deferred as a representation task, backed by curriculum case 032's 27.41x wall ratio in the current 38-case sweep. The full trace curriculum completed 28/38 under the 3x/1.5x ceilings with correctness intact; the known performance-ceiling cases remain unchanged. |
| 034 follow-up | Gate 0 rerun after 039/040 on ARM64 with macOS `xctrace` CPU Counters. A neutral 500k arithmetic loop produced 5,658 samples with real ARM stencil bridges (average counter vector `3853,445,4986,694`) and 1,898 samples with the complete fallback (average `4373,1301,3292,991`); `sample` separately confirmed rendered-region activity and inlining. The counters are nonzero and materially different, so the gate is not below noise. However, the generated ARM catalog contains only straight-line leaves and two-instruction dispatch bridges, with no cold basic block or fallthrough branch. Consequently hot-cold splitting has no applicable transform today; no bytes were changed and no benchmark claim is made. |
| 043 | Audited stencil-local boxing constants after 040/041. No existing region or leaf references `TAG_PREFIX`, `TAG_SHIFT`, or `TAG_MASK` twice: bridge rows only patch a dispatch pointer, numeric rows consume unboxed doubles, and the property leaf copies a tagged word. A scratch-register prologue would be pure overhead, so task 043 is closed with no eligible region; no code or semantics changed. |
| 044 | Replaced hand-written region-key accessors and per-region CFG test scaffolding with the declarative `generated_region_admissions!` macro in `stencil_select.rs`. Its single declaration covers all 21 catalog regions, derives `RegionKey::from_opcodes` accessors, verifies exact catalog operation sequences, and includes the externally-reachable-interior rejection case. No handler or fallback semantics changed. The post-refactor architecture report (`target/architecture-size-after-044.json`) reports 31 catalog rows and a 14,921,832-byte `__text` section; no per-region runtime metadata or duplicated slow-path bodies were introduced. Full gates: 592 no-default tests, 602 execution-trace tests, `cargo check -p quench-node`, and the curriculum's unchanged 28/38 optional-check result (all cases output-correct). |
| 045 | Profile-gated no-go for a shared global megamorphic cache. Execution-trace profiling of curriculum case 014 recorded `GetN` 0/404 and `SetN` 0/402 hits/misses, 809 named-property misses, 399 `layout:target` mismatches, and no named-property hits. The extracted degraded sequence therefore had 0/399 observed `(shape,name)` re-encounters; cases 013 and 015 instead remained within their bounded local IC behavior. No cross-site cache was implemented: the measured hit opportunity is absent, and the complete ordinary resolution fallback remains unchanged and bounded. |
| 046 | Profile-gated no-go for a rope/cons-string representation. Isolated timing of the case-032 shape on the ARM64 trace build put repeat-only at about 0.02 s and modest concat-only at about 0.04 s, while the equivalent `trim().split(/\s+/)` path took about 0.82 s and 13.8 billion instructions. The target outlier is therefore split/regex dominated, not concatenation dominated. A separate `s += 'x'` scaling probe did grow from 0.10 s (20k) to 2.21 s (200k), but that is not the measured case-032 bottleneck. No string representation change was made. |
| 047 | Profile-gated no-go for call-frame pooling/reuse. A DWARF `/usr/bin/sample` run of a recursive Ackermann workload captured 2,858 samples; the recursive `execute_call`/`execute_code_frame_completion_with_owner` path accounted for 436 samples at each active call depth, while frame-construction helpers appeared only as small leaf entries (the sampled `child_registers` path was 2 samples, with no standalone hot frame allocator). Fib profiling likewise showed environment allocation is frequent (392,907 allocations for 392,836 calls) but did not isolate setup CPU share. Existing curriculum baselines remain correctness-clean (case 010 about 14.13x and case 029 about 1.06x Node wall time on the current ARM64 sweep). No frame pool or layout shortcut was implemented; ordinary call-frame isolation and fallback remain unchanged. |
| 057 | Added a frequency-bounded, full-pass trial-deletion collector for the Rc object/function graph. The pass follows QuickJS's three phases: internal-edge subtraction, restoration from externally-owned nodes, then clearing edges of unreachable cycles. The registry is weak and populated at mutable object-property writes and function creation; closure environments (including captured parent frames) are traversed and cleared through canonical binding slots, while WeakFunction edges remain non-owning. Hash-indexed registry admission avoids quadratic bookkeeping. Unit tests cover an unreachable object cycle and visibility of closure-captured values. Re-running `tools/cycle-audit.mjs` after the collector gives plain-object N=50,000 20.3 MiB (previously 71.6 MiB), closure-capture N=50,000 21.8 MiB (previously 359.9 MiB), and the DeltaBlue-shaped probe 60.6 MiB. A 100,000-closure long-running loop stayed at 21.1 MiB peak RSS (9.2 s), demonstrating bounded growth. The collector uses a 512 KiB minimum allocation-byte trigger; the threshold adapts to surviving graph size, preserving amortized full-pass traversal without a per-operation tax. |

Rows intentionally point to reproducible checks rather than embedding a
benchmark-specific threshold in production code.

Task 035's fresh optimized neutral-corpus gate (`target/micro-neutral-after-
035.json`) produced 100/100 exact matches and Score **253.099**, up from the
task 033 snapshot's 249.059.  Aggregate engine/oracle ratios were 0.7462 wall
time, 0.3074 peak RSS, and 1.0841 retired instructions.  The source change is
catalog metadata only; no fixture-specific behavior or semantic fast path was
added.

## Task 053 regex profile

The execution-trace run of the v8-v7 RegExp fixture (`target/regexp-trace-run.err`)
shows match execution, rather than compilation, as the dominant cost. The
highest-volume URL pattern executed 5,932 times with 5.85 ms total compilation
and 3.430 s total matching; `^ba` executed 13,322 times with 3.87 ms compile
and 29.1 ms match time. Across the run the bounded compiled-pattern cache had
28,107 hits and 271 misses. The optimized release fixture run is finite
(2.36 s, 41.0 MiB peak RSS) but still emits `RESULT:RegExp:error`, so it has no
valid score or output-equality result yet.

Curriculum case 032 (`trim().split(/\\s+/)`) is output-correct and measured
about 0.76 s / 19.8 MiB on the trace-enabled debug artifact, versus the Node
oracle's roughly 0.03 s. Its split path uses the compiled backend directly,
so the current trace schema reports no per-pattern `regexp` row for this path;
the wall/instruction profile nevertheless isolates the split/match path rather
than concatenation (case 032's total was about 15.4 billion retired
instructions). This is a measurement-instrumentation gap, not evidence that
matching is cheap. Recommendation: keep the existing bounded compile cache and
scope a future narrow match-engine task after the RegExp correctness failure is
fixed; do not attempt an Irregexp-scale rewrite under this survey.

The RegExp output failure was narrowed with a temporary, source-neutral
diagnostic wrapper: all 95 local `re0`–`re94` bindings are non-null immediately
after `RegExpBenchmark` construction, but the first `runBlock0` use of `re7`
observes a null binding (`re7` is the `/(\\d*)(\\D*)/g` local). A small
closure with eight regex locals and repeated `exec` calls remains correct, so
this is not evidence for a regex-pattern-specific fast path; it points to a
large-closure/local-environment lifetime issue that must be isolated before
the v8-v7 fixture can contribute a valid score. The diagnostic source was
temporary and no benchmark identity is present in production code.

## Follow-up: cycle-collector root safety

The large-closure diagnosis exposed a second, general correctness issue in the
new cycle collector. During an allocation checkpoint between repeated calls,
the collector had no persistent root for the active global object (or the
currently executing constructor/frame), so it could clear live closure
bindings even though they were reachable from host/global state. The fix
admits the current global object as a graph root and protects every execution
environment for the core frame-entry drivers; pooled frames also refuse to
clear a suffix whose `SlotStore` is held by a captured binding.

Evidence is source-neutral: a returned closure retaining five regular
expressions and an object remains correct after 10,000 intervening allocations
and repeated calls, and `tools/cycle-audit.mjs` remains bounded for the
`closure_capture` probe (22.2 MiB at N=50,000). The fix preserves the complete
collector fallback and does not identify any benchmark or fixture.

## Task 070: complete v8-v7 ground truth

The ARM64 release artifact was re-run with the tracked v8-v7 runner at the
300-second escalation tier after the closure/root-safety fix. All eight
fixtures completed with `output_equal:true`: Richards 57.1 (3.67 s,
15.6 MiB), DeltaBlue 49.9 (5.27 s, 79.2 MiB), Crypto 17.2 (215.17 s,
33.9 MiB), RayTrace 168 (15.84 s, 18.4 MiB), EarleyBoyer 75.3 (66.55 s,
152.2 MiB), RegExp 13.9 (225.18 s, 1.59 GiB), Splay 381 (3.61 s,
433.7 MiB), and NavierStokes 216 (23.44 s, 18.1 MiB). The engine score
geomean is 71.84 versus Node's 87,774.56. The aggregate peak-RSS ratio,
including the now-completing RegExp process, is 2.416x; this exposes a real
memory outlier rather than hiding it behind the former timeout/error state.
RegExp remained CPU-active under a five-second DWARF `sample` and reached the
completion marker before the 300-second limit, so it is very slow but finite,
not a hang. The earlier 10-second and 60/120-second snapshots remain above as
historical measurements.

## v8-v7 trajectory (paired speed and memory)

| Snapshot | Complete / 8 | Engine score geomean | Aggregate RSS ratio | Neutral score | Curriculum |
| --- | ---: | ---: | ---: | ---: | ---: |
| `b2da584d6` release root-fix baseline (300 s) | 7 / 8 | 89.25* | 0.7882* | 109.665 (100/100) | 40.0 speed, 233.5 memory (29/38 under ceilings) |
| `2fff2aa70` release root-safety completion (300 s) | 8 / 8 | 71.84 | 2.416 | 285.028 (100/100) | 178.6 speed, 377.2 memory (33/38 instrumentation; 38/38 output) |
| `f76c6ff7a` post-048/049/054/059 findings (300 s) | 8 / 8 | 67.91 | 2.504 | 285.389 (100/100) | 178.1 speed, 377.3 memory (33/38 instrumentation; 38/38 output) |
| `625abe21e` production no-trace rebaseline (300 s) | 8 / 8 | 70.23 | 2.160 | 295.381 (100/100) | 182.6 speed, 377.3 memory (34/38 instrumentation; 38/38 output) |
| `c87fb7b53` ARM64 rendered-address correction (300 s) | 8 / 8 | 69.91 | 2.133 | 295.423 (100/100) | 195.9 speed, 387.7 memory (38/38 output; no trace counters) |
| `3ee862587` post-071/072 ARM64 release recheck (300 s) | 8 / 8 | 66.87 | 2.417 | 288.865 (100/100) | 182.95 speed, 378.22 memory (33/38 under ceilings; 38/38 output) |

\* The RegExp fixture is excluded because it emits an error; this is not a
whole-suite score. The row is a paired measurement record, not an optimization
claim. The required anti-cheat scan was run for this snapshot; its 1,382 text
matches are semantic API/module names (for example `RegExp`), with no fixture
identity detection in production code.

The `625abe21e` row uses the optimized artifact built without the optional
`execution-trace` feature for the v8-v7 and neutral measurements, so production
Score is not paying for instrumentation-only branches. All eight v8-v7 fixtures
completed with `output_equal:true`: engine scores were Richards 56.0, DeltaBlue
48.8, Crypto 16.8, RayTrace 163, EarleyBoyer 73.9, RegExp 13.6, Splay 373,
and NavierStokes 211. Summed peak RSS was 2,339,880,960 bytes versus Node's
1,083,408,384 bytes (2.160x). The paired curriculum check used a separate
optimized `execution-trace` artifact and remained output-correct for all 38
cases (34/38 optional checks), with speed 182.6 and memory 377.3. This is a
build-configuration rebaseline, not a semantic optimization claim; the v8-v7
target remains open.

The `c87fb7b53` row is a fresh production no-trace run after fixing the
ARM64 native-leaf address contract. All eight fixtures again completed with
`output_equal:true`: engine scores were Richards 55.6, DeltaBlue 48.5,
Crypto 16.8, RayTrace 164, EarleyBoyer 72.7, RegExp 13.6, Splay 368, and
NavierStokes 211. Summed peak RSS was 2,281,603,072 bytes versus Node's
1,069,793,280 bytes (2.133x). The neutral and curriculum values in the table
are paired with the same corrected source tree; the curriculum no-trace run
was 38/38 output-correct (speed 195.9, memory 387.7). This is a measured
trajectory row, not a claim that the ~60k capstone target has been reached.

The `3ee862587` recheck used `target/release/quench-node`, one run per fixture,
and the tracked runner's 300-second timeout. Every fixture returned
`output_equal:true`: Richards 52.1 (4.51 s, 15.6 MiB), DeltaBlue 46.4
(6.58 s, 87.2 MiB), Crypto 16.6 (223.47 s, 34.7 MiB), RayTrace 161
(16.58 s, 18.4 MiB), EarleyBoyer 70.4 (72.27 s, 151.4 MiB), RegExp 13.8
(226.88 s, 1.67 GiB), Splay 352 (3.73 s, 433.7 MiB), and NavierStokes 181
(27.89 s, 18.4 MiB). The paired neutral run was 100/100 with overall score
288.865; the trace-enabled curriculum run was 38/38 output-correct, with
speed 182.95 and memory 378.22 (33/38 under optional performance ceilings).
The current tree's anti-cheat scan found only generic semantic names (such as
the `RegExp` builtin), with no fixture-identity checks in production code.

## Task 069 Crypto completion/profile finding

The ARM64 artifact was re-run through the tracked v8-v7 runner with a
300-second timeout. Crypto did not complete within that window
(`engine_status=null`, with no result marker or score), so it remains a
confirmed slow workload rather than a valid score. A five-second DWARF
`sample` of the actual `quench-node` child (not the `/usr/bin/time` wrapper)
captured the main thread in `execute_call` → `execute_direct` →
`execute_interpreter`, with repeated nested `methods::execute_call_method` and
`execute_callee` frames. This indicates broadly distributed call/interpreter
overhead rather than an isolated bigint or allocation primitive. The sample
showed active nested execution, not a tight non-progressing loop. No
Crypto-specific fast path was added; a general call-dispatch investigation is
the appropriate follow-up and must retain the neutral/curriculum gates.

## Task 055 allocation-throughput survey

The three allocation-heavy v8-v7 workloads were profiled on the ARM64
release artifact with `QUENCH_EXEC_TRACE=1` and `/usr/bin/time -l`. The trace
allocation counters are event counts, not a claim that each event has equal
CPU cost; they are compared with total handler events and with the slow-lane
breakdown.

| Benchmark | Trace wall / retired instructions / peak RSS | Allocation events (environment + other + descriptor) | Handler events | Allocation-event share | Slow lane and dominant handlers |
| --- | --- | ---: | ---: | ---: | --- |
| Splay | 10.68 s / 235,921,011,030 / 447,152,896 bytes | 961,942 + 1,560,581 + 229,286 = 2,751,809 | 37,539,990 | 7.3% | 4,542,406 (12.1%); Branch 1,959,955, MakeObject 766,083, Call 766,080, Unary 489,721 |
| RayTrace | 45.00 s / 932,164,929,169 / 12,976,560 bytes | 3,958,886 + 11,799,004 + 103,453 = 15,861,343 | 148,434,464 | 10.7% | 12,001,630 (8.1%); TraceSite 5,289,355, Construct 2,197,739, Branch 1,818,973, Unary 1,124,080, CallMethod 902,258 |
| EarleyBoyer | 224.25 s / 4,705,282,236,127 / 115,950,048 bytes | 17,072,274 + 10,090,763 + 111,995 = 27,275,032 | 861,856,714 | 3.2% | 169,361,717 (19.7%); TraceSite 75,822,923, Branch 46,282,168, Unary 16,697,870, Call 10,004,170, Construct 8,769,589 |

The corresponding heap lifecycle counters are balanced after the cycle
collector: Splay environment/object/array allocated-dropped were
961,942/963,012, 1,142,910/1,142,567, and 417,626/417,617; RayTrace had
3,958,886/3,959,882 environments, 11,368,039/11,366,788 objects, and
430,833/430,756 arrays; EarleyBoyer had 17,072,274/17,072,660
environments, 9,746,223/9,679,513 objects, and 205,421/313,762 arrays.
These are high allocation counts, but the execution-trace slow lanes and
short native samples are dominated by dispatch, tracing, calls, construction,
and property/loop handlers rather than a sustained allocator stack. The
profiles do not isolate Rc retain/drop work as the dominant CPU share, and no
bounded local pool has a defensible target.

Task 055 is closed as a profile-gated **no-go**: do not rewrite the Rc memory
model or add a generational collector on this evidence. A future allocator
task would need a new, site-specific profile that attributes CPU time (not
just allocation volume) to one general allocation site, while preserving the
complete ordinary semantics and RSS bounds.

## Task 048: profiled loop-body region scope

Task 048 adds one bounded `LOOP_BODY` region row to the generated stencil
catalog. The admission is based on execution-trace data from neutral arithmetic
loops, not fixture identity: the recurring straight-line window is
`LoadLocalChecked, LoadLocalChecked, Add, StoreLocal, Move, UpdateLocal,
Return` (about 5,000 occurrences in curriculum case 017 and about 3,000 in
case 019). The sequential executor validates every instruction and the whole
window's single-entry CFG proof; stale, unknown, hostile, or interior-entry
matches fall back atomically to the ordinary handlers. The existing shorter
regions and all complete interpreter paths remain intact.

Fresh release/`execution-trace` measurements were paired before and after the
change. The 100-case neutral corpus remained fully correct and moved from
Score 283.887 (speed 219.622, memory 366.957) to Score 285.265 (speed
221.848, memory 366.811). Curriculum cases 017-019 remained 3/3 output and
instrumentation-correct; speed/memory scores were 233.4/377.0 before and
227.1/378.4 after, within normal run noise and with no regression claim. The
required full ARM64 v8-v7 cross-check at the 300-second tier also completed
all eight fixtures with `output_equal:true`: engine scores were Richards 53,
DeltaBlue 47.4, Crypto 16.8, RayTrace 158, EarleyBoyer 70.8, RegExp 13.8,
Splay 355, and NavierStokes 189 (score geomean 67.6204); peak RSS was recorded
per fixture, including the existing RegExp outlier. This region remains
architecture-gated/opt-in on ARM64 pending task 068's separate native
call-boundary decision.

The complete gates passed after the change: runtime library tests with and
without `execution-trace` (619/629 tests), `cargo check -p quench-node`, the
neutral 100/100 differential run, targeted CFG and fused-vs-ordinary
differential tests, and `git diff --check`. No production path references a
benchmark name, source path, or fixture identity.

## Task 049: bounded polymorphic variant survey

Task 049 was evaluated as a profile-gated design decision rather than by
adding speculative variants. Curriculum case 013 exercised a property read
rotating across three stable shapes (`GetN.hits = 597`, `misses = 5`), while
hostile case 021 exercised the same access family with shape churn (3,377
handler events). Both completed with correct output and the existing bounded
generic-IC/fallback machinery; the current trace vocabulary does not expose a
finite per-site shape-set fact that could safely select a build-time 2/3/4-
shape stencil variant. `RegionKey` describes opcode/fact sequences and
`PatchValues` carries runtime shape identities, so combining those identities
into static catalog rows would either be unbounded or duplicate the generic
IC's already-correct work.

The measured cases were already below the Node wall-time ceiling (case 013:
0.43x wall, 0.29x RSS; case 021: 0.32x wall, 0.27x RSS), with no evidence of
a bounded pre-rendered variant producing a general win to offset its N-times
rendered code cost. Task 049 therefore closes as an evidence-backed **no-go**:
retain the bounded IC chain and complete fallback, and do not add build-time
polymorphic variants until instrumentation proves a reusable finite fact
combination with a measured benefit.

## Task 059: scoped arena allocation audit

The direct audit found no safe candidate for moving JavaScript data to a new
bump arena. `FrameStack` already owns a contiguous, geometrically reserved
`Vec<Frame>` with a hard depth limit, and `RegisterWindow`/`RegisterFile` keep
active slots in one canonical word vector. Non-tail calls move the caller
window into `CallContinuation` and retain caller code, environment, arguments,
and guards until an arbitrary return; closures and captured locals can outlive
the current call. These references may cross coroutine, exception, or host
boundaries, so their non-escape cannot be proved by the current
Proven/Guarded/Unknown facts. Resetting an arena at call return would be
unsound. Temporary parser/string values likewise feed general `Value` APIs and
have no structural non-escape proof.

Call-heavy measurements were recorded before making this no-go decision:
curriculum case 010 completed output-correct at 1.66x Node wall time (RSS
0.25x), case 029 at 0.27x (RSS 0.26x); Richards/DeltaBlue completed with
`output_equal:true` at 3.17/6.57 s and 16.5/88.6 MiB respectively (scores
53.8/46.1). The existing frame reuse and cycle collector remain the safe
bounded mechanisms. Task 059 therefore closes without a new arena; a future
arena task needs a build-time escape proof for a concrete value class rather
than allocation volume alone.

## Task 060: request-arena Gate 0

Task 060 is intentionally blocked at its mandatory Gate 0. No concrete
web-framework or host integration has specified the ownership promise, async
boundary, or required API shape for a request-scoped arena. The runtime cannot
prove that arbitrary request code does not retain a value through a module,
Promise, timer, or host callback; implementing an implicit arena now would
violate the repository's escape-safety and no-corruption requirements. No
production code or allocation semantics were changed. A concrete host use
case and explicit opt-in contract are required before this task can proceed.

## Task 054: TypedArray fast-path survey

The existing `AGetI`/`ASetI` region rows are opcode-level bridges, not a
plain-array-only admission rule. With `QUENCH_ENABLE_AARCH64_STENCILS=1`, a
typed-memory probe (`Uint8Array`, 480 indexed writes and reads) entered the
same native region bridge (`leaf_hit = 956`); the bridge then revalidated the
whole instruction window and called the canonical handlers. Thus typed-array
access is already admitted to the bounded region mechanism, with complete
fallback, rather than being silently treated as a new specialized semantic.

The handler-level trace makes the remaining distinction explicit: the probe
recorded `AGetI = 480`, `ASetI = 480`, `packed_array_get = 0`, and
`packed_array_miss = 960`, plus 480 `RequireObjectCoercible` slow events. The
packed-number fast path intentionally accepts only `ArrayKind` ordinary
arrays; typed-array storage is handled by the generic typed-array/property
gateway. Extending that gateway into a new stencil would therefore be a new
value-representation/element-kind mechanism, not an admission correction, and
there is no evidence in this task that it is safe for every supported element
kind (`Int8` through `Float64`, plus bigint views).

The required v8-v7 NavierStokes cross-check completed correctly on the ARM64
release artifact: `output_equal:true`, 26.27 s wall time, score 192, and
19.0 MiB peak RSS (Node 2.03 s, 58.1 MiB). Since task 054 changes no code and
the existing bridge already covers typed-array opcodes with the canonical
fallback, there is no before/after optimization claim. Task 054 closes with
this evidence-backed finding; any future typed-array specialization needs a
separate representation-level task and per-kind differential coverage.

## Task 073: capstone trajectory row

The fresh paired capstone run used the ARM64 release artifact with the
300-second tracked v8-v7 timeout, the unchanged neutral 100-case corpus, and
the 38-case Deegen curriculum. All eight v8-v7 fixtures completed with
`output_equal:true`; engine scores were Richards 53.2, DeltaBlue 47.9, Crypto
16.7, RayTrace 159, EarleyBoyer 70.7, RegExp 13.9, Splay 362, and NavierStokes
188, for a score geomean of 67.9137 versus Node's 89,156.37. Summed peak RSS
was 2,589,868,032 bytes versus Node's 1,034,256,384 (2.504x); the RegExp
1.67-GiB process remains the dominant memory outlier. The neutral run was
100/100 with Score 285.389 (speed 222.199, memory 366.549). The curriculum
had 38/38 output-correct cases and 33/38 instrumentation/performance-passing
cases; the five known OSR/string cases exceeded the runner's proxy ceilings,
without semantic mismatches.

This row is a measurement record, not a claim of reaching the ~60k target:
the profiled LOOP_BODY coverage from task 048 and the no-go findings in
049/054/059 did not materially change the real-suite score, and ARM64 native
stencil execution remains opt-in pending task 068. The required anti-cheat
grep was run for this cycle; matches are generic semantic names such as the
`RegExp` builtin and no production path detects fixture identity, source file,
or benchmark-specific input.

## Task 073: ARM64 entry-cache follow-up row

The current working tree (task 068's typed-entry cache and in-place native-plan
borrows) was measured with the same 300-second tracked runner, three-run
neutral corpus, and one-run curriculum. All eight v8-v7 fixtures were
`output_equal:true`: Richards 55.9 (3.061 s, 15.3 MiB), DeltaBlue 48.8
(5.444 s, 80.1 MiB), Crypto 16.9 (220.161 s, 33.8 MiB), RayTrace 166
(16.065 s, 18.1 MiB), EarleyBoyer 74.3 (67.536 s, 151.3 MiB), RegExp 13.7
(227.075 s, 1.67 GiB), Splay 373 (3.636 s, 433.7 MiB), and NavierStokes
215 (23.510 s, 18.3 MiB). The engine score geomean is 70.7067; summed peak
RSS is 2,576,498,688 bytes versus Node's 1,070,956,544 bytes (2.4058x).

| Snapshot | Complete / 8 | Engine score geomean | Aggregate RSS ratio | Neutral score | Curriculum |
| --- | ---: | ---: | ---: | ---: | ---: |
| working tree, task-068 typed-entry cache/Rc-borrow | 8 / 8 | 70.7067 | 2.4058 | 300.833 fallback / 291.261 native (100/100 each) | 67.321 fallback / 69.661 native (31/38 each; 38/38 output) |

After centralizing admission in `stencil_policy.rs` and restoring the ARM
optimizing-view gate, the exact release artifact produced a fresh neutral
100/100 result: Score **299.336** (speed 239.289, memory 374.452). The
trace-enabled curriculum remained 31/38 with all 38 outputs correct, speed
**69.0**, and memory **338.6**. These are confirmation measurements for the
policy refactor, not a fused-region win; the v8-v7 row is recorded only after
the matching no-trace release run completes below.

The paired opt-in native neutral run had speed 232.396 and memory 365.036;
fallback had speed 240.144 and memory 376.859. The native curriculum run had
speed 69.661 and memory 338.767 versus fallback speed 67.321 and memory
340.855. Thus the typed entry removes steady-state work and helps the synthetic
long numeric loop and curriculum speed, but the aggregate neutral composite is
not a net win and the full-suite RegExp RSS outlier remains. ARM64 native
stencils stay opt-in; this row records progress toward the capstone and does
not claim the ~60k target.

The required production diff anti-cheat scan for this row found no fixture
identity strings, and all runtime tests, formatting, and diff checks passed.

The matching no-trace release artifact (with ARM stencil capabilities off, so
the relocation path is not on this production hot path) completed all eight
v8-v7 fixtures with `output_equal:true`. Its score geomean
was **66.2817** and summed peak RSS was 2,576,662,528 bytes versus Node's
1,090,027,520 bytes (**2.3639x**). Per-fixture scores were Richards 56.4,
DeltaBlue 47.6, Crypto 16.7, RayTrace 165, EarleyBoyer 74.3, RegExp 13.5,
Splay 326, and NavierStokes 154. This is a correctness and trajectory record
for the exact tree, not evidence that ARM stencils should be enabled by
default; the RegExp memory outlier and allocation-heavy Splay/NavierStokes
paths still dominate the aggregate.

The targeted trace sweep for arithmetic cases 017--019 remained 3/3 output
correct after the AArch64 chaining work: fallback speed/memory was **87.9 / 341.8**
versus opt-in ARM **87.8 / 339.5**. The small spread is not a measured net
throughput win, so the direct native leaves and fused bridge capability remain
opt-in/gated while a wider native state ABI is designed.

## Architecture policy follow-up

The working tree now derives all physical stencil admissions from one
`ExecutionPolicy` (`stencil_policy.rs`). Compile-time architecture facts and
the explicit `QUENCH_ENABLE_AARCH64_STENCILS` opt-in are converted once into
capabilities for scalar leaves, dispatch bridges, fused regions, and the
separate beyond-paper optimizing view. ARM scalar leaves remain opt-in;
dispatch bridges and fused regions remain disabled because the current ARM
region implementation still re-enters the Rust handler loop. The ARM
optimizing view remains x86_64-gated until a composed native region exists.
This removes repeated target branches without claiming a performance win or
changing JavaScript semantics.

The AArch64 fallthrough renderer now composes its head and return tail in one
arena allocation: `FADD` falls through to a patched `B`/imm26 relocation and
the tail performs the sole `RET`. `StencilArena::make_executable` invalidates
the published range with the platform cache-maintenance primitive before the
RX transition. The broader multi-op catalog remains bridge-backed and is still
disabled on ARM; no claim is made that those regions execute fused machine
operations yet.
The companion `/usr/bin/sample` capture is retained as
`target/arm-stencil-chain.dwarf.sample`; it sees the published ARM mapping as
anonymous machine code in the release artifact, so instruction attribution is
reported as unknown rather than inferred from wall time.

## Task 068 recheck after LOOP_BODY coverage

Enabling `QUENCH_ENABLE_AARCH64_STENCILS=1` was re-tested after task 048's
seven-op region was added. The unchanged fallback/default run scored 285.389
on the neutral 100-case corpus (speed 222.199, memory 366.549); native ARM64
enabled scored 236.048 (speed 152.400, memory 365.609), with 100/100 outputs in
both runs. Curriculum cases 017-019 likewise remained correct but fell from
the fallback profile (speed 227.1, memory 378.4) to native-enabled speed 116.9
and memory 376.0. The regression is therefore not explained by missing region
coverage; the current native call boundary and per-entry executable transfer
still cost more than the canonical Rust path. The ARM64 default remains off,
and no unsafe ABI shim was introduced.

## Task 068 ARM64 address-contract correction

DWARF/symbol sampling of the opt-in ARM64 path isolated a concrete lifecycle
bug rather than an ABI problem: `StencilArena::render_or_get` returns an
absolute address, while the Move and property leaves treated that value as an
arena offset and called `address` a second time. Every such execution returned
`Exhausted`, discarded the mapping, and paid a fresh mmap/mprotect/munmap cycle.
The leaves now pass the rendered address directly; targeted Move and property
tests verify execution and cache reuse without remapping. The complete
interpreter fallback remains unchanged on every physical or semantic miss.

On the current ARM64 host, the corrected trace-enabled neutral corpus was
100/100: fallback Score 282.143 (speed 220.936, memory 343.350) versus
opt-in native Score 284.858 (speed 224.968, memory 344.748). The paired
38-case curriculum remained 34/38 instrumentation/performance passes in both
configurations: fallback speed 184.5/memory 378.5, native speed
184.1/memory 375.6; cases 017/019 were 0.34x/0.47x wall versus Node on
fallback and 0.37x/0.43x with native leaves. Thus the fix removes the
pathological remapping cost and improves the neutral trace run, but the
curriculum speed is still marginally lower and memory is not better. The
`QUENCH_ENABLE_AARCH64_STENCILS` opt-in therefore remains off by default; no
default-on claim is made until both gates are net-positive.

## Task 068 steady-state ARM64 entry caching

The next DWARF/profile pass targeted the remaining per-hit boundary rather than
adding more stencil bytes. `sample` on a long ARM64 numeric loop still shows
the rendered leaf as anonymous machine code (the expected consequence of
copy-and-patch publication), while the Rust symbols account for the one-time
`StencilArena::render_or_get`/`NativeBinaryPlan::execute` setup. The plan now
retains the validated typed entry pointer after successful W^X publication;
subsequent numeric, Move, and own-property hits skip lifecycle, cache,
protection, and address checks. The pointer is cleared whenever the arena is
discarded, and unit coverage proves the arena remains reused. The baseline
plan's native-leaf accessors also borrow the existing `RefCell` in place
instead of cloning an `Rc` per instruction, removing reference-count traffic
from the same hot boundary. A fresh current-tree DWARF sample still attributes
steady-state work to the canonical dispatch loop and only sparse samples to
`NativeBinaryPlan::execute`/render setup, consistent with the cached-entry
design.

On the ARM64 host, three release runs of a 5,000,000-iteration numeric loop
measured fallback wall times of 1.75/1.76/1.75 s versus 1.62/1.64/1.63 s with
`QUENCH_ENABLE_AARCH64_STENCILS=1`; retired instructions fell from about
36.51B to 35.39B and cycles from about 7.00B to 6.49B. The paired three-run
neutral corpus remained 100/100 output-correct: fallback Score 296.306
(speed 234.977, memory 373.642) versus native Score 296.181 (speed 236.417,
memory 371.053). This is a real steady-state speed win for long numeric
regions, but the small memory cost and neutral composite parity do not justify
flipping the default; ARM execution remains opt-in pending a broader positive
gate. The paired one-run curriculum sweep stayed 31/38 (all 38 outputs
correct) and improved its speed score from 67.8 fallback to 69.3 native, while
memory moved from 341.8 to 340.1; the same five known performance-ceiling
cases remain the failures.

After the final Rc-borrow cleanup, a current-tree three-run neutral recheck
remained 100/100: fallback Score 299.043 (speed 238.020, memory 375.712)
versus native Score 297.456 (speed 237.366, memory 372.759). This small
release-to-release spread is still within the existing host-noise band and
does not support default-on ARM execution; a new full v8-v7 trajectory row is
deferred until that suite is rerun against this exact tree.

## ARM64 optimizing-plan admission experiment (reverted)

The already-generated AArch64 dispatch-region bytes were temporarily admitted
through `FunctionCode::executable_optimizing_plan` under the existing
`QUENCH_ENABLE_AARCH64_STENCILS=1` opt-in. This remained a general mechanism
experiment: no opcode, fixture, or source identity was inspected, and every
bridge miss retained the canonical fallback. On the ARM64 release artifact,
the neutral corpus changed from Score 284.181 (fallback) to 282.457 (native),
and the curriculum changed from speed 186.2/memory 384.5 to speed
173.0/memory 381.2. All outputs remained correct, but both speed gates
regressed, so the experiment was reverted and the optimizing view remains
x86_64-only. This confirms that the current multi-op AArch64 bridge does not
yet amortize its call boundary; no trajectory score claim is made.

## AArch64 fused numeric chain (Add -> Add)

The first genuinely composed native region is now generated from one
`add_chain` declaration. Its AArch64 bytes are `FADD d0,d0,d1; FADD
d0,d0,d2; RET` (x86-64 is the equivalent two-`ADDSD` sequence). The baseline
admits it only when the second `Add` consumes the first result, all three input
words are numeric, and the second third-operand does not alias the transient
result. The machine code has one typed entry (`extern "C" fn(f64,f64,f64)`),
one allocation/protection transition, and one return; all guards and aliasing
uncertainty fall back to the two complete canonical handlers.

On the ARM64 host, a release micro-run of three million `(x+1)+2` iterations
reported 1.13/1.13/1.13 s with the default fallback and 0.99/0.99/1.00 s with
`QUENCH_ENABLE_AARCH64_STENCILS=1` (about 12% lower wall time after warm-up).
The generated chain unit test also verifies `1.5+2.25+4.0 == 7.75` and cache
reuse without a second allocation. `target/add-chain.dwarf.sample` is kept as
the required profile artifact; macOS `sample` reports the copied RX mapping as
anonymous code, so no instruction-level attribution is claimed from that
capture.
The DWARF-enabled optimized capture `target/add-chain.dwarf-profiling.sample`
symbolizes the surrounding `vm_runtime.rs` dispatch frames while correctly
leaving the copied RX bytes anonymous; this separates setup/dispatch samples
from the leaf itself. This is a targeted proof that composition removes one VM boundary,
not evidence to enable every ARM region or the optimizing tier.

## Task 073: fused-chain full-suite cross-check (2026-09-04)

The rebuilt release artifact was run through the tracked v8-v7 harness with
the Node oracle, one run, and the documented 300-second per-fixture timeout.
All 8/8 fixtures completed with `output_equal:true`. Engine scores were
Richards **56.2**, DeltaBlue **48.6**, Crypto **16.9**, RayTrace **166**,
EarleyBoyer **73.8**, RegExp **13.7**, Splay **373**, and NavierStokes **212**;
the score geomean was **70.5340** versus Node **86,693.1060**. Summed peak RSS
was 2,575,581,184 bytes versus Node 1,071,923,200 bytes (**2.4028x**), with
RegExp still accounting for 1,789,034,496 bytes.

| Snapshot | Complete / 8 | Engine score geomean | Aggregate RSS ratio | Neutral score | Curriculum |
| --- | ---: | ---: | ---: | ---: | ---: |
| release baseline, ARM stencil policy default-off | 8 / 8 | 70.5340 | 2.4028 | 291.454 (100/100) | speed 181.7 / memory 375.3 (34/38; 38/38 output) |

For the separately tracked stencil-reachable subset, the three numeric-heavy
fixtures scored Crypto **16.9**, RayTrace **166**, and NavierStokes **212**
(subset geomean **84.0963**). With `QUENCH_ENABLE_AARCH64_STENCILS=1`, the
same targeted harness run produced Crypto **16.8**, RayTrace **155**, and
NavierStokes **213** (subset geomean **82.1625**), all output-equal; the spread
is not a positive stencil result. This keeps a real stencil signal visible
separately from Richards/DeltaBlue's generic Branch/TraceSite slow lane. The
neutral and curriculum cross-checks used the same tree; no benchmark identity
is inspected by production code. The chain's microbenchmark gain remains a
localized proof of boundary removal, not evidence to enable every ARM region
or claim that the 60k capstone target has been reached.

## Task 053 follow-up: remove the compiled-backtracking dependency (2026-09-04)

The DWARF capture `target/regexp-backend.dwarf.sample` identified compiled
backtracking and its growing capture-state vectors in the RegExp hot path.
The first replacement was deliberately narrow: regular ASCII patterns are
compiled once with the `regex` byte automata backend (`unicode(false)` so
`\w` and character classes retain the non-Unicode JavaScript meaning), while
patterns using unsupported ECMAScript constructs or Unicode inputs continue
through Quench's repository-owned parser/interpreter. Replacement and split
now acquire the same cached source/flags program used by `exec` instead of
rebuilding a parser and bytecode on every call. The `regress` dependency and
all production references to it are removed; this is a guarded physical
execution choice, not a second semantic model.

| Same-tree RegExp run | Score | Wall time | Peak RSS | Observable output |
| --- | ---: | ---: | ---: | --- |
| automata candidate | 59.3 | 54.17 s | 1,790,197,760 B | equal |
| `QUENCH_DISABLE_COMPILED_REGEXP=1` control | 22.8 | 137.41 s | 1,788,264,448 B | equal |

The earlier candidate-only run (62.9 / 51.2 s) is within the expected host
noise band; the paired control still shows the material effect. The anchored
literal specialization added in the same pass removes the high-frequency
`^literal` calls from the generic engine without inspecting fixture names.
An independently named 200k-iteration `^ab` replacement clone measured
4.10 s with the automata path versus 4.13 s with the disabled-backend control,
so the clone is neutral rather than an isolated-win claim.
The RSS outlier is unchanged, so allocation/representation remains a separate
track. Full-suite and curriculum gates must be re-run before any broader
default-on policy change.

## Call IC weak-identity probe (reverted, 2026-09-04)

The current full-suite rebaseline puts Crypto at **16.8** (220.19 s), making
its call/interpreter path the next measured target. A narrowly scoped probe
changed `CallableCache::lookup` from `Weak::upgrade` on every entry to a raw
pointer plus `strong_count` check, retaining weak edges and explicitly
rejecting dead entries to preserve allocator-reuse safety. This is a general
identity-cache change, not fixture-specific logic.

The renamed one-million-call clone was instruction-neutral (13.399B versus
13.398B retired instructions) and only varied by cycle noise (2.621B versus
2.592B cycles). The real paired Crypto run went from **16.8 / 220.19 s** to
**16.6 / 223.17 s**, both output-equal. Because neither gate was positive, the
probe was reverted. The remaining call cost is therefore the frame/dispatch
boundary itself; no weak-cache representation change is claimed.

## Stencil provenance ledger (2026-09-04)

The schema-2 `instruction-category-ledger.mjs` was rerun over the same 138
micro/curriculum fixtures with the trace-enabled release artifact. The default
ARM policy produced no stencil rows, as expected. With the explicit
`QUENCH_ENABLE_AARCH64_STENCILS=1` diagnostic opt-in, the new Rust-side
`stencil` category accounted for **3.08%** of counted events (4,037,356
observations), while `compact` fell from 15.72% to 12.02%. The largest sites
were `code=4:pc=2:binary` (839,009 hits, 0 misses),
`code=2:pc=4:binary` (236,099 hits / 6,272 misses, 97.4% hit rate), and
`code=2:pc=10:move` (217,174 hits, 0 misses). This confirms that the counter
is mapping real native decisions rather than silently counting only admission
attempts; it does not claim a score win, because the trace build is
instrumented and the ARM stencil policy remains opt-in pending paired
fallback/native benchmark gates.

## Target-architecture reconciliation (2026-09-04)

The direct audit of the target-architecture proposal confirms that no new
plan hierarchy or task is warranted. Task 044's generated
`generated_region_admissions!` declaration is the canonical build-time
RegionKey/CFG wiring. Task 048 has already added the largest currently
admitted seven-op `LOOP_BODY` span; its bytes are a single bridge stencil that
validates and executes canonical handlers, not runtime-generated machine code.
Task 049 surveyed bounded polymorphic variants and correctly closed as a
no-go because no reusable finite fact combination was measurable. The
`Native*Plan` structs in `machine.rs` are physical arena/cache/ABI wrappers
around those build-time records, not a parallel selector. The remaining work
is therefore limited to extending 048/049's build-time catalog and sharing a
compatible physical boundary template; runtime must remain lookup-only with
complete fallback.

## Corpus-wide instruction-category ledger (2026-09-04)

The new `tools/instruction-category-ledger.mjs` was validated against the
trace-enabled release artifact (`cargo build --release --features
execution-trace`) over 138 standalone micro/curriculum fixtures. The command
must set `QUENCH_EXEC_TRACE=1`; without that opt-in the tool correctly reports
missing snapshots. All 138 fixtures emitted a parseable snapshot, for a
grand total of 125,568,327 counted events.

| Category | Events | Share | Largest names | Interpretation |
| --- | ---: | ---: | --- | --- |
| `events` | 103,028,422 | 82.05% | `owned_word_read` 71.1%; `value_decode` 7.6%; `register_file_read` 7.6% | diagnostic counters, not retired instructions |
| `compact` | 19,777,386 | 15.75% | `LoadLocal` 18.0%; `LoadConst` 14.5%; `Return` 12.3% | existing compact interpreter lane |
| `environment_children` | 1,180,558 | 0.94% | `16:4` 72.5%; `19:5` 14.0% | environment-shape attribution |
| `slow` | 1,155,124 | 0.92% | `TraceSite` 35.8%; `Branch` 26.6%; `RequireObjectCoercible` 13.6% | generic semantic handlers |

This ranking changes the triage rule: `events` is intentionally excluded from
optimization decisions because it measures the diagnostic machinery itself.
Among semantic categories, `compact` dominates; however `LoadLocal` and
`LoadConst` are already in the release-only inline hot path, so this ledger is
frequency evidence rather than proof that a new cache will help. The earlier
weak-callable identity probe remains a no-go (near-zero retired-instruction
change and a Crypto regression). A candidate for the compact lane must first
show a reduced category share in a fresh ledger comparison, then pass paired
same-tree benchmark gates.

## RegExp capture-workspace and call-target measurement (2026-09-04)

The trace build was extended to report generated builtin names and to include
compact-call targets in the corpus ledger; scored builds retain the coarse,
zero-cost labels. Across the 38-case Deegen curriculum, `call_targets` was
256,696 events: 97.3% direct `Function` calls, with `floor` (1.2%) and
`push` (0.5%) the largest builtin names. A broad builtin-call IC is therefore
not the next justified mechanism.

The RegExp trace shows 2,887,450 `exec` calls, 1,267,860 `replace` calls,
328 regex-cache misses, 1,166,540 match-result allocations, and a peak RSS of
1,797,816,320 bytes in the trace-enabled diagnostic process (the retained
counter snapshot makes that RSS non-comparable to scored artifacts). The
compiled ASCII backend now reuses its
`CaptureLocations` workspace and uses `find_at` for no-capture patterns. The
runtime test suite remains 629 passed; three same-artifact RegExp runs were
58.4--59.3 score (Node 21,751--22,252), matching the prior 59.3 result within
noise. A fresh production verification of the same fixture is
output-equal at score **61.7** with **31,162,368 B** peak RSS (Node
70,664,192 B), so the historical 1.79 GiB number must not be used as a
production memory gate without reproducing the exact artifact. The dominant
remaining cost is match-result/object construction, not pattern compilation.

## Numeric constant representation probe (2026-09-04)

The corpus ledger ranks `LoadConst` at **14.5%** of compact events. A
representation-only probe now writes `Constant::Number` directly through the
register file's tagged-number slot; nonnumeric constants retain the complete
`Value` conversion path. On a renamed two-level numeric-loop clone, the
same-tree production binaries measured **3.56--3.61 s** with the direct write
versus **3.64 s** for the conversion baseline after warm-up (roughly 1--2%,
inside the observed 2--3% host-noise band). The clone's result stayed
`107500000`; runtime tests remain **629 passed**. This is a broad O(1)
representation improvement, but not a score-gate claim until the neutral and
curriculum corpora are rerun. The execution-trace build deliberately routes
through the diagnostic dispatcher, so its `LoadConst` share is expected to
remain unchanged; this probe must be judged with scored-build paired timings,
not by treating the diagnostic ledger as a proxy for release-only inlining.

## Corpus ledger refresh after representation probe (2026-09-04)

The refreshed schema-2 ledger covered **138 fixtures** with **126,861,366**
counted events. Excluding the intentionally diagnostic `events` row (81.61%),
the semantic ranking is `compact` **15.59%**, `environment_children` **0.93%**,
and `slow` **0.91%**; within `compact`, `LoadLocal` is 18.0%, `LoadConst`
14.5%, and `Return` 12.3%. The direct-number constant write is therefore a
release-only representation probe: the trace artifact confirms workload
frequency but cannot expose its inlined implementation. The full report and
facts log are `target/instruction-category-ledger-after-full.json` and
`target/instruction-facts-after-full.jsonl`.

## Fixed-arity direct-builtin call probe (reverted, 2026-09-04)

Because RegExp invokes a registered builtin with one argument, a temporary
path bypassed `CallArguments` and the generic callable gateway for immutable
builtin tags (while preserving register-aware mutators and constructor
semantics). The real fixture moved from **61.2 / 51.70 s** in the control to
**62.3 / 50.84 s** in one paired run, output-equal with comparable RSS. The
renamed regex-call clone measured **0.64--0.67 s** with the candidate versus
**0.64--0.66 s** without it, with identical output. Since the clone showed no
comparable improvement and the fixture delta is inside host noise, the probe
was reverted; no direct-builtin speedup is claimed.

## Proven-local environment threading and regex fallback completeness (2026-09-04)

The interpreter and baseline dispatch states now carry the active immutable
`Environment` where the caller owns it. `LoadLocal` and `StoreLocal` can
therefore use direct proven word transfers without opening a TLS closure on
each instruction; deleted, immutable, and uninitialized bindings retain the
canonical fallback. On the renamed local-accumulation clone, paired production
timings were **2.00--2.46 s** with the change versus **2.01--2.08 s** for the
same-tree baseline. The three-run spread overlaps, and the v8-v7 RegExp/Splay
pair stayed output-equal (RegExp **61.9** vs baseline **61.8**; Splay **377**
vs **377**, both within the host's broad variance). This remains
representation groundwork, not a measured score win.

The regex fallback no longer truncates its backtracking state vector at 4096
entries: that bound could silently discard a valid ECMAScript alternative. The
compiled linear backend remains the normal path; unsupported constructs now
preserve complete matching semantics at the cost of potentially exponential
fallback state on adversarial patterns. This
matches the QuickJS `libregexp.c` shape (compiled bytecode plus a small inline
execution stack that grows only on overflow) while keeping Quench's existing
capture and UTF-16 fallback machinery.

## Packed-array mutation transition (2026-09-04)

`ArrayData::set_index` now derives a packed array's monotonic kind directly
from an existing numeric overwrite (`Limb28 → Int → Double`, or
`PackedValue` for a non-number) instead of rescanning the dense payload. The
scan remains for hole fills, sparse growth, aliases, and other structural
uncertainty. A renamed 5-million-store array clone stayed output-identical;
paired production timings were **3.17--3.62 s** with the transition versus
**3.11--3.19 s** for the baseline, so this is an O(1) representation fix but
not a demonstrated wall-clock gain yet (the indexed numeric fast path already
bypasses `set_index` for much of this shape).

## Arithmetic-glue admission probe (rejected, 2026-09-04)

A temporary precompiled handler was tested against the actual lowered stream.
The first declared five-op shape was not emitted; trace transitions showed the
common numeric sequence is `LoadLocalChecked → AddConst → Move`. Wiring that
shape into the ordinary dispatcher produced zero successful stencil hits (all
admission attempts missed) and added guard/table work. The renamed local-loop
clone measured roughly **2.08--2.18 s** for the candidate versus **2.00--2.02 s**
for the preserved baseline after warm-up. The probe was removed and the
build-time `ARITHMETIC_GLUE` row restored to its original canonical fact; no
negative specialization remains enabled by default.

The micros corpus itself is now validated against Quench through a temporary
output adapter (Quench exposes a no-op `print`, while the frozen harness uses
`print` when present). Numeric, calls, locals, arrays, and composition smoke
groups all passed with output equality. Paired `locals` size sweeps (15
scenarios, two process pairs) remained output-equal and showed candidate
within-engine medians between **0.887 and 0.995×** the preserved baseline,
with no qualification claim; the stale harness-only attempt is retained as an
invalid artifact.

The latest uninstrumented v8-v7 subset check (one run, unchanged fixtures)
also remained output-equal: RegExp scored **62** versus Node **22071** at
**31 MiB** peak RSS, and Splay scored **381** versus Node **83968** at
**434 MiB**. This confirms correctness and records the current gap; it is not
evidence of a throughput or geometric-mean improvement.

## Freeze-time register-width fact (2026-09-04)

Function entry no longer sizes temporary registers from `FunctionCode::len()`
(instruction count). `CodeArena` derives the maximum register operand for each
lowered range once, stores that width beside the immutable range metadata, and
`build_registers`/generator entry selects it with a four-slot floor. Unknown
slow paths retain safe resize-on-write behavior. This keeps frame shape as one
derived fact rather than adding a runtime scan or a second frame model.

The focused locals micros sweep (15 scenarios, two independent process pairs)
was output-correct; candidate medians were **0.875–1.014×** the preserved
pre-change binary overall, with the eight-local `many` sibling at
**0.875–0.934×**. The full runtime suite remained **629 passed, 1 ignored**.
These are development paired results, not full qualification or a claim that
all call/closure/generator frame paths are complete; nested residual semantics
remain covered by the regression suite and need broader memory/RSS measurement.

## Micros arrays/composition follow-up (2026-09-04)

Using the frozen Edition 1 harness through the temporary output adapter, the
frame-width candidate (`549b7edee9f09bcaa7e2f6bfa065f4839143062740019db6745d6446ec9db0d1`)
was paired with the preserved same-tree baseline
(`d5498217ca08b89942e6152d4340ac4be80037d470f61f7a30aa880bbcc1cf93`). All
33 arrays/composition scenarios were output-correct. The arrays sweep was
noise-level overall (candidate/baseline median **0.999×**, range
**0.808–1.061×**, RSS median **1.035×**), so the packed-array transition is
not a measured throughput win yet. Composition was more encouraging but still
inside the host variance band (median **0.972×**, range **0.944–1.006×**, RSS
median **1.031×**); this is evidence that frame/local work can survive mixed
language behavior, not proof of a stencil or kernel gain. Reports are
`target/micros/arrays-candidate-framelayout.json`,
`target/micros/arrays-baseline-loadlocal.json`,
`target/micros/composition-candidate-framelayout.json`, and
`target/micros/composition-baseline-loadlocal.json`.

## Native scalar ABI parity (2026-09-04)

The AArch64 scalar numeric leaf now enters through the same small
preserve-none-equivalent wrapper as the existing three-operand fused chain:
`v0/v1` carry the two numbers and `v0` carries the result, with a raw `blr`
and no compiler-generated call-frame assumptions. This only changes the
physical boundary; it does not enable ARM native admission (the policy remains
off by default because the current leaf/bridge regions have not demonstrated
an end-to-end win). The opt-in ARM smoke run passed all six numeric cases after
the change. No speedup is claimed until a paired opt-in ARM run measures
retired instructions and the real numeric subset; the default scored binary is
unchanged in policy.

## Fused-chain live-result admission guard (2026-09-04)

The existing two-Add physical chain returns only the second destination. A
conservative build-time scan now rejects it when the first destination appears
in any later compact operand, preventing a stale intermediate from becoming
observable. The new regression test covers that case; the full runtime suite
passes **630 tests (1 ignored)**. This intentionally trades some possible
fusion coverage for a proof-friendly residual boundary; no timing gain is
claimed until a genuinely live-safe chain is measured on real loop shapes.

## Pinned StoreLocal probe (reverted, 2026-09-04)

Routing ordinary interpreter `StoreLocal` through the already-held environment
was tested as a companion to the pinned `LoadLocal` path. A two-pair micros
sample appeared faster (**0.85×** on `locals/many/medium`), but a matched
three-pair, 150 ms warmup/100 ms window repeat moved to **1.41×** the preserved
baseline. The result is therefore inconclusive-to-negative under the required
noise discipline; the change was reverted and the baseline `store_proven`
path remains active. Reports are retained at
`target/micros/locals-many-storelocal-repeat.json` and
`target/micros/locals-many-baseline-isolate.json`.

## Proven non-nullish coercion elision (2026-09-04)

The arrays/read trace exposed a repeated slow gateway in the actual lowered
loop: `GetNQuickened -> RequireObjectCoercible -> LoadLocal -> AGetI`. The
existing skip check was present only in the unplanned interpreter dispatcher
and required immediate adjacency, so baseline loop fragments still retired
one coercibility check per indexed read. The general fact is exact: a tagged
register word distinguishes `null`/`undefined` from every value that passes
`RequireObjectCoercible`. Baseline-plan execution now applies that fact before
dispatch, without requiring a later AGetI/ASetI to be adjacent; unknown or
nullish words still run the canonical slow operation.

Paired trace diagnostics on `arrays/read/small/17` changed
`RequireObjectCoercible` from **2,113** executions to **0**, slow handlers from
**2,728** to **615**, and total handler events from **28,022** to **23,796**;
`AGetI` stayed at **2,113**. The result remained output-correct. A same-tree
uninstrumented sweep using candidate binary
`3b79fd75f01b0a7fbed4ddf6befe78d7db40fe55390515a444f0027dcc714e7a` and before
binary `c00dcd52d06427be4dbfa7abb5c8a24a8a01263524642ad6e6f6a45d5e17a26d`
was output-correct for all three read sizes. Candidate/before timing ratios
were **0.968x (small, fail)**, **0.954x (medium, inconclusive)**, and
**0.947x (large, inconclusive)** under five process pairs; RSS ratios were
**1.003x, 0.995x, and 1.003x** (all passing). The direction is encouraging but
the timing bounds do not establish a qualification-grade win. Reserved
read/write/holey/sparse/grow/presized controls (36 scenarios) and the complete
runtime suite (**632 passed, 1 ignored**) remained correct. Reports:
`target/micros/arrays-read-coercible-paired-5.json` and
`target/micros/arrays-read-after-coercible-2.json`.

## Quickened named-load word transfer (2026-09-04)

The same trace showed 4,253 `GetNQuickened` hits. Their shared IC path was
decoding the receiver and cloning a slot payload for validation, then decoding
again to write the destination. `SlotWord::plain_tagged_bits` now validates
identity-sensitive `BindingCell`/`WeakFunction` payloads without materializing
`Value`; rewritten named loads retain the canonical tagged word directly.
Misses, descriptor/accessor changes, and identity-sensitive payloads still use
the complete property gateway.

The before/after trace kept compact, slow, and packed-array counts unchanged,
while `value_decode` fell **18,182 -> 13,929** and owned-word reads fell
**175,062 -> 166,554**. The isolated uninstrumented sweep (candidate
`cb582d4de2dacf558d542b819f2edee20291d86fad0da790c9351863efeb7bd5` versus the
same-tree coercion-elision binary `3b79fd75f01b0a7fbed4ddf6befe78d7db40fe55390515a444f0027dcc714e7a`)
was output-correct and passed timing bounds at **0.936x / 0.927x / 0.936x**
(small/medium/large; five process pairs). RSS ratios were **0.994x / 1.001x /
1.005x**, all passing. All 36 reserved array read/write/holey/sparse/grow/
presized scenarios passed, as did the complete runtime suite. Report:
`target/micros/arrays-read-word-paired-5.json`.

## Corpus instruction-category ledger (2026-09-04)

The schema-2 ledger was run over **139** standalone micros/deegen fixtures
with the execution-trace artifact (build metadata and raw facts are in
`target/instruction-category-ledger-current.json` and
`target/instruction-facts-current.jsonl`). The grand total was **122,185,955**
counted events. The top categories were `events` **84.35%** (dominated by
`owned_word_read` **71.3%** of that category), `compact` **12.78%** (top names:
`LoadConst` 18.4%, `Return` 15.7%, `LoadLocalChecked` 11.4%, `Binary` 10.5%),
`environment_children` **0.97%**, `slow` **0.88%** (mostly `TraceSite` 48.1%
and `Branch` 27.1%), and call-shape/target categories at **0.27%** each.
This is a triage ledger over diagnostic counters, not CPU time; it points at
shared representation/dispatch work and does not by itself justify enabling
ARM native bridges. The ledger had zero failed trace fixtures and appended
15,183 fact records.

## Composed array block entry (2026-09-04)

The existing `RegionKey` catalog previously admitted `array_loop_body` only as
a dispatch-shaped row. It now declares the lowered five-op shape
`LoadLocalChecked -> AGetI -> Add -> ASetI -> Return` and routes that key to a
single statically wired executor. Entry proofs cover local initialization,
plain dense array representation, bounds/number tags, operand wiring and
aliasing; the store commits only after every proof succeeds. Holes, prototypes,
non-number values and stale shapes take the complete canonical bridge. The
executor performs no per-op baseline dispatch for the admitted path and records
`composed_array_loop` separately in stencil diagnostics. ARM remains default-off;
`QUENCH_ENABLE_AARCH64_STENCILS=1` exposes this composed key through the
optimizing view for direct validation. Unit coverage exercises actual
`ExecutableCode` lowering, mutation, return state and a holey hostile fallback;
the full micros sweep is intentionally deferred until the infrastructure gate
is complete.

## ARM64 numeric-loop composition and generated ABI routing (2026-09-04)

The direct AArch64 loop stencil now separates one-time result initialization
from its condition header: the entry branch skips initialization on subsequent
iterations and the backward branch targets the compare header. Rendered-byte
coverage passes zero-, one-, many-iteration cases plus a non-zero initial
result, with exact `[1,2,3] -> [2,3,4]` mutation and preserved loop-carried
result. The ordinary OXC/lowering test discovers the same 19-op residual span,
constructs its baseline plan, and executes the selected rendered bytes at the
lowered loop PC; the per-plan witness reports a real native entry.

Region planning now iterates the generated declaration table rather than a
parallel key allowlist. Each generated `RegionRecord` carries a target-aware
`RegionAbi` (`Scalar`, `Bridge`, `ArrayKernel`, or `ArrayNumericLoop`), and
construction/invocation fail closed on ABI mismatch. Scalar Add/Move/property
rows therefore cannot be passed a `NativeRegionContext` pointer, while array
rows use raw contexts only on AArch64 and remain bridge/fallback rows elsewhere.
The host and AArch64 runtime suites each pass **642 tests** (one ignored), and
the Node host suite passes **4 tests**. Encoding verification succeeds with
`QUENCH_VERIFY_STENCIL_ENCODINGS=1`; micros remain gated pending the remaining
resource/root/safepoint and broader region-lifetime completion work.

## Region ownership and frame-root checkpoint (2026-09-04)

Rendered cache entries now carry a monotonic executable-arena owner token;
lookups require the same owner before a raw address can be called. This closes
the mapping-reuse/dangling-entry case while keeping the existing bounded cache
and W^X arena disposal model. Native array-loop admission is capped at 4,096
iterations until the physical ABI has an interrupt-poll instruction, with
larger loops taking the complete residual path. Call-frame root protection now
visits tagged register words directly instead of allocating an intermediate
snapshot vector. Focused owner, root, and ARM byte-execution tests pass; a
shared multi-plan slab and full native safepoint/root map remain unimplemented.
