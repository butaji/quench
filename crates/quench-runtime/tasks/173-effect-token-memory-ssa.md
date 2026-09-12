# 173 — Effect-token MemorySSA and load/store elimination

Status: planned

Give every heap observation an explicit immutable location key such as
`Property(shape, slot)`, `DenseElements(backing)`, `Binding(environment, slot)`, or
`UnknownHeap`. Loads consume an effect token; stores, allocation, prototype mutation,
and calls produce a new token. Merge points use memory phis. A bounded cached clobber
walk answers whether a previous load or store reaches the current operation unchanged.

This is a deliberately small JS-specific analogue of LLVM MemorySSA, not a general alias
analysis framework. Shape guards, monotone fuses, array-backing guards, and Task 177
function summaries refine locations; unknown effects conservatively clobber the relevant
domain. Proven cases enable load CSE, store-to-load forwarding, and dead-store removal
before stencil tiling.

Also support demand-driven load sinking in the same forward CFG pass: if a load is used
only in one successor and no intervening operation may alias its location key, clone/move
the load into that successor before tiling. Do not create a separate global scheduler.
V8's CFG experience shows why: source-order effect chains otherwise force loads needed by
different branch arms to execute before the branch, while a block-ordered representation
makes the legal placement explicit.

Effects remain first-class categorical obligations: a pure stencil morphism preserves
the token, while an effectful kernel returns a successor token. The token is quoted
analysis data and emits no runtime object.

Acceptance: aliasing/prototype/call/exception tests prevent unsound reuse; diagnostics
name the reaching definition and eliminated operation; a repeated property/dense-load
microbenchmark removes the second machine load; Richards/DeltaBlue/RayTrace counters and
complete V8v7 A/B improve. Alias-walk and phi budgets are named constants.

Sources: <https://www.llvm.org/docs/MemorySSA.html> and
<https://v8.dev/blog/leaving-the-sea-of-nodes>.
