# 90 — Array higher-order method fusion (map/filter/forEach/reduce)

Status: planned

`arr.map(f).filter(g).reduce(h)`-shaped chains are idiomatic JS and, executed literally,
allocate an intermediate array per stage and pay a closure-call ([[35]]) per element per
stage. [[35-closure-callback-call-caching]] removes the per-call resolution cost but not
the intermediate-allocation or multi-pass cost; [[24-escape-analysis-scalar-replacement]]
and [[33-allocation-site-escape-analysis]] can eliminate the intermediate array's heap
allocation only if fusion has already turned the chain into one pass with no observable
escape.

Recognize a static chain of `Array.prototype.map`/`filter`/`forEach`/`reduce`/`some`/
`every` calls at a call site where the receiver is guarded to a stable dense
element-kind array ([[32]]) and every callback in the chain is guarded stable
([[35]]), and compose the chain into a single native loop stencil: one index-driven
pass over the source array's elements, inlining each stage's callback body (or, when a
callback body cannot be inlined, calling it in place) and threading the fused-per-stage
value directly to the next stage or the accumulator, materializing an output array (for
`map`/`filter`) only where the chain's result is actually consumed. This is a
composition of stencils, not a special-cased pattern-matcher on method names: each
stage is a `Stencil<ElemState, ElemState>` morphism and the chain compiles to their
sequential composition per [[01-stencil-category-core]].

Acceptance: a static `map`/`filter`/`reduce` chain over a guarded dense array compiles
to one loop with no intermediate array allocation when its result is immediately
consumed (verified by an allocation counter), and correctly falls back to per-stage
generic calls when a receiver or callback guard misses mid-chain; alternating A/B on
suites using array chaining shows a measured gain.
