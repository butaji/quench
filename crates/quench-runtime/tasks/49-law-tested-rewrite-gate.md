# 49 — Category-law test required for every new rewrite rule

Status: planned

Turn "proof-enabled aggressiveness" into an enforced contribution rule rather than a philosophy: every new rewrite added to the quotient-category framework ([[23-egraph-rewriting]]) or any new combinator added alongside `identity`/`compose`/`then_kernel` ([[01-stencil-category-core]]) must ship with a law test (the rewrite preserves semantics; if it claims a categorical property such as associativity or identity-compatibility, that property is itself tested, following the pattern of `stencil_category_free_monoid_laws`).

Concrete steps:
1. Add a lint/CI check (extending [[44-per-task-regression-gate]]) that flags a new entry in the rewrite-rule table without a corresponding law test in the same change.
2. Document the required test shape (a small template: construct two composition orders / two rewrite applications, assert byte or semantic equivalence) so contributors don't need to invent the proof style each time.
3. Retroactively audit existing rewrites ([[18-identity-erasure-peephole]]) against this template to confirm they already satisfy it, closing any gap before the rule becomes mandatory for new work.

Acceptance: CI rejects a rewrite-table addition lacking a law test; the existing rewrite set passes an audit against the documented template with no exceptions; a deliberately-introduced unsound rewrite (used only as a CI self-test, not shipped) is caught by the gate before merge.
