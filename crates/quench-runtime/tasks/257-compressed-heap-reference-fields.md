# 257 — Compressed heap references in representation-proved fields

Status: planned

After the stable VM heap exists, represent pointer-only object fields and shape-proved
reference slots as cage-relative offsets rather than native pointers. Keep the public
NaN-boxed `Value` word and raw F64 storage unchanged: unlike V8's boxed-double value
model, this VM currently benefits from carrying a double directly in one 64-bit word.

Define one immutable `HeapRef32` representation with named `HEAP_CAGE_BYTES`,
`HEAP_CAGE_ALIGNMENT`, `HEAP_REFERENCE_SHIFT`, and null/sentinel encodings. Task 203's
pinned VM/heap register supplies the cage base. Task 171's typed region and Task 173's
memory graph keep a reference compressed across load-to-store flows and insert expansion
only when a dereference or canonical kernel ABI actually requires a native pointer.
Compression before stores is likewise derived, not scattered through property helpers.

Candidate storage includes shape, prototype, environment-parent, function-image, and
shape-typed object-reference slots. Generic `Value` arrays remain 64-bit unless Task
165 proves a homogeneous pointer representation. The GC scans the compressed form
directly and validates cage membership; host pointers and kernel-code pointers never use
`HeapRef32`.

This is a memory/locality experiment, not the next score-critical task. V8 reports up to
43% heap reduction and some CPU/GC improvement from broad pointer compression, but those
numbers do not transfer directly because this VM's direct-double NaN boxing changes the
slot population and tradeoff.

Acceptance: all encodings and offsets are named and compile-time asserted; cage overflow
fails closed; GC, weak/sentinel, and host-boundary tests pass; disassembly proves the
pinned base avoids recomputing cage alignment and load/store chains eliminate redundant
expand/compress pairs; heap bytes and cache-miss counters improve on object-heavy suites;
full V8v7 A/B is non-regressing before default enablement.

Primary source: V8's pointer-compression design, pinned root/cage base, decompression
elimination, and measured tradeoffs: <https://v8.dev/blog/pointer-compression>.
