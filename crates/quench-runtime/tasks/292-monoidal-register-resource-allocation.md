# 292 — Registers as resources in a monoidal category: composable, exposed lifetimes

Status: planned

[[30-stencil-register-allocation-quality]] treats register allocation as "an
implementation-quality concern of a single stencil's own codegen," explicitly orthogonal
to composition; [[251-cross-stencil-register-allocation-ceiling]] asks whether that
scoping itself creates a structural ceiling; [[64-commutative-parallel-composition]] and
[[172-ownership-ssa-value-flow]] separately handle commutativity-licensed reordering and
value ownership. None of the four gives register *lifetime* a compositional structure of
its own — a monoidal category is the standard tool for exactly this: model two
independent sub-computations' register/value requirements as a tensor product
`A ⊗ B`, where independence (no shared mutable resource) is a checkable structural fact,
not an ad hoc side-condition re-derived per optimization. Two morphisms
`f : A → A'` and `g : B → B'` compose as `f ⊗ g : A ⊗ B → A' ⊗ B'` exactly when their
resource sets are disjoint — the same disjointness [[64]] already needs to prove
commutativity, restated as the defining law of the monoidal product rather than a
one-off proof obligation per composition site.

**Concrete payoff distinct from [[30]]/[[64]]/[[172]] individually.** Once register/value
lifetimes are exposed as the monoidal category's resources rather than hidden inside
each stencil's private codegen, [[30]]'s register allocator gains a structural signal it
does not have today: which values are genuinely independent (tensor-separable) across a
composed region's sub-morphisms, and therefore safe to allocate to overlapping physical
registers at different points without any risk of one composition's live range
colliding with another's — this is the compositional generalization [[251]]'s
register-allocation-ceiling experiment needs on the *positive* side (what a whole-region
allocator, once built, should exploit), complementing that task's *diagnostic* side
(measuring how bad today's per-stencil-local allocation already is).

Concrete steps:
1. State the monoidal structure explicitly for `Stencil<In, Out>`: what the tensor
   product `⊗` means for two independently-composed sub-regions (their combined
   input/output fact state is the pointwise product, per [[240]]'s already-stated
   product-kernel criterion — this task supplies the register/resource-lifetime layer
   that criterion doesn't yet track).
2. Verify the defining law (`f ⊗ g` is well-defined exactly when `f` and `g`'s resource
   sets are provably disjoint) against at least one existing composed region from
   [[128]]/[[131]]'s coarse-region vocabulary, confirming the disjointness check is
   structurally available where it's needed, not something to invent per site.
3. Feed the resulting explicit resource-lifetime map to [[30]]'s register allocator as
   an additional input beyond its current per-stencil-local view, and measure whether
   allocation quality improves specifically at composition boundaries — directly
   testing whether this closes some of the gap [[251]] measures.

Acceptance: the monoidal tensor-product structure and its disjointness law are stated
explicitly for `Stencil<In, Out>` composition, with a passing test against at least one
real composed region; [[30]]'s allocator is measurably informed by the exposed
resource-lifetime map (not merely documented as a nice idea), verified by a reduction in
cross-boundary spill/reload traffic on the region tested in [[251]]'s own experiment;
alternating A/B on the affected region shows a measured gain with zero correctness
regression across the full V8v7 suite.

Primary sources: standard monoidal-category treatment of resource-sensitive composition,
e.g. Mac Lane, *Categories for the Working Mathematician* (monoidal categories, already
implicitly used by this project's product-kernel work in [[240]]); the specific
resource-as-tensor-factor framing for register/value lifetimes parallels linear-logic-
style resource treatments of imperative state (Girard's linear logic, standard reference
for "a resource is consumed exactly once" reasoning relevant to [[172]]'s ownership SSA).
