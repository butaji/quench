# 61 — Confluence/critical-pair check for the rewrite-rule set

Status: planned

[[49-law-tested-rewrite-gate]] requires every new rewrite rule to ship with a law test proving *that rule alone* is semantics-preserving. This does not prove the *rule set* is confluent — two independently-sound rules whose left-hand patterns can overlap on the same term may rewrite it via different orders to different (though each individually "valid-looking") results, making the optimizer's output order-dependent. This is a real, well-understood gap: term-rewriting theory (Knuth-Bendix completion) exists specifically because individually-correct rules do not compose safely by default.

Concrete steps:
1. For every pair of rules in [[23-egraph-rewriting]]'s rewrite table, mechanically check whether their left-hand-side patterns can overlap on a common term shape (a "critical pair").
2. For every critical pair found, verify both rewrite orders converge to the same normal form (or to provably-equivalent terms under the existing category laws in [[01-stencil-category-core]]) — this is the confluence check itself.
3. Add this check to the CI gate alongside [[49-law-tested-rewrite-gate]]'s per-rule law test, so a new rule cannot merge if it introduces a non-confluent critical pair with any existing rule, without either fixing the overlap or proving convergence.

Acceptance: the current rewrite table (identity erasure, any rules added under [[23-egraph-rewriting]]) passes a full critical-pair audit with zero unresolved non-confluent pairs; CI rejects a new rule addition that introduces an unresolved critical pair with an existing rule; a deliberately-crafted pair of individually-sound but jointly non-confluent rules (used only as a CI self-test) is caught before merge.
