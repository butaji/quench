# 69 — Whole-program supercompilation via categorical colimit over the call graph

Status: planned

Given closed-world mode ([[46-closed-world-mode]]) makes the whole program available for analysis, apply supercompilation (Turchin) — repeated specialization of a program against its own call structure until reaching a fixpoint — as a more aggressive generalization of [[20-inline-via-node-composition]]'s single-pass inlining. Frame "the optimized program" as a categorical colimit over the call graph: iteratively specialize each hot call chain into one fused stencil, re-examine the specialized result for further specialization opportunities, and stop at the fixpoint where no further specialization changes the term (up to the equivalence already defined by [[23-egraph-rewriting]]/[[66-normal-form-checkpoint]]'s normal form).

This is distinct from plain inlining depth: [[20-inline-via-node-composition]] splices a callee's node tree once at a call site; supercompilation additionally *specializes the spliced result against the specific arguments/context of that call site* (e.g. a callee with a boolean parameter that's always `true` at one call site gets specialized to the `true` branch only, at that site), which can eliminate entire branches inlining alone cannot.

Concrete steps:
1. Scope eligibility conservatively: only call chains where [[46-closed-world-mode]]'s closed-world analysis has already proven argument values or shapes stable at a call site are candidates, avoiding runaway specialization (supercompilation is known to risk code-size blowup without a termination bound).
2. Define the fixpoint termination condition explicitly (bounded specialization depth, or reaching [[66-normal-form-checkpoint]]'s normal form with no further reduction possible) before implementation, not as an afterthought.
3. Measure code-size growth against speed gain per specialization step, since unlike [[20-inline-via-node-composition]]'s bounded inlining, supercompilation's whole point is to go further than a size-capped heuristic would.

Acceptance: at least one call chain in the V8v7 suite (candidate: deltablue's `Planner`/`Constraint` dispatch chain, or richards' scheduler) is fully specialized end-to-end via this mechanism with a documented code-size/speed tradeoff; the termination bound is enforced and tested (a call chain that would specialize unboundedly is caught and capped, not allowed to blow up); results are validated via [[05-performance-harness]] as a genuine aggregate win, not just a synthetic microbenchmark improvement.
