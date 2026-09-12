# 111 — Ownership-only clearing for pooled virtual registers

Status: complete

Treat lowered virtual registers as definitely assigned compiler temporaries. When a
register vector returns to the pool, release only heap-tagged values and leave immediate
number/boolean/null/undefined bits unspecified. On reuse, existing slots may contain any
immediate value; lowering must write a register before reading it, and all direct stencil
destination guards already accept any non-heap old value. Newly grown slots are still
initialized to undefined.

This invariant removes stores from the release edge without weakening ownership: no
pooled register may retain an `Rc`-backed value. Environment pools deliberately retain
the stronger all-undefined invariant because JS bindings may observably begin undefined.
Represent these as two distinct products instead of one misleading shared reset policy.

Acceptance: add a test proving register pools retain no heap ownership while environment
pools remain all undefined; existing bytecode, closure, direct-stencil, and ownership
tests pass; the complete V8v7 smoke passes; an exact alternating six-run A/B against
[[110-constant-condition-control-stencils]] improves aggregate without crossing the
standing -5% component floor. Revert and record rejection otherwise.

Result: rejected. The invariant passed all 46 tests and
`reports/owned-register-pool-smoke.jsonl`, but the exact alternating six-run comparison
in `reports/owned-register-pool-ab-6/comparison.txt` changed aggregate score from
830.067 to 827.922 (-0.26%); Navier–Stokes fell 1.57%. Scanning every slot with a
heap-tag branch costs more than the immediate stores it avoids. The implementation was
reverted and [[110-constant-condition-control-stencils]] remains accepted.

A future ownership-set design must maintain sparse heap-slot metadata as values are
written, so release visits only owned slots. Do not repeat a full conditional scan.
