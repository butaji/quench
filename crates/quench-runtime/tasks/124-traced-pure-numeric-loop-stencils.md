# 124 — Traced pure-numeric loop stencils

Status: complete

Generalize [[123-traced-numeric-dense-loop-stencil]] from numeric loops that happen to
contain a dense-array access to every sufficiently large loop expressible by the same
typed numeric/local IR. This is a capability predicate over bytecode only: at least one
numeric arithmetic/comparison operation, no calls/allocations/coercive operations,
single-entry/single-exit trace, and the existing ownership/escape proof. There is no
source identity, benchmark name, literal fingerprint, or hot-path detector.

The same category is reused unchanged: quote to a flat typed loop morphism, derive one
entry guard, compose rustc/LLVM AOT leaves, close the symbolic backedge, and retain
canonical public fallback labels. A pure-numeric region simply has the identity dense
context and therefore no array views to install. It must not introduce a parallel
compiler or duplicate arithmetic semantics.

Acceptance: release tests and the complete V8v7 semantic smoke pass; diagnostics record
eligible/linked/guarded/executed pure-numeric regions outside Crypto/Navier; native
profiles show less `dyn_block_step` work; accept only after a stable complete-suite
alternating A/B improves aggregate with no suite below the standing component floor.

## Result: rejected and reverted

Replacing the dense-access requirement with a general numeric-operation predicate was
implemented without changing the category, guard, linker, or AOT vocabulary. All 63
release tests and the complete smoke passed. Diagnostics in
`reports/task124-pure-numeric-region-stats.txt` showed **zero** eligible regions in
Richards, DeltaBlue, RayTrace, Earley-Boyer, RegExp, and Splay: every sufficiently large
loop crossed unsupported property/call/effect operations. Crypto linked two additional
regions (17 versus 15); Navier remained at 16.

The stable focused six-run comparison in
`reports/task124-pure-numeric-focused-ab-6/comparison.txt` regressed Crypto 6.14%,
Navier–Stokes 1.08%, and their aggregate 3.64%. The eligibility relaxation and its test
were reverted; the accepted Task 08 binary semantics are restored. This closes the
question with negative evidence: expanding useful coverage now requires a typed
property/call-capable region, not a broader predicate over the existing numeric IR.
