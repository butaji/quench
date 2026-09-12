# 155 — Monotone runtime-invariant fuses

Status: planned

Represent stable runtime facts such as unmodified builtin/prototype relationships as
one-way cells: `Intact -> Popped`. Composite fuses are conjunction nodes over primitive
fuses. A stencil may guard the composite cell once at region entry, or register its
instance as a dependent so popping invalidates/relinks it to the generic stencil.

The state transition is monotone and effects are edge-confined: every mutation capable
of breaking a fact owns the corresponding pop operation. A popped fuse never silently
becomes intact, avoiding temporal unsoundness. Kernels are immutable; only dependency
metadata and patchable instances change.

First uses: packed-array iteration assumptions, canonical `Array.prototype` methods,
and prototype-chain absence proofs. Do not add a fuse until all invalidating mutations
are enumerated and tested. No hotness or execution count participates.

Model the primitive event vocabulary after JavaScriptCore's actual dependency classes:
`ShapeTransitioned(shape)`, `PropertyReplaced(shape, key)`, and
`GlobalBindingMutated(cell)`. Derived promises such as
`PrototypeMethodConstant(receiver_shape, key, callee)` are conjunctions over those
events. A successful method-call stencil can therefore check the receiver shape and
transfer directly to the callee; it does not repeatedly walk the prototype chain or
reload and re-check the function. Each mutating semantic operation owns the only event
emission capable of popping its promise, keeping invalidation at the effect edge.

Acceptance: one-byte/composite checks replace repeated prototype walks; mutation tests
pop every relevant fuse and route existing instances safely to generic semantics;
dependency memory is bounded by named constants; full V8v7 A/B passes.

Primary sources: <https://firefox-source-docs.mozilla.org/js/how-we-optimize.html> and
JavaScriptCore's structure-transition, property-replacement, and global-variable
watchpoints: <https://webkit.org/blog/10308/speculation-in-javascriptcore/>.
