# 29 — Induction-variable strength reduction

Status: planned

Recognize induction variables (a binding incremented by a loop-invariant step each iteration, such as `i = i + 1`) inside a `loop_`-typed body and rewrite dependent expressions that scale linearly with it (for example `arr[i * 4]`) into an incrementally-updated value carried alongside the induction variable, avoiding the per-iteration multiply. Implement as a rewrite rule inside the quotient-category framework from [[23-egraph-rewriting]], specialized to the `Ctx → Ctx` shape already guaranteed by the `loop_` combinator, and complementary to the associativity-based hoisting in [[21-loop-invariant-hoist-associativity]] (hoisting removes loop-invariant work; this reduces loop-variant work).

Acceptance: a loop indexing an array with a linear function of the loop counter compiles with an incremented offset instead of a per-iteration multiply, verified by instruction-count comparison; a loop where the indexing expression is not linear in the induction variable is correctly left unrewritten; existing loop composition and correctness tests pass unchanged.
