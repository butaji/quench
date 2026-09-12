# 207 — Category-theory literature grounding and JS/native semantic-gap inventory

Status: planned

Several tasks use categorical vocabulary (functor, natural transformation, colimit,
adjunction, Galois connection, lens/optic, e-graph/quotient category) without a cited
grounding in the literature those structures come from: [[01-stencil-category-core]],
[[22-osr-tier-transition]], [[46-closed-world-mode]], [[60-galois-connection-guard-elision]],
[[61-confluence-critical-pair-check]], [[65-composed-property-lens]],
[[67-egraph-as-primary-strategy]], [[69-supercompilation-colimit]], and
[[70-polyhedral-loop-adjunction]] have no primary source and no explicit statement of
which laws their construction must satisfy to actually be the structure it is named
after. Loose categorical language that doesn't cash out to checked laws is decoration,
not a design tool — it cannot be used to reason about correctness or to find where the
current implementation is not yet the structure it claims to be.

Ground each task in the literature it is actually an instance of, and use that grounding
to produce a concrete inventory of where JavaScript's dynamic semantics still forces a
detour through generic, non-native-shaped code — the actual gap this project exists to
close:

1. **[[01]] as compiling-to-categories.** Conal Elliott's *Compiling to Categories*
   (ICFP 2017) gives the general recipe this task already follows informally: interpret
   a program as a morphism in a chosen category (here, `Stencil<In, Out>`) via a
   cartesian-closed functor from the source semantics. State which CCC operations
   (composition, product, exponential) `Stencil` actually implements and which it does
   not yet (does `Stencil` have products — parallel composition of two independent
   regions — corresponding to [[64-commutative-parallel-composition]]? Does it have an
   exponential, i.e. a first-class representation of "a stencil parameterized by
   another stencil," relevant to [[35]]/[[163]]'s callback/caller customization?).
2. **[[60]] as an actual Galois connection.** Cousot & Cousot's abstract interpretation
   framework defines a Galois connection as an adjoint pair `(α, γ)` between a concrete
   and abstract lattice with `α(c) ⊑ a ⟺ c ⊑ γ(a)`. State the concrete lattice (program
   states admitting a guard-elision decision) and abstract lattice (the guard-elision
   analysis result) explicitly, and verify the adjoint inequality holds for the guards
   [[47]]/[[25]] already elide, not just for the mechanism's name.
3. **[[61]] as Knuth-Bendix confluence.** Confluence/critical-pair checking is the
   Knuth-Bendix completion procedure's termination condition (via Newman's Lemma: local
   confluence plus termination implies global confluence). State the rewrite system's
   termination measure explicitly (what strictly decreases on every rewrite in [[23]]'s
   quotient category) before claiming confluence is even a well-posed question to ask.
4. **[[65]] as profunctor optics.** The lens laws (get-put, put-get, put-put) are
   already named in [[65]]; verify against van Laarhoven/profunctor-optics literature
   that the *composition* operation used there is lawful lens composition, not merely
   "combine two guards," which is a weaker and possibly unsound operation if either
   level's setter is not total.
5. **[[67]]/[[23]] as equality saturation.** The `egg`/`egglog` line of work (Willsey et
   al., PLDI 2021) is the direct primary source for e-graph-based quotient-category
   rewriting already planned here; its extraction-cost separation is exactly
   [[62-pluggable-cost-extraction]]'s stated design — cite it there directly.
6. **[[69]] as a categorical colimit over the call graph.** State the diagram (objects =
   per-function stencil categories, morphisms = call edges) and what the colimit's
   universal property actually buys over ad hoc whole-program merging: a colimit gives
   *the* canonical merged object such that every function's local view factors through
   it uniquely — verify [[46]]'s shape-lattice unification has this uniqueness property,
   not just "a" merged shape.
7. **[[70]] as a categorical adjunction.** State the two functors (between the
   unscheduled iteration-domain category and the scheduled/tiled loop category) and
   verify an actual adjunction (unit/counit satisfying triangle identities), not just
   "loop transformation feels like it has a left and right direction."

Deliverable: for each task above, either (a) a stated law/property with a passing test
demonstrating the implementation actually satisfies it, matching the discipline already
required by [[49-law-tested-rewrite-gate]], or (b) an explicit note that the current
mechanism is only categorically-flavored and a plan for what would make it the real
structure. Use the resulting gap list to rank which native/C-level semantic mismatches
(boxing, prototype dispatch, GC, exception unwinding — the actual subjects of [[71]],
[[24]], [[38]]) are blocked on a categorical tool that isn't rigorously built yet versus
one that already is.

Acceptance: every listed task has either a cited primary source plus a stated law with a
test, or an explicit downgrade note; no task claims a categorical structure by name
without one of those two outcomes; the resulting gap inventory is added as a ranked list
to this file with a pointer to which existing planned task (if any) already covers each
gap.

Primary sources:
- Compiling to categories: <http://conal.net/papers/compiling-to-categories/compiling-to-categories.pdf>
- Cousot & Cousot, abstract interpretation (Galois connections): <https://www.di.ens.fr/~cousot/COUSOTpapers/POPL77.shtml>
- Newman's Lemma / Knuth-Bendix completion: standard term-rewriting reference, e.g. Baader & Nipkow, *Term Rewriting and All That*
- Profunctor optics: <https://arxiv.org/abs/1703.10857>
- egg: Fast and Extensible Equality Saturation (PLDI 2021): <https://dl.acm.org/doi/10.1145/3434304>
