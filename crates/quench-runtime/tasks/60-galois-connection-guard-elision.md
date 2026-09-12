# 60 — Formalize guard-elision analyses as a Galois connection

Status: planned

[[46-closed-world-mode]] and [[47-compile-time-guard-elision]] currently scope their mutability/closed-world analysis as "build an analysis, test it" — sound only to the extent the test suite happens to cover. Reframe it as an explicit Galois connection (abstract interpretation, Cousot & Cousot): a concrete domain (actual program behavior — every assignment/reassignment a binding can undergo across the whole program) and an abstract domain (a small lattice, e.g. `Open ⊑ Closed` or a shape lattice), connected by an explicit abstraction function `α` and concretization function `γ` satisfying `α(concrete) ⊑ abstract ⟺ concrete ⊑ γ(abstract)`.

This is a stronger correctness argument than task 47 currently has: soundness follows from the connection's monotonicity property (provable once, structurally) rather than from the absence of a counterexample in the test suite so far — the same gap category theory already closes elsewhere in this plan (proven composition laws vs. fuzzed correctness) applies here to static analysis specifically.

Concrete steps:
1. Define the concrete semantic domain precisely: the set of all assignment/reassignment events a binding or class constructor can be subject to across the whole program (per [[46-closed-world-mode]]'s whole-program scope).
2. Define the abstract lattice and the `α`/`γ` pair; prove (by structural induction over the language's assignment forms, not by testing) that the pair forms a Galois connection.
3. Derive the "closed" judgment used by [[47-compile-time-guard-elision]] as exactly "the least upper bound of `α` over all assignment sites is `Closed`" — a mechanical consequence of the connection, not a separately-hand-coded rule.

Acceptance: the abstraction/concretization pair's monotonicity is proven (a short structural argument, checked like the existing category-law tests in [[01-stencil-category-core]]), not merely tested; [[47-compile-time-guard-elision]]'s "closed" judgment is derived from this connection rather than implemented as an independent ad hoc check; a deliberately adversarial reassignment pattern (used only as a soundness self-test, not shipped) is correctly rejected as "open" by construction, not by having been anticipated in a test case.
