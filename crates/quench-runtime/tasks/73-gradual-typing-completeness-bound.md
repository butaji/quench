# 73 — Gradual-typing completeness bound for guard elision

Status: planned

[[60-galois-connection-guard-elision]] establishes the *soundness* half of the closed-world guard-elision analysis (never eliminate a guard that could actually fail). Gradual typing theory (Siek & Taha) supplies the complementary *completeness* question this task set has not yet addressed: how much of JS's dynamism can be statically resolved before hitting a principled stopping point, versus chasing diminishing-return static proofs indefinitely in [[46-closed-world-mode]]/[[47-compile-time-guard-elision]].

Define an explicit completeness criterion — analogous to gradual typing's "gradual guarantee" (a program's behavior degrades predictably, not arbitrarily, as static information decreases) — so [[46-closed-world-mode]]/[[47-compile-time-guard-elision]]'s analysis has a principled place to stop rather than an open-ended "prove more things closed" effort with unclear diminishing returns.

Concrete steps:
1. Characterize the space of JS dynamism [[46-closed-world-mode]]'s closed-world analysis is trying to eliminate (reassignment, prototype mutation, `eval`) as a lattice from "fully static" to "fully dynamic," analogous to gradual typing's type-precision lattice.
2. State a gradual-guarantee-style property: as more of the program is proven closed, guarded-fast-path coverage increases *monotonically* and predictably — never regressing coverage by making an unrelated part of the program more dynamic (i.e., closing one binding never de-optimizes an already-closed one elsewhere).
3. Use this property to define a concrete stopping criterion for [[47-compile-time-guard-elision]]'s analysis effort: once further static proof effort yields sub-threshold additional closed bindings (measured via [[37-aot-template-coverage-by-frequency]]-style frequency data), further investment in the analysis itself is deprioritized in favor of other tasks.

Acceptance: the monotonicity/gradual-guarantee property is proven for [[60-galois-connection-guard-elision]]'s Galois connection, not merely assumed; a documented, evidence-based stopping criterion exists for [[46-closed-world-mode]]/[[47-compile-time-guard-elision]]'s analysis investment, preventing open-ended effort with no defined "done"; a test demonstrates that closing one binding never regresses another already-closed binding's guard elision.
