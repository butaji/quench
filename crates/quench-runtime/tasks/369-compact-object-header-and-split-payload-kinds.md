# 369 — Compact object header and split payload kinds

Status: planned

Measure and reduce the physical footprint of ordinary objects after total native cover is
available. Replace the one-size-fits-all `ObjectCell` payload with a compact common header
and explicit payload kinds for ordinary objects, arrays, functions and other exceptional
objects. Pack a 32-bit `ShapeId`, kind and mark/flags fields; give allocation-site-proven
ordinary shapes a named small number of inline `RawValue` slots; allocate out-of-line
property storage only on overflow. Array elements remain separate and do not inflate every
ordinary object.

Derive layout from one macro-defined schema shared by runtime, tracing and AOT stencil
views. The shape determines pointer maps, inline-slot count and out-of-line capacity.
Release native IC paths must not carry a dynamic borrow-state check. Slow Rust views may
validate ownership at their boundary but cannot change offsets or duplicate layout facts.
All header/slot sizes, alignments, inline capacities and flags are named constants with
compile-time/runtime assertions.

Preflight records current allocation counts, object-kind distribution, median property
count, bytes allocated per suite iteration, cache misses where available, and the exact
native load/store sequence. First candidate separates ordinary and array payloads and adds
bounded inline slots; pointer compression is explicitly out of scope. Acceptance requires
GC/shape/transition/array stress tests, lower bytes and indirections on the targeted path,
complete V8v7 correctness and alternating A/B improvement.

Primary sources: JSC object/structure representation
<https://webkit.org/blog/10308/speculation-in-javascriptcore/>, V8 named-property/elements
separation <https://v8.dev/blog/fast-properties>, and V8 instance-size slack tracking
<https://v8.dev/blog/slack-tracking>.
