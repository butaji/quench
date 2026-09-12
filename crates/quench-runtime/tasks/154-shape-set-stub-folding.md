# 154 — Shape-set stub folding and minimorphic loads

Status: planned

When multiple IC cases have the same quoted recipe and property offset but differ only
in receiver shape, normalize them into one case:

`GuardShape(S1); LoadSlot(K)` + `GuardShape(S2); LoadSlot(K)`
`=> GuardShapeSet({S1,S2}); LoadSlot(K)`

This is a semantics-preserving rewrite over Task 153 data. The set is immutable,
hash-consed, bounded by a named maximum, and order-normalized so folding is associative
and independent of observation order. Same-offset cases share one result operation;
different offsets remain a bounded PIC or transition to Task 159's shared megamorphic
kernel. Case transitions depend on new shapes, never heat counters.

Acceptance: law/property tests cover order independence, duplicate elimination, and
miss equivalence; disassembly shows one slot load after the folded guard; IC code and
data size decline on polymorphic object suites; full V8v7 A/B passes.

Primary sources: <https://firefox-source-docs.mozilla.org/js/cacheir.html> and
<https://webkit.org/blog/10308/speculation-in-javascriptcore/>.

