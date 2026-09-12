# 363 — Primary-source algorithm research, round thirty-four

Status: complete

Find additional general-purpose techniques that can move the stencil-only VM toward
C-like execution while preserving the standing constraints: OXC plus Rust, no
third-party VM, no runtime LLVM, no interpreter fallback, no runtime hotness threshold,
no V8v7/source identities in selection, and one quoted semantic source lowered to
rustc/LLVM-cooked `StencilTemplate` instances or shared immutable `Kernel` values.

## Decisive local evidence

The Task 358 residual census shows that adding stencil families has not made native
execution total. Counting residual block-kernel entries against native entries gives:

| Suite | residual block entries | native entries | residual share |
|---|---:|---:|---:|
| Crypto | 556,282 | 198,986 | 73.65% |
| DeltaBlue | 1,108,414 | 679,978 | 61.98% |
| Earley-Boyer | 3,656,064 | 1,693,375 | 68.34% |
| Navier-Stokes | 6,005 | 575 | 91.26% |
| RayTrace | 1,107,812 | 429,901 | 72.04% |
| RegExp | 274,411 | 33,276 | 89.19% |
| Richards | 890,564 | 364,410 | 70.96% |
| Splay | 1,872,667 | 767,925 | 70.92% |

These counters are not instruction counts and must not be compared as cycles. They do
prove the structural failure: one unsupported effect still demotes a large block to the
generic Rust executor. Task 361 fixed one high-frequency constructor family and improved
the aggregate by 0.72%, but exact-pattern work cannot close a roughly fourfold score gap.

The new research therefore does not justify another hand-written superinstruction. It
justifies making native coverage total, then carrying proofs and values across that
continuous native graph.

## Primary-source synthesis

Deegen's result is a product, not a single stencil trick: bytecode specialization,
register pinning, tag-register optimization, IC slabs, type-check removal, strength
reduction, outlined slow paths, and hot/cold layout all contribute. Copy-and-patch is the
physical emitter that makes those already-specialized semantics cheap to instantiate;
it is not an optimizing analysis by itself. Sources:
<https://arxiv.org/abs/2411.11469> and
<https://compilers.stanford.edu/publications/copy-and-patch/>. The Deegen paper was first
submitted on 18 November 2024.

V8 explicitly describes Sparkplug's no-IR baseline design as having a low optimization
ceiling. Maglev crosses that ceiling with one compact SSA CFG, a liveness/loop prepass,
forward abstract frame interpretation, specialized nodes, representation selection, and
a simple forward register allocator. Proven numbers stay unboxed and in the appropriate
register bank; frame-state metadata exists only to reconstruct a generic continuation at
a side exit. Source: <https://v8.dev/blog/maglev>.

Static and lazy basic-block versioning establish the smallest useful specialization
algorithm. A bounded context map specializes blocks, propagates facts through successors,
and makes a failed specialization terminate at a side exit instead of merging generic
state back into the optimized loop. The published lazy implementation removed 71% of
type tests with a five-version bound; static BBV also removes overflow and bounds checks
without requiring a runtime hotness policy. These are external results, not projected
local speedups. Sources: <https://arxiv.org/abs/1411.0352> and
<https://doi.org/10.4230/LIPIcs.ECOOP.2024.28>.

Typed shapes extend the same block context with property representation and shape facts,
allowing one successful shape guard to license subsequent offset loads and eliminate
redundant checks. The evaluated system reports 48% fewer type tests, 17% less code, and
25% lower execution time over its corpus. Again, those figures justify the mechanism,
not a prediction for this VM. Source: <https://arxiv.org/abs/1507.02437>.

CPython's current copy-and-patch pipeline independently supports the staging choice:
specialized bytecode is decomposed into micro-ops, a whole sequence is optimized, and
only then are LLVM-cooked stencils patched into an executor. The stencil source is
generated from the same bytecode definitions, preventing a second semantic
implementation. Source:
<https://github.com/python/cpython/blob/main/InternalDocs/jit.md>.

The `weval` partial evaluator supplies the no-hotness version of whole-function
specialization: specialize an SSA CFG interpreter against fixed bytecode, preserve
reconvergent control flow and loops, and residualize only dynamic work. Its implementation
targets Wasm, so it is not a dependency; its context-specialization algorithm maps to the
existing Rust `RegionPlan` and build/load-time stencil selector. Source:
<https://cfallin.org/pubs/pldi2025_weval.pdf>.

V8's deoptimization explanation supplies a crucial optimization law: a failed guard is
a terminal edge. If generic slow behavior rejoins the optimized loop, its arbitrary
effects poison liveness, load elimination, and escape facts. In this VM the destination
is another always-stencil, general-purpose continuation—never an interpreter. Source:
<https://v8.dev/blog/wasm-speculative-optimizations>.

V8 and JSC object documentation confirms the later representation work: shapes identify
fixed property offsets; indexed elements are separate and become packed numeric storage
when possible; monomorphic ICs reduce access to a structure guard plus direct load/store.
Sources: <https://v8.dev/blog/fast-properties> and
<https://webkit.org/blog/10308/speculation-in-javascriptcore/>.

## One Lisp-shaped pipeline

Keep one canonical quoted program:

`DynCode -> RegionPlan<MicroOp> -> abstract-reduce to fixpoint -> costed cover ->`
`StencilExpr -> size/layout -> one copy/patch/link effect`.

Every fact belongs to one immutable product value:

`Context = Rep × Location × Shape × Ownership × Range × Effects`.

Operation, block, loop, function, inlined callee, and IC arm are nested regions over the
same `MicroOp` leaves. They are not separate stencil systems. Composition at every level
is sequence/CFG composition of typed morphisms; `Kernel` and `StencilInstance` differ
only in physical realization and remain connector-compatible.

## Ranked experiments to try

1. **Finish the hostless call continuum — Tasks 362, 146, and 181.** A successful user
   call must guard the target, initialize the reusable guest activation, jump to the
   post-prologue guest entry, and return through a patched continuation without entering
   the Rust ABI. Task 362 is already the active bounded experiment; do not interrupt it
   with another call-helper wrapper.
2. **Make the native cover total — Tasks 309, 157, 316, 348, and 131.** Decompose every
   bytecode into semantic micro-ops. Cover every block with primitive templates and
   custom-connector shared kernels, then let larger cooked regions compete by cost. An
   unsupported operation may invoke one explicit kernel edge; it must not demote the
   rest of the block to `dyn_block_step_impl`. Burn register/local offsets, literals,
   IC data, and continuations into holes.
3. **Add bounded static context versions — Tasks 171, 144, 152, 164, and 158.** Build
   versions at load time from statically derived and immediately observed IC facts, with
   named version limits. Guard once at entry, keep `I32`/`F64`, shapes, and locations in
   the context through the region, and materialize only on terminal side exits to generic
   stencil entries. This has no hotness threshold and never stops using stencils.
4. **Move IC success arms into the surrounding machine state — Tasks 145 and 153.** A
   property/call hit is `GuardShape ; LoadOffset ; GuardTarget ; Continue`, not a Rust
   function call. Misses select or widen a bounded immutable arm immediately and use a
   shared megamorphic kernel after the bound; the site remains copy-on-write so shared
   instances stay immutable.
5. **Propagate and eliminate effects — Tasks 172, 329, 191, 176, and 321.** Once the
   micro-op graph is continuous, forward loads/stores, transfer last-use ownership,
   scalar-replace nonescaping allocations, and omit barriers for freshly allocated
   objects. These transformations cannot pay while each bytecode rematerializes a
   canonical frame or calls a generic helper.
6. **Use representation-specialized storage — Tasks 165, 214, 265, and 233.** After
   typed regions exist, keep numeric fields and packed arrays unboxed, hoist element-kind
   and bounds guards to region entry, and generalize through explicit side exits. This is
   the credible C-like path for Crypto and Navier-Stokes.
7. **Complete the stable tracing heap — Tasks 148, 193, 320, and 321.** Move the remaining
   Rust-owned strings/functions/environments into trivial word handles, add precise maps,
   and make allocation a bump-pointer stencil with an outlined collection kernel. This
   removes ownership barriers that still prevent native composition.
8. **Optimize physical realization last — Tasks 189, 291, 322, 347, and 52.** Place
   shared kernels within direct-branch range, price copied bytes against seams removed,
   apply static block layout, and verify the rustc/LLVM cooker matrix in disassembly.
   Code placement cannot rescue generic semantic calls, so it follows the continuum.

## Immediate experiment contract

After Task 362, the next implementation should be one bounded vertical slice of Tasks
309/157/316: choose a general semantic family, lower it to micro-ops, prove that mixed
blocks remain natively covered on both sides of an unsupported effect kernel, and show in
disassembly that successful leaves contain no `InlineSite` operand decoding or host ABI
call. Instrument entry counts, helper edges, site loads, copied bytes, and side-exit
reasons. Accept only after the complete alternating V8v7 A/B gate.

Do not add a new semantic architecture, runtime optimizer, benchmark-trained template,
or hand-written exact block family. The existing task graph already contains the needed
work; this research round changes its priority and sharpens the acceptance boundary.
