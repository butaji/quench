# 50 — Deployment-mode-scaled specialization budgets

Status: planned

V8/JSC cap polymorphic cache width and inlining depth for shared-memory, many-tab browser deployment. A single-purpose embedding (CLI, server, bundled app) has no such constraint. Make specialization aggressiveness a configurable budget instead of a fixed constant, so this VM can be deliberately more aggressive than a browser JIT would dare in the deployment contexts where that's actually a win.

Concrete steps:
1. Parameterize the bounded-polymorphic cache size in [[31-bounded-polymorphic-guards]] (currently scoped as a fixed 2-4 entries), the inlining eligibility bound in [[20-inline-via-node-composition]], and the vectorization threshold in [[39-vectorized-numeric-stencils]] behind a single `DEEGEN_SPECIALIZATION_BUDGET` setting (small/default/aggressive) rather than separate hardcoded constants.
2. Measure code-size and executable-memory growth at each budget level against the V8v7 suite to characterize the memory/speed tradeoff explicitly.
3. Document which budget level is appropriate for which deployment shape (embedded/constrained vs. long-running server process) so the tradeoff is a deliberate choice, not an accident of whatever constant a given task happened to pick.

Acceptance: the three specialization-bound tasks (31, 20, 39) read from one shared budget configuration instead of independent constants; a measured table exists showing score/memory tradeoff at each budget level; the default budget matches or exceeds current per-task defaults with no regression.
