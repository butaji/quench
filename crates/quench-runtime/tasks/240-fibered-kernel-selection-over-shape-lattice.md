# 240 — Fibered kernel selection: the machine-code pipeline as a Grothendieck construction over proven shapes

Status: planned

States precisely what "data shapes and O(1) execution are a product of category theory
and stencils/kernels" means as a checkable structural claim, rather than an analogy: the
kernel selector this project's entire stencil-selection machinery implements
(`build.rs`'s AOT catalog, [[36]]'s direct-opcode dispatch, [[128]]'s coarse-region
vocabulary, [[157]]'s costed tiling) should *be* — not merely resemble — the total
category of a **Grothendieck fibration** over the base category of proven facts (shapes
from [[07]]/[[135]], numeric/dense region facts from [[120]], closed-coproduct tags
from [[220]], and every other exactness fact from [[231]]'s family), with the fiber over
each base object being the (possibly singleton) set of compiled kernels licensed for
exactly that fact. A fibration is the standard categorical structure for "a family of
categories indexed by proofs, with every morphism in the base having a canonical lift
into the total space" — precisely the shape "given this specific proven fact, here is
exactly the machine code licensed for it, and refining the fact (a shape transition, a
narrowed occurrence type per [[239]]) lifts canonically to a corresponding kernel
transition" that this project's whole stencil-selection design already informally
implements.

**Where the categorical product enters, concretely.** [[120-typed-numeric-dense-region-ir]]
already combines two independent facts (numeric representation, dense-array shape) into
one region IR rather than nesting two separate guards; [[157-costed-multi-granularity-stencil-tiling]]
prices candidate fusions by *dynamic operations removed*, exactly the right cost measure
for the question this task makes precise: when two proven facts are genuinely
independent (their fibers factor as a categorical product — knowing fact A tells you
nothing about which value fact B takes, and vice versa), the licensed kernel space is
the *product* of the two individual fiber sets, and the correct compiled form is one
product kernel checking both facts together in one combined stencil, never two nested
guards paying two separate branch/dispatch costs for information that was always jointly
available. When two facts are *not* independent (fact B's fiber is constrained by which
value fact A took — e.g. a closed-coproduct tag and the field layout that tag implies),
the product structure would be wrong to impose: the correct form is a **dependent
sum/sigma** over the base fact, not a product, and treating it as a product would either
under-specialize (missing the correlation) or require redundant, unreachable fiber
cases.

This gives [[157]]'s tiling-cost question a precise categorical answer instead of a
heuristic one: *tile two facts into one kernel exactly when their fibers form a product,
never when they form a dependent sum with unreachable combinations* — a structural,
checkable criterion (does fact B's fiber size/content vary with which element of fact
A's fiber was selected) rather than a cost-model guess arrived at by trial tiling.

Concrete steps:
1. State the base category explicitly: objects are proven-fact tuples reachable at a
   compiled program point (shape, numeric representation, dense/sparse element kind,
   closed-coproduct tag, narrowed occurrence type from [[239]]); morphisms are the
   proof-refining transitions [[07]]/[[150]]'s shape transitions and [[121]]'s
   hoisted-guard narrowing already perform.
2. State the fiber over each base object as the set of compiled kernels
   ([[02]]/[[36]]'s AOT catalog) licensed for exactly that fact-tuple, and verify the
   fibration's defining property (every base morphism has a canonical Cartesian lift —
   a shape transition has a canonical corresponding kernel transition, not an ad hoc
   one chosen per call site) holds for at least [[120]]'s existing numeric/dense region
   family, since it is the most mature multi-fact combination in the codebase today.
3. Apply the product/dependent-sum criterion from above to at least one currently
   nested-guard site (a candidate from [[130]]/[[137]]'s static-property region work,
   which already combines shape and property-slot facts) and either confirm its current
   combined-kernel form is the categorically correct product, or identify that it is
   actually a dependent sum wrongly treated as flat and should be restructured.
4. Feed the result back into [[157]]'s cost model: replace (or corroborate) its
   trial-and-price tiling heuristic with this structural criterion wherever the
   criterion applies cleanly, keeping the cost-based fallback only for combinations
   where independence cannot be statically determined.

Acceptance: the base category, fiber, and Cartesian-lift property are stated explicitly
and verified for [[120]]'s numeric/dense family; at least one existing nested-guard site
is analyzed under the product/dependent-sum criterion with a concrete disposition
(confirmed product, or restructured as a correctly-modeled dependent sum); [[157]]'s
tiling decisions for the analyzed sites are shown to agree with the structural
criterion's prediction, or a documented discrepancy is resolved; no runtime behavior
changes unless a site is found genuinely miscompiled (an under-specialized dependent sum
wrongly flattened as a product), in which case the fix is treated as a correctness
result with its own A/B, not folded silently into this task's grounding work.

Primary sources:
- Jacobs, *Categorical Logic and Type Theory* (Studies in Logic, 1999) — the standard
  fibration/Grothendieck-construction reference for indexed families of categories in a
  programming-languages setting, directly applicable to "proofs index kernel
  categories."
- Mac Lane, *Categories for the Working Mathematician* — the categorical product
  construction this task's independence criterion specializes.
