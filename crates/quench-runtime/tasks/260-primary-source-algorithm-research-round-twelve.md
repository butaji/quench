# 260 — Primary-source algorithm research, round twelve

Status: complete

Research additional VM and compiler algorithms under the standing contract: every
function executes a stencil image from first entry; the runtime performs bounded
selection, copy, patch, composition, and sharing only; rustc/LLVM cooks a finite,
general catalog AOT; no interpreter fallback, hotness threshold, third-party VM, or
benchmark/source identity participates in execution.

## Findings

### 1. Use context specialization to derive whole-function stencil programs

The weval transform specializes an interpreter CFG with a worklist keyed by
`(Context, Block)`, separate block/value maps, and a constant-propagation lattice. Its
important result for this project is the algorithm, not the Wasm implementation: split
analysis context at bytecode PCs and directed branch values so the resulting CFG follows
the guest program, including joins and loops, rather than the semantic dispatch loop.

Adapt this at quote/build time across Tasks 149, 171, and 144. The semantic Rust macro is
the single source; the worklist derives bounded context-specialized `RegionPlan` data;
the final functor tiles that data into pre-cooked templates. Do not add weval, Wasm, a
runtime LLVM invocation, or a second interpreter. Switch value specialization belongs to
Task 186.

### 2. Make the compiler state one canonical product, not independent passes

The six single-pass compilers studied by Titzer all use abstract interpretation. A useful
abstract value tracks at least constantness, current register/location, and whether its
canonical memory slot is current. Local gets/sets then often emit no instructions;
constants fold; immediate/addressing variants are selected directly; and values spill
only when registers are exhausted or an observation requires memory state.

Task 171's canonical fact therefore becomes:

`AbstractValue = Rep × Constant × Location × Materialization × Shape × Ownership`

All other views are projections. Task 158 performs the live-range/next-use prepass and a
single forward register walk. Tasks 164 and 193 materialize only at `MayObserve` or
`MayGc` edges. CFG snapshots use parent/delta sharing with periodic flattening, so a
large local file cannot turn branches into quadratic copying.

### 3. Track known tag bits explicitly (new Task 261)

weval identifies partial-known-bits optimization as a direct way to erase repeated
NaN-box tag checks and boxing/unboxing pairs after guarded inlining. LLVM's `KnownBits`
uses two bitsets for bits proven zero and proven one. This VM needs a small domain over
its own 64-bit `RawValue`, not LLVM at runtime: guards refine masks, joins intersect
facts, and macro-generated transfer functions preserve tag/payload knowledge.

This is more precise than only naming a value `Object` or `Number`: it can prove that a
mask, shift, payload extraction, or tag comparison is redundant while leaving unrelated
bits unknown. Task 261 owns the representation-specific algorithm; Task 174 remains the
general sparse conditional propagation worklist.

### 4. Treat IC recipes as the reusable speculation language

SpiderMonkey uses one linear CacheIR normal form in every engine outside its C++
interpreter: guards, idempotent pure operations, and one terminal result/effect. This
confirms Task 153 as the semantic bridge between a shared immutable kernel and a patched
stencil instance. A property/call/`instanceof` optimization should add recipe vocabulary
or a recipe arm, not another selector branch and bespoke five-op handler.

Fuses complement recipes: one composite byte can summarize multiple runtime invariants.
Tasks 155 and 229 own this. A recipe records the fuse dependency; invalidation changes
the fuse/event state and redirects to a general stencil, without rewriting semantics.

### 5. Bound shape propagation at two versions first

Typed shapes plus shape propagation eliminated 48% of dynamic tests and reduced
execution time 25% in Higgs, but unlimited shape propagation caused pathological Splay
code growth. The evidence-backed starting point was two shape versions. Task 144 already
defines `MAX_STATIC_BLOCK_VERSIONS`; Tasks 144 and 152 must separately count total block
versions and shape versions, begin at the named value two, and measure code size as an
acceptance metric rather than treating more specialization as monotone improvement.

### 6. Guarded inlining is valuable because it exposes later simplification

Production engines use call ICs both to accelerate calls and to identify guarded inline
targets. The weval work notes the larger payoff: once call and IC implementations share
one body, tag checks and boxing/unboxing pairs become visible to generic rewrites.
Therefore Tasks 146/177/181 should feed the same RegionPlan and Task 261 facts; a direct
call that still flushes every value through a canonical frame is only a partial win.

### 7. Heap work remains necessary, but follows execution-boundary evidence

JSC's object model and allocator validate shared structures, inline property capacity,
fixed-size cells, generational allocation, and explicit barriers. They reinforce Tasks
148/156/162/193. They do not explain the current stencil boundary by themselves, so the
comparative profile and cross-stencil register-allocation ceiling (Tasks 253 and 251)
remain ahead of a broad heap rewrite.

## Ranked next experiments

1. Run Tasks 253/254 and 251; quantify helper/frame traffic versus V8/JSC and the exact
   cost of independent stencil compilation.
2. Implement the minimal Task 171 RegionPlan with Task 144 context keys and the
   parent/delta abstract state described above; enable no rewrite initially.
3. Add Task 261 known-tag-bit propagation and Task 158's forward register placement;
   measure erased tag checks, frame loads/stores, and boxing pairs.
4. Express property/call specialization through Task 153 recipes, Task 152 shape facts,
   and Task 155 fuses; stop adding hand-authored condition families.
5. Compose direct calls through Tasks 146/177/181 without canonical-frame round trips.
6. Replace hot `Rc` ownership with Tasks 148/162/193, then add inline object storage from
   Task 156.
7. Let Tasks 149/157 generate and select only the macro-derived LLVM-visible supernodes
   justified by the resulting fact/cost data.

## Rejected adaptations

- Do not add weval, TPDE, a Wasm engine, or any third-party VM/backend. Their algorithms
  are references; rustc/LLVM remains the AOT cooker and this project owns runtime linking.
- Do not add runtime meta-tracing or a hotness gate. Context specialization operates on
  the complete bytecode CFG before first stencil execution.
- Do not interpret `RegionPlan`, `IcExpr`, or `KnownBits` at runtime. They remain quoted
  build/link data and disappear after the single final emission.
- Do not infer that copy-and-patch alone approaches optimizing-JIT/C performance. V8's
  own documentation says independently emitted baseline instructions lack redundant-load
  elimination, strength reduction, inlining, and cross-operation register allocation.

## Primary sources

- weval whole-program partial evaluation and context specialization:
  <https://cfallin.org/pubs/pldi2025_weval.pdf>
- Single-pass abstract state, register/constant tracking, and on-demand materialization:
  <https://arxiv.org/pdf/2305.13241>
- SpiderMonkey CacheIR and fuses:
  <https://firefox-source-docs.mozilla.org/js/how-we-optimize.html>
- V8 Maglev SSA, known-node facts, representation selection, and register allocation:
  <https://v8.dev/blog/maglev>
- Typed shapes and bounded shape propagation:
  <https://arxiv.org/abs/1507.02437>
- LLVM `KnownBits` representation and context-aware analysis:
  <https://llvm.org/doxygen/KnownBits_8h_source.html> and
  <https://llvm.org/doxygen/ValueTracking_8h.html>
- Deegen baseline-JIT generation and optimization inventory:
  <https://arxiv.org/abs/2411.11469>
- V8's explicit baseline-versus-optimizing compiler boundary:
  <https://v8.dev/docs/wasm-compilation-pipeline>
- JavaScriptCore structures, ICs, OSR, and allocation sinking:
  <https://webkit.org/blog/10308/speculation-in-javascriptcore/>

