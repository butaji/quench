# 389 — Primary-source and codebase opportunity audit

Status: complete

Audit the engine and the V8v7 execution frontier for additional general-purpose
algorithms and implementation patterns. This review used three independent read-only
code audits—runtime representation, stencil/cooker/linker, and per-suite dynamic
coverage—then checked the conclusions against primary papers and official engine design
documents. The accepted performance fact entering the review remains Task 381's exact
same-run candidate geometric mean of 2297.84. Reaching 10000 requires a 4.35x aggregate
gain; improving only one of eight suites by 10x would move the geometric mean by only
`10^(1/8) = 1.33x`.

## Corrected local facts

Several common recommendations are already implemented and must not be proposed as if
they were absent:

- `Value` is already an eight-byte NaN-boxed word, not a padded Rust enum.
- Ordinary objects already use canonical shapes, offset slots, direct own-property ICs,
  dense indexed storage, and a custom tracing stable-cell heap.
- Lexical environments already have a depth-plus-slot `NameIc` fast path.

The remaining representation costs are narrower: strings and functions still use
reference-counted Rust allocations, property values are out-of-line, object headers carry
metadata for mutually exclusive payload kinds, guest activations are cached only one deep
per call site, and capture analysis heap-promotes more lexical state than necessary.

## What the sources add

1. **Context-driven basic-block versioning (BBV).** Instead of running a heavyweight
   global optimizer, clone a bounded number of block versions keyed by an incoming type
   and shape context, propagate the context while selecting stencils, and fall back to a
   generic version when the version budget is exhausted. Published JS experiments report
   removing 71% of type tests on average and speedups up to 50%. Typed shapes plus shape
   propagation later removed 48% of tests and reduced execution time 25% across the
   paper's suite. In this VM the admissible variant is deterministic first-execution
   multi-versioning with named global limits—not a hotness detector:
   <https://arxiv.org/abs/1411.0352> and <https://arxiv.org/abs/1507.02437>.
2. **A compact SSA-like planning context can stay cheap.** Maglev uses one liveness and
   loop prepass, forward abstract interpretation, representation selection, and a simple
   forward register allocator. The transferable algorithm is not “add another compiler
   tier”; it is to let the immutable quoted region carry value identity, known type/shape,
   and physical location so guards, boxing, loads, and stores disappear across stencil
   boundaries. Split tagged and untagged spill areas keep GC metadata simple:
   <https://v8.dev/blog/maglev>.
3. **PIC dispatch chains should compose property lookup and the call.** The original
   polymorphic-inline-cache work reports Richards 52% faster than without PICs, and JSC
   treats property and call ICs as both fast execution and nearly free type feedback.
   This supports a general `GetMethod + GuardCallee + EnterGuest + ReturnContinuation`
   recipe rather than separate Rust crossings:
   <https://jnamaral.github.io/CDOL/papers/HolzleECOOP91.pdf> and
   <https://webkit.org/blog/10308/speculation-in-javascriptcore/>.
4. **Shapes must become propagating context, not repeated guards.** JSC structures carry
   names, attributes, offsets, and often the prototype; transition/watchpoint machinery
   keeps common objects cacheable. V8 distinguishes in-object properties from an
   out-of-line store. The local engine already has shapes, so the next algorithms are
   shape propagation, prototype watchpoints, in-object slots, transition caching, and a
   dictionary fallback—not another string map:
   <https://webkit.org/blog/10298/inline-caching-delete/>,
   <https://v8.dev/blog/fast-properties>.
5. **Allocation should be a stencil fast arm.** WebKit's current design writes the bump
   pointer and object header in generated code, calling a shared helper only when the
   allocation buffer is exhausted. Segregated fixed-size cells also amortize allocator
   metadata and improve locality. Apply this to object, guest-frame, and closure
   allocation after their headers are split and compacted:
   <https://webkit.org/blog/17899/introducing-the-jetstream-3-benchmark-suite/> and
   <https://webkit.org/blog/12967/understanding-gc-in-jsc-from-scratch/>.
6. **Allocation sinking is a region rewrite.** Once property and delete/call behavior is
   visible as quoted effects, non-escaping objects, argument arrays, and closure shells
   can be scalar-replaced. This is a pure `StencilExpr -> StencilExpr` rewrite before
   final link; exceptions, identity observation, and escaping references are explicit
   blockers: <https://webkit.org/blog/10298/inline-caching-delete/>.
7. **Elements require a representation lattice.** Keep named properties separate from
   indexed elements and transition monotonically among packed/holey I32, F64, and tagged
   forms. A region that guards the kind once can use direct base-plus-index loads with
   loop-carried bounds facts: <https://v8.dev/blog/elements-kinds>.
8. **Copy-and-patch needs variants and register pass-through, not only a larger catalog.**
   The source algorithm plans/selects first, then performs one DFS copy-and-patch pass;
   its speed comes from prebuilt implementation variants with literals, slots, branches,
   and register choices patched into them. Deegen combines that with pinned state,
   specialization, IC slabs, type-check removal, slow-path extraction, and code layout:
   <https://arxiv.org/abs/2011.13127> and <https://arxiv.org/abs/2411.11469>.
9. **The linker representation should be a graph.** Model content blocks, external
   kernels, labels, and relocation edges first; lay them out, allocate final RW memory,
   apply fixups once final addresses exist, then publish RX. This keeps immutable kernels
   shared and removes the current template-instance-image-mmap copy chain. LLVM JITLink
   is the design reference, not a runtime dependency:
   <https://llvm.org/docs/JITLink.html>.

## Dynamic opportunity map

Residual block entries are structural reach counts, not predicted speedups. They show why
the next work must close large dependency chains:

| Suite | Largest residual families observed |
|---|---|
| Richards | Call 404,621; And 126,690; Or 47,316; Shr 9,990 |
| DeltaBlue | Call 504,164; Construct 11,842 |
| Crypto | GetComputed 294,924; And 213,377; Shl 210,176; Shr 209,187; Call 176,252 |
| RayTrace | Call 227,553; Construct 133,134; unary negate 11,652 |
| Earley-Boyer | Call 997,225; StoreName 422,461; Construct 355,812; InstanceOf 224,287 |
| RegExp | Call 223,793; string literal 41,236; StoreName 32,968; bitwise about 32,958 each |
| Splay | NewObjectFromRegisters 594,722; Call 418,928; string literal 302,095; NewArrayFromRegisters 302,090 |
| Navier-Stokes | only 297 residual block entries while numeric regions execute about 1.77 million iterations |

Native sampling supplies the dependency cost hidden by counts. `dyn_block_step_impl`
accounts for about 34% of Crypto samples and 39% of Richards samples. Richards also pays
about 12.5% in guest-slot reset and roughly 17% across execution wrapping, call-argument
IC, reusable completion, call IC, setter, and binding initialization. Therefore a leaf
`Call` stencil that still enters the Rust helper is not closure of the measured path.

## Ranked implementation portfolio

### P0 — Make physical composition expressive

1. Generalize the cooker from named special cases to
   `PatchSite { offset, encoding, binding }`, with named encodings for AArch64 branch,
   conditional branch, load/store displacement, add-immediate, mov-wide immediate,
   literal load, raw value, and pointer bindings (Tasks 316, 367).
2. Materialize bytecode/site state only at observable exits and slow edges (Task 366).
3. Carry bounded live I32/F64/tagged values in fixed physical locations across a region,
   with explicit entry/exit morphisms and spills (Tasks 158, 292, 309, 385).
4. Use a weighted cover over immutable recipes instead of greedy pattern order. Cost
   guards, conversions, spills, code bytes, relocation count, seams, and slow-edge
   probability; the result is a normalized free sequence before one effectful link
   (Tasks 157, 377).

### P0 — Remove the whole call boundary

5. Build a general property-load/method-call PIC slab and direct guest-call/return edge:
   shape/prototype guard, slot load, callee guard, frame link, direct branch-and-link, and
   native return continuation. The miss alone may enter Rust (Tasks 145, 152, 368, 379).
6. Replace the one-entry callsite activation cache with depth-aware stack/arena guest
   frames, and reset only live argument/local ranges. This targets the measured Richards,
   DeltaBlue, Earley-Boyer, and Splay costs (Tasks 146, 168, 336, 379).

### P1 — Reduce allocation and memory traffic

7. Split the current object into compact payload kinds and add a small named in-object
   slot prefix; preserve a shared shape and out-of-line overflow store (Tasks 156, 369).
8. Split immutable `FunctionKernel` metadata/code from lightweight environment-bearing
   `FunctionInstance`, then make closure creation a bump allocation plus field stores
   (Tasks 14, 16, 42, 147).
9. Compute actual free-variable sets and heap-promote only escaping captured slots; use a
   stack/register frame for non-capturing calls (Tasks 41, 76).
10. Intern identifier/property-key atoms and make immutable atom strings non-refcounted;
    then add views/ropes or small strings for genuinely dynamic text (Tasks 11, 230, 280).
11. Add packed/holey I32/F64/tagged array backings and hoist kind/bounds guards across
    loops (Tasks 165, 233, 265, 385).

### P1 — Make publication and measurement honest

12. Link directly into the final RW mapping and publish RX once; keep kernel nodes as
    shared external code edges (Task 348).
13. Add non-perturbing per-template entry and slow-edge counters. Existing coverage
    modes disable the direct-stencil path, so they cannot validate native reach (Tasks 81,
    331, 382).
14. Use static block layout, tail merging, and hot/cold segmentation only after the new
    graph and native sampling identify actual I-cache/branch costs (Tasks 276, 289, 322).

## Lisp/category shape

There is one quoted representation and one effectful edge:

```text
DynCode
  -> RegionPlan<ValueId, Context, Effect, Exit>
  -> rewrite*                 ; shape/type propagation, scalar replacement, fusion
  -> costed cover             ; KernelRef | StencilTemplate(bindings)
  -> CodeGraph                ; labels + typed relocation edges
  -> link once                ; final RW bytes, then RX publication
```

The category objects are physical contexts; morphisms are recipes/stencils with explicit
effects and exits. Shared immutable `Kernel` nodes and patched `StencilInstance` nodes are
both arrows in the same graph and compose through the same typed connector. `seq` is a
normalized free monoid at each compatible context; branches are labeled graph edges;
loops close a typed backedge. No abstraction level is represented as thousands of leaves:
opcode, block, loop, function, and program are all closed composites of the same quoted
nodes, and any closed composite may be frozen as a reusable template or shared kernel.

## Decision

Do not add more isolated bitwise, literal, property, or call leaves. The next smallest
foundational experiment is a general immediate/raw-value patch encoding plus one real
bounded register-resident chain, accepted only if disassembly proves that its conversion
and frame/site seams disappeared. The next strategically large vertical slice is the
property-load plus direct guest-call/return recipe, because calls dominate six suites.
Both must use general semantic recipes, named limits, and whole-suite validation—never
benchmark identity, a hotness counter, or V8v7-shaped pattern selection.
