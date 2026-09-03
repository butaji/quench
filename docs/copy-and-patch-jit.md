# Copy-and-patch region-stencil tier

This document describes `copy_patch_jit`, a copy-and-patch region-stencil
tier layered on top of the fact-generated interpreter. It is not a mechanically
enforced exception; the project targets the best available VM design rather
than gating design space by policy.

## Relationship to the Deegen paper

The design adopts one technique from Deegen (arXiv:2411.11469) — Copy-and-Patch
code generation (Xu and Kjolstad, 2021) as used for Deegen's baseline JIT tier
(§7). This document covers only that tier. The project now targets Deegen's
full two-tier JIT-capable VM — profiling-triggered tier-up, OSR entry,
deoptimization, and an optimizing JIT — tracked separately in
`docs/deegen-alignment.md` (tasks 027-031); this tier is that plan's
prerequisite baseline, not its ceiling. Deegen's own baseline JIT design goal
is stated as "to compile as quickly as possible" with code quality only "a
secondary priority" (§7.1); this tier goes further and drops the "compile"
step almost entirely by working from a build-time-derived, pre-rendered
stencil catalog rather than a runtime code generator.

## Data model

- `Fact` — reuse the existing `vm_op!`/`Proven`/`Guarded`/`Unknown`
  classification as-is; a stencil never gets a second fact representation.
- `RegionKey` — canonical specialization key: `hash(region_id, fact_vector)`.
  Same facts always hash to the same key.
- `Hole` — `{ offset: u16, kind: HoleKind }`.
- `HoleKind` — a closed enum: `Imm32 | Disp32 | Rel32 | Ptr64`. No generic
  relocation engine.
- `Stencil` — `{ bytes: &'static [u8], holes: &'static [Hole] }`, pure data.
- `PatchValues` — a read-only view into the existing `QuickeningSite`
  shape/callee/slow-path state (`crates/quench-runtime/src/quickening.rs`);
  never a second copy of that fact.
- `BoxingFact` — a declared description of the existing `JsValue` tagged
  layout (task 017) so type-check strength reduction has one source of
  truth, mirroring the Deegen paper's boxing-scheme description APIs
  (§4.2/Appendix A.2, consumed by §5.1's type-based optimization) without
  renegotiating the boxing scheme itself.

## Design (mirrors the paper's techniques, not its scope)

- **Region stencils, not opcode stencils.** Stencils are fused **regions** (a
  loop body, a property-access chain) admitted by existing Proven/Guarded
  facts, not one-stencil-per-bytecode as the paper does (Appendix B compiles
  one bytecode at a time) — this is this project's own extension beyond the
  paper. A fused region is only admitted after a build-time proof that its
  interior has no externally-reachable entry point besides its declared one,
  so hot-cold splitting and jump-to-fallthrough stay sound across the fused
  boundary.
- **Type-check elimination as one named algorithm.** Mirrors the paper's
  algorithm 𝒜 (§5.1): one reusable build-time pass over a region's semantic
  function and a fact predicate, not per-operation ad hoc logic.
- **Runtime does select/alloc/copy/patch/execute only.** If instruction
  selection, register allocation, or CFG analysis ever shows up at runtime,
  that is a real optimizing JIT and out of scope.
- **Callee-directed tail dispatch is a prerequisite (task 026).** The paper's
  fast-path fallthrough elimination (§7.1) only has meaning once "what runs
  next" is a callee-supplied address, not a value a driver loop interprets —
  mirroring the paper's continuation-passing interpreter design (§6.1).
- **Patch data before patching code.** The stencil lifecycle
  (`Cold -> Rendered -> Installed -> Repatch -> Retired`) rewrites only
  `PatchValues` when the installed stencil's holes still cover a new fact,
  and only re-renders/copies code when they do not — using the same bounded
  degrade-tier limit as interpreter-side quickening (task 014) before
  retiring to the ordinary path permanently. `Repatch` is the effectful half
  of the same named idempotent-probe/effectful-apply interface
  `QuickeningSite::observe` already implements as its pure half (mirroring
  the paper's λi/λe generic-IC split, §5.2).
- **Memoization, not hotness-triggered tier-up.** Rendered regions are
  memoized by `RegionKey` so identical `(region, fact)` combinations are
  never re-rendered. This memoization is eager per admitted fact
  combination, never hotness-triggered — the paper's profiling/tier-up
  machinery (§3) is explicitly out of scope and would need its own task.

## Design invariants (not mechanically enforced)

These remain load-bearing for correctness even without a gate script:

1. Every `copy_patch_jit` task depends on one of the correctness/dispatch
   work (011, 016, 019, 026) or another `copy_patch_jit` task, so the tier
   is not built ahead of its prerequisites.
2. Every `copy_patch_jit` path keeps the complete ordinary interpreter as its
   fallback on any Unknown fact, hole-table miss, or patch failure — the same
   rule that governs every other guarded fast path in this project
   (see `docs/benchmark-integrity.md`).

This tier's implementation breakdown (tasks 021-026) is closed; see git
history for those task files. The remaining gap to the paper's full design —
tier-up, OSR, deopt, and the optimizing JIT tier — is tracked in
`docs/deegen-alignment.md` and `tasks/index.json` (tasks 027-031).

## Current implementation evidence

The build script emits a fused Number Add+Return x86-64 fragment, a guarded
property region with a typed `Ptr64` hole, and a two-region `Rel32`
fallthrough variant. Each generated key is derived from that region's
generated opcode slice, so the eligibility facts have one source.
Runtime selection is a single `RegionKey` lookup; rendering uses the bounded RW
arena, copies and patches before the one-way RX transition, and invokes the
installed Add fragment through the arena-owned ABI entry. Invalid keys, stale
cache addresses, protection failures, and execution errors all return to the
ordinary interpreter callback. The executable differential test is covered by:

```text
RUSTFLAGS='-Awarnings' cargo test -p quench-runtime --lib stencil_ --quiet
```

The interpreter continuation path now invokes the handler-supplied callee
target directly; the ordinary driver remains only the frame entry/exit shim.
This evidence still does not claim a complete baseline-JIT or platform-specific
assembly tail-call backend.
