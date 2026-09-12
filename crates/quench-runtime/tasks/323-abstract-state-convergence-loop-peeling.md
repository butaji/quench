# 323 — Abstract-state convergence loop peeling

Status: planned

Use [[171-quoted-ssa-cfg-region-normal-form]] to simulate a bounded number of loop
iterations in the abstract domain from [[214]]. Peel the smallest prefix for which the
loop-header context becomes more precise and then reaches a stable fixpoint. Typical
general cases are first-iteration initialization, one-time shape/element-kind
transition, and phis that become constants or narrow representations after a few
iterations.

The output is ordinary quoted CFG data:

`preheader ; peeled_iteration^n ; residual_loop`

Both peeled and residual bodies lower through the same micro-op functor and costed
stencil cover. This is a finite composition/fold, not a special loop runtime or a new
stencil kind. Reject peeling when the loop may execute fewer than the peel count unless
guards preserve zero/short-trip behavior, when throwing/allocating effects would be
duplicated unsafely, when facts do not strengthen, or when the named copied-byte and
register-pressure budgets fail.

Acceptance: zero/one/many-trip, early-exit, exception, shape-transition, overflow,
minus-zero, NaN, and mutation tests pass; diagnostics name the fact made invariant and
the chosen peel count; residual-loop guards or phis measurably decline; complete V8v7
alternating A/B improves.

Primary source: LLVM's loop peeling implementation and its phi-stabilization example,
<https://llvm.org/doxygen/LoopPeel_8cpp_source.html>.

