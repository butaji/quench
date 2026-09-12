# 360 — Primary-source algorithm research, round thirty-three

Status: complete

Research the next general-purpose score experiments after Task 357 made the guest return
continuation explicit, Task 358 accepted traced-object native transfers, and Task 359
proved that a selected coarse property-store stencil can still execute entirely on its
slow edge. Preserve the standing constraints: OXC plus Rust, no third-party VM, stencil
execution from first entry, no runtime hotness threshold, no benchmark/source identities,
and rustc/LLVM only in the build-time cooker.

## New local evidence

A current five-second Richards sample attributes **55.01%** of samples to
`dyn_block_step_impl`; the next largest leaves are `executes` at **11.13%** and
`reset_value_slots` at **8.64%**. Property/call helpers are individually smaller because
the generic block executor owns the enclosing work. The eight-suite 20 ms census is in
`reports/task358-current-residual-census/`.

Task 359 selected three new direct blocks, passed all 132 tests, and still sent the exact
target shape to the generic executor **1,280,750** times in a 100 ms Earley-Boyer run.
The native arm could copy traced object handles, numbers, and immediates, but correctly
rejected stores whose source or displaced value retained Rust `Rc` ownership. Its complete
A/B was -0.31%, so the implementation was removed. This proves that syntax-level stencil
coverage is no longer the right unit: ownership and destruction must be quoted effects.

## Primary-source synthesis

Deegen's baseline JIT burns bytecode and IC-derived expressions into instructions, moves
cold logic into AOT paths, eliminates next-bytecode jumps by fallthrough, and makes IC
stubs operate on the surrounding machine state. Its own evaluation says the baseline JIT
is weakest on number-crunching loops because copy-and-patch baseline code is not an
optimizing tier by itself. Source: <https://arxiv.org/html/2411.11469>, especially
Sections 5.3, 7.1, 7.2, and 8.3.

V8 Sparkplug and SpiderMonkey Baseline confirm the useful baseline invariant: translate
already-resolved bytecode directly, retain a compatible frame, eliminate operand decoding
and dispatch, and use ICs for dynamic specialization. They do not claim that templates
alone eliminate tagging, frame traffic, or helper semantics. Sources:
<https://v8.dev/blog/sparkplug> and
<https://firefox-source-docs.mozilla.org/js/how-we-optimize.html>.

Swift Ownership SSA supplies the right representation for the Task 359 failure: values
are `Owned`, `Guaranteed`, or trivial; copying and destruction are explicit operations;
each owned value has one lifetime-ending use on every path. CPython's current JIT reports
that explicit unique-reference tracking permits reference-count removal and register
allocation, and Rust exposes supported raw `Rc` increment/decrement operations for an
intermediate implementation. Sources:
<https://github.com/swiftlang/swift/blob/main/docs/SIL/Ownership.md>,
<https://github.com/python/cpython/blob/main/Doc/whatsnew/3.15.rst>, and
<https://doc.rust-lang.org/std/rc/struct.Rc.html>.

For the permanent representation, JSC uses one-word tagged values pointing at GC cells,
allocates young objects in an eden generation, and makes the common write barrier a small
cell-state test. V8 likewise separates fast named-property slots from indexed elements and
uses shapes plus element-kind specialization. Sources:
<https://github.com/WebKit/WebKit/blob/main/Source/JavaScriptCore/runtime/JSCJSValue.h>,
<https://webkit.org/blog/12967/understanding-gc-in-jsc-from-scratch/>,
<https://v8.dev/blog/fast-properties>, and <https://v8.dev/blog/elements-kinds>.

For C-like regions, Maglev's compact SSA CFG and lazy basic-block versioning give the
smallest credible next layer. BBV carries a type context through successors and reports
eliminating 71% of dynamic type tests; adding typed shapes reports 25% mean execution-time
reduction in its evaluated corpus. These justify an experiment, not a projected local
gain. Sources: <https://v8.dev/blog/maglev>, <https://arxiv.org/abs/1411.0352>, and
<https://arxiv.org/abs/1507.02437>.

## One representation across every abstraction level

Keep a single quoted `RegionExpr` whose leaves are semantic micro-operations and whose
nodes describe control and composition. Primitive operations, basic blocks, loops,
functions, inlined callees, and IC arms differ only in region boundaries and context;
they are not parallel stencil systems.

The compile-time context is the product:

`Context = Representation × Location × Shape × Ownership × Effects`.

A lowered code piece is a typed morphism `CodePiece<In, Out>`. `Kernel` and patched
`StencilInstance` inhabit the same category and obey the same connector contract; they
differ only under a final physical-realization functor:

- choose a shared immutable `Kernel` for large or parameter-invariant cold/effect logic;
- choose `StencilTemplate -> StencilInstance` when burning operands, shape IDs, offsets,
  constants, or branch targets removes work from the successful edge;
- hash-cons an already-patched instance only when its complete patch vector and connector
  context are identical.

Effects form explicit obligations accumulated during composition. In particular,
`StoreField` carries `Take | Copy` source ownership, displaced-value destruction, and
`NoBarrier | Barrier` heap obligations. These are resolved by proofs and tiling, never by
an ad-hoc tag branch hidden inside each AOT handler.

The Lisp staging boundary stays strict:

`quote bytecode -> derive SSA/facts -> macroexpand reducers to fixpoint -> costed tile ->`
`one size/layout pass -> one copy/patch/link effect`.

## Ranked experiments

1. **Ownership-taking property stores — Task 172 over Tasks 128/145.** Lower a proven
   last-use property source as `StoreTake`, transfer its owner to the property slot, clear
   the source frame slot, and schedule one explicit `DestroyValue` for the displaced
   value. Start with terminal and no-observer regions, but select by ownership/liveness
   facts rather than opcode-string shape. A rustc-cooked `Rc` destroy leaf is only an
   intermediate bridge; Task 148 ultimately makes all heap references trivial word moves.
   Re-attempt Task 359's grammar only after counters prove the store effect itself stays
   native.
2. **Finish the guest call continuum — Tasks 146/181, then 145/153.** Task 357 provides
   the explicit return target and multi-entry image. The remaining hit edge must reserve
   the callee frame, transfer arguments, jump to `guest_entry`, and return through the
   patched continuation without a Rust ABI call. Then compose
   `GuardShape ; LoadSlot ; GuardCallee ; EnterGuest` as an in-function IC arm.
3. **Make native cover total — Tasks 36/37/128/157/272/316/348.** Replace the binary
   choice “recognized coarse block or `dyn_block_step_impl`” with a costed cover that can
   always tile a block from primitive stencils and shared effect kernels. Burn every
   register/local offset, literal, IC-data address, and control target. The acceptance
   metric is eliminated generic entries and host calls, not selected stencil count.
4. **Static context-versioned loop regions — Tasks 144/152/158/171/326.** At load time,
   build bounded versions keyed by number kind, shape, element kind, and value location.
   Guard once at an entry, keep loop-carried values in registers, and rebox/materialize
   only at explicit exits. Compiler-worklist demand selects versions; runtime heat never
   does.
5. **Complete the stable tracing heap — Tasks 148/162/193/320/321.** Move strings,
   functions, regexps, and captured environments out of transitional `Rc` ownership into
   traced stable cells. Add precise safepoint maps, then a non-moving young generation and
   composable store barrier. This makes ordinary copies and property/call edges one-word
   operations and removes Task 359's ownership class of failures permanently.
6. **Representation-specialized arrays — Tasks 12/165/214/233/265.** Use a monotone
   `PackedI32 -> PackedF64 -> PackedValue -> Holey/Dictionary` lattice, direct indexed
   stencils, and loop-entry bounds proofs. This is the main path for Crypto,
   Navier-Stokes, and numeric portions of RayTrace after typed regions exist.
7. **Recipe-derived builtin kernels — Tasks 183/212/227/280 and 87.** Express common
   builtins as CacheIR-like `guards ; idempotent operations ; terminal effect` recipes so
   the same definition produces a shared kernel or an inline instance. Prioritize only
   after per-builtin counters establish reach; regex requires a real native matcher rather
   than wrapping the current matcher in a stencil.
8. **Use rustc/LLVM harder at build time — Tasks 43/52/203/209/276/322/347.** Generate
   semantic variants with Rust macros, pin connector/tag state, mark slow paths cold,
   compare named cooker configurations, split hot/cold sections, and verify disassembly.
   A successful fast leaf must contain no `InlineSite` operand decoding, unintended
   prologue/epilogue, or host helper call.

## Rejected directions

- another exact multi-op pattern whose internal effect still takes the generic edge;
- a stencil that merely wraps `dyn_block_step_impl` or a Rust call helper;
- per-transfer ownership predicates repeated in every leaf;
- duplicated block/loop/function stencil types instead of one hierarchical `RegionExpr`;
- runtime hotness thresholds, benchmark names, property names, or source identities in
  selection;
- code-size growth without a predicted helper/load/guard/branch removal.

## Routine for every next candidate

Before coding, record the dynamic reach, exact semantic boundary removed, expected fast
path disassembly, copied/shared byte budget, and fallback morphism. Add counters for each
guard rejection reason, not only “selected.” After semantic tests, run an alternating
complete-suite A/B and reject any candidate that misses the standing aggregate/component
floors. Re-run the residual census whenever one of the generic executor, guest call,
ownership, or region boundaries materially changes.

