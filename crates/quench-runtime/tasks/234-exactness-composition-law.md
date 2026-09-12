# 234 — Composability law for chained exact abstractions

Status: planned

A sharp, easy-to-get-wrong question [[231]]/[[232]] leave open: if two *adjacent* guards
are each individually exact, is their composite automatically exact too? In general
Galois-connection theory the answer is not free — composition of Galois insertions is
itself a Galois insertion only under a stated condition (the intermediate domain's
insertion must actually compose functorially, not merely have two individually-exact
endpoints with an unrelated middle), so this needs its own proof, not an assumption.

This matters concretely for [[65-composed-property-lens]]: a chain `a.b.c.d` composes
per-level lenses into one combined guard. If level `b`'s shape is exact (per [[231]]'s
site 1 — a fully-seen, unmutated object literal) and level `c`'s shape is *also*
individually exact, does the composed chain need zero guards, one combined guard, or
does exactness fail to compose and leave a residual check — this is not obvious a
priori, and getting it wrong in either direction is a real defect: claiming full erasure
when composition actually loses exactness is a correctness bug per [[224]]'s blame
argument; failing to erase a genuinely-composable exact chain is a missed but safe
optimization.

Concrete steps:
1. State lens composition ([[65]]) categorically as composition of the two levels'
   classifying maps ([[232]]), and derive the general composition law: under what
   condition on the intermediate object does `χ_{m₁} ; χ_{m₂}` (composed) remain
   constant-true given `χ_{m₁}` and `χ_{m₂}` are each individually constant-true. State
   this as a proof, not an assumption — the expected answer is that composition of two
   *provably-constant* classifying maps is always constant (constancy is closed under
   sequential composition trivially, unlike general exactness), but this should be
   verified against [[65]]'s actual lens laws rather than presumed.
2. Apply the result to [[65]]: a chain where every level satisfies [[231]]'s exactness
   criterion individually should compile to a chain access with zero guard instructions,
   not the "one combined guard" [[65]]'s current text describes as its target — this
   task either strengthens [[65]]'s acceptance criterion or documents precisely why full
   erasure does not hold, with a concrete counterexample if one exists.
3. Produce one worked example of each case for the test corpus: a chain fully composed
   of exact levels (erased entirely) and a chain with one inexact level (checked only at
   that level, exact on either side of it), demonstrating the boundary is handled
   correctly rather than degrading to "check everything" or over-erasing.

Acceptance: the composition law is stated and proven (not assumed) for lens/guard
composition specifically; [[65]]'s chained-access acceptance criterion is updated to
reflect the proven result; both worked examples (fully exact chain, partially exact
chain) produce correct, verified-by-disassembly instruction counts; a negative test
demonstrates that an incorrectly-assumed-exact intermediate level is caught, not
silently miscompiled.

Primary sources: builds directly on [[231]]'s and [[232]]'s existing citations
(Cousot POPL 1997; Mac Lane & Moerdijk); Riehl or Awodey's standard category theory text
for the general fact that composition of monomorphisms is a monomorphism (the base
fact this task's stronger claim — composition of *isomorphisms* is an isomorphism —
specializes from).
