# 328 — AArch64 conditional-compare guard products

Status: planned

Lower a normalized conjunction of independent numeric/tag/shape guard atoms as one
rustc/LLVM-cooked guard stencil. Write the Rust predicate as one pure boolean expression
whose operands have no effects or trapping loads, allowing LLVM's AArch64 backend to form
`cmp; ccmp; ...; b.cond` and branch once to the shared failure continuation.

The categorical input is [[279-dominance-safe-guard-products]]'s proven product of guard
morphisms; this task changes only its target lowering. Composition stays compatible with
ordinary stencils and shared kernels. A failed product materializes exactly the same
canonical state as the equivalent sequential guards.

Start with two and three scalar `RawValue` number checks. Expand to exact shape IDs only
when AOT-visible loads are proven safe and disassembly still forms the intended sequence.
Use named constants `MIN_CONDITIONAL_COMPARE_GUARDS` and
`MAX_CONDITIONAL_COMPARE_GUARDS`; do not embed numeric policy literals in selection code.

Acceptance: property tests prove conjunction equivalence and identical slow exits;
extracted AArch64 contains one conditional branch for an accepted product; a sequential
guard variant remains available for unsupported targets; guard-heavy numeric-block and
full-suite alternating A/B decide retention.

Primary source: LLVM's AArch64 conditional-compare pass explicitly forms conditional
compare chains to reduce branching and code size:
<https://llvm.org/doxygen/AArch64ConditionalCompares_8cpp_source.html>.

