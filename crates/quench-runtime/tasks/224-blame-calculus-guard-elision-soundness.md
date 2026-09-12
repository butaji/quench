# 224 — Blame-calculus soundness argument for guard elision and generic-path deletion

Status: planned

[[47-compile-time-guard-elision]] deletes a guard when a binding is provably stable;
[[220]]/[[221]] delete a runtime `instanceof`/tag check when a coproduct is provably
closed; [[222]] deletes per-field guards when a constructor is provably total over its
shape. Each of these needs the same soundness property and currently argues for it
separately, per task: *if the static proof holds, the deleted check could never have
failed anyway* — i.e., deleting it cannot turn a program that would have hit the slow
path into one that silently does the wrong thing on the fast path.

Wadler & Findler's blame calculus gives this property a name and a general proof
technique: the **blame theorem**, "well-typed programs can't be blamed" — in a system
that inserts casts/checks at the boundary between precisely-typed and dynamically-typed
code, a run-time check failure (blame) can only ever be attributed to the
*dynamically-typed* side of a boundary, never the statically-typed side. Recast into
this project's terms: a value flowing through a region this VM has statically proven
stable (a guard [[47]] elided, a coproduct [[220]] closed, a shape [[222]] proved total)
can never be the *cause* of a runtime check failure at that region's boundary — if a
check would fail, the failure is attributable to a value that entered from a region the
static proof did not cover, which is exactly the boundary where the generic/fallback
path is still correctly present.

This gives elision tasks a load-bearing correctness argument stronger than "we tested it
and it passed": a structural guarantee that *no* elision this project performs can ever
be the reason a wrong value reaches a fast path, because the fast path's own entry
boundary is exactly where blame would have to land, and that boundary is precisely what
remains checked. Practically, this argument only holds if elision sites are actually
structured as boundaries in the blame-calculus sense — a check at the *edge* of a proven
region, not scattered mid-region — which is a concrete design constraint this task
should verify against [[47]]'s and [[220]]'s actual implementation shape, not merely
assert.

Concrete steps:
1. State, for [[47]]'s guard elision specifically, what "the boundary" is (the point
   where a value could first enter the proven-stable region from outside it — e.g. a
   function parameter, a property read from an unproven object, a call return) and
   confirm every remaining check in that task's design sits exactly at such a boundary.
2. Restate [[220]]'s closed-coproduct proof and [[222]]'s shape-totality proof in the
   same boundary terms, and identify the one shared "is this actually a boundary or did
   we delete a check that wasn't at one" pitfall each should specifically test against.
3. Produce one negative test per elision task: a program constructed so that, if the
   boundary property did *not* hold, a wrong value would slip through undetected — each
   such test should currently fail to slip through (correctly caught at the true
   boundary), demonstrating the blame property holds for the actual implementation, not
   only for the argument on paper.

Acceptance: the boundary property is stated precisely for [[47]], [[220]], and [[222]];
each has at least one negative test specifically targeting a violation of that property,
and all pass (no wrong value slips through); this task's boundary criterion is added as
a required check in [[49-law-tested-rewrite-gate]]'s discipline for any future elision
task, so elision work is reviewed against "is the remaining check at the true boundary"
rather than case-by-case intuition.

Primary sources:
- Wadler & Findler, *Well-Typed Programs Can't Be Blamed*: <https://homepages.inf.ed.ac.uk/wadler/papers/blame/blame.pdf>
- Ahmed, Findler, Siek & Wadler, on blame and gradual typing generally: <https://homepages.inf.ed.ac.uk/wadler/topics/blame.html>
