# 64 — Lock-free parallel composition for provably commutative sub-morphisms

Status: planned

[[40-parallel-ahead-of-link-compilation]] only parallelizes linking of independent functions. Associativity (proven in [[01-stencil-category-core]]) permits reordering composition but not necessarily *concurrent, uncoordinated* construction — that additionally requires commutativity, a distinct and stronger property. Following the CRDT/MapReduce precedent (an operation proven associative *and* commutative — a commutative monoid — can be applied in any order on any thread and reconciled without coordination), identify which specific stencil composition steps are provably commutative, not merely associative, and make exactly those eligible for lock-free parallel construction.

Concrete steps:
1. Audit composition sites in [[17-seq-flatten-linear-link]]'s flattening and [[23-egraph-rewriting]]'s equivalence-class construction to identify which sub-compositions are commutative (e.g. independent sibling opcode stencils within a block with no data dependency between them) versus merely associative-but-order-sensitive (e.g. sequential control flow, where reordering changes observable behavior even though regrouping doesn't).
2. For the commutative subset only, allow construction across multiple threads writing into disjoint parts of the same `StencilNode` tree without a lock, reconciled by simple structural merge (since commutativity guarantees the merge order doesn't matter).
3. Keep this strictly opt-in per identified-commutative site — never assume commutativity from associativity, since conflating the two is the exact class of bug this task exists to avoid.

Acceptance: at least one real composition site (e.g. independent-block construction within a function) is identified as commutative with a proof distinct from its associativity proof, and is verified to run correctly under concurrent construction without a lock; a site that is associative but not commutative is explicitly documented as ineligible, with the distinguishing argument stated; no regression in single-threaded construction correctness.
