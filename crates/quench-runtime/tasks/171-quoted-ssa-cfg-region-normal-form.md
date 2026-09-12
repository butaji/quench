# 171 — Quoted SSA/CFG region normal form

Status: in_progress

Introduce one immutable, cold `RegionPlan` for a function or selected region. It contains
basic blocks, block arguments/phis, typed value definitions, explicit control edges,
effect-token edges, ownership uses, and canonical side-exit frame-state identities.
All liveness, dominance, loop, alias, ownership, and cost views are derived from this one
fact representation and are not independently mutable sources of truth.

Build it with a liveness/loop prepass and a single forward abstract interpretation, using
pre-created loop phis for variables assigned inside a loop as described by Maglev. This
refines Task 68: use a cache-friendly CFG rather than migrating to a general Sea of Nodes.
The existing tree/free-monoid `StencilExpr` remains the final emission form; lowering
`RegionPlan -> StencilExpr` must preserve identity and composition.

The plan is quote-stage data only:

`DynCode -> RegionPlan -> rewrite to fixed point -> tile -> StencilExpr -> emit once`

No `RegionPlan` survives in a callable image. Every arena size, block count, iteration
budget, and value count limit uses a named constant.

Acceptance: irreducible or unsupported control safely lowers through general stencil
blocks; loop-phi, exception-edge, and dominance tests pass; differently grouped bytecode
sequences normalize identically; construction and linking remain bounded; complete
V8v7 smoke is unchanged before any optimizing rewrite is enabled.

Sources: <https://v8.dev/blog/maglev> and
<https://v8.dev/blog/leaving-the-sea-of-nodes>.

## 2026-09-10 minimal RegionPlan slice

The first executable slice now quotes every non-empty `DynCode` into one immutable
`RegionPlan` before stencil selection. It validates the canonical block partition,
derives successor edges, and computes register live-in/live-out bitsets with a bounded
backward fixed point. `DynOp` owns the sole register-read/register-write facts; former
ad-hoc operand matchers and block consumers now project from that representation.
Block shape tracing, allocation-site shape folding, direct-selection accounting,
snapshot placement, effect reentry accounting, direct-call accounting, function
quoting, and body emission all walk the plan's blocks.

This slice deliberately performs no optimizing rewrite. Exception-handler edges,
block arguments/phis, abstract values, effects, dominance, and materializer identities
remain required before liveness can license destructive transforms. A jump to the
bytecode-end sentinel is modeled as a valid function exit, not as a missing block.

All 95 release tests pass. The five-pair complete-suite A/B in
`reports/task171-region-plan-full-ab-5/comparison.txt` measured 1886.11 -> 1885.77
(-0.02%); no component moved by 2%. The quoted plan is therefore retained as neutral
infrastructure. It remains cold compiler data and is absent from callable images.

## Round-twelve refinement

Use one canonical per-value product rather than separate mutable compiler tables:
`AbstractValue = Rep × Constant × Location × Materialization × Shape × Ownership`.
Registers, dirty/canonical frame state, known constants, shape facts, and ownership are
projections of that product. CFG-edge states are persistent parent/delta snapshots with
a named maximum chain depth and periodic flattening; merges visit only changed/live
values. This prevents branch-heavy functions with many locals from copying the complete
state at every edge while keeping every derived analysis deterministic.

The worklist and maps are explicitly keyed by `(ContextId, BlockId)` and
`(ContextId, ValueId)`, following weval's context-specialized CFG algorithm. Context IDs
are interned small integers; a repeated key widens/reprocesses the existing block rather
than creating an unbounded clone. Source:
<https://cfallin.org/pubs/pldi2025_weval.pdf>.

## Copying-reducer discipline

Optimization passes consume one immutable RegionPlan and derive a new RegionPlan while
maintaining an old-value-to-new-value map. Do not mutate nodes in place or maintain a
second side representation of rewritten facts. Each reducer is a small pattern
transformer; a pipeline is reducer composition; normalization and verification run at
the explicit phase boundary before the next pass.

This is the Lisp staging rule made operational: quoted input, pure macroexpansion into
new quoted data, then one final stencil emission. V8 moved its current Turboshaft CFG
pipeline to this copying/reducer model because CFG order makes state tracking and loop
rewrites predictable and the old Sea-of-Nodes visitation model repeatedly revisited
mostly unchanged nodes. Use the pattern, not V8's implementation:
<https://v8.dev/blog/leaving-the-sea-of-nodes> and
<https://chromium.googlesource.com/v8/v8/+/refs/heads/main/src/compiler/turboshaft/copying-phase.h>.

## Round-twenty-one refinement: optimize semantic micro-ops, not opaque bytecodes

Task 309 supplies the missing internal algebra. `RegionPlan` should contain decomposed
guards, pure operations, locations, effects, calls, and exits derived from the one
bytecode semantic definition—not treat each complex `DynOp` as an opaque optimization
unit. CPython's current copy-and-patch rationale explicitly reports that compiling base
bytecode did not provide enough optimization potential; its productive pipeline
optimizes finer micro-ops and only then emits stencils. Here those micro-ops remain cold
compiler data and are never an interpreter or runtime dispatch format.

This makes the desired rewrite scale precise: one dominating guard can feed multiple
bytecodes, one ownership fact can remove several cleanup operations, and a caller/callee
composition can expose loads/stores on both sides of a call. `RegionPlan -> StencilExpr`
continues to be the only lowering, but stencil selection consumes the reduced micro-op
normal form rather than attempting an ever-growing list of raw-bytecode patterns.

## Round-twenty-two construction algorithm

Use Braun et al.'s sealed-block algorithm to construct block-argument SSA directly from
the register bytecode. A use in an unsealed block creates an incomplete block argument;
sealing resolves predecessor arguments and erases trivial phis. This yields minimal,
pruned SSA without a separate dominance-frontier insertion pass and matches the
immutable CFG/block-argument representation already chosen here. Cranelift's `SSABuilder`
is a production Rust reference for the exact predecessor/sealing data structure; it is
not a dependency. Sources: <https://pp.ipd.kit.edu/uploads/publikationen/braun13cc.pdf>
and <https://docs.rs/cranelift-frontend/latest/src/cranelift_frontend/ssa.rs.html>.

## Round-thirty-nine typed fact provenance and compact reducer storage

Extend the one `AbstractValue` product with two facts, not side tables:

```text
Concreteness = Proven | Guarded(AssumptionId) | Dynamic
ReflectionCapability = Closed | MayReflect(EffectId)
```

Ownership, non-escape, and reflection freedom are static facts and cannot be recovered by
inspecting a runtime `Value`. A contextual call/version signature carries them explicitly.
Type, shape, element kind, and call target may be proven statically or guarded dynamically;
the provenance determines whether lowering emits no check, an assumption guard, or the
generic morphism. Unknown calls, `eval`, `with`, proxies, accessors, and observable identity
contribute explicit reflection/effect capabilities and conservatively kill only the facts
they can invalidate.

Store blocks, operations, values, predecessors, and uses in compact index-addressed arenas
and derive iteration ranges from block layout. Each optimization remains a copying reducer
with one forward traversal where its transfer function permits it. Record node visits,
changed nodes, bytes allocated, reducer time, and peak temporary bytes; add a regression
fixture that rejects repeated mostly-unchanged visitation or a pointer-rich parallel graph.

This preserves the Lisp rule: one quoted record owns every fact, reducers derive a new
quote, and only final linking mutates bytes. The typed-IR source motivates explicit gradual
type, ownership, reflection, function-version, and contextual-dispatch facts:
<https://mlaurent.ovh/publications/typed_ir.pdf>. V8's CFG/copying-reducer account motivates
the storage and traversal acceptance criteria:
<https://v8.dev/blog/leaving-the-sea-of-nodes>.
