# 65 — Composed property-access lens for chained access

Status: planned

[[08-property-inline-caches]] and [[31-bounded-polymorphic-guards]] cache each property-access level independently, so a chain like `a.b.c.d` (common in prototype-heavy JS — deltablue's `inheritsFrom` chains are exactly this shape) pays per-level guard/dispatch overhead even when every level is stable. Model a shape-guarded property access as a lens: a composable get/set pair satisfying the standard optic laws (get-put: writing back what you read is a no-op; put-get: reading after writing returns what you wrote; put-put: writing twice keeps only the last write) — this is a categorical structure (profunctor optics) already used in production functional codebases for exactly this composable-nested-mutation problem.

Concrete steps:
1. Define a property-access lens type over the existing shape-guarded IC representation from [[08-property-inline-caches]], stating and testing the three optic laws against it the same way [[01-stencil-category-core]] tests category laws.
2. Define lens composition: two lenses (`a→b` and `b→c`) compose into one lens (`a→c`) with a single combined guard (both shapes stable) and a single combined slot-offset computation, rather than two independently-dispatched IC lookups chained together.
3. Wire chain-shaped property access (`a.b.c.d`) in the compiler to build one composed lens per chain rather than one IC per `.` in the source, guarded once at the chain's entry and falling back to per-level generic access only where the composed guard fails.

Acceptance: a 3+ level property chain with stable shapes at every level compiles to one guard check and one combined offset computation, verified by instruction-count comparison against today's per-level IC chain; the three lens laws are tested and hold for the composed lens exactly as for each individual level; a chain where an intermediate level is polymorphic correctly falls back to per-level access at that point rather than failing the whole chain's fast path.
