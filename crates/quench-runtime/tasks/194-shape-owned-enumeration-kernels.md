# 194 — Shape-owned enumeration kernels

Status: planned

Attach a shared immutable enumerable-atom list to each eligible shape. `ForInInit`
borrows it when receiver/prototype validity matches; `ForInNext` returns atom references
without allocating strings. Indexed elements form a separately composed prefix.
Dictionary, deletion/prototype mutation, symbols, accessors, and future proxy semantics
use one configurable slow `KeyAccumulator` kernel.

The current path allocates `Vec<String>`, clones every property name, formats every array
index, and allocates another `Rc<String>` per yielded key. The new enumeration kernel is
derived entirely from shape descriptors and composes with any loop body.

Acceptance: enumeration order, inherited properties, non-enumerable shadowing, mutation
during enumeration, deletion, indices, and symbols pass; stable-shape iteration allocates
no key vector or yielded strings; dedicated counters must show material workload before
performance acceptance; full V8v7 A/B does not regress.

Primary source: V8's EnumCache and KeyAccumulator design,
<https://v8.dev/blog/fast-for-in>.

