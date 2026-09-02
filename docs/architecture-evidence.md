# Architecture and performance evidence

This document records reproducible checks for the fact-generated VM plan,
which is interpreter-first with one bounded, gated exception — the
copy-and-patch region-stencil tier described in
[`copy-and-patch-jit.md`](copy-and-patch-jit.md). It is an evidence index, not
a benchmark score and never selects a production path.
Run commands from the repository root; write generated reports under
`target/` so they remain disposable.

## Structural gates

```sh
node tools/check-vm-architecture.cjs
node tools/architecture-size-report.cjs target/debug/quench-node \
  > target/architecture-size.json
cargo fmt --all -- --check
git diff --check
```

The architecture gate checks the single `vm_op!` catalog, contiguous generated
IDs, handler coverage, task-queue integrity, runtime feature boundaries, and
the absence of workload identity tokens in `quench-runtime/src`.

The size report is the task-010 complexity ledger: the current optimized
artifact records 28 generated catalog rows, 466 runtime Rust files, 5.58 MB of
runtime source, and a 14.09 MB `__text` segment. It is descriptive evidence;
it does not authorize a specialization.

The execution-seam inventory is intentionally mechanical: `ir.rs` owns the
catalog and compact encoding, `machine.rs` owns code arenas and site
attachment, `quickening.rs` owns bounded physical cache state, and
`vm_runtime.rs`/`vm_dispatch.rs` own ordinary semantic gateways. The benchmark
crate and `tools/` contain measurement adapters only; they are not imported by
the runtime's execution path.

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

It produced 100/100 exact observable matches. The aggregate engine/oracle
ratios were 0.964x wall time, 0.428x peak RSS, 1.135x retired instructions,
and 1.002x cycles (overall index 156.04). This is a reproducibility snapshot,
not a production dispatch rule or a claim about Bun.

The same run against the locally installed Bun oracle is retained at
`target/micro-neutral-bun-evidence.json`: it also produced 100/100 exact
matches (2.358x wall time, 0.710x peak RSS, 4.466x instructions, and 3.143x
cycles relative to Bun). Node and Bun are measurement oracles only; neither
result enables workload-specific runtime behavior.

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

The dispatch boundary now returns an explicit `DispatchTransition` carrying
the next program counter and completion. The driver consumes that transition;
branch and jump successors are decoded from the catalog's control facts. This
is the interpreter-only groundwork requested by task 019; it does not add CPS,
computed-goto, executable memory, or a JIT.

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
| 001–005, 007–008 | `ir.rs` catalog tests, compact encoding tests, and the architecture gate |
| 002, 010, 012 | `tools/check-vm-architecture.cjs` and this document |
| 006, 011 | runtime/node tests plus the 001–100 neutral corpus |
| 009 | `SharedBinaryFact` adapter test and Wasm numeric lowering tests |
| 013–015 | quickening unit tests and the execution-trace profile snapshot |
| 016, 019 | cold-symbol audit, `sample` profile, and `DispatchTransition` tests |
| 017–018 | tagged-value/shape unit tests and `target/micro-object-evidence.json` |
| 021–026 | `docs/copy-and-patch-jit.md`, stencil unit/differential tests, and the architecture gate's copy_patch_jit invariant checks |

Rows intentionally point to reproducible checks rather than embedding a
benchmark-specific threshold in production code.
