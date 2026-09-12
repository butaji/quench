# 321 — Fresh-allocation write-barrier elision and clustering

Status: planned

Derive write-barrier obligations from [[173-effect-token-memoryssa]] and [[172-ownership-ssa-for-value-flow]].
A store into the most recent young allocation requires no generational barrier while no
safepoint or unknown effect intervenes. Immediate/non-heap and read-only kernel-root
values also require none. Remaining stores to the same destination object within one
effect region share one remembered-set operation when intervening effects cannot expose
or collect the object.

Model this as an obligation reducer over quoted effects:

`AllocateYoung(o) ; Store(o,a) ; Store(o,b) ; Publish(o)`

becomes initializing stores followed by publication, with zero barriers. For an old or
unknown-age object, a cluster of stores emits one guarded remember operation plus the
stores. The reducer preserves effect order; it never moves a store across calls,
allocation, throw, prototype mutation, or safepoints.

The emitted fast path is a small cooked stencil and the remembered-set insertion is a
shared cold kernel. Debug builds verify every elided barrier against the heap age and
remembered-set invariants.

Acceptance: initializing-store, intervening-GC, alias, phi, exception, old-to-young,
young-to-young, and immediate-value tests pass; generated constructor/object-literal
regions show zero initializing barriers; barrier counters drop on object-heavy suites;
complete alternating A/B improves.

Primary sources:
<https://chromium.googlesource.com/v8/v8/+/HEAD/src/heap/WRITE_BARRIER.md> and
<https://github.com/WebKit/WebKit/blob/main/Source/JavaScriptCore/dfg/DFGPlan.cpp>.

