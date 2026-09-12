# 07 — Immutable hidden classes and fixed property slots

Status: in_progress

Replace string-keyed property lookup as the canonical object representation with an immutable shared shape plus a fixed `Vec<Value>` slot array. A shape owns stable property-name-to-offset metadata and transitions; an object owns only its current shape identity and values.

Shapes are kernels in the memory model: immutable and shared across all objects with the same layout. Adding a property follows or creates a transition. Deletion and dictionary-like mutation have an explicit general path rather than corrupting slot invariants.

Acceptance: shape transition/property-order/prototype correctness tests, measured hit rates, and Richards/DeltaBlue A/B results.

Current state: ordinary objects now use an immutable shared `Shape` and fixed `Vec<Value>` slots. Shape construction is canonical by ordered key sequence, including deletion transitions. Tests cover sharing, independent slot values, transitions, and returning to an existing shape. Function-owned properties remain separate and need evaluation.

Measurement: `reports/shapes-ab/comparison.txt` shows Richards +6.34% and DeltaBlue +2.20%, but aggregate −0.81% in a noisy three-run comparison. Keep as architectural groundwork; do not claim a suite-wide win yet.

Task 150 completes the transition-DAG execution path: repeated property additions and
removals now follow cached `(parent shape, key)` edges instead of cloning and hashing the
complete layout. Its confirmed full-suite aggregate improves 7.06%, with double-digit
gains in RayTrace, Earley-Boyer, and Splay. Atom IDs and function-property unification
remain open parts of this broader task.
