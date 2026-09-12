# 40 — Parallel, speculative ahead-of-link compilation

Status: planned

`image: Rc<OnceCell<MaterializedStencil>>` makes linking a pure, referentially transparent function of AST identity ([[01-stencil-category-core]], [[14-code-image-reuse]]) — purity is exactly what makes it safe to compute a function's image before it is first called, on a background thread pool, keyed off the same immutable-OXC-identity interning already used for function images. For call-graph-heavy benchmarks (deltablue, earley-boyer) this hides link latency behind other work instead of paying it synchronously on the first call to every function.

Scope this as speculative and non-blocking: a call that arrives before the background link completes falls back to synchronous on-demand linking exactly as today, so this is strictly additive and cannot change program semantics or ordering, only when the `OnceCell` happens to already be filled.

Acceptance: a call-graph-heavy program shows a measured reduction in aggregate synchronous link time via [[05-performance-harness]]; a function called before its background link completes still executes correctly via the existing synchronous path with no duplicate or racing link of the same `OnceCell`; no change to output on any existing correctness test.

Note on framing: referential transparency is a real and correctly identified enabling property, but the proposal itself (background-compile before first call on a thread pool) is a standard AOT/JIT technique independent of the category structure — it would be designed identically without the Stencil category existing. Do not cite this task as evidence of categorical payoff beyond "purity makes parallelization safe," which is true but modest.
