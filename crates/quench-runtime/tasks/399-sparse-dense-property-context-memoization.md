# 399 — Sparse dense-property context memoization

Status: complete

Memoize the expensive validated context created when a static property produces the dense
array consumed by a composed numeric region. This is the Lisp/functional equation
`context = validate(frame events)`: the immutable region plan is the function, object
shape/backing/prototype identities are its explicit inputs, and a per-activation sparse
memo holds only the derived result. Mutation remains confined to existing object boundaries.

## Canonical representation and laws

```text
CachedRegionContext = RegionPc × ValidatedRegionContext
ActivationCache      = SparseSeq<CachedRegionContext>
reuse(plan, frame, cached) =
    if plan.revalidate(frame, cached) then cached else plan.validate(frame)
```

Only plans with a `DenseArray` property result may cache. Numeric-only and source-array
plans are deliberately rebuilt because their guards are cheaper than cache lookup. A cached
property view retains the owner object and checks owner identity, shape, prototype chain,
slot identity/value, array object identity, dense backing pointer, backing version, length,
packed-number classification, and array prototype identity before reuse. GC root traversal
visits retained property owners and array objects in active and reusable frames.

This is a derived categorical identity morphism over the already-proven context, not a new
semantic execution tier. Both cache hit and miss install the same `ValidatedRegionContext`
expected by the same immutable stencil instance. No runtime count changes which code exists,
and no benchmark or source identity participates in admission.

## Rejected broad cache

The first implementation allocated `Vec<Option<ValidatedRegionContext>>` at bytecode width
for every activation and cached every guard plan. The three-pair screen at
`reports/task397-adjacent-loop-composition/guard-cache-quick-ab/comparison.txt` measured
**-6.37%** aggregate despite Crypto improving 6.09%; Richards, DeltaBlue, RayTrace,
Earley-Boyer, and Splay all regressed materially. It was removed. This failure is retained
as evidence that an algebraically valid memo still needs a cost-appropriate physical
representation.

The replacement is an initially empty sparse vector, populated only for the expensive
dense-property context. Its screen at
`reports/task397-adjacent-loop-composition/sparse-cache-quick-ab/comparison.txt` measured
**+1.51%** aggregate and **+5.01%** Crypto. A 20 ms Crypto diagnostic recorded 146,350
cache hits and 15,331 misses while preserving 754 real guard failures.

## Verification and acceptance

Mutation tests prove a cached context rejects both dense backing/content-class changes and
replacement of the property value, while a fresh validation succeeds after a compatible
replacement. All 152 release tests and the stencil-cooker audit pass.

Task 397 records the combined exact acceptance: **2333.57 -> 2368.66 (+1.50%)**, interval
**[+0.96%, +2.13%]**, at
`reports/task397-adjacent-loop-composition/exact-vs-accepted/comparison.md`. The retained
binary SHA-256 is
`77b0f2b745e35e7f4b9d938cfe42b902022dc1bf77a8ed7830ff94a170241470`.
