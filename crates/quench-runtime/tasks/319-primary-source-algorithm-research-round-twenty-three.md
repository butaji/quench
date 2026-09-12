# 319 — Primary-source algorithm research, round twenty-three

Status: complete

Research additional VM/compiler algorithms under the standing contract: every function
executes stencils from first invocation, runtime machine-code construction is finite
copy/patch/share, kernels are immutable shared code, there is no interpreter fallback or
execution-count hotness gate, and selection is general rather than V8v7-shaped.

## Main conclusion

The current 2083.86 best checkpoint is 20.84% of the 10000 gate. No newly found local
peephole plausibly supplies the missing factor. The measured 18.48–29.23% share in
`dyn_block_step_impl`, visible frame construction/completion, and Task 318's neutral
three-role operand experiment say that the next order remains:

1. finish [[316-typed-operand-holes-and-site-load-erasure]] across ordinary leaves;
2. build [[309-semantic-micro-op-algebra]] and tile it with
   [[157-costed-multi-granularity-stencil-tiling]] so an unsupported operation no longer
   demotes a whole block to a Rust executor;
3. finish [[181-native-direct-user-call-continuation-regions]] so a monomorphic guest
   call does not construct and complete a Rust sidecar across the native edge;
4. propagate typed/ownership facts through [[171-quoted-ssa-cfg-region-normal-form]] and
   keep values in connector registers.

Deegen's own result supports this boundary: copy-and-patch is the emission mechanism,
while register pinning, type-check removal, strength reduction, ICs, and hot/cold
outlining are the optimizations that make the generated bodies competitive. CPython's
copy-and-patch documentation independently says compiling ordinary bytecode was too
coarse and that its useful optimization happens over smaller micro-ops.

## Genuinely additional algorithms

### [[320-sticky-mark-generational-object-collection]]

The current stable-cell collector scans the full initialized object heap on every
collection. Add a non-moving young/old distinction using sticky mark state and a
remembered set. Eden collections scan roots, young objects, and remembered old objects;
occasional full collections reset/trace the complete heap. This preserves stable object
addresses required by ICs while targeting Splay's explicit automatic-memory-management
workload and allocation-heavy Richards/RayTrace/Earley-Boyer.

### [[321-fresh-allocation-barrier-elision-and-clustering]]

Once generations exist, do not insert a barrier after every pointer store. The effect
IR proves initializing stores into the most recent young allocation until a safepoint,
and clusters remaining barriers by destination object. V8 documents that initializing
stores require no barrier; JavaScriptCore has a dedicated store-barrier clustering
phase. This is a direct use of the effect-token algebra, not an ad hoc opcode exception.

### [[322-static-exttsp-stencil-block-layout]]

Run an ExtTSP-style greedy chain layout on the closed stencil CFG with static semantic
edge weights: lexical fallthrough, loop backedge, return, exception, and slow-path
weights. Combine it with bounded tail duplication and hot/cold sectioning. LLVM's
implementation greedily merges/splits block chains to maximize a distance-sensitive
fallthrough/i-cache score. The algorithm is normally profile-guided; this VM must use
only deterministic static weights from [[209-static-branch-weight-metadata]].

### [[323-abstract-state-convergence-loop-peeling]]

Peel a bounded number of initial iterations only when abstract interpretation proves
that loop-header facts become strictly more precise and then stabilize. LLVM's loop
peeler explicitly exploits phis that become known after peeling. Here the useful cases
are representation/shape transitions, first-iteration initialization, and bounds facts.
The transform is general CFG data rewriting and is rejected when effects, code growth,
or the named peel bound make it unprofitable.

## Benchmark relevance

| Suite | Highest-leverage general mechanisms |
|---|---|
| Richards | direct guest calls, property/call IC bodies, barrier clustering, young GC |
| DeltaBlue | direct calls, typed property slots, MemorySSA, multi-level block cover |
| Crypto | Word32 islands, burned operands, direct calls/inlining, loop range proofs |
| RayTrace | constructor allocation, direct calls, shape fields, young GC/barrier elision |
| EarleyBoyer | direct recursive calls, allocation/GC, strings, typed list/property paths |
| RegExp | matcher-plan fusion and reusable scratch after literal compilation reuse |
| Splay | bump allocation plus sticky-mark young collection and initializing stores |
| NavierStokes | register-resident F64 regions, burned operands, bounds/alias versioning |

This table is workload classification, not a benchmark-identity selector. The official
suite describes Splay as exercising automatic memory management and Navier-Stokes as
heavily manipulating double arrays; all mechanisms remain selected from semantic CFG,
type, shape, ownership, and allocation facts only.

## Primary sources

- Deegen, first submitted 18 November 2024: <https://arxiv.org/abs/2411.11469>.
- Copy-and-Patch: <https://arxiv.org/abs/2011.13127>.
- CPython micro-op and stencil JIT design:
  <https://github.com/python/cpython/blob/main/InternalDocs/jit.md> and
  <https://peps.python.org/pep-0744/>.
- V8 Sparkplug's bytecode-to-native/frame design: <https://v8.dev/blog/sparkplug>.
- V8 hidden classes/elements: <https://v8.dev/blog/fast-properties> and
  <https://v8.dev/blog/elements-kinds>.
- JavaScriptCore speculation, IC, object, and GC design:
  <https://webkit.org/blog/10308/speculation-in-javascriptcore/>,
  <https://webkit.org/blog/3362/introducing-the-webkit-ftl-jit/>, and
  <https://webkit.org/blog/12967/understanding-gc-in-jsc-from-scratch/>.
- V8 write-barrier elimination rules:
  <https://chromium.googlesource.com/v8/v8/+/HEAD/src/heap/WRITE_BARRIER.md>.
- LLVM ExtTSP implementation and paper:
  <https://github.com/llvm/llvm-project/blob/main/llvm/lib/Transforms/Utils/CodeLayout.cpp>
  and <https://arxiv.org/abs/1809.04676>.
- LLVM loop peeling implementation:
  <https://llvm.org/doxygen/LoopPeel_8cpp_source.html>.
- Official V8v7 source/description:
  <https://chromium.googlesource.com/v8/v8.git/+/dd3f1ecf719afd21b4c695c776b4da2fb494ef92/benchmarks/>.

No score claim changed during this research task. Published improvements are evidence
for experiment ordering, never projected local gains.

