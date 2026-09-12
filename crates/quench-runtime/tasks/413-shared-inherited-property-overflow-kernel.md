# 413 — Shared inherited-property overflow kernel

Status: complete

Preserve one monomorphic inherited-property guard chain in each copied property stencil and
move all secondary PIC ways into one immutable executable Kernel shared by every site and
function image. This directly follows the Task 410–412 arity sweep: additional ways are
valuable for Richards, while copying them into every StencilInstance loses globally through
code size and instruction-cache pressure.

The quoted composition has two compatible categorical levels:

`Connector -> InlinePropertyPrimary -> PropertyOverflowContext`

`PropertyOverflowContext -> SharedPicKernel -> Connector`

`PropertyOverflowContext` is a fixed internal ABI containing the frame/site, receiver and
prototype facts already loaded by the copied primary stencil, plus instance-specific next
and slow continuations. The rustc/LLVM-cooked shared kernel folds over a bounded immutable
array of secondary cache-way data and tail-transfers to the supplied continuation. Its code
bytes are mapped once and referenced by address; only per-site data grows. There is no
runtime instruction selection, key/source/benchmark special case, hotness detector, or
interpreter fallback.

Keep the kernel cold in the Lisp staging sense: one immutable template-level definition,
one link-time address, and effects confined to cache fill/invalidation. Source cache state,
published raw cache state, GC filtering, and tests must derive from one normalized way
representation and named bounds. The copied primary and shared overflow kernel are both
morphisms and must compose under the existing typed connector interface.

Acceptance: ABI/layout assertions; semantic and native tests for primary hit, every overflow
way, replacement, mutation, and GC invalidation; disassembly proves one copied guard chain
and one indirect tail edge to a separately mapped closed kernel; image accounting proves
kernel code memory is constant as sites/functions grow; Richards miss/fill counters fall;
and the randomized nine-pair exact V8v7 gate has a strictly positive aggregate confidence
interval with no component-floor violation.

## Result

Rejected and removed. Two general-purpose layouts were tested against the accepted Task 385
baseline:

1. Routing overflow lookup from every copied property stencil reduced Richards misses, but
   enlarged the plain `GetStatic` stencil from 176 to 264 bytes. The full component screen in
   `reports/task413-shared-overflow-kernel-screen-3/comparison.txt` measured a -0.06% aggregate
   change, including -4.60% on RegExp and -0.85% on Splay. It did not earn the randomized gate.
2. Restricting the overflow edge to fused property-call stencils avoided the broad code-size
   tax. In the same 20 ms Richards preflight window, prototype misses fell from 49,699 to 7,474,
   but score fell from 1,203 to 785 (-34.75%). Call-cache misses also rose from 26,474 to 30,222.
   The persistent raw outputs are in
   `reports/task413-call-only-overflow-kernel-preflight/`.

The result is useful despite rejection: cache-hit rate is not the objective. The extra helper
edge, guard loop, and larger site state cost more than the recovered inherited-property hits.
The source is restored to the one-way monomorphic inherited-property cache; there is no
overflow-kernel residue in the runtime or AOT handlers. Build and all 159 release tests pass.
