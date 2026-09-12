# 199 — Copy-on-write array-literal kernels

Status: planned

Store eligible array-literal backing values once as an immutable kernel. Each evaluation
creates a distinct array header referencing that backing; the first indexed mutation,
length change, element-kind transition, or escaping mutable view detaches into owned
storage. Object identity remains distinct even while elements are shared.

The literal recipe contains element representation and immutable backing identity;
instantiation and detach are two compatible morphisms. Only data-property literals whose
elements are immutable kernel values qualify. Spreads, accessors, holes with observable
prototype behavior, and effectful expressions use ordinary construction.

Acceptance: aliasing and all mutating array methods detach correctly; repeated literals
share backing but not identity/properties; eligible evaluation copies zero element words;
allocation/copy counters justify selection; full V8v7 A/B does not regress.

Primary source: JSC's copy-on-write int32, double, and contiguous indexing variants,
<https://github.com/WebKit/WebKit/blob/main/Source/JavaScriptCore/runtime/IndexingType.h>.

