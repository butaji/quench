# 371 — Assumption-polymorphic native continuation dispatch

Status: planned

On a typed stencil guard failure, transfer directly to the most-specific compatible
prelinked native continuation instead of reconstructing a fully generic frame and entering
a coarse Rust block kernel. This is the Deoptless operation adapted to an always-stencil,
copy-and-patch VM: there is no interpreter destination and no runtime LLVM invocation.

## Canonical representation

Keep the mechanism quoted and data-driven:

```text
ExitKey         = ExitPc × ExitReason
Context         = Rep × Location × Shape × Ownership × Range × Effects × PcState
Continuation    = Stencil<ContextRequired, ContextProduced>
Candidate       = ContextPredicate × EntryTag × Arc<StencilInstance>
Catalog         = Map<ExitKey, NonEmpty<Candidate>>
```

`ContextPredicate` uses the existing product-lattice facts and defines the partial order
`a <= b` exactly when `a` implies `b`. Link-time construction enumerates the finite
contexts already demanded by Task 144, composes continuation expressions through the same
free-monoid/category core as ordinary code, patches immutable instances, and installs a
bounded catalog. The named constant `MAX_CONTINUATIONS_PER_EXIT` bounds code and lookup;
the final candidate is always `GENERIC_STENCIL_CONTINUATION`, never an AST interpreter.

Selection is one pure operation: among accepting candidates, choose the unique maximal
predicate under implication. Reject ambiguous incomparable maxima when linking. A direct
entry is legal only when the live machine context satisfies the candidate's typed connector;
otherwise select a weaker candidate. Do not eagerly materialize values already resident in
the required registers/slots.

The only mutable runtime datum is a site-local dispatch cell or bounded cache of chosen
candidate identities. `Kernel` and `StencilInstance` targets are immutable and reference
shareable. A miss may publish another existing candidate but cannot patch shared code,
change the catalog's semantic order, or create a benchmark-specific version.

## First slice

Use one guarded numeric loop with `I32`, `F64`, and unknown representations. Prelink three
general bytecode-derived continuations at its overflow/type exit. Show in disassembly that
an `I32 -> F64` failure transfers native-to-native, retains compatible live values, and
does not call `dyn_block_step_impl`, allocate a frame, or materialize unrelated slots.
Then exercise alternating representations to prove the bounded catalog remains correct
without hotness or recompilation.

## Laws and acceptance

- Context acceptance is reflexive and transitive; selection is independent of insertion
  order and returns the most-specific unique compatible candidate.
- Every continuation composes with its exit connector at compile time and rejoins a legal
  successor context.
- The generic candidate makes the catalog total for every legal JS state.
- Shared target addresses and bytes do not change after publication.
- Counters distinguish typed continuation hits, weaker-context hits, generic selections,
  ambiguity rejections, materialized values, native/Rust transitions, and unique/candidate
  code bytes.
- Correctness includes exceptions, overflow, `NaN`, negative zero, shape invalidation, and
  ownership/GC root reconstruction at effect edges.
- Alternating paired exact V8v7 A/B must improve the aggregate without violating component
  floors; otherwise retain only the reusable infrastructure or revert the selection path.

Primary source: Deoptless native-to-native OSR and context-predicate continuation dispatch
<https://arxiv.org/abs/2203.02340>.

Depends on Tasks 01, 03, 14, 16, 17, 48, 144, 149, 158, 164, 171, 203, 271, 309, 330,
346, and 348.

## First executable experiment

Task 396 applies this mechanism to the existing numeric-region guard boundary, where local
code inspection proves every failed conjunct currently demotes the complete region to
`execute_region_fallback`. It first measures exact failure constructors with Task 393, then
prelinks one strongest context, one weaker native context, and the total semantic-kernel
case. This bounded experiment must pass before expanding continuation catalogs to property,
call, or OSR-entry exits.
