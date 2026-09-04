# Architecture and performance evidence

This document records reproducible checks for the fact-generated VM plan,
including the copy-and-patch region-stencil tier described in
[`copy-and-patch-jit.md`](copy-and-patch-jit.md). It is an evidence index, not
a benchmark score and never selects a production path.
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
reverted; equivalent subsystem-specific fault injection for the remaining
four tests is still outstanding.

Task 063 extends the module with three collection invariants. Fresh ARM64
debug runs measured 1.53x (packed-array index-0 access at lengths 10 versus
100,000), 0.92x (Map/Set access across 10 versus 1,000 unrelated instances),
and 2.38x (Map/Set/Array iteration after 0 versus 2,000 add/delete cycles)
without tracing; all are below the 16x scaling bound. The collection tests
also pass under `execution-trace`. These are scaling checks only; deliberate
subsystem fault-injection validation remains to be recorded before task 063
is considered fully closed.

Task 065 adds closure, recursion-frame, and argument-marshaling invariants.
ARM64 debug ratios were 0.81x/0.75x for closure creation across 10 versus
1,000 closures, 1.12x/1.17x for per-frame recursion cost at depths 10 versus
100, and 0.74x/0.72x for marshaling across 10 versus 1,000 unrelated
functions (untraced/traced). All 14 architecture tests pass in each
configuration. Deliberate fault-injection validation for these new claims is
still outstanding.

Task 066 adds raw allocation and release checks. With 200,000 timed
allocations/drops (enough to amortize heap setup), ARM64 ratios were 1.51x
and 1.39x for live heaps of 10 versus 10,000 without tracing, and 1.56x and
1.52x with tracing. The two invariants pass alongside the preceding 16
architecture tests in both configurations. No arena-specific invariant is
claimed because task 059 is not implemented; deliberate allocation fault
injection remains outstanding.

Task 067 adds bounded stencil-tier checks. Region lookup (10,000 versus
100,000 repeated probes) measured a 1.01x per-probe ratio; a full 16-entry
render memo history produced a 1.20x cache-hit ratio; and arena bump
allocation (1,000 versus 10,000 placements) measured 0.25x after timer/setup
amortization. All three pass the documented <16x bounds. These tests exercise
selection/cache/allocation mechanics without executing native bytes; the
ARM64 executable path remains explicitly opt-in via
`QUENCH_ENABLE_AARCH64_STENCILS`, so no claim of native stencil execution is
made by this invariant suite.

Task 058 audit: quench already hash-conses ordinary-object layout facts in
`value.rs` (`OBJECT_LAYOUTS` plus hash buckets), and the existing
`equal_property_sequences_share_one_layout_fact` test proves two independent
objects reuse one `semantic_layout_id`. The interner is thread-local and its
structural key is the visible property-name sequence; descriptor attributes
and prototype identity are not part of that derived layout fact. This is
therefore Partial rather than a new implementation claim: extending the key
would require a representation/IC audit, and no semantics-changing change is
justified without that evidence.

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
both feature configurations. As with task 063, deliberate fault-injection
validation for each new subsystem claim remains outstanding.

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
