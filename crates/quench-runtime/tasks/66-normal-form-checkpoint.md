# 66 — Canonical normal form checkpoint for the quotient category

Status: planned

Following ZX-calculus's discipline (a categorical rewrite system that converges every equivalence class to a canonical graph-like normal form before circuit extraction), define a canonical normal form that every equivalence class in [[23-egraph-rewriting]] reduces to before [[62-pluggable-cost-extraction]]'s cost-based extraction runs, rather than treating rewriting as an open-ended search with no defined stopping point. This gives a deterministic, debuggable checkpoint ("is this term in normal form yet?") and keeps link time bounded, complementing [[17-seq-flatten-linear-link]]'s linear-time flattening guarantee.

Concrete steps:
1. Define the normal form precisely: a canonical ordering/shape for `StencilNode` equivalence classes (e.g. flattened `Seq`, identity-erased per [[18-identity-erasure-peephole]], with a deterministic canonical ordering for any commutative sub-terms identified in [[64-commutative-parallel-composition]]).
2. Prove rewriting under the current rule set terminates at this normal form (a termination argument, distinct from but complementary to [[61-confluence-critical-pair-check]]'s confluence check — termination and confluence together give a unique normal form per Knuth-Bendix theory).
3. Add a link-time assertion that rewriting has reached normal form before extraction begins, so a non-terminating or incompletely-applied rewrite sequence fails loudly at link time rather than silently extracting from a partially-rewritten term.

Acceptance: every equivalence class reaches a single, deterministic normal form regardless of rewrite application order (verified against [[61-confluence-critical-pair-check]]'s confluence guarantee); link time for reaching normal form is bounded and measured, not open-ended; a deliberately non-terminating rule (used only as a self-test) is caught by the termination check rather than hanging or silently extracting early.
