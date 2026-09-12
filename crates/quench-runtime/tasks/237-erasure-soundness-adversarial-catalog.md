# 237 — Adversarial negative-test catalog for exactness-based erasure

Status: planned

Every erasure task in this thread ([[231]], [[232]], [[233]], [[234]], [[235]], [[236]])
deletes a branch on the strength of a proof, not a runtime fallback — meaning a wrong
proof is a silent correctness bug, not a missed optimization, which is a strictly higher
stake than every prior guard-cheapening task in this project. [[224-blame-calculus-guard-elision-soundness]]
already asks each elision task for one boundary-violation negative test; this task is
the concentrated, adversarial version of that discipline specifically for the
exactness/erasure family, because "prove the check unnecessary and delete it" invites a
specific, recurring failure mode across every site: the proof was correct for the cases
the author thought of, and silently wrong for one they didn't.

Concrete steps: for each erasure site accepted under [[231]]/[[233]]/[[234]]/[[235]]/[[236]],
construct the adversarial program deliberately designed to violate the site's stated
precondition in the least obvious way available, and confirm the current implementation
either (a) correctly fails to classify the site as exact for that program (the proof
apparatus itself catches the violation before erasure), or (b) if it does erase and the
adversarial program produces a wrong result, treat that as a confirmed defect and revert
the erasure until fixed — never treat a found violation as acceptable "edge case"
scope-narrowing after the fact.

Concrete adversarial cases to construct, one per site family:
1. **[[231]] site 1 / [[222]] totality** — an object literal whose field-setting
   constructor is exact per the stated proof, but a getter is later added to the
   *prototype* (not the instance) for one of its field names, changing observable
   property-access behavior without touching the instance's own shape.
2. **[[231]] site 2 / [[220]] closed coproduct** — a program that dynamically extends
   the coproduct's tag set at a point the static closure proof did not examine (a
   conditionally-defined subclass reachable only through a code path the proof's
   reachability analysis under-approximated).
3. **[[233]] ABCD bounds** — a loop whose affine bound is correct on entry but whose
   backing array is resized by a callee invoked from within the loop body, invalidating
   the entry-envelope proof mid-loop.
4. **[[235]] singleton call target** — a call site proven closed-world-singleton, then
   examine whether any *catch handler*, `finally` block, or generator resumption point
   could reintroduce a second reachable definition the colimit's reachability analysis
   missed.
5. **[[236]] pure unit insertion** — a sub-computation classified pure that in fact
   throws under a specific numeric edge case (e.g. division producing `NaN` versus a
   thrown `RangeError` in a mode this project supports), where "pure" was proven only
   against the non-throwing control-flow paths.

Acceptance: all five adversarial cases (or the actual current set of accepted erasure
sites, whichever is larger) are constructed as concrete test programs and run against
the implementation; every one either correctly fails classification (erasure did not
happen) or is confirmed a genuine defect with the erasure reverted pending a fix — no
case is silently redefined as out of scope without an explicit, reviewed decision
recorded in the relevant task; this catalog is re-run as a standing regression suite
whenever a new erasure site is proposed under this family, per [[49-law-tested-rewrite-gate]]'s
existing discipline extended to cover exactness proofs specifically.

Primary source: this task operationalizes [[224]]'s existing blame-calculus discipline;
no additional primary source beyond it.
