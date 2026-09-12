# 26 — Feedback-directed morphism selection

Status: planned

Maintain a small per-site feedback record (observed shapes, call targets, or numeric types) independent of the category itself. Use it to choose which of several parallel `Stencil<Ctx, Ctx>` candidates with identical connector types to link at a given site — for example, a guarded-fast-path candidate from [[25-generalized-speculative-guards]] versus the generic candidate — before the first link, instead of always starting generic and only specializing after a failure is observed.

Note on framing: this is standard feedback-directed compilation, the same mechanism every profile-guided JIT uses — describing the choice as "a coproduct over morphisms with matching In/Out types" is accurate but not load-bearing; the categorical structure only guarantees the two candidates are legally interchangeable at that site (already guaranteed by [[01-stencil-category-core]]'s composition typing), it does not itself decide which one to pick. Do not cite this task as evidence of categorical payoff; it would be designed identically in a non-categorical JIT.

The same feedback record also drives the sibling optimization [[28-profile-driven-block-layout]]: branch-taken frequency for composition-order decisions.

Acceptance: a call site or arithmetic op with a stable observed type/shape links its guarded candidate on first compilation rather than only after an initial generic-path deopt; feedback records are bounded in size (no unbounded per-site history); a site with unstable/polymorphic feedback correctly falls back to the generic candidate rather than thrashing between guarded candidates.
