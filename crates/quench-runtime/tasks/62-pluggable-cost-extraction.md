# 62 — Pluggable cost-based extraction, separated from rewrite-rule construction

Status: planned

[[23-egraph-rewriting]] currently couples equivalence-class construction (applying rewrite rules) and cost-based extraction (picking the cheapest representative) into one pass. Following the Cascades query-optimizer architecture (rule set, cost model, and search strategy as three independent components), split extraction into a pluggable module that accepts the already-built equivalence classes and a swappable cost function.

This matters concretely once [[55-architecture-target-matrix]] introduces a second backend: the same proven equivalence classes (built once, independent of target) can be re-extracted with an AArch64 cost model or an x86-64 cost model ([[56-x86-64-sysv-backend]]) without re-deriving or re-verifying any rewrite rule — only the extraction step's cost function changes per architecture.

Concrete steps:
1. Define a minimal cost-function interface (e.g. `fn cost(&StencilNode) -> Cost`) that extraction calls against, rather than a hardcoded cost heuristic baked into the rewrite pass.
2. Provide at least two concrete cost functions to prove the abstraction is real, not aspirational: a code-size cost model and an instruction-count/latency cost model (informed by [[54-llvm-capability-catalog]]'s per-architecture instruction data).
3. Verify the same equivalence classes, extracted under each cost function, produce different but each individually valid outputs — confirming the separation is genuine, not cosmetic.

Acceptance: rewrite-rule construction and extraction are independently testable modules with no cost-model logic inside the rule-application code; switching the cost function changes extraction's output without touching or re-running rule construction; an architecture-specific cost function (per [[55-architecture-target-matrix]]) can be swapped in without any change to [[23-egraph-rewriting]]'s equivalence-class logic.
