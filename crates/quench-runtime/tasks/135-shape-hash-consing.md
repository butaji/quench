# 135 — Structural hash-consing of hidden-class shapes

Status: complete

## Audit result

The proposed mechanism was already present before this task was written, so no runtime
change is warranted. `ShapeRegistry::by_keys` is a thread-local structural interning table
keyed by the complete ordered `Vec<String>` layout. `ShapeRegistry::intern` returns the
existing `Rc<Shape>` on a hit, and every property insertion/removal transition calls
`intern_shape`. The existing `property_shapes_are_shared_and_transitions_are_canonical`
test verifies root sharing, independent `x` transitions sharing by `Rc::ptr_eq`, and
removing `y` returning to the original canonical `x` shape.

The current `Shape` intentionally excludes prototype identity: prototypes live on
`Object`, while `PropertyIcSite` separately guards receiver shape plus prototype-object
identity and every prototype shape in the chain. Adding prototype identity to the own
property layout key would duplicate that existing guard structure and reduce sharing.

No code or benchmark A/B was needed because the requested property is already an
invariant of the accepted implementation. The stale premise is retained below as the
historical proposal.

Follow-up: [[150-cached-shape-transition-edges]] preserves this canonical full-layout
interner while adding direct parent/key transition edges. This removes repeated complete
layout hashing on established transitions and improves the confirmed aggregate 7.06%.

[[07-hidden-classes]] gives each object an immutable shared `Shape`, but two objects
that reach the same property layout via independent transition paths (e.g. two object
literals with the same keys in the same order written at different source locations, or
the same literal shape constructed inside two different loop iterations before either
existed) currently allocate two structurally identical `Shape` values rather than
sharing one. QuickJS interns shapes globally instead: `quickjs.c`'s `JSRuntime.shape_hash`
table (`init_shape_hash`/`resize_shape_hash`/`get_shape_hash`, `quickjs.c:5132-5179`) hashes
a shape by its ordered property-atom sequence plus prototype pointer, and every shape
transition first probes this table for an existing match before allocating a new
`JSShape`. The payoff is twofold: shape *identity* comparison for a guard becomes a
single pointer compare regardless of how the shape was reached, and the number of live
shape allocations for programs with many structurally-similar object literals (a common
pattern — config objects, records, point-like literals in loops) is bounded by the
number of distinct layouts, not the number of construction sites or iterations.

Add the same interning to `Shape` construction: a runtime-wide table keyed by a
structural hash over the shape's ordered `(key, transition-parent)` sequence plus
prototype identity, consulted on every transition (property add, in particular the
literal-construction path exercised by [[98-inline-literal-construction]] and
[[101-immediate-literal-construction]]) before allocating a new `Shape`. This is
independent of [[42-whole-program-hash-consing]], which interns `StencilNode` code
shapes; this task interns runtime *data* shapes. A hash-consed shape is immutable for
the remainder of the program the same way a hash-consed `StencilNode` is under [[42]]
— no later transition may mutate a shared `Shape` in place, only produce a new
transition target.

Acceptance: two object literals with the same key order constructed at different call
sites (and the same literal constructed on two different loop iterations) share one
`Shape` by pointer identity after construction, verified directly; a guard comparing
shape identity ([[08-property-inline-caches]], [[31-bounded-polymorphic-guards]]) reduces
to one pointer compare regardless of construction path; live `Shape` allocation count on
a shape-repetitive workload (object-literal-heavy or record-heavy source, similar in
spirit to earley-boyer's dispatch-idiom repetition noted in [[42]]) decreases relative to
the non-interned baseline; existing shape transition/property-order/prototype
correctness tests continue to pass; alternating A/B shows no regression before this
becomes the default path.
