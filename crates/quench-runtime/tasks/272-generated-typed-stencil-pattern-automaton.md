# 272 — Generated typed stencil pattern automaton

Status: planned

Make one immutable recipe table the source of truth for stencil patterns. Each recipe
declares a typed RegionPlan DAG pattern, semantic/effect constraints, input/output
contexts, external/symbolic holes, AOT handler generator, and static target cost. A
build-time macro/compiler derives both:

1. rustc source for the finite AOT stencil-template catalog; and
2. a bottom-up typed matcher/selector which computes a minimum-cost cover.

Model each context-compatible recipe as a weighted morphism. For a straight-line region,
selection is shortest path in the finite product of bytecode position and connector
context. For expression trees, use BURS-style bottom-up states. For a DAG, begin with a
bounded greedy/DP hybrid and defer exact integrated instruction selection/register
allocation when the state budget would explode. `MAX_PATTERN_WIDTH`,
`MAX_SELECTOR_STATES_PER_NODE`, `MAX_CONTEXT_VARIANTS`, and every cost component are
named constants.

Static predicates encode representation, liveness, effects, control-entry, and target
immediate constraints. Pattern order has no semantic meaning: ties resolve by a named,
stable recipe identity. The generated matcher emits an explanation record in diagnostic
mode and no counters or branches in normal linked code.

This replaces hand-written families of `select_*_supernode` logic incrementally. Task
267 and Task 268 become seed recipes and golden tests. The result remains an ordinary
`StencilExpr`; the automaton chooses morphisms but does not become a new execution tier.

Acceptance: one recipe addition generates catalog and selector coverage together;
regrouping the same RegionPlan gives the same cover; brute-force enumeration matches the
selector on bounded random trees/sequences; effect/control negative cases never match;
catalog and selection budgets are enforced at build time; full correctness and V8v7 A/B
pass.

Primary sources: LLVM's generated, typed DAG instruction selector
<https://llvm.org/docs/CodeGenerator.html>; MLIR declarative rewrite patterns and static
benefits <https://mlir.llvm.org/docs/DeclarativeRewrites/>; bottom-up optimal tree
pattern matching <https://doi.org/10.1016/0096-0551(90)90006-B>; Copy-and-Patch's finite
tree-shaped stencil library <https://arxiv.org/abs/2011.13127>.

Round-nineteen input discipline: consume Task 157's overlap-resolved candidate dictionary
as data. The automaton must not rediscover frequency, depend on declaration order, or
encode training-corpus identity. Candidate mining chooses the bounded vocabulary;
typed shortest-path/BURS selection chooses the legal minimum-cost composition for a
particular function. Keeping those phases separate preserves one quoted representation
and makes both outputs reproducible from manifest hashes.
