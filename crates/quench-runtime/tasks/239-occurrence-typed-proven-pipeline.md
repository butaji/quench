# 239 — The proven pipeline: occurrence typing as the foundation for flow-sensitive static execution

Status: planned

**The synthesis this whole thread has been building toward, made explicit.** [[219-js-to-native-lowering-functor]]
states the target functor `L : JS -> Native`; [[223-agt-derived-guard-synthesis]] derives
each guard mechanically as a coercion between precise and gradual types; [[231]]/[[232]]
give the exact criterion for when such a coercion is provably redundant and erasable;
[[233]]–[[236]] instantiate that criterion at concrete sites. What has not yet been
stated is the thing that makes all of this add up to *"JS execution behaves as statically
typed code at any given point in time"* rather than merely "a collection of individually
justified local optimizations": a single, continuous discipline covering *every* program
point, not just the ones an individual task happened to target.

**The missing piece has a name and a direct primary source.** Tobin-Hochstadt &
Felleisen's **occurrence typing** (*Logical Types for Untyped Languages*, ICFP 2010,
building on *The Design and Implementation of Typed Scheme*, POPL 2008) is exactly the
formal discipline for this: after a runtime type-test/branch in a dynamically-typed
program, the type of the tested value is *provably narrowed* along each resulting
control-flow path, and that narrowed type is exact and can be relied on statically for
every subsequent operation *until* a control-flow join or an effect reintroduces
uncertainty. This is the categorical structure this project's guards have been
implementing all along without naming it: a guard is not merely a classifying map
checked once ([[232]]) — its *result*, on the taken branch, is a proof that persists as
a definite, narrower type for the rest of that branch's extent, and every stencil
compiled within that extent should be compiled *as if statically typed at the narrowed
type*, with zero further checks against the same fact, until the extent ends. Nasso &
Castagna's *Revisiting Occurrence Typing* (2022) and *On Type-Cases, Union Elimination,
and Occurrence Typing* (POPL 2022) give the modern, semantically complete treatment of
exactly this narrowing-and-persistence structure, including its interaction with union
elimination — directly relevant to [[34]]/[[220]]'s coproduct dispatch, where a taken
tag branch should narrow the value to *that one summand type* for its entire extent, not
merely license one dispatch decision.

**What this reframes, concretely.** Every per-site erasure task in this family
([[231]]–[[236]]) currently proves "this one check is redundant here." Occurrence typing
gives the general reason *why*, uniformly: the guard immediately preceding a stencil
region already established a narrowed type for that region's extent, and every operation
inside that extent should be compiled against the narrowed type directly — not because
each operation was separately proven exact, but because it inherits the narrowing from
the guard that dominates it, exactly the way a typed language's type checker propagates
a refined type through a conditional's true branch. This is a stronger, more systematic
claim than "audit sites one at a time": it says the compiler should carry a **narrowed
type environment alongside every compiled region**, threaded through composition
([[01]]'s `Stencil<In,Out>` already has the right shape — `In`/`Out` are exactly where a
narrowed-type annotation belongs) rather than treating each guard's downstream effect as
a separate discovery.

**Where an extent ends — stated honestly, not glossed over.** A narrowed type's validity
extent is bounded by exactly the boundaries [[224]]'s blame-calculus discipline and
[[216]]'s Kleisli effect model already identify: a control-flow join with a differently-
narrowed (or unnarrowed) path, or an effect that could invalidate the narrowing (a call
to unknown code, a property write through an aliasing reference). Getting this boundary
wrong in either direction is the same class of risk [[237]]'s adversarial catalog exists
to catch — over-extending a narrowed-type region past its true extent is a correctness
bug, not a missed optimization, and this task inherits rather than replaces that
discipline.

**"Trust, but verify"** (Rastogi, Chaudhuri & Hosmer, POPL 2012) is the concrete
practical mechanism for pairing this with production reality: a two-phase typing
discipline where a first phase types optimistically (occurrence-typing narrowing
applied maximally, as this task proposes) and a second phase verifies the optimistic
typing's soundness against what the language's full dynamic semantics can actually
produce — directly the same role [[238]]'s debug-only verification harness already
plays for this project's erasure family, now grounded in a named, published discipline
rather than an ad hoc convention.

Concrete steps:
1. Formalize the narrowed-type environment as a first-class part of `Stencil<In,Out>`'s
   `In`/`Out` connector state ([[01]]), so composition ([[17]]) automatically threads a
   region's narrowed type into everything composed after it, rather than requiring each
   downstream operation to separately re-derive or re-check the same fact.
2. Restate each of [[231]]'s three sites, plus [[233]]–[[236]]'s four variations, as
   instances of one narrowing rule (a guard/branch narrows its taken extent's type
   environment) rather than seven independently-justified special cases — this should
   be a genuine simplification of the existing tasks' proof burden, not additional work
   layered on top, and any site where the general rule does *not* subsume the specific
   proof should be flagged as a real discrepancy to resolve, not smoothed over.
3. Extend narrowing across a composed chain automatically ([[234]]'s composition law
   becomes a corollary of the type environment threading through composition, rather
   than a separately-proven fact about lenses specifically).
4. Wire [[238]]'s debug-only verification harness to check every narrowed-type
   boundary's extent, not just each erasure site in isolation, closing the gap between
   "proven at this one point" and "provably typed for this whole region."

Acceptance: the narrowed-type environment is threaded through `Stencil<In,Out>`
composition and demonstrably subsumes [[231]]'s three sites and [[233]]-[[236]]'s four
variations as instances of one rule, with any genuine discrepancy documented rather than
hidden; a region spanning multiple composed stencils (not just one guard's immediate
successor) compiles with zero re-verification of a fact already established earlier in
the same narrowed extent, verified by disassembly; extent boundaries (join points,
effect boundaries) are confirmed correct against [[237]]'s adversarial catalog extended
to cover multi-region extents, not just single-guard sites; alternating A/B on the full
V8v7 suite shows a measured gain from the additional cross-region erasure this
generalization enables beyond what per-site erasure already captured.

Primary sources:
- Tobin-Hochstadt & Felleisen, *Logical Types for Untyped Languages* (ICFP 2010):
  <https://www2.ccs.neu.edu/racket/pubs/icfp10-thf.pdf>
- Castagna et al., *On Type-Cases, Union Elimination, and Occurrence Typing* (POPL 2022):
  <https://www.irif.fr/~gc/papers/popl22.pdf>
- Castagna & Nasso, *Revisiting Occurrence Typing*: <https://dl.acm.org/doi/10.1016/j.scico.2022.102781>
- Rastogi, Chaudhuri & Hosmer, *Trust, but Verify: Two-Phase Typing for Dynamic
  Languages* (POPL 2012): <https://arxiv.org/pdf/1504.08039>
