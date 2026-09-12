# 314 — Pure-diamond if-conversion stencil tiling

Status: planned

Recognize a tiny SSA diamond whose two arms are speculatable, non-throwing, non-allocating,
ownership-neutral pure expressions and whose join selects one word. Replace it with a
typed predicate plus select micro-op and tile that form with a rustc/LLVM-cooked stencil
that lowers to the target's conditional-select idiom (AArch64 `csel`/`csinc`/`csetm`
where profitable).

The legality predicate derives entirely from Task 309's effect/exception/ownership facts.
The cost model compares both eager arm costs plus select latency against branch and
layout costs; named maximum arm-operation and code-byte constants bound the transform.
No runtime branch profile or hotness counter participates. Failure to prove either arm
safe preserves the original coproduct/branch.

This is a normal rewrite `diamond -> select` followed by ordinary stencil tiling, so the
result remains composable at block, loop, and function levels. It does not add a special
runtime node or hand-coded assembly.

Acceptance: semantic/property tests cover NaN, signed zero, overflow, and false-path
non-evaluation; effectful/throwing arms are rejected structurally; extracted AArch64
code proves conditional-select lowering for at least one family; complete alternating
V8v7 A/B decides retention.

Primary source: LLVM's target-aware machine if-conversion implementation and legality
checks, <https://llvm.org/doxygen/IfConversion_8cpp_source.html>.

Depends on Tasks 54, 62, 128, 164, 171, 172, 173, 209, 271, and 309.
