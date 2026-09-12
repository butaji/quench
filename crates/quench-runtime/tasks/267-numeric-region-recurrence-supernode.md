# 267 — Numeric-region recurrence supernode

Status: complete

Tile the general numeric-region tail

`ReadLocal ; NumberLiteral ; Add/Subtract ; WriteSameLocal ; Backedge`

as one rustc/LLVM-cooked stencil. The selector is derived only from `DynCode` def/use,
operand equality, numeric-region guard facts, and control-flow targets. Intermediate
registers must have no uses outside the matched chain, and no control edge may enter an
interior operation. There is no source-name, benchmark, literal-value, or hotness test.

The supernode keeps the local number in an FPR for the arithmetic and materializes only
the loop-carried local at the categorical exit. Its outgoing continuation remains the
same symbolic backedge connector as the five leaves it replaces. A separate cooked
counted variant preserves `DEEGEN_NUMERIC_REGION_STATS` iteration accounting without a
normal-path statistics branch.

This is the first measurable Task 158 slice: a larger morphism chosen from the same
finite template catalog, not an ad-hoc whole-loop compiler. Unmatched code retains the
existing per-operation stencil composition, and guard failure retains the canonical
original-block fallback.

Acceptance: selector negative tests reject live intermediates, mismatched slots,
nonconsecutive PCs, and interior branch targets; cooked-template tests cover add/subtract
and counted execution; complete release tests and V8v7 smoke pass; linked-supernode and
iteration diagnostics prove execution; alternating Crypto, Navier-Stokes, and complete
suite A/B determine acceptance.

Sources: Copy-and-Patch supernodes <https://arxiv.org/abs/2011.13127> and V8 Liftoff's
register-cache principle <https://v8.dev/blog/liftoff>.

## Result

The selector and four macro-generated AOT variants are wired. Normal add/subtract
variants contain no runtime type/statistics branch; counted variants update the existing
region-iteration counter. The selected 64-byte normal stencil replaces four numeric
leaves plus a jump while retaining the private symbolic backedge label.

Evidence:

- 92 release tests pass. New tests execute add and counted-subtract cooked templates and
  prove that intermediate virtual registers stay untouched. Selector tests reject live
  intermediates, different local slots, nonconsecutive PCs, and interior control entry.
- A 20 ms complete-suite correctness smoke completes all eight suites.
- With numeric-region statistics enabled, Crypto links 9 recurrence supernodes and
  reports 2,375,182 counted iterations in 50 ms; Navier-Stokes links 16 and reports
  4,386,740 iterations.
- Ten alternating-order stable 200 ms pairs in
  `reports/task267-targeted-stable-pairs.md` improve Crypto median
  `1339.5 -> 1352.0` (`+0.93%`) and Navier-Stokes `5736.0 -> 5804.0` (`+1.19%`).
- The longer targeted artifact at `reports/task267-targeted-ab-7` was thermally unstable
  and reported aggregate `-1.71%`; it is retained as conflicting variance evidence.
- The five-repetition complete-suite comparison at `reports/task267-full-ab-5` improves
  aggregate `1836.06 -> 1849.71` (`+0.74%`). Every component stays above the standing
  `-5%` floor; the directly affected reported medians are Crypto `-0.45%` and
  Navier-Stokes `-2.60%`, while the stable isolated confirmation above is positive.

Accepted executable: `/tmp/deegen-task267-region-recurrence`, SHA-256
`3fc4903bdca3cc42ec76cd09fe9dd57f3d01341689b35da24700408baaf3aad0`.

This does not complete Task 158. It validates coarse, register-retaining tiling, but the
remaining arithmetic graph still materializes every value in the virtual register array.
