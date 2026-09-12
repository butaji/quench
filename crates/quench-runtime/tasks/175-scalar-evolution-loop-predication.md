# 175 — Scalar evolution and loop predication

Status: planned

Recognize affine induction recurrences in Task 171's loop phis: initial value, constant
step, latch predicate, and conservative trip-count/range expressions. For each dense
access whose index is affine in the induction variable, prove a loop-wide bounds
envelope. Replace per-iteration bounds/kind checks with one preheader guard and compose
two ordinary loop morphisms: a predicated fast stencil loop and the general stencil loop.

The first scope is canonical increasing/decreasing integer loops with one latch and no
overflow ambiguity. Unknown, coercing, wrapping, multi-exit, or mutated-length cases stay
general. No dynamic hot-check selection from ABCD is used; every eligible selected loop
gets the same bounded static proof.

This is the traced-category form of loop versioning: the preheader chooses between two
closed loop morphisms with the same input/output contexts. Bounds expressions and proofs
remain quoted until linking.

Acceptance: tests cover zero-trip loops, negative steps, overflow, length mutation,
holes, typed widening, side exits, and exceptions; disassembly shows one envelope guard
and no body bounds checks for accepted loops; Crypto/Navier-Stokes/RayTrace and complete
V8v7 alternating A/B improve. Analysis limits are named constants.

Sources:
<https://research.ibm.com/publications/abcd-eliminating-array-bounds-checks-on-demand>,
<https://www.llvm.org/docs/doxygen/LoopPredication_8cpp_source.html>, and
<https://llvm.org/doxygen/classllvm_1_1ScalarEvolution.html>.

Task 214 now owns the canonical numeric range/edge-case fact. This task derives affine
trip counts and array envelopes from that fact; it must not maintain a second interval
analysis whose overflow, NaN, or minus-zero conclusions can drift.
