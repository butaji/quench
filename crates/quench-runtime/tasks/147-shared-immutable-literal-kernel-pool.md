# 147 — Shared immutable literal kernel pool

Status: complete

Generalize Task 142's successful kernel/instance split to every literal-like runtime
artifact. Classify lowered constants structurally:

- immutable and identity-free: share one kernel word directly, such as source string
  literals and interned property atoms;
- immutable implementation plus mutable JS wrapper: share the kernel but allocate the
  wrapper, as with RegExp compiled automata plus `lastIndex`;
- mutable or observably identity-bearing: share only construction metadata, never the
  resulting object, array, function closure, or environment.

Store shared kernels in the immutable linked function image or a structural pool keyed
by semantic identity. A `StencilTemplate` patches a reference to that kernel into each
`StencilInstance`; repeated execution never reconstructs it. A shared instance is valid
only when its external patch obligations and observable identity contract are identical.

Start with ordinary string literals, because `LoadLiteral::String` is immutable yet the
current path can allocate an `Rc<String>` on every execution. Measure allocation and
Earley/Splay impact before adding ropes or a small-string representation from Task 11.

Acceptance: a focused test proves strings share their immutable storage while mutable
literals remain distinct; allocation counters show one backing allocation per literal
kernel rather than per execution; all semantic tests and complete V8v7 A/B pass.

## Result

Rejected and reverted. The first leaf was implemented as a representation change:
`Literal::String` owns the canonical `Rc<String>` kernel in the immutable bytecode and
execution copies that word into `Value::String`. This removes both the per-execution
allocation and the stored duplicate source `String`, while making illegal mutable-literal
sharing inexpressible because object/array literals are different variants.

All 77 release tests and the complete smoke suite passed, including a focused identity
test proving repeated evaluations reused exactly one immutable string allocation. The
four-run, 200 ms alternating comparison in
`reports/task147-string-kernel-full-ab-4/comparison.txt` changed aggregate 1604.13 to
1597.83 (-0.39%). Earley-Boyer improved 1.25%, but six other components were flat or
slower and Navier-Stokes declined 2.79%. The implementation was removed because the
standing gate requires an aggregate improvement, not just semantic elegance.

This evidence rules out site-local `Rc<String>` sharing as a priority. General string
work should proceed through Task 11's atom IDs or a VM heap where copying a shared string
does not itself retain/release an `Rc`, rather than reinstalling this experiment.
