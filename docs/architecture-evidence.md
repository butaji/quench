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

Rows intentionally point to reproducible checks rather than embedding a
benchmark-specific threshold in production code.

Task 035's fresh optimized neutral-corpus gate (`target/micro-neutral-after-
035.json`) produced 100/100 exact matches and Score **253.099**, up from the
task 033 snapshot's 249.059.  Aggregate engine/oracle ratios were 0.7462 wall
time, 0.3074 peak RSS, and 1.0841 retired instructions.  The source change is
catalog metadata only; no fixture-specific behavior or semantic fast path was
added.
