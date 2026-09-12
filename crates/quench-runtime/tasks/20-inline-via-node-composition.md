# 20 — Cross-function inlining as deferred node composition

Status: in_progress

A `Stencil`'s `image: Rc<OnceCell<MaterializedStencil>>` is forced lazily and memoized once, per [[01-stencil-category-core]] and [[14-code-image-reuse]]. Before either side's `OnceCell` is forced, a call site's stencil and a small callee's stencil are both still quoted `StencilNode` trees, so inlining reduces to `caller_prefix + callee.node + caller_suffix` — the same `+`/`compose` operator already used for opcode and block composition, applied one level higher, at a call site instead of within one function body.

Scope this to a bounded, general-purpose eligibility rule (leaf-sized callee, no captured-closure creation inside the callee, matching connector state at the call boundary) so it stays a general capability rather than a benchmark-shaped special case, consistent with [[04-bytecode-coverage-map]]'s constraint against aspirational or benchmark-specific labels.

Acceptance: an inlined call site produces byte output equal to composing the callee's own materialized image at the splice point (i.e., inlining changes layout, not semantics); a recursive or otherwise-ineligible callee is left as a normal call; [[14-code-image-reuse]]'s cache-hit counters distinguish inlined call sites from linked calls.

## SCC-bottom-up implementation algorithm

Do not implement inlining as recursive ad hoc cloning during emission. Build one immutable
call graph from quoted `StencilExpr` functions, compute strongly connected components, and
visit the condensation DAG bottom-up. Within one recursive SCC, consider every original
call edge once before reconsidering edges exposed by an inline. This is LLVM's practical
defense against superlinear growth in highly connected call graphs, adapted to stencil
composition rather than LLVM IR mutation.

The canonical decision value is an `InlinePlan`, derived from Task 317's existing
`FunctionCallRecipe`; it is not a second call metadata representation. It records:

- caller/callee and call-site identities;
- an alpha-renaming from callee locals/registers into a disjoint caller frame suffix;
- argument, receiver, and result bindings;
- normal-return, exceptional-return, and side-exit continuation labels;
- estimated copied stencil bytes, retained shared-kernel references, and connector moves.

Applying a plan is a pure quote rewrite. Replace the call node with callee entry bindings,
the alpha-renamed callee body, and a return rewrite that moves the result to the caller
destination and jumps to the caller continuation. Shared immutable `Kernel` references are
never copied. `StencilTemplate`s remain quoted until the whole rewritten caller is tiled,
instantiated, and linked once. Capturing calls, dynamic `arguments`, unsupported arity,
unknown callees, recursion beyond the bounded policy, and failed guards remain ordinary
stencil/kernel calls; no interpreter path is introduced.

Use named policy constants rather than literals:
`MAX_INLINE_CODE_BYTES`, `MAX_INLINE_FRAME_SLOTS`, `MAX_INLINE_DEPTH`,
`MAX_INLINE_SCC_ITERATIONS`, and `MAX_CONTEXT_VERSIONS_PER_CALLEE`. Initial values are an
experiment matrix, not semantics. Profit is estimated as eliminated guest-frame
construction/completion, argument ownership transfers, and call/return connector moves
minus copied instance bytes, extra frame slots, and predicted spill pressure. Post-inline
macroexpansion runs to a fixed point before the single emission pass, exposing the larger
region to Tasks 329, 144, and 158.

First bounded experiment: exact-target, noncapturing, exact-arity leaf functions only. A
successful disassembly must contain neither `make_frame` nor `complete_dyn_frame` for the
inlined edge. Report eligible/rejected reasons, eliminated frame edges, added code bytes,
and resulting region size before trusting a wall-time change.

Primary source: LLVM's inliner is bottom-up over call-graph SCCs and deliberately delays
newly exposed transitive edges until one SCC pass has completed to limit pathological code
growth: <https://llvm.org/docs/doxygen/Inliner_8cpp_source.html>.

## Initial dynamic reach

Task 343 implements a feature-gated, canonical eligibility classifier before the rewrite.
One 300 ms V8v7 census classifies 15.81% of Richards calls, 37.88% of DeltaBlue calls,
14.02% of RayTrace calls, 95.33% of RegExp calls, and 10.54% of Splay calls as exact
straight-line leaf candidates. This is enough reach to implement the bounded experiment.
Crypto's 0.20% also proves the initial subset cannot be the final design: nested calls and
control-flow rewriting remain part of the SCC-bottom-up implementation, not ad hoc future
opcodes.

## Task 349 zero-reach result

The first physical rewrite used the only safe current hook—pure `DynCode -> DynCode` after
OXC bytecode compilation and before `DynJitCode::build` derives PC-indexed metadata. The
current implementation does not yet retain the unforced caller/callee `StencilNode` DAG
described in this task's original architectural sketch, so later node-level splicing would
invalidate CFG, liveness, IC, site, label, and relocation facts.

The same-owner hoisted resolver passed its semantic tests but applied to zero calls in a
complete V8v7 smoke and was removed. Task 343's dynamic census must not be quoted as static
rewrite coverage. Before another body-splice experiment, split [[317]]'s call recipe into a
pure immutable binding/environment identity available before linking and a linked entry
component. Use that one fact to resolve global, sibling, and prototype-installed targets;
then rerun a static applied-site census before generating an AOT guard.

## Task 351 static reach gate

Task 351 supplies the pre-link immutable recipe and resolves direct outer bindings with
exact identity and closure-environment compatibility. The measured leaf-only intersection
is nonzero but concentrated: Earley-Boyer executes 283,051 calls that are exact,
environment-compatible, and accepted by the existing initial classifier. Crypto executes
321,011 exact compatible direct-binding calls and Splay executes 87,121, but none of those
targets are in the straight-line leaf subset. Richards and RayTrace execute none of the
resolved direct-binding user sites; their useful targets are property/receiver derived.

This passes the evidence gate for one bounded general direct-binding leaf experiment. It
also fixes the scope of the larger design: property-call target recipes and hierarchical
nested-call composition are required before inlining can become cross-suite infrastructure.

## Round-thirty-nine non-local portfolio selection

Do not spend the function's inline budget greedily in source or discovery order. After the
SCC-bottom-up pass constructs every legal `InlinePlan`, choose the complete function/SCC
portfolio before applying any rewrite. Each candidate records eliminated call/frame seams,
newly exposed typed-region operations, constant arguments, callee bytes, connector moves,
frame-slot growth, predicted register pressure, recursion depth, and shared kernels retained
by reference.

Use a deterministic bounded knapsack/priority pass followed by a named number of local
exchange steps. Static loop nesting, exact call-target/context facts, and the cost model may
weight benefit. Runtime execution frequency, a hotness threshold, V8v7 identity, and source
filename are forbidden selection inputs. Apply the selected plans together, then rebuild
the one canonical `RegionPlan` and run ordinary reducers to a fixed point; never mutate and
re-cost the call graph piecemeal while iterating candidates.

Acceptance adds adversarial tests where greedy source order consumes the budget before a
smaller candidate that closes a larger native region, and where candidate interactions make
two individually weak inlines jointly profitable. Selection must be invariant under call-edge
enumeration order. Report total bytes, frame slots, removed seams, region closure, and rejected
candidate reasons before the full A/B gate.

Primary source: WebKit replaced local per-call-site decisions with a non-local inlining
selection over all candidates under one code-size budget:
<https://webkit.org/blog/17899/introducing-the-jetstream-3-benchmark-suite/>.
