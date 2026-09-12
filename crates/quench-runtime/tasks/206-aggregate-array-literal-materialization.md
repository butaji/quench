# 206 — Aggregate array-literal materialization

Status: in_progress

Represent an array literal as one aggregate bytecode after its element expressions have
been evaluated in source order. The operation owns an ordered list of optional source
registers: `Some(register)` is an element and `None` is an elision. Its single semantic
edge clones each source once, builds the dense backing at final capacity, attaches the
realm array prototype, and publishes the array once.

This is a quote-stage representation improvement, not a source-pattern benchmark hack.
All ordinary array literals use the same operation regardless of suite, source location,
length, or value types. Spread remains on the existing unsupported/fallback policy until
it has an explicit iterable effect representation. Allocation-before-element-evaluation
is not observably distinct in the supported language subset: allocation cannot be
observed before publication, and the VM has no WeakRef, finalizer, or recoverable OOM.

Categorically, the element computations compose into a product context and the aggregate
materializer is one morphism from that context to an array value. In Lisp terms the
ordered register recipe is the canonical quoted data; the semantic kernel and later AOT
stencil are derived interpretations of that one fact. Do not duplicate element/property
semantics in a special benchmark helper.

Initial evidence: `reports/task206-splay-residual.txt` records approximately 937k entries
for a general object-literal block containing a ten-element numeric array. The old
lowering executes `NewArray` plus ten `LoadLiteral`/`SetStatic` pairs, including decimal
property-key parsing and repeated backing growth.

Acceptance: holes and trailing elisions have correct length and `undefined` reads;
element expressions retain left-to-right effects and exception behavior; nested and
heap-valued elements retain ownership; all tests and complete V8v7 pass; alternating A/B
shows reduced Splay block work without a complete-suite regression. After this slice,
wire the same aggregate operation to an AOT rustc stencil through an explicit allocation
edge, rather than inventing a second semantic implementation.

