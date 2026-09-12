# 232 — Subobject classifier framing: a guard as an erasable constant-true morphism

Status: planned

Companion to [[231-exact-galois-insertion-guard-erasure]], from a different but
converging categorical direction: what a guard *is*, precisely, rather than when it can
be skipped. [[219-js-to-native-lowering-functor]] already calls a guard "the proof
obligation that makes a native operation licensed at all" but leaves that informal. Topos
theory has an exact structure for this: a monomorphism `m : A ↪ X` (the inclusion of "the
values this fast path is licensed for" into "all values that could reach this program
point") has a unique classifying map `χ_m : X → Ω` into the subobject classifier, such
that `A` is exactly the pullback of the arrow `true : 1 → Ω` along `χ_m`. A guard, stated
this way, is literally an implementation of `χ_m`: it answers, for the concrete value
that actually arrived, whether it lies in the licensed subobject — the same question a
`Bool`-valued shape/tag check already answers, now given the structure that explains
*why* that check is the right thing to compute and what "always true" would formally
mean.

The payoff of stating it this way: composing an already-known-constant morphism with
anything is free. If a program point's characteristic map `χ_m` is provably the constant
map `true : X → Ω` (every value that can reach the point already lies in `A`, i.e. the
inclusion `A ↪ X` is an isomorphism, not merely mono) then the "check" is composition
with a constant, which is an identity operation on the rest of the computation and costs
nothing to elide — this is a different but equivalent statement of
[[231]]'s exactness criterion, useful because it gives a second, independently-checkable
formulation: instead of proving `α ∘ γ = id` on an abstract-interpretation lattice, prove
`χ_m = true` as a fact about a classifying map, and use whichever formulation is easier
to state for a given guard family.

Concrete steps:
1. For each guard family already in the codebase ([[08]]'s shape guard, [[25]]'s
   speculative guard, [[220]]'s tag guard, [[229]]'s protector-cell check), state its
   subobject `A ↪ X` explicitly: what is `X` (every value syntactically reachable at
   this program point) and what is `A` (the licensed subset the fast path assumes).
2. For the sites [[231]] already targets for erasure, verify the *same* sites satisfy
   `A ↪ X` being an isomorphism (this should agree with [[231]]'s exactness proof — if
   it does not, one of the two framings has a bug, which is itself a useful
   cross-check, not wasted work).
3. For sites that remain genuinely checked ([[229]]'s protector-cell family), state
   explicitly that `A ↪ X` is a proper (non-isomorphic) inclusion, and that `χ_m`'s
   runtime cost is exactly the protector-cell load — grounding why that specific
   mechanism (one global O(1) flag, not a full re-derivation) is the cheapest possible
   implementation of a non-constant classifying map for that subobject shape
   (a cofinite/rare-mutation subobject, where the classifying map only needs to track
   "has the one bad event happened yet," not re-examine the value each time).

Acceptance: every guard family in the codebase has its subobject `A ↪ X` and classifying
map stated explicitly in this task's text; the erasable sites from [[231]] are confirmed
to correspond exactly to isomorphic inclusions under this framing (cross-check, not a
new implementation); no new runtime code results from this task — it is a formal
grounding pass, like [[207]], whose acceptance is conceptual agreement between the two
independent formulations (Galois-insertion exactness and subobject-classifier
constancy) rather than a new measured performance number.

Primary sources:
- Mac Lane & Moerdijk, *Sheaves in Geometry and Logic* — the standard reference for
  subobject classifiers and the pullback-characterization of monomorphisms in a topos.
- Lambek & Scott, *Introduction to Higher Order Categorical Logic* — the
  Curry-Howard-adjacent framing connecting a classifying map to a decidable proposition,
  relevant to why an isomorphic inclusion's classifying map is erasable at the type
  level (a proof-irrelevant `true` needs no runtime representation).
