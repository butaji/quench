# 144 — Static bounded context-versioned stencil regions

Status: planned

Apply Static Basic Block Versioning (SBBV) to the quoted stencil program, at basic-block
and loop granularity rather than as another collection of tiny fast-path fragments. A context
`Gamma` records each live value's proven representation and machine location, for
example `Unknown`, `I32`, `F64`, `Boolean`, `Object(shape)`, or `DenseElements(kind)`.
A linked region is the typed morphism `Stencil<GammaIn, GammaOut>`.

Before emitting bytes, traverse the quoted CFG with a breadth-first work queue. A branch
narrows its successor contexts; a requested `(block, context)` pair either reuses an
existing version or creates a new quoted version. No program execution, feedback counter,
benchmark identity, or first-arrival event chooses the versions: every function starts by
executing the resulting stencil graph.

Bound live versions per source block with `MAX_STATIC_BLOCK_VERSIONS`. When the bound is
exceeded, delay merging until the candidate is popped from the work queue, then merge the
two most similar contexts upward in the representation lattice and redirect their incoming
edges. The merge must conservatively cover both inputs and may lose facts; it must not
silently select one input. Keep reachability derived from the rewritten CFG so an
unreachable version is not charged against the bound. Start with the paper's evidence-backed
two-version policy, expressed only through the named constant and measured against other
values.

Keep the representation quoted until linking:

`Bytecode CFG -> Context propagation -> rewrite to fixed point -> StencilExpr -> emit once`

Within one version, check each fact at the region boundary, keep numeric values unboxed
and shape/element facts live across internal operations, and rebox only at an effect edge
whose ABI requires a generic `Value`. The general fallback is another stencil morphism;
it is never the AST interpreter.

`Gamma` also carries value-equivalence classes and the relative range facts owned by Task
214. A successful guard refines data once; all dominated operations consume that proof.
This allows SBBV to isolate a checked first iteration and emit a steady-state loop version
without repeating type, callee, shape, overflow, or bounds checks that the loop context
already proves.

Every speculative exit uses the canonical persistent frame state from Task 164. Join and
loop-phi contexts choose a common representation explicitly; conversions occur at the
join or materialization edge rather than being rediscovered inside each opcode stencil.

This integrates Tasks 19, 25, and 128 rather than duplicating them: those tasks define
guard objects and effect leaves, while this task supplies the bounded region-versioning
algorithm that prevents guards from recurring in every small leaf. It follows the SBBV
work-queue, delayed-context-merge, interval-narrowing, and symbolic-vector-bound algorithms
(<https://doi.org/10.4230/LIPIcs.ECOOP.2024.28>) and Maglev's use of known-node information
(<https://v8.dev/blog/maglev>) while preserving this VM's copy-and-patch-only runtime.

Acceptance: named version/representation constants; semantic and category-law tests;
counters for versions, context merges, entry guards, reboxes, and generic-version selections; inspection
of a representative linked loop proves that stable values are not repeatedly tagged or
guarded; complete V8v7 A/B improves without violating component floors.

## Lazy-BBV construction refinement

Construct versions on first *compiler worklist demand*, never first runtime execution.
Intern each request by `(BlockId, ContextId)` and compile successors using the refined
outgoing context. A repeated request reuses the same version. Once
`MAX_STATIC_BLOCK_VERSIONS` is reached, route new demands through one deterministic
generic/widened context rather than cloning without bound. This imports lazy BBV's low
analysis cost while retaining the project's no-hotness and compile-everything policy.

Treat exact object shape as part of `ContextId`, so Task 152 can consume one dominating
shape guard throughout a version. Measure eliminated tag/shape checks against code-byte
growth; the point is context preservation, not version count.

Additional primary sources: lazy basic-block versioning
<https://arxiv.org/abs/1411.0352> and typed shapes with BBV
<https://arxiv.org/abs/1507.02437>.

## First implementation slice after research round twenty-four

Do not begin with arbitrary context products. Version only numeric tag facts for one
ordinary acyclic block, keyed by `(BlockId, NumericContextId)`, with the named limit
`MAX_STATIC_BLOCK_VERSIONS = 2`. A guard refines the successor context; subsequent
numeric micro-ops consume that fact and select already cooked stencils without repeating
the tag test. When two contexts exceed the bound, route both through one explicit widened
version. This is enough to prove that a categorical context object survives composition
and changes emitted code, while keeping the state space inspectable.

The second slice adds exact shape identity and fixed property offset, then loop headers.
The third adds exact-arity/function-code entry contexts. This order is deliberate: the
current VM still spends 18.48--29.23% of sampled time in `dyn_block_step_impl`, and the
expanded operand-hole experiment in Task 318 was neutral. Reusing one proof across a
block removes semantic work; burning another address into an otherwise generic leaf does
not.

## Interprocedural context transfer

Once Task 20 has physically erased an eligible call boundary, extend the same worklist key
from `(BlockId, ContextId)` to `(FunctionId, EntryBlockId, ContextId)` for the inlined
callee. Transfer proven argument representations and exact shapes into the callee entry;
transfer each return representation into the caller continuation. This is not a second
interprocedural type analysis: it is the same bounded context-versioning operation over a
larger quoted graph.

For a linked but non-inlined direct call, Task 177's summary is the conservative transfer
function. For an inlined call, the callee graph computes the precise transfer. Recursion
and version-budget overflow widen to the canonical unknown context. The named
`MAX_CONTEXT_VERSIONS_PER_CALLEE` and `MAX_INTERPROCEDURAL_CONTEXT_DEPTH` bound growth;
neither runtime execution count nor benchmark identity participates.

Measure tag/shape checks removed on both sides of the former call edge separately from
frame edges removed by Task 20. Interprocedural BBV eliminated 94.3% of dynamic type tests
on average in its 26-benchmark evaluation and achieved speedups up to 56%; those results
justify trying the algorithm but are not projected onto this VM:
<https://arxiv.org/abs/1511.02956>.

## Context canonicalization and reuse

Versioning must not clone a block merely because the same facts arrived by a different
path. Before looking up `(BlockId, ContextId)`, project `Gamma` to facts live at that block,
normalize the product lattice, intern the result, and use the resulting canonical
`ContextId`. Task 42 owns cross-function and physical-image hash-consing; this task owns the
local invariant that equal live contexts mean the same quoted block version.

The work queue therefore contains canonical demands, not paths. Its deterministic order
and delayed widening make the output independent of predecessor discovery order. Add law
tests for normalization idempotence, join commutativity/associativity, dead-fact erasure,
and identical emitted graphs under permuted predecessor order. These laws are the defense
against the redundant context explosion measured by context-guided splitting research:
<https://kar.kent.ac.uk/109418/>.

## Failure-directed materialization

Do not lower the entry context as one conjunction with one generic failure edge. Factor the
guard formula into a shared-prefix decision DAG and materialize every admitted `(BlockId,
ContextId)` as a compatible native leaf. Task 396 is the bounded first experiment. It proves
whether a failed numeric/dense/property fact can transfer to a weaker native version while
retaining the other facts, instead of discarding the whole region. The version worklist and
guard DAG are two derived views of the same canonical context demands, never independently
maintained graphs.
