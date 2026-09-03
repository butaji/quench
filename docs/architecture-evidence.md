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

Rows intentionally point to reproducible checks rather than embedding a
benchmark-specific threshold in production code.
