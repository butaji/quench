# 244 — Software prefetch instructions for proven strided numeric loops

Status: planned

Memory-*access* gap distinct from every existing array/numeric-loop task: [[32]],
[[39]], [[120]]–[[124]] establish that a loop's array accesses are guarded,
packed-numeric, and safe to lower to a native traced loop, but none of them emit a
prefetch instruction for the loop's own predictable future accesses — the CPU's
hardware prefetcher handles simple unit-stride access reasonably well on its own, but a
compiler-inserted software prefetch (`llvm.prefetch`, matching LLVM's own
`LoopDataPrefetch` pass) still measurably helps for larger strides, and this project's
AOT stencil templates ([[02]]) are exactly the place such an intrinsic belongs: emitted
once, at cook time, into the template every traced numeric loop instance reuses, not
computed per-execution.

navier-stokes is the direct corpus target: it is a fixed-grid numerical simulation
(the `js-engine-benchmark` navier-stokes suite), which is structurally a strided
array-traversal workload — exactly LLVM's own `LoopDataPrefetch` pass's target shape,
and exactly the suite already showing the largest single-task percentage gains in this
project's ledger (see [[123]]'s 346.44% Navier-Stokes improvement, [[89]]'s 23.58%),
suggesting its access patterns are already a proven hot spot worth this additional pass.

Concrete steps:
1. Confirm navier-stokes's actual grid-array access stride(s) directly from
   `/private/tmp/js-engine-benchmark/v8-v7/navier-stokes.js` (per this project's
   standing discipline of verifying against the real corpus rather than assuming a
   textbook stride shape) — do not proceed on an assumed access pattern.
2. Extend [[123]]/[[124]]'s traced numeric/dense loop stencil family with a
   prefetch-emitting variant: once a loop's index recurrence is proven affine (the same
   precondition [[233-abcd-exact-bounds-check-erasure]] already establishes for bounds-
   check erasure — this task should reuse that proof, not re-derive it), the stride is
   known at compile time and a fixed-distance-ahead prefetch can be emitted directly
   into the AOT template.
3. Name the prefetch distance as an explicit policy constant tuned from measurement
   (matching LLVM's own `PrefetchDistance`/`MaxPrefetchIterationsAhead` configuration
   knobs, cited below), not left as an unexplained number.
4. Gate emission on the stride actually being "large enough to be beneficial" — LLVM's
   own `isStrideLargeEnough` check exists because prefetching a unit-stride access the
   hardware prefetcher already handles can be pure overhead; this task must include the
   same gate, not prefetch unconditionally.

Acceptance: navier-stokes's dominant grid-traversal loop(s) compile with an emitted
software-prefetch instruction at a measured, justified distance, verified by
disassembly; a unit-stride loop correctly receives no prefetch (the beneficial-stride
gate correctly excludes it), verified by a negative test; alternating A/B on
navier-stokes specifically shows a measured gain, and the full V8v7 suite shows no
regression on suites without qualifying strided loops.

Primary sources:
- LLVM `LoopDataPrefetch` pass (the direct AOT-time analog this task adapts):
  <https://llvm.org/doxygen/LoopDataPrefetch_8cpp_source.html>
- Clang `__builtin_prefetch` / `#pragma clang loop prefetch`, for the explicit-annotation
  alternative if autovectorization-style automatic insertion proves too coarse for this
  project's specific traced-loop shape: <https://discourse.llvm.org/t/rfc-adding-support-pragma-clang-loop-no-prefetch-for-prefetch/68597>

Source for corpus evidence: `/private/tmp/js-engine-benchmark/v8-v7/navier-stokes.js`
(local V8v7 corpus checkout); prior measured gains:
`reports/task123-traced-region-ab-6/comparison.txt` (346.44% Navier-Stokes improvement).
