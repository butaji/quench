# 48 — Sub-morphism-scoped re-specialization on guard failure

Status: planned

Replace whole-function deopt-and-recompile with patching just the failed guard's `StencilNode` subtree, exploiting the fact that a `Stencil` is a quoted, independently-recomposable tree (`StencilNode::Seq`) rather than a monolithic compiled unit.

Concrete steps:
1. Identify the guard-failure entry point already required by [[25-generalized-speculative-guards]]/[[31-bounded-polymorphic-guards]] and change its bail-out target from "recompile whole function" to "recompose only the guarded node with a widened bounded-polymorphic cache or the generic fallback."
2. Ensure the surrounding `Seq` composition ([[17-seq-flatten-linear-link]]) supports replacing one child node's materialized image without invalidating sibling nodes' already-linked `Rc<StencilInstance>`s (via [[16-kernel-dedup-by-identity]]'s reference-sharing model).
3. Add a counter distinguishing "sub-morphism patch" from "full function recompile" so regressions toward the coarse-grained path are visible in the coverage/perf reports ([[04-bytecode-coverage-map]], [[05-performance-harness]]).

Correctness constraint (cross-checked against [[16-kernel-dedup-by-identity]] and [[42-whole-program-hash-consing]]): both of those tasks intentionally make one `Rc<StencilNode>`/`Rc<Kernel<_,_>>` shared across multiple unrelated composition sites. A guard-miss patch must **never mutate a shared node in place** — doing so would silently repatch every other site sharing that reference, which is a correctness bug, not a missed optimization. The patch must always be copy-on-write against the hash-cons/kernel-dedup table: allocate a new node for the patched site's local composition, leave the original shared `Rc` and every other referent of it untouched, and only that one call site's `Seq` is updated to point at the new node. [[42-whole-program-hash-consing]] must likewise treat any node it has shared as immutable-forever once shared, never patched in place, so this constraint is symmetric across both tasks.

Acceptance: a guard failure inside a large function re-links only the affected node, verified by the new counter and by confirming unrelated sibling stencils' `Rc` addresses are unchanged after the patch; a guard failure at a hash-consed or kernel-deduped site produces a new node/kernel reference at that one site while every other site sharing the original reference is unaffected and unrelinked; a polymorphic-churn microbenchmark (repeated guard failure/re-guard cycling) shows lower aggregate re-link cost than whole-function recompilation on the same workload.

## Prepatched immutable alternatives

Do not make an executable `StencilInstance` self-modifying. For every finite specialization
family selected for a site, instantiate and patch the candidate images before publication,
store them in an immutable `SpecializationSet`, and let the site's edge-confined dispatch
cell point at the selected member. First observation may atomically publish a different
member, but it may not rewrite bytes in a shared instance. This preserves the project's
`StencilTemplate -> StencilInstance` and shared-`Kernel` memory model while importing the
useful part of runtime stencil specialization: selection is a pointer publication, not a
new patch/link pass.

If all alternatives have the same connector contract, switching members is categorical
substitution of one morphism for another with identical domain and codomain. If they do
not, the replacement is illegal rather than repaired by an adapter. Bound every set with
the named `MAX_PREPATCHED_SPECIALIZATIONS_PER_SITE`; overflow selects the canonical generic
stencil continuation. Count candidate bytes and dispatch-cell changes separately so a
speedup cannot hide unbounded code growth.

Primary-source motivation: the 2025 copy-and-patch JIT for R pre-patches type-specialized
variants and installs a selected variant after observing types, reporting high compilation
throughput and speedups over GNU R. This adaptation deliberately changes its in-place code
mutation into immutable-instance selection because instances can be shared in this VM:
<https://fikovnik.net/publications/vmil25.pdf>.
