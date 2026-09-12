# 409 — Primary-source optimization research, round forty-five

Status: complete

This round searched for additional general-purpose algorithms after Task 408 and reduced
them against the existing task graph. The accepted checkpoint remains **2401.58**; this
research and the Task 385 link-wiring compile check do not produce a new score.

The central conclusion is that the VM does not need more isolated opcode stencils. It needs
one small optimizing *planner* whose facts survive across opcode, block, loop, and call
boundaries. Large stencil bricks remove dispatch seams only after that planner has proved
which tag checks, boxes, frame stores, property guards, calls, and allocations may vanish.

## Ranked algorithms to try

1. **Deopt-first native regions (`330`, `396`, `385`).** Make the main image a
   single-entry/multiple-side-exit native morphism. A failed guard reconstructs only live
   values and jumps to the total generic stencil/kernel at the same bytecode PC; it never
   enters an interpreter. This turns a JS operation's slow diamond into a cold edge and lets
   later checks be hoisted or deleted. JavaScriptCore describes OSR exits as the mechanism
   that flattens speculative control flow; SpiderMonkey stores reconstruction metadata for
   the same reason.
2. **Static bounded basic-block versioning (`144`, `152`, `323`).** Version blocks by a
   normalized incoming context of representations, shapes, element kinds, callees, and
   effect epochs. At loop headers, compute a widening fixpoint and optionally peel one
   iteration before choosing the stable version. This is performed whenever a function is
   linked—there is no hot-path detector or interpreter tier. A named version budget ends in
   one total generic version. Published JS BBV results report 71% fewer type tests and up to
   50% speedup; interprocedural BBV reports 94.3% test elimination and up to 56%, which are
   mechanism evidence rather than predictions for this VM.
3. **Representation splitting at loop phis (`158`, `214`, `264`, `385`).** Infer consumer
   demands backward, select `I32`, `U32`, `F64`, tagged, or reference locations, and split a
   value at the few edges that require another representation. This is the specific cure for
   Crypto's repeated ToInt32 conversions and Navier-Stokes' boxed F64 traffic. Maglev uses a
   separate loop-phi representation-selection phase because backedges otherwise force a
   prematurely generic representation.
4. **Effect-versioned guard and load elimination (`173`, `195`, `279`, `329`).** Give heap,
   property, prototype, dense-element, lexical, and realm effects distinct version tokens.
   Then run one forward value-numbering table plus loop-invariant hoisting and partial
   redundancy elimination. Repeated shape checks and loads disappear only while the relevant
   token is unchanged. This is a single rewrite over quoted SSA, not separate optimizers per
   object operation.
5. **Shape propagation and typed fields (`152`, `165`, `368`).** A successful property IC
   should refine the receiver to `Shape<S>` and the result to the slot's field representation.
   Subsequent accesses become offset loads with no repeated hash lookup or shape check.
   Transition/prototype watchpoints invalidate the assumption. Typed-shape BBV research
   reports 48% fewer tests, 17% smaller code, and 25% lower runtime across its suite.
6. **Call-target polyvariance and native continuation inlining (`20`, `163`, `177`, `379`,
   `401`).** Treat a property-load/callee-guard/frame-link/direct-call/return-continuation as
   one recipe. Exact targets inline their quoted function graph under a size/recursion budget;
   small polymorphic sets clone one continuation per target; the remainder calls a shared
   kernel through the same connector. JavaScriptCore reports a 38% Raytrace gain from
   polyvariant devirtualization, and calls remain the largest measured local residual family.
7. **Scalar replacement with edge materialization (`24`, `172`, `176`, `191`).** Represent
   non-escaping object/array/closure allocations as virtual products of fields. Forward
   stores and loads, delete dead objects, and materialize only on an identity-observing or
   escaping edge. This targets RayTrace constructors, Splay nodes, Earley-Boyer cons-like
   data, and temporary argument arrays without inventing benchmark-specific stencils.
8. **Element-kind lattice plus range/BCE (`165`, `175`, `233`, `265`).** Track packed/holey
   and I32/F64/tagged backing kinds monotonically. A loop preheader guards kind and length;
   scalar evolution proves the induction range; the body uses one base pointer and scaled
   address with no repeated tag, kind, or bounds checks. Out-of-range and hole cases side-exit
   because JavaScript requires prototype lookup. This is the numeric-array path needed after
   register residence.
9. **Weighted tree/DAG covering for stencil granularity (`157`, `272`, `313`, `377`).** Use
   dynamic programming for straight-line/tree regions and the existing bounded hypergraph
   search for DAGs. The cost includes conversions, spills, patches, code bytes, helper edges,
   and I-cache pressure—not just stencil count. Leaves, superinstructions, blocks, loops,
   inlined callees, and shared kernels are alternative covers of the same quote.
10. **Cold outlining and code layout (`276`, `289`, `291`, `322`, `406`).** Outline rare
    miss/reconstruction tails, merge identical tails, place loop SCCs contiguously, and do
    final-link AArch64 branch/address relaxation. This should be attempted only after PMU
    evidence shows front-end or reach costs; larger copied stencils can otherwise lose by
    exhausting the instruction cache.
11. **Inline bump allocation plus a nursery (`162`, `191`, `192`, `320`, `321`).** Allocation
    stencils should compare/bump a thread-local frontier and initialize a known shape in
    native code; exhaustion calls one shared kernel. Fresh objects omit write barriers.
    Survival/capacity transitions may update allocation-site policy, but execution heat never
    selects whether code is compiled.
12. **Matcher automata selected from a quoted RegExp plan (`87`, `245`).** Classify patterns
    as deterministic one-pass, machine-word NFA, or general. The immutable automaton is a
    shared `Kernel`; literals/targets may be patched into a `StencilInstance`. This is lower
    priority because RegExp is already one of the strongest components.

## Unified Lisp/category form

Do not implement twelve pipelines. Extend the one quoted graph:

```text
SemanticGraph
  -> AbstractContext fixpoint
       {representation, shape, element_kind, callee, range, ownership, effects}
  -> rewrite*                 ; GVN/PRE, guard deletion, scalar replacement, inlining
  -> PhysicalCover            ; TemplateInstance | shared Kernel at any granularity
  -> ReconstructionTable      ; zero-code descriptions for cold exits
  -> link once                ; the only byte mutation
```

Contexts are category objects. `StencilTemplate -> StencilInstance` and immutable `Kernel`
references are compatible morphisms. Sequential composition is allowed only when physical
contexts match; joins use explicit parallel-copy/reconstruction arrows. The abstract
interpreter is a compile-time analysis over quoted data, not an execution interpreter. Every
generated function uses stencils/kernels from first execution.

## V8v7-directed order without V8v7-shaped code

| Order | General mechanism | First measured pressure it should remove |
|---|---|---|
| 1 | representation-typed regions | Crypto Word32 conversion/frame traffic; Navier F64 boxing |
| 2 | property/call continuum and polyvariant inlining | Richards, DeltaBlue, RayTrace, Earley-Boyer call boundaries |
| 3 | effect/shape propagation | repeated property/prototype guards in object-heavy suites |
| 4 | scalar replacement + inline allocation | RayTrace, Splay, Earley-Boyer allocation and field traffic |
| 5 | element kinds + range/BCE | Crypto and Navier dense loops |
| 6 | matcher/string representation work | remaining RegExp and string-heavy residuals |
| 7 | cold layout/address relaxation | only after PMU proves a front-end bottleneck |

The order is evidence-driven, but no selector may inspect a benchmark name, source line,
property spelling, execution count, or hotness threshold.

## Experiment discipline

For each mechanism, reject before A/B unless disassembly and counters prove its intended
erasure. Examples: a Word32 region must show one entry conversion, raw `w` operations, and
one exit materialization; a property-call recipe must show a shape/holder/callee guard and a
direct guest edge with no Rust helper; a virtual object must show zero allocation on its
non-escaping path. Only then run correctness, forced GC, and randomized full-suite A/B.

## Primary sources

- Deegen and its copy-and-patch/quickening/IC optimization inventory:
  <https://arxiv.org/abs/2411.11469>
- Copy-and-Patch stencil variants and supernodes: <https://arxiv.org/abs/2011.13127>
- Lazy and interprocedural basic-block versioning:
  <https://arxiv.org/abs/1411.0352> and <https://arxiv.org/abs/1511.02956>
- Typed object shapes and shape propagation: <https://arxiv.org/abs/1507.02437>
- V8 Maglev representation selection, loop phis, liveness, and register allocation:
  <https://v8.dev/blog/maglev>
- V8 hidden classes/properties and element-kind lattice:
  <https://v8.dev/blog/fast-properties> and <https://v8.dev/blog/elements-kinds>
- JavaScriptCore speculation, OSR exits, ICs, watchpoints, and type propagation:
  <https://webkit.org/blog/10308/speculation-in-javascriptcore/>
- JavaScriptCore polyvariant devirtualization and inlining:
  <https://webkit.org/blog/3362/introducing-the-webkit-ftl-jit/>
- SpiderMonkey CacheIR's guard/pure/result recipes and stub sharing:
  <https://firefox-source-docs.mozilla.org/js/cacheir.html>
- SpiderMonkey's SSA, scalar replacement, GVN, LICM, ranges, BCE, load/unbox folding,
  scheduling, and late edge-case passes:
  <https://firefox-source-docs.mozilla.org/js/MIR-optimizations/index.html>
- V8 escape-analysis overview: <https://v8.dev/blog/disabling-escape-analysis>
- V8 allocation-site presizing/pretransitioning/pretenuring:
  <https://research.google/pubs/memento-mori-dynamic-allocation-site-based-optimizations/>
- LuaJIT optimization inventory (forwarding, DSE, BCE, sinking, fusion, unrolling):
  <https://luajit.org/running.html>
- LLVM patchpoints/stackmaps as the model for explicit live-state obligations:
  <https://llvm.org/docs/StackMaps.html>
