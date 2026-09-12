# 198 — Read-only kernel heap and static root table

Status: planned

Place immutable shapes, atoms, built-in descriptors, literal kernels, and canonical heap
values in a sealed arena excluded from tracing. Give every item a deterministic root-table
slot; stencil instances load it relative to the fixed VM/kernel base rather than owning
or retaining another reference. References from mutable objects to this heap require no
write barrier.

The table is the single representation of kernel identity. Layout generation, Rust
constants, linker metadata, and GC scanning rules are derived from one build artifact and
versioned with a named ABI version. This complements Task 189: the read-only heap stores
data identity while the near-code arena stores executable kernels.

The layout builder may cluster immutable roots/shapes that share a semantic predicate so
a multi-case test becomes one cage-relative unsigned range check. Apply this only when
the existing NaN-box tag cannot already express the predicate in one compare and when
disassembly proves the range replaces multiple loads/comparisons. Cluster membership is
generated from the same root manifest; handwritten numeric address ranges are forbidden.

Acceptance: roots are immutable and excluded from tracing; deterministic offsets are
compile-time/runtime asserted; no per-function copies exist; snapshot/ABI mismatch fails
closed; root-relative loads and barrier removal are proven by disassembly/counters; full
V8v7 A/B does not regress.

Primary sources: V8 static roots <https://v8.dev/blog/static-roots> and embedded built-ins
<https://v8.dev/blog/embedded-builtins>.
