# 157 — Costed multi-granularity stencil tiling

Status: planned

Select a minimum-cost cover of each quoted basic block/region from compatible catalog
members: primitive templates, fused recipes, basic-block stencils, traced loops, and
shared kernels. Dynamic programming operates on the normalized sequence/DAG and emits
the same `StencilExpr<In, Out>` type, so larger bricks remain categorically compatible
with smaller ones.

Costs are build-time/architecture data: expected helper calls, guard count, frame loads,
spills, ownership operations, branch edges, copied bytes, and patch count. They are not
runtime heat or benchmark identity. Exact semantic/effect/context matching is mandatory;
an identity edge has zero cost. Keep the first implementation to straight-line blocks
with a named maximum candidate width, then extend to region DAGs only if A/B justifies it.

This task must not reward fusion merely for reducing stencil count. Copy-and-patch has
already removed interpreter dispatch, and prior superinstruction work shows modest gains
when dispatch is the only saving.

Distinguish two very different candidates in the cost model. A copied concatenation of
already-cooked leaves saves only branch/dispatch seams and is expected to have the small
benefit seen for static interpreter superinstructions. A rustc/LLVM-cooked supernode
generated from one semantic macro can also fold addressing, keep values in registers,
remove checks, and outline slow paths. Copy-and-Patch reports using close to 100,000
general tree-shape supernodes for its high-level compiler; that is evidence for a
generated, deduplicated finite catalog with explicit build/code-size budgets, not for a
hand-written opcode-pair list.

Acceptance: selector output is invariant under sequence regrouping; brute-force tests
match the dynamic program on small inputs; diagnostics explain each selected cover and
cost; compile/link time stays linear in bytecodes times bounded candidates; full V8v7
A/B passes.

Primary sources: <https://arxiv.org/abs/2011.13127> and
<https://doi.org/10.1002/spe.434>.

Round-fifteen refinement: Task 272 derives the selector automaton from the same immutable
recipe records that generate the AOT catalog. Use BURS-style bottom-up states for trees
and shortest path over `(position, connector context)` for linear regions. This task
continues to own the costs and legal-cover semantics; Task 272 owns generated matching,
so matcher order cannot silently become a second cost model.

## Round-nineteen refinement: overlap-aware candidate discovery

Generate the finite superinstruction candidate set offline before solving the cover.
Normalize eligible instruction/RegionPlan sequences by semantic opcode, effect class,
connector context, and symbolic operands; mine repeated substrings/subgraphs with a
dictionary-compression objective; then score the *net* benefit after resolving overlap
between candidates. A frequent candidate that is always covered by a more profitable
one must not consume catalog budget.

Candidate discovery may use the language conformance corpus and an explicitly separate
training corpus, but never V8v7 source names, filenames, or benchmark-specific constants;
V8v7 remains a holdout acceptance suite. Runtime selection still uses the generated
typed automaton and static cost table, never execution counts. The Ethereum VM
superinstruction work supplies the useful algorithmic piece—dictionary synthesis plus
greedy overlap handling—while this VM retains typed semantic/effect constraints and
rustc/LLVM-cooked recipes:
<https://www.xiaowenhu.com/files/splashws24vmilmain-p91-p-a53cc9a2c0-81338-final.pdf>.

## Round-twenty-two concrete failure mode

Current whole-block families are exact-slice matchers: one unsupported operation can
demote an otherwise coverable block to the generic Rust range. Replace that all-or-none
choice with shortest path over `(micro_op_position, ConnectorContext)`. Single-op
kernel/stencil leaves guarantee total coverage; a 4-op region, a 3-op supernode, and
three primitive leaves may cover one ordinary block as `4 + 3 + 1 + 1 + 1` without any
special case. Costs must charge every native/Rust/native boundary heavily, reflecting
Task 284's measured regression from isolated atoms. LLVM's instruction selectors use
the analogous declarative, costed pattern approach:
<https://llvm.org/docs/CodeGenerator.html> and
<https://llvm.org/docs/GlobalISel/InstructionSelect.html>.

## Round-twenty-six plan/materialize discipline

Adopt LLVM VPlan's three explicit phases for every coarse loop candidate: Legal derives
obligations without changing the input quote; Plan constructs and costs one or more immutable
alternatives; Materialize emits exactly the selected stencil expression. The same candidate
object must estimate cost and emit code, preventing the estimator and linker from drifting.
Scalar fallback is an ordinary zero-transform plan. Vector width, unroll/interleave factor,
predication, and remainder shape are fields in candidate data with named bounds:
<https://llvm.org/docs/VectorizationPlan.html>.

## Round-thirty physical-storage choice

Treat a shared `Kernel` and a copied `StencilInstance` with the same typed connector and
semantics as two physical implementations of one candidate morphism. The selector must
price the choice explicitly: a kernel pays its branch/call seam, ABI moves and possible
distance penalty but contributes its code bytes once; an instance pays copied bytes,
patches and instruction-cache footprint but may fall through and keep values in connector
registers. This is instruction replication generalized to the project's two-block model,
not a new semantic operation.

Keep the choice static and architecture-costed. Runtime execution counts and V8v7 identity
must not decide which form is emitted. Record selected kernel references, copied instances,
seams removed and bytes added so [[291]] can retire a replication that stops paying for
itself. Primary evidence: dynamic superinstructions and replication
<https://www.complang.tuwien.ac.at/cd/papers/A73-full.pdf>.

## Task 384 correction to the cost model

Charge connector transitions and representation conversions at every tile boundary, not
only helper calls and copied bytes. Task 384 made more valid small stencils execute and
slowed Crypto by roughly 2.7x in a diagnostic screen; replacing those regions with the
coarse kernel restored performance. Task 385 is the next realization: a register-resident
I32 composite may win only because it amortizes entry/exit conversion across the whole
island. A sequence of individually boxed I32 leaves must lose to the kernel in the static
physical cover even if it closes more opcode coverage.

## Round-thirty-nine offline oracle input

Task 394 supplies candidates and measured static costs; this task remains the only runtime
cover selector. Import the generated `TemplateRecipe` Pareto frontier rather than adding a
second learned/heuristic matcher. A rule is eligible only when its semantic/effect/context
signature exactly covers the quoted micro-op demand and its patch schema is relocation
closed. Kernel and patched-instance realizations remain alternative physical costs for the
same morphism.

Re-estimate the smaller-rule cover and connector costs whenever the cooker configuration or
ABI fingerprint changes. Stale oracle measurements invalidate the generated catalog rather
than silently falling back to their old costs. V8v7 never supplies rule-training data and
never changes the selector; it remains only the final performance holdout.

## Round-forty-six whole-image fitness budget

Add a function-level constraint to the cover dynamic program. A locally cheap tile is not
admissible when the complete cover exceeds named limits for copied bytes, continuation
fragments, side exits, or connector transfers relative to an equivalent shared-kernel cover.
The selector must be able to collapse adjacent fragments or choose the kernel realization
even when each fragment independently looks profitable.

This guards a demonstrated copy-and-patch failure mode: a 2026 CPython report describes a
single synthetic function split across roughly 22 tail-linked traces and about 18 MB of JIT
code. The exact workload and limits do not transfer, but the mechanism establishes that
per-fragment fitness is insufficient:
<https://github.com/python/cpython/issues/149212>.
