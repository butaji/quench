# 196 — Bounded loop normalization algebra

Status: planned

Add a costed library of CFG rewrites over the same traced-loop stencil value:

- rotate eligible `while` loops into entry-guard plus contiguous body/latch;
- unswitch loop-invariant representation/shape/fuse branches outside the trace;
- peel an initialization/transition iteration when it strengthens the residual context;
- fully or partially unroll only when Task 175 proves a trip-count bound.

SBBV supplies a general first-iteration isolation rule: if an entry guard or first body
effect strengthens the back-edge context, emit one checked entry version and a separate
steady-state loop version whose header consumes the strengthened context. This is CFG
versioning, not a source-pattern recognizer, and it applies equally to number tags, array
bounds, receiver shapes, call targets, and prototype fuses.

These are derived combinators, not new runtime node kinds. Each returns an ordinary
stencil expression and uses Task 157's cost model. Named constants cap duplicated bytes,
unswitch arms, peel count, and unroll factor; the cost includes frame/register pressure,
not just removed branches.

Acceptance: zero-trip, fallthrough, `break`/`continue`, exceptions, effects, and ownership
order pass; guard invariance is proved through Tasks 173/177, never guessed; structural
tests show steady-state transfers/guards removed; each transform can be disabled and is
accepted only through isolated then combined V8v7 A/B.

Primary sources: LLVM loop rotation, unswitching, and unrolling documented at
<https://www.llvm.org/docs/Passes.html> and
<https://llvm.org/docs/doxygen/classllvm_1_1SimpleLoopUnswitchPass.html>; SBBV's
first-iteration/steady-state loop separation at
<https://doi.org/10.4230/LIPIcs.ECOOP.2024.28>.
