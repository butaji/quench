# 158 — Register-resident CPS stencil planner

Status: in_progress

Plan register residence across a whole quoted region before emission, then select
rustc/LLVM-cooked stencil variants whose continuation connectors preserve those values.
The categorical object is the physical context `Gamma`: value representation plus its
fixed register or frame location. A variant is a morphism between contexts; composition
is legal only when the boundary contexts match.

Use a deterministic two-pass algorithm modeled on Copy-and-Patch: compute live ranges
and a bounded register plan, then choose pass-through, consume, produce, or spill
variants. Task 203 defines the pinned VM-state/tag connector registers; this task owns
only the remaining named pool of temporary value registers. Task 149 derives the finite
variants at build time; no runtime assembler or LLVM is added.

Start intra-region. Task 146 later permits compatible argument and return contexts to
cross calls. Exceptional/effect exits materialize the canonical frame state before
entering shared kernels.

Acceptance: representative numeric and property blocks keep live values in physical
registers across multiple stencil boundaries; frame load/store and `LoadLocal` counters
fall materially; spill/context law tests pass; disassembly and full V8v7 A/B pass.

Primary source: <https://arxiv.org/abs/2011.13127>.

## SSA allocation and edge connectors

Keep RegionPlan in SSA through register assignment. Construct live intervals directly
from SSA def/use and loop order, preserve lifetime holes, model pinned/fixed AArch64
registers as fixed intervals, and split an interval only when pressure requires it.
Resolve block arguments after allocation as one parallel-copy set per CFG edge. The
edge copier coalesces equal locations, emits acyclic moves in dependency order, and
breaks a remaining move cycle with the named scratch register or a named spill slot.

This is not a separate lowering subsystem. A resolved edge move is a composable
`Stencil<GammaPred, GammaSucc>` selected from the same finite AOT move/load/store
catalog; identity edges emit no bytes. Keeping phi elimination after allocation lets
the allocator coalesce loop-carried and join values before paying for moves, and avoids
the lost-copy and swap bugs of sequential phi lowering.

Primary sources: Wimmer and Franz's SSA linear scan algorithm, including integrated
SSA destruction <https://c9x.me/compile/bib/Wimmer10a.pdf>, and Boissinot et al.'s
correctness/code-quality separation for out-of-SSA translation
<https://doi.org/10.1109/CGO.2009.19>.

## Round-twelve refinement

Make the bounded allocator concrete: a liveness/next-use prepass computes linear live
ranges; one forward walk maintains the abstract physical-register state. Reuse an
existing register first, then a free register, then evict the value with the farthest
next use after preferring rematerializable values. Local reads/writes update abstract
state and emit no code when the required value is already resident. The canonical frame
slot carries a materialized/dirty fact and is flushed only at a `MayObserve`/`MayGc`
edge or an actual eviction.

This combines Maglev's documented prepass plus forward allocation with the single-pass
compiler abstract state in Titzer's survey; it remains a selector over finite cooked
stencil connectors, not a runtime assembler:
<https://v8.dev/blog/maglev>, <https://arxiv.org/pdf/2305.13241>.

CPython 3.15 independently validates the immediate copy-and-patch payoff: its official
JIT notes report that basic register allocation now avoids stack operations in stencil
traces. Borrow the algorithmic lesson only; this VM does not adopt CPython's runtime
trace recorder or hotness policy:
<https://github.com/python/cpython/blob/main/Doc/whatsnew/3.15.rst>.

Task 267 has landed the first bounded slice: a general five-operation numeric recurrence
is one cooked morphism and retains its arithmetic value across the former leaf
boundaries. It improved the complete-suite median aggregate by 0.74%, but only covers
one tail form. This task remains in progress until arbitrary SSA live ranges and CFG
edge transfer recipes use typed physical connector contexts.

Task 281 lands a second bounded slice: a four-bytecode local/literal arithmetic
expression writes its observable destination local directly and omits three dead
register materializations. Allowing a destination distinct from the source raises the
complete-suite aggregate from 1928.09 to 1941.53 (+0.70%). This confirms that values
retained across composition boundaries remain useful, but four unique Navier-Stokes
sites are not a substitute for the arbitrary SSA allocator required by this task.

Task 282 tested a first-class `NumberConnector` carrying an `f64` through separately
cooked producer, binary-step, and exit morphisms. The implementation was type-correct,
removed intermediate register-file traffic, and cut a representative five-bytecode
chain from about 192 to 100 copied bytes, but only two Crypto and three Navier-Stokes
sites selected it. A ten-pair focused A/B regressed 0.52%, while a five-pair full-suite
A/B produced a noise-level +0.11%. The 40-template family was rejected and removed.
This sharpens the remaining requirement: generate connector states from whole-region
allocation facts so a small, reusable vocabulary covers arbitrary SSA chains and CFG
edges; do not grow a handwritten source-by-operator cross product.

## Liftoff cache-state refinement

Use V8 Liftoff's production baseline algorithm as the concrete merge procedure. The
first predecessor creates the block's canonical cache state. Each later predecessor
derives a `TransferRecipe` without mutating that state: retain the same physical
register when compatible, reuse an already-equivalent register, take a free register,
then spill. The recipe is one parallel-copy set lowered to the existing finite
move/load/store stencil catalog. It must account for cycles exactly once through the
named edge scratch register or named edge spill slot.

Within a block, pin current operands before requesting destinations; select a free
register first, otherwise evict the lowest-priority live range. Compare the existing
farthest-next-use policy with JavaScriptCore's priority greedy order, using named and
deterministic weights for loop depth, use density, rematerialization cost, and cold
side-exit-only uses. The allocator choice is quote-stage data and must not introduce a
runtime mode or hotness threshold.

Sources: V8 Liftoff cache-state API and implementation
<https://chromium.googlesource.com/v8/v8.git/+/7b2b9233d6e1981781a1572e1cf935049ef06b0f/src/wasm/baseline/liftoff-assembler.h>,
and JavaScriptCore's greedy allocator report
<https://webkit.org/blog/17899/introducing-the-jetstream-3-benchmark-suite/>.

Round-fifteen prerequisite: arbitrary register-valued connectors require Task 270's
preserve-none continuation ABI. Internal tail transfers never return, so a C ABI
callee-save contract is semantically dead and can force the very spills this task exists
to remove. Keep Task 267's already-landed fused slice, but do not call the general
planner complete until disassembly proves the continuation ABI preserves no registers.

## Round-seven refinement

Classify values that are cheaper to recreate than spill—small constants, tag values,
immutable shape/kernel pointers, and fixed frame addresses—as rematerializable facts in
the register plan. Canonical side-exit state from Task 164 is a cold use: it must remain
reconstructible, but it must not force every value live only for a possible slow exit to
occupy a hot-path register or spill slot. JavaScriptCore's B3 backend documents optimal
constant materialization and OSR-exit-aware register allocation as separate requirements
of good dynamic-language code generation:
<https://webkit.org/blog/5852/introducing-the-b3-jit-compiler/>.

## Round-twenty-six register-bank refinement

Make the connector context record a physical register bank as well as a register identity.
Choose GPR for tagged words/pointers and FPR for proven F64 values; charge every cross-bank
copy in Task 157's tiling cost and prefer a whole connected numeric island in one bank.
This is one more field in the existing allocation fact, not a second allocator. LLVM
GlobalISel similarly performs register-bank selection before final instruction selection
and explicitly minimizes cross-bank copies:
<https://llvm.org/docs/GlobalISel/Pipeline.html>.

## Round-forty-three cache-state automaton

Vmgen's dynamic-superinstruction work supplies a useful concrete interpretation: cache
state and superinstruction selection are one product, not independent optimizations. Model
each region boundary as a normalized `ValueLocationMap` and each cooked stencil as a
transition between maps. Composition inserts a cache-state transfer only when contexts differ;
identical maps compose through the identity. This is the register-VM analogue of stack-cache
state transitions and prevents dispatch removal from merely exposing frame traffic.

Source: Vmgen <https://doi.org/10.1002/spe.434>.

## Round-forty-six alias-preserving multi-location state

Separate SSA value identity from the VM locals that currently name it. `Move` and a local
store may make several locals aliases of one value; keep one physical register location for
that value and update only the persistent name-to-value map. A later definition splits the
alias without forcing the untouched names to move. At a merge, value equivalence is retained
only when every predecessor agrees; otherwise normal block arguments and parallel copies
apply. Side exits materialize each required canonical slot from the shared location once.

This is stronger than adjacent copy deletion in Task 372 and smaller than general graph-
coloring: it is a property of the abstract cache state and therefore composes through the
same typed stencil connectors. Titzer's study calls the analogous baseline technique
“multiple register allocation” and reports that disabling it had a significant execution-
quality cost, sometimes larger than disabling constant tracking:
<https://arxiv.org/abs/2305.13241>.

Add counters for zero-byte alias updates, shared-location fanout, split aliases, merge loss,
and exit materializations. The first acceptance fixture must prove that `a=x; b=a; c=b`
retains one physical value and performs no frame traffic until an observable edge.

Task 416 lands the first alias-preserving slice: numeric moves and forwarded local reads can
bind another SSA name to an existing physical location, unique locations—not names—determine
lane pressure, and temporary conversions are keyed by physical source location. Its fixture
proves live two-name fanout without a physical move. Current V8v7 register regions contain no
surviving numeric moves, however, so the emitted function images were unchanged and no score
gain is claimed. Merge agreement, alias splitting at non-SSA boundaries, and one-per-location
exit materialization remain open here.
