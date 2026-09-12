# 351 — Pre-link call recipe and static target reach census

Status: complete

Test a pre-link `CallBindingRecipe` representation and add an exact static-target reach
census. The analysis value owns binding layout, parameter slots, `this` and `arguments`
slots, capture, and `arguments` use. The accepted runtime continues to use the compact
ABI-facing `FunctionCallRecipe`; frame creation and the initial inline classifier consume
that canonical linked value.

This preserves the Lisp/data-first boundary without confusing logical and physical
representation. The initially attempted always-present wrapper changed `DynJitCode`
locality and screened at -0.71% aggregate, with Richards at -3.13%. A compacted wrapper
still screened at -0.60%. Both were rejected. The analysis recipe is therefore compiled
only for tests/census; the runtime ABI record and its backing storage keep their prior
memory layout. No execution policy or hotness threshold is introduced.

The `inline-census` feature also performs block-local origin analysis before linking.
It recognizes a callee loaded from an outer binding, resolves that binding without
executing JS, records the exact raw function identity and whether the target closure's
outer environment is pointer-identical to the caller's outer environment, and attaches
the classification to the existing call site by `(source_id, span_start)`. The runtime
census intersects that static fact with the canonical inline decision. The default
binary contains none of the resolution table or counters.

One 300 ms V8v7 pass produced:

| Suite | Compiled call sites | Resolved user sites | Compatible sites | Exact executions | Compatible executions | Initially eligible + compatible executions |
|---|---:|---:|---:|---:|---:|---:|
| Richards | 73 | 1 | 1 | 0 | 0 | 0 |
| DeltaBlue | 177 | 3 | 3 | 158 | 158 | 0 |
| Crypto | 448 | 68 | 68 | 321,011 | 321,011 | 0 |
| RayTrace | 133 | 1 | 1 | 0 | 0 | 0 |
| Earley-Boyer | 540 | 191 | 150 | 2,108,209 | 1,539,559 | 283,051 |
| RegExp | 1,379 | 117 | 117 | 1,724 | 1,724 | 0 |
| Splay | 71 | 5 | 5 | 87,121 | 87,121 | 0 |
| Navier-Stokes | 83 | 30 | 30 | 2,136 | 2,136 | 0 |

No observed exact direct-binding target changed identity during the run. The result
rejects two over-broad hypotheses:

- Task 349's zero reach applies only to same-owner hoisted leaf targets, not to static
  direct bindings in general.
- A leaf-only inliner is not the main cross-suite answer. Its safe general reach is
  concentrated in 283,051 Earley-Boyer calls. Crypto and Splay expose exact targets but
  require nested-call/control-flow composition; Richards and RayTrace are dominated by
  computed property/receiver calls.

The next call work should therefore be staged as a general recipe algebra:

1. direct-binding `GuardExactValue ; InlineBody ; ReturnContinuation` for the bounded
   Earley slice;
2. property-call `ShapeGuard ; LoadFunctionSlot ; GuardExactValue` target recipes for
   Richards/RayTrace/DeltaBlue;
3. hierarchical call-body composition so nested calls remain morphisms instead of
   excluding Crypto/Splay targets.

Evidence is in `reports/task351-static-call-target-reach/*.txt`. Focused release tests
cover immutable recipes, origin propagation/invalidation, and the canonical inline
classifier.

The final default binary is `a02330525ba32d3d371027349f5029c75a5120c9f966644783c66053bafc9688`.
Its three-pair 300 ms comparison at
`reports/task351-feature-only-census-full-ab-3/comparison.txt` measures the frozen
baseline at 2285.23 and the accepted default at 2292.60 (+0.32%, noise-level parity).
