# Implementation plan

This is Quench's only implementation queue. It implements ADR 0005 in order;
do not revive superseded IR, JS-builtin, TypeGraph, or multi-plane work.
Each slice is minimal, deletes the replaced representation in the same change,
and is gated by affected unit tests (only when permitted), the module suite,
the current test262 stage, formatting, and clippy.

## 0. Preserve conformance while creating the seam

- [ ] Keep the test262 runner boundary intact and fix current-stage failures in
  related families.
- [ ] Identify and delete duplicate semantic helpers before moving a family.
- [ ] Establish reducer entry points beside current execution only where a
  complete semantic family can move without a permanent compatibility path.

## 1. OXC + ProgramDb

- [ ] Parse with OXC and consume OXC semantic scopes/symbols directly.
- [ ] Define `ProgramDb` and the `Proven` / `Guarded` / `Unknown` fact lattice.
- [ ] Add fact provenance and invalidation rules; facts may not bypass
  observable behavior.
- [ ] Reduce one complete lexical-binding family to slots, then delete its
  runtime name-resolution duplicate.
- [ ] Release OXC arenas after reduction unless explicitly retained by tooling.

## 2. Semantic reducer and residual ops

- [ ] Establish `value`, `place`, `effect`, `control`, and `define` contexts.
- [ ] Declare the semantic kernel and give each operation one canonical owner.
- [ ] Introduce `ops!` as the sole operation declaration: tags, operands,
  verifier, encoding/decoder, dispatch metadata, and disassembly derive from it.
- [ ] Migrate complete semantic families to residual ops; delete their old
  evaluator paths in the same slice.
- [ ] Add only guarded/measured specializations and generic fallback paths.

## 3. Compact runtime data

- [ ] Introduce `HeapRef(u32)` and tagged `Value` behind one value API.
- [ ] Declare heap layouts with generated tracing, casts, size, snapshot, and
  debug metadata; keep allocation and GC algorithms explicit.
- [ ] Migrate ordinary objects to shapes and slots, then add bounded property
  sites (`Cold`, `Mono`, `Poly`, `Generic`) with guard/fallback behavior.
- [ ] Use slot frames and capture slots; do not allocate runtime places.

## 4. Control and primordials

- [ ] Make `Completion` the one abrupt-control algebra while retaining the
  normal-PC fast path.
- [ ] Introduce shared continuations for generator/async capture and resume;
  keep protocol-specific transitions separate.
- [ ] Declare builtin metadata and generate realm images, descriptors, and
  primordial installation. Replace executed JS bootstrap construction.
- [ ] Keep builtin algorithms readable Rust fast/generic handlers.

## 5. Specialization and deferred compilation

- [ ] Generate specialization selection, guards, fallback, quickening data,
  and profile counters from operation declarations.
- [ ] Generate superinstructions only from proven primitive compositions and
  only after measurement.
- [ ] Consider a baseline compiler only after interpreter profiling proves
  dispatch is the bottleneck; it must consume exact residual ops.

## Non-negotiable checks

- No optimization through Proxy, accessors, coercion, `Symbol.toPrimitive`,
  dynamic prototype mutation, direct `eval`, realms, or completion order.
- No parallel semantic representation, handwritten specialized semantics, or
  speculative optimizer/JIT/type-runtime work.
- No test counts, rates, baselines, or failure logs in task prose. The stage
  command is the conformance SSOT; `tasks/index.json` holds stage workflow only.
