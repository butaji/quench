# 187 — Multi-entry exact-arity function images

Status: planned

Give one linked function image named entry labels for generic, exact-arity, and
noncapturing calls. The entries contain only distinct prologues and converge on one body;
the body is never duplicated. A monomorphic call recipe with matching arity patches
directly to the exact entry, while default/rest/observable-`arguments` or arity mismatch
uses the generic entry.

This applies the category's multi-entry interface directly: each `EntryTag` has its own
input context but all entries reach the same body context. It extends Tasks 146 and 181
without adding a tier or a hotness decision.

Acceptance: under/over application, defaults, rest, `arguments`, recursion, exceptions,
closures, constructors, and receiver semantics pass; exact-arity sites bypass parameter
count and padding work in disassembly/counters; full V8v7 A/B improves.

Primary sources: JSC's distinct arity-check and normal baseline entries,
<https://github.com/WebKit/WebKit/blob/main/Source/JavaScriptCore/jit/JIT.cpp>, and V8's
single-frame argument layout, <https://v8.dev/blog/adaptor-frame>.

