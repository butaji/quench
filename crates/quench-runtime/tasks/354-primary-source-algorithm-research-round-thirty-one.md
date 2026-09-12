# 354 — Primary-source algorithm research, round thirty-one

Status: complete

Re-evaluate the next score experiments after the measured rejection of Tasks 352 and 353.
The research question is narrow: which general VM structures remove an executed boundary
large enough to move V8v7, while preserving first-execution stencil compilation, immutable
kernels, copy-patched instances, and the no-third-party-VM constraint?

## Primary-source findings

Deegen's baseline JIT removes interpreter dispatch, burns bytecode and IC constants into
instructions, splits cold slow paths, and identifies polymorphic ICs as its most important
high-level optimization. Its crucial implementation detail is that IC stubs are not ABI
functions: they operate directly on the surrounding JIT machine state and may branch to
different in-function continuations. The inline slab begins as a miss jump and is patched
with a cache arm after observation. Source:
<https://arxiv.org/abs/2411.11469>, especially Sections 7.1–7.2.

V8 Sparkplug validates direct bytecode-to-machine lowering without an IR and a frame layout
compatible with other execution modes. It also states what this buys: decoded operands and
next-bytecode dispatch disappear. Our shared Rust block executor does not yet meet that bar
merely because a native wrapper reaches it. Source: <https://v8.dev/blog/sparkplug>.

JavaScriptCore's baseline template JIT puts integer arithmetic guards and operations in the
machine-code template, and treats comprehensive property/call polymorphic ICs as the other
main source of its speedup. Its optimizing tiers reuse a monomorphic structure fact as one
shape check followed by direct field loads. Sources:
<https://webkit.org/blog/10308/speculation-in-javascriptcore/> and
<https://webkit.org/blog/3362/introducing-the-webkit-ftl-jit/>.

SpiderMonkey CacheIR supplies a suitable immutable recipe normal form: guards, idempotent
pure operations, and one terminal result. Stub fields such as shapes and slots are separate
from immediates, allowing one code kernel to serve multiple sites; specialized chains widen
to megamorphic and generic states under bounded policy. Source:
<https://firefox-source-docs.mozilla.org/js/cacheir.html>.

Lazy BBV eliminated 71% of dynamic type tests and reported speedups up to 50% with five
versions per block. Typed shapes combined with BBV eliminated 48% of relevant tests and
reduced execution time by 25% across its corpus. Interprocedural BBV reports known callee
identity for 90% of executed calls and 94.3% type-test elimination after entry and call-
continuation specialization. Sources: <https://arxiv.org/abs/1411.0352>,
<https://arxiv.org/abs/1507.02437>, and
<https://doi.org/10.4230/LIPIcs.ECOOP.2016.7>.

## Local evidence and ranked experiments

1. **Fused machine-state property-call IC slab** — Tasks 145, 146, 153, 181, and 316.
   Current profiles count millions of inherited property and direct-call hits in Richards,
   DeltaBlue, and RayTrace. One hit arm must guard receiver shape, load the method slot,
   guard the callee, push the guest frame, jump to its linked entry, and return through a
   patched continuation without a Rust call. This has the highest plausible ceiling.
2. **Burn every ordinary operand into native templates** — Tasks 36 and 316. A copied
   stencil that reloads `InlineSite` and enters `dyn_block_step_impl` has not removed
   decoding. Require direct frame/register offsets, literal bits, IC pointers, and
   continuations in the instance, with cold semantics outlined.
3. **Static typed block versions** — Tasks 144, 152, 158, and 329. Use compiler-worklist
   demand, not runtime heat, to create bounded versions keyed by tag, shape, and location.
   One entry proof must feed every dominated property/arithmetic operation; values stay in
   registers until a real effect edge.
4. **CacheIR-like recipe sharing** — Task 153. Keep `IcExpr` immutable and hashable. A
   shared `Kernel` serves recipes whose concrete fields remain in data; a
   `StencilTemplate -> StencilInstance` path burns fields only when its lower seam cost
   repays copied bytes. Both are morphisms with identical connector contexts.
5. **Costed whole-block/loop cover** — Task 157. Select among kernels, IC arms, primitive
   stencils, supernodes, and typed block versions by removed helper calls, loads, guards,
   ownership traffic, and bytes. Stencil count alone is not a useful objective.

## Process correction

Every new performance candidate now needs a preflight record before implementation:
dynamic reach, complete boundary removed, expected no-helper disassembly, maximum code/data
growth, and semantic fallback. Tasks 352/353 prove that a real hit counter is necessary but
not sufficient. For call work, fewer than roughly one million affected executions per
200 ms suite window is diagnostic unless the fragment composes into a larger region.

No new duplicate implementation subsystem is created. This research updates the existing
canonical tasks and the Task 301 order. V8v7 remains a holdout acceptance suite; source
names, benchmark identities, and runtime hotness thresholds are forbidden selectors.
