# 70 — Polyhedral loop transformation as a categorical adjunction

Status: planned

[[21-loop-invariant-hoist-associativity]] and [[29-induction-variable-strength-reduction]] each handle one specific loop-transformation pattern via a dedicated argument (associativity-based hoisting; induction-variable rewriting). The polyhedral compilation model (used in Halide, TVM, and LLVM's Polly) generalizes both: represent a loop nest as an integer-point lattice (the iteration domain) and a transformation as an affine map between "loop nest space" and "schedule space" — an adjunction structurally analogous to [[60-galois-connection-guard-elision]]'s Galois-connection framing, but for loop scheduling instead of static analysis soundness.

Evaluate replacing [[21-loop-invariant-hoist-associativity]]/[[29-induction-variable-strength-reduction]]'s two separate pattern-matched optimizations with one polyhedral rewrite that derives both (and others: loop fusion, tiling, interchange) as instances of finding a better point in schedule space under the adjunction, rather than adding a new dedicated task per loop-transformation pattern as they're discovered.

Concrete steps:
1. Define the iteration-domain representation for a `loop_`-typed `Stencil<Ctx, Ctx>` region (per [[01-stencil-category-core]]'s existing loop combinator) as a polyhedral set.
2. Define the adjunction: an abstraction from "concrete loop nest" to "schedule space," and a concretization back, analogous in structure to [[60-galois-connection-guard-elision]]'s `α`/`γ` pair.
3. Re-derive [[21-loop-invariant-hoist-associativity]]'s loop-invariant hoisting and [[29-induction-variable-strength-reduction]]'s induction-variable strength reduction as two specific schedule transformations under this one framework, and verify no capability is lost relative to their original dedicated-task formulations.

Acceptance: [[21-loop-invariant-hoist-associativity]] and [[29-induction-variable-strength-reduction]]'s existing acceptance criteria are met by the unified polyhedral rewrite, not merely superficially similar output; at least one additional loop transformation not previously planned (loop fusion or tiling) is derived "for free" from the same adjunction, demonstrating the generalization has real payoff beyond re-deriving what already existed; the polyhedral representation's soundness (transformations preserve loop semantics) is proven structurally, not tested case-by-case.
