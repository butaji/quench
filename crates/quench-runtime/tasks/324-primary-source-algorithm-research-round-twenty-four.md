# 324 — Primary-source algorithm research, round twenty-four

Status: complete

Research additional VM/compiler algorithms under the standing execution contract:
stencils run from the first invocation, no interpreter or hotness threshold selects
code, runtime code generation is finite copy/patch/share, kernels are immutable shared
instances, and no benchmark identity or source spelling participates in selection.

## Result: the next gains require preserving facts and state across boundaries

The expanded Task 318 experiment burned local, move, unary, binary, and comparison
operands into rustc-cooked instructions and measured 2045.48 -> 2042.17 (-0.16%). This
is useful negative evidence: copy-and-patch is an emission method, not by itself an
optimizer. The highest-priority researched algorithms all remove repeated semantic work
across several bytecodes or across a call edge.

### 1. Bounded static basic-block versioning — implement next after the call edge

Task 144 is the correct smallest optimizing compiler for this architecture. Its cache key
is the category object `(BlockId, Gamma)`; its value is a composed stencil morphism. A
successful guard refines `Gamma`, and all later operations in that block consume the fact
without rechecking it. Static BBV specifically avoids runtime profiles and reports useful
results with only two versions per block, matching the no-hotness requirement.

Start with numeric facts in acyclic blocks, then exact shapes, then loops. Do not add a
second tree of hand-fused opcode patterns: Task 309's micro-op quote plus Task 157's
costed cover must be the one input representation.

### 2. Function-code-keyed closure call IC — refine the current exact-value IC

Task 35 now records Deegen's two-mode call IC algorithm. The current `CallIcSite` keys on
the complete tagged function value and publishes one environment pointer. That misses
when a factory creates many closures from the same body. On a same-code/different-value
miss, transition to a recipe keyed by immutable function-code identity and take the
environment from the current closure. Compose the hit with Task 181's in-place call and
return continuation. Caching the target without deleting Rust frame construction is not
the optimization described by the source.

### 3. Register residence plus ownership SSA — make copy-and-patch code act like C

CPython's current copy-and-patch JIT reports gains from basic register allocation that
removes stack traffic and from proven-safe reference-count elimination. These are direct
production corroboration for Tasks 158 and 172. The unit must be a micro-op/SSA region,
not a leaf stencil: allocate physical connector contexts once, emit parallel-copy edge
recipes, and derive explicit copy/consume/destroy operations from ownership facts.

### 4. Shape propagation and minimorphic offset classes

Typed shapes plus block versioning removed repeated shape/type tests in Higgs. JSC also
distinguishes the useful case where several shapes share the same property offset: one
shape-set guard followed by one fixed-offset load. Implement Tasks 152 and 154 only after
the block context exists; a collection of per-site shape stencils cannot propagate the
proof to the next access.

### 5. In-place calls, inline IC arms, and a separate slow-path data stream

Deegen's generated call uses an in-place VM stack frame and a native continuation. Its IC
hit is code embedded in the caller's live machine context; its slow semantics are AOT
kernels reached with a compact `SlowPathData` record. Hot and cold code are emitted into
separate streams, and the common fallthrough is arranged to remove the final dispatch
jump. This confirms the combined direction of Tasks 145, 181, 276, and 322. A native
stencil that immediately calls a Rust `prepare/execute/finish` helper still preserves the
expensive boundary and does not satisfy this pattern.

### 6. Element-kind lattice and out-of-bounds state

V8 stores indexed elements separately from named properties and tracks a monotone element
kind such as packed small integers. An out-of-bounds observation changes the site's future
requirements because JavaScript must consult the prototype chain. Task 265 should model
both facts explicitly: `ElementsKind` is object state; `BoundsMode` is IC-site state.
Neither belongs in an implicit fallback branch. A packed/in-bounds arm is a direct indexed
load/store stencil; holey/out-of-bounds behavior is an explicit shared kernel.

### 7. Reusable IC recipes — memory optimization, not the next execution win

Reusable Inline Caching observed that the same hidden class appears at multiple sites and
that many handlers are context-independent. Its useful local translation is already Task
153's split: canonical recipe shape selects one immutable shared Kernel, while concrete
shape/slot/callee fields live in per-site data or a patched StencilInstance. Constructor
shape transitions may pre-seed dependent site recipes later, but IC-miss setup is not the
current steady-state bottleneck, so this stays behind native hits and context propagation.

### 8. Trace trees — borrow the region shape, reject the runtime policy

TraceMonkey's single-entry/multiple-exit typed traces avoid internal joins and can be
optimized in linear time; side exits grow into a tree. The applicable part is the quoted
region shape and canonical side-exit state (Tasks 164, 171, 241, and 322). Its interpreter,
loop counters, runtime trace recorder, and hot-side-exit policy violate this project's
contract and are not adopted. Static weights and bounded compiler-worklist demand choose
the initial superblock instead.

## Ranked experiment order

1. Task 181 + Task 35: native in-place direct/closure calls with native continuations.
2. Task 309 + Task 144: numeric micro-ops and two-version static BBV.
3. Task 158 + Task 172: connector-register allocation and ownership-derived cleanup.
4. Task 152 + Task 154: shape propagation and minimorphic offset folding.
5. Task 265: packed-Smi/packed-double element kinds and explicit bounds-mode ICs.
6. Tasks 145/276/322: inline IC arm, cold stream, and static fallthrough layout.
7. Task 153/16: share context-independent recipe kernels and deduplicate code memory.

This ordering targets measured boundaries. It is not a projection that any cited paper's
published speedup will transfer to this VM.

## Primary sources

- Deegen paper, first submitted 18 November 2024:
  <https://arxiv.org/abs/2411.11469>.
- Deegen baseline-JIT design and disassembly:
  <https://sillycross.github.io/2023/05/12/2023-05-12/>.
- Static BBV, including the no-profile convergence algorithm and two-version result:
  <https://drops.dagstuhl.de/entities/document/10.4230/LIPIcs.ECOOP.2024.28>.
- Lazy BBV: <https://arxiv.org/abs/1411.0352>.
- Interprocedural BBV: <https://arxiv.org/abs/1511.02956>.
- Typed shapes and shape propagation: <https://arxiv.org/abs/1507.02437>.
- V8 Maglev's compact SSA/abstract-state/register-allocation design:
  <https://v8.dev/blog/maglev>.
- JavaScriptCore speculation, minimorphism, and polyvariance:
  <https://webkit.org/blog/10308/speculation-in-javascriptcore/>.
- CPython's copy-and-patch micro-op architecture:
  <https://github.com/python/cpython/blob/main/InternalDocs/jit.md>.
- CPython 3.15 register allocation and reference-count elimination:
  <https://github.com/python/cpython/blob/main/Doc/whatsnew/3.15.rst>.
- V8 elements kinds: <https://v8.dev/blog/elements-kinds>.
- Reusable Inline Caching: <https://iacoma.cs.uiuc.edu/iacoma-papers/pldi19_2.pdf>.
- TraceMonkey trace trees: <https://mozilla.github.io/pdf.js/web/compressed.tracemonkey-pldi-09.pdf>.

No score claim changed during this research task. The accepted checkpoint remains
2083.86; Task 318's expanded measurement is a neutral experimental result.
