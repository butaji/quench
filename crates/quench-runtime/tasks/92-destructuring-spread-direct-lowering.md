# 92 — Direct stencil lowering for destructuring and spread

Status: planned

Array/object destructuring (`const [a, b] = arr`, `const {x, y} = obj`) and spread in
call/array/object position (`f(...args)`, `[...a, ...b]`, `{...obj}`) are common
in modern JS source and, naively lowered, desugar to a sequence of generic
property/iterator operations at every use site rather than to direct stencil-native
slot moves.

Lower each destructuring/spread form directly to the stencils that already exist for
its guarded fast case instead of through a generic desugaring:
- array destructuring with a guarded dense source array lowers to direct indexed loads
  ([[32-element-kind-guarded-arrays]]), not the iterator protocol from [[88]];
- object destructuring with a guarded shape lowers to direct slot loads via the same
  connector [[08-property-inline-caches]] uses for `a.b`, not a generic per-key lookup;
- array spread of a guarded dense array lowers to a bulk element copy, not a per-element
  push through the generic array-append path;
- object spread of a guarded-shape source lowers to a bulk slot copy into the target's
  shape-transition path from [[07-hidden-classes]], not a per-key set.

Each of these is a case of composing an existing guarded connector with a bulk/direct
data-movement stencil rather than introducing new runtime machinery — the value of this
task is closing the gap between "the fast primitive exists" and "the syntax that most
naturally produces this pattern reaches it."

Acceptance: destructuring/spread over guarded-shape/guarded-array sources compiles to
direct slot/index operations with no iterator-protocol or generic per-key dispatch,
verified by instruction-count and call-count comparison against the naive desugaring;
polymorphic/unguarded sources correctly fall back to the fully generic path;
alternating A/B on suites using destructuring/spread shows a measured gain.
