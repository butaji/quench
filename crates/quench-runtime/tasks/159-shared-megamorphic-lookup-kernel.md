# 159 — Shared megamorphic lookup kernel

Status: planned

After a site's bounded inline cases are exhausted, route it to an immutable shared
kernel backed by a cache keyed by stable identities such as `(ShapeId, AtomId, AccessKind)`
or `(CalleeId, CallAbi)`. Cache entries contain slot/target recipes, not owned JS values,
and obey shape/prototype invalidation. The per-site stencil instance contains only a
reference and continuation patch, so megamorphic behavior does not multiply code memory.

Use a bounded open-addressed table with named capacity/load constants and a collision-
safe identity check. The generic semantic kernel remains the final miss path. Transition
to this state is caused by distinct-case overflow, not execution frequency.

The first table experiment should copy V8's bounded two-probe policy rather than adding
an unbounded probing loop to every access: a primary direct-mapped table keyed by
`(ShapeId, AtomId, AccessKind)` and a differently hashed smaller secondary table. On
replacement, move the primary victim to secondary and overwrite the selected secondary
entry. Each entry stores only stable keys plus a shared handler/slot recipe; prototype
handlers retain their own validity guard. Compare this against the existing generic
lookup with named table sizes before considering a Swiss-table IC path.

Acceptance: deliberately megamorphic get/set/call tests remain correct across mutation;
code memory stays bounded as distinct shapes grow; lookup avoids string hashing on cache
hits; monomorphic and folded-shape cases do not regress; full V8v7 A/B passes.

Primary sources: <https://firefox-source-docs.mozilla.org/js/cacheir.html>,
<https://webkit.org/blog/10308/speculation-in-javascriptcore/>, and V8's two-level
megamorphic stub cache
<https://chromium.googlesource.com/v8/v8/+/0f581e4b99ee923e7ebae72e64ee58999ff74b5d/src/ic/stub-cache.h>.
