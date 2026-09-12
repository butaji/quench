# 392 — Demand-projected feedback plane

Status: planned

Allocate and update runtime feedback only when a surviving compiled recipe has an explicit
consumer for that fact. The one canonical quoted form is:

```text
FeedbackDemand = {
  site: SiteId,
  fact: Type | Shape | ElementKind | CallTarget | LexicalAddress | Prototype,
  consumer: RecipeId,
  lifetime: Image | Realm | ShapeEpoch,
  merge: First | BoundedSet | Widen,
}
```

`RegionPlan -> rewrite* -> costed cover` derives the demand set after dead definitions,
inlined calls, erased guards, and kernel/template choices are known. `FeedbackLayout` is a
packed projection of that set. `InlineSite`, IC objects, diagnostic reports, and linker
bindings all derive their offsets from the projection; none stores a second independent
list of feedback fields.

## Rules

- A statically proven fact has no runtime slot.
- A dead result or removed recipe has no runtime slot.
- A recipe whose generic implementation cannot consume a fact has no runtime slot.
- Runtime selection may use the first observed semantic case or a bounded polymorphic set;
  it may not record execution counts or wait for a hotness threshold.
- Static-only facts such as ownership, reflection freedom, or proven non-escape remain in
  the typed `Context`; they are never guessed from a runtime value.
- Budget overflow widens to the existing generic stencil/kernel case. It never drops the
  feedback required to preserve an accepted specialized case.

Represent the runtime plane as compact POD arrays grouped by fact kind. A template receives
only the base pointer/offsets it actually uses through Task 390's patch algebra. Immutable
`Kernel` and `StencilTemplate` objects never contain mutable observations; equal linked
instances may share a feedback plane only when site identity and invalidation lifetime are
also equal.

## Implementation slices

1. Add a diagnostic use-def census: every allocated field and every write records which
   recipe later read it. Task 393 supplies the non-perturbing instrumentation path.
2. Derive demands for `CallTarget`, own-property `Shape + Slot`, and `NameIc` address facts.
   Keep existing physical layouts behind adapters until the projection is proven complete.
3. Pack only demanded sites and patch direct stencils to the packed offsets.
4. Extend the same projection to type/representation and element-kind facts after Tasks
   144/171 consume them.

All fact kinds, merge widths, layout alignments, and budgets are named enums/constants.
There is no raw numeric kind tag or unnamed polymorphism limit in lowering or linking.

## Acceptance

- Every feedback field has at least one surviving `RecipeId` consumer; every recipe read
  resolves to exactly one field or an explicit static fact.
- Dead-code elimination and inlining deterministically remove their obsolete demands.
- Differential tests compare generic semantics and specialized results for mono-, poly-,
  invalidated-, and widened sites.
- A diagnostic V8v7 pass reports allocated fields, writes, reads, never-read fields, bytes,
  and invalidations by semantic fact kind, not benchmark identity.
- Release disassembly adds no counter/hotness branch. The full correctness suite passes.
- Retain a physical packing or write-elision change only after the alternating exact V8v7
  gate passes; otherwise retain only the census and record the rejected projection.

Primary source: the 2026 feedback characterization reports that recording can be costly
and that much recorded data is unused or statically predictable. Those external ratios are
motivation, not expected local gains:
<https://doi.org/10.4230/LIPIcs.ECOOP.2026.16>.

Related typed-IR source for static facts that runtime dispatch cannot recover:
<https://mlaurent.ovh/publications/typed_ir.pdf>.

