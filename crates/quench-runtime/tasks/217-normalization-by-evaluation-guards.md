# 217 — Normalization by evaluation for guard and expression canonical forms

Status: planned

[[18-identity-erasure-peephole]] and [[23-egraph-rewriting]]/[[67-egraph-as-primary-strategy]]
approach expression/guard simplification as rewriting: apply rules until none apply
(peephole) or saturate a quotient category and extract the cheapest representative
(e-graph). [[61-confluence-critical-pair-check]] then has to separately prove the
rewrite system is confluent so the result doesn't depend on rewrite order. Normalization
by evaluation (NbE) sidesteps the ordering question entirely for the sublanguage it
covers: instead of rewriting syntax to syntax, *evaluate* an expression into a semantic
domain (reflecting free variables/unresolved guards symbolically where needed) and then
*reify* the semantic value back into syntax. Because the semantic domain has no notion
of "the order rules were tried," the reified result is the unique normal form by
construction — confluence is a corollary of the construction, not a separately proven
property.

This is directly applicable to the sublanguage of pure guard/arithmetic expressions this
project already isolates as its own connector category ([[19-guard-typed-connectors]]):
booleans, numeric comparisons, and shape/type guards compose under well-understood
algebraic laws (De Morgan duality, comparison-chain transitivity, boolean short-circuit
identities) for which an NbE procedure is a standard, well-trodden construction —
reflect each guard/comparison into a semantic representation (e.g. a decision-diagram or
normal-form boolean combinator value), combine semantically, reify back into a
`StencilNode` guard chain.

Concrete steps:
1. Define the semantic domain for the guard/arithmetic sublanguage precisely (what a
   reflected guard value looks like, closed under the operations [[19]]/[[25]] already
   need: conjunction, negation, numeric-range comparison).
2. Implement `reflect`/`reify` and verify the round-trip law (`reify(reflect(e))` is
   semantically equal to `e` for every guard `e` in the existing test corpus) before
   using it for anything performance-related.
3. Replace the applicable slice of [[18]]'s peephole rules (the guard-simplification
   subset specifically, not the whole peephole pass) with NbE normalization, and compare
   against the existing rule-based result on the full corpus.

Acceptance: `reflect`/`reify` round-trip law holds on the complete guard/arithmetic test
corpus; NbE-normalized guard chains are semantically equivalent to (and, where
applicable, no larger in instruction count than) the peephole-rewritten baseline on
every existing test; a guard-simplification case the current peephole rules miss (a
compound condition needing multiple non-local rewrite steps to reach its simplest form)
is correctly normalized in one NbE pass, demonstrating this is strictly more complete
for its sublanguage, not merely differently implemented; alternating A/B shows no
regression.

Primary sources:
- Normalization by evaluation, standard construction: Berger & Schwichtenberg,
  *An Inverse of the Evaluation Functional for Typed λ-calculus* (LICS 1991) — the
  original reflect/reify NbE construction.
- Accessible modern overview with worked examples: <https://plfa.github.io/> (Normalization
  chapter, *Programming Language Foundations in Agda* — applies the same reflect/reify
  discipline to a small typed language, directly analogous to the guard sublanguage here).
