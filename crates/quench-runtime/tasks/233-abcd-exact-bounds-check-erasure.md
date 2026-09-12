# 233 — ABCD-style array-bounds guard erasure as an exact-abstraction instance

Status: planned

Concrete variation of [[231-exact-galois-insertion-guard-erasure]] applied to
[[175-scalar-evolution-loop-predication]], which already cites ABCD (Bodik, Gupta &
Sagiv) as reporting up to 45% of dynamic bounds checks *eliminated*, not merely
cheapened, across a whole loop by widening a per-iteration `i < length` check into one
loop-entry envelope. [[175]]'s text does not yet state *why* this is sound in the same
terms [[231]]/[[232]] now give the rest of the guard-elision work — doing so both
strengthens [[175]]'s own correctness argument and gives it the same erasure standard
(zero remaining check inside the loop body, not a cheaper one).

Restate the ABCD transformation as an exact-abstraction instance: the per-iteration
bounds check `0 ≤ i < length` is a classifying map `χ_m` (per [[232]]) for the subobject
"indices this iteration is licensed to read." Once scalar evolution ([[175]]) proves `i`
follows an affine recurrence bounded by the loop's own entry/exit condition, the *set* of
`i` values the loop body will ever see is known exactly at compile time — the same
"nothing left to distinguish at runtime" condition [[231]] states for its three sites.
The one entry-envelope check (`0 ≤ i₀` and `i₀ + step × (iterations - 1) < length`) is
where the real, non-erasable classifying map lives; every per-iteration check inside the
loop body is checking a subset already proven contained, i.e. its own `χ_m` is
provably constant-true, and should be erased entirely, not retained-and-cheapened.

Concrete steps:
1. State the loop-entry envelope check as [[175]]'s one remaining, non-erasable
   `χ_m`, and the per-iteration in-body check as the erasable one, using [[231]]'s
   criterion explicitly (this should be a direct instantiation, not new theory).
2. Implement the erasure: the fused numeric/dense loop stencil family
   ([[123]]/[[124]]) emits zero bounds-check instructions per iteration once the
   envelope is established, verified by disassembly.
3. Handle the failed-envelope case explicitly and honestly: when scalar evolution cannot
   prove the affine bound (a non-affine index expression, a loop-carried mutation of the
   bound), the fallback is the existing per-iteration guarded loop, unchanged — this task
   adds a faster proven case, it does not weaken the general one.

Acceptance: a loop matching [[175]]'s target shape (affine induction variable, statically
provable bound) compiles with zero per-iteration bounds-check instructions in the loop
body, verified by disassembly; a loop that does *not* satisfy the envelope precondition
(a negative test: bound mutated inside the loop, non-affine index) correctly falls back
to the existing checked path, not a silently-erased-and-wrong one; alternating A/B on
loop-heavy suites (raytrace, navier-stokes) shows a measured gain consistent with or
exceeding ABCD's reported elimination rate.

Primary sources: reuses [[175]]'s existing ABCD and LLVM loop-predication citations;
[[231]]'s Cousot exactness citation for the restated soundness argument.
