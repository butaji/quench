# 243 — Columnar (struct-of-arrays) storage for homogeneous-shape object collections

Status: planned

Memory-organization gap not covered by [[07]]/[[135]]/[[150]] (shape *identity*) or
[[156-allocation-site-inline-object-storage]] (per-object slot layout): none of these
address the *physical* transposition question — when many live objects share one proven
shape (the exact condition [[07]]'s shape system already establishes and [[135]]'s
hash-consing already deduplicates the shape descriptor for), should their field values
be stored array-of-structs (each object's fields interleaved together, one allocation
per object — today's implicit default) or struct-of-arrays (one contiguous array per
field, shared across every object of that shape)? These are not equivalent at the
memory-access level: AoS is right when a loop reads/writes *all* of an object's fields
together (a single `Vector` used as `v.x + v.y + v.z`); SoA is right when a loop reads
*one* field across *many* objects (`for (const p of particles) sum += p.mass` — the
classic pattern behind physics/particle simulations, and structurally what
navier-stokes's grid-cell arrays already resemble even though they are not currently
object collections).

This is squarely a data-oriented-design question, not a shape-identity question — a
correctly-proven shape ([[07]]) is a *precondition* for this transposition (every object
in the columnar store must share the exact same field layout, which is exactly what a
shared `Shape` already guarantees) but does not by itself decide which physical layout
is faster; that depends on the access pattern at each consuming loop, which is a
property of the *code*, not the *shape*.

Concrete steps:
1. Identify, in the V8v7 corpus, at least one site where a loop iterates many
   same-shape objects reading/writing one or two fields each (a strong SoA candidate) —
   deltablue's constraint-graph traversal and raytrace's per-pixel/per-object color
   accumulation are the most likely candidates given their existing profile presence;
   confirm with an actual grep/read before committing, per this project's standing
   discipline of verifying against real corpus usage rather than assuming.
2. Design a columnar backing store keyed by `(Shape, field)`: one contiguous array per
   field, indexed by a stable per-object slot id, allocated when [[192]]'s allocation-
   site policy observes a stable shape with a stable, large population (name the
   population threshold as an explicit policy constant, consistent with this project's
   "no unexplained numeric threshold" discipline) rather than unconditionally for every
   shape.
3. Give a per-field-access stencil ([[08]]/[[129]]) a columnar-load variant alongside
   its existing per-object slot-load variant, selected by which backing store the
   accessed object's shape is currently using; a single-field loop over a columnar
   collection should read as one contiguous, prefetch-friendly (see
   [[244-loop-data-prefetch-strided-arrays]]) stream, not one pointer-chase per object.
4. State the honest cost side explicitly: an *individual* object read touching many
   fields becomes more expensive in a columnar store (each field is a separate cache
   line/array), so this transposition must never be applied unconditionally — only where
   step 1's access-pattern evidence justifies it, with the existing AoS in-object layout
   remaining the default.

Acceptance: at least one corpus-verified SoA-favorable site (from step 1) is converted
to columnar storage and shows a measured cache-behavior/instruction-count improvement
for its dominant access loop; an AoS-favorable site (multi-field-per-object access) is
confirmed to correctly remain in the existing per-object layout, with a negative test
confirming columnar storage is not applied there; alternating A/B on the converted
site's suite shows a measured gain with zero correctness regression across the full
V8v7 suite.

Primary sources:
- Struct-of-Arrays vs. Array-of-Structs, cache-locality tradeoff: <https://medium.com/@azad217/structure-of-arrays-soa-vs-array-of-structures-aos-in-c-a-deep-dive-into-cache-optimized-13847588232e>
- Mixed AoS/SoA as the actually-optimal answer in practice (motivating this task's
  per-site, not universal, application): general data-oriented-design literature, e.g.
  <https://medium.com/@michaelstebel/thinking-in-data-layout-not-objects-bfef321e083d>
