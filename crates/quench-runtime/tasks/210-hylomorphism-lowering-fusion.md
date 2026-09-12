# 210 — Hylomorphism fusion for AST-to-stencil lowering

Status: planned

AST-to-stencil lowering is, structurally, an anamorphism (unfold the OXC AST into some
intermediate walk state) followed by a catamorphism (fold that walk into a composed
`StencilNode`/`Stencil<In,Out>` value). Written naively, that means the intermediate
structure — an explicit IR tree, a `Vec` of partial results, whatever shape the walk
takes — actually gets materialized in memory before the fold consumes it. The
hylomorphism fusion theorem (Meijer, Fokkinga & Paterson's Bird-Meertens formalism; see
also Takano & Meijer's *shortcut deforestation*) states that an anamorphism immediately
followed by a catamorphism over the same functor can always be rewritten into one
recursive pass — a hylomorphism — that never builds the intermediate structure at all,
*provided* the two are genuinely expressed as coalgebra/algebra pairs over a common
functor. This is not a heuristic peephole rewrite; it is a law, checkable once per
functor shape and then applicable everywhere that shape recurs.

This is the categorical justification, not just the informal motivation, for why
[[17-seq-flatten-linear-link]] wants to avoid materializing an intermediate `Seq` tree
before flattening, and why [[68-dependence-graph-ir]]/[[120-typed-numeric-dense-region-ir]]
should be scrutinized for accidental non-fused ana/cata pairs (build the region IR, then
immediately consume it once into stencils — a candidate hylomorphism that may currently
allocate the IR for no reason beyond its being convenient to write).

Concrete steps:
1. Identify every lowering stage in `src/dynjit.rs`/`src/main.rs` that both constructs an
   intermediate tree/list from the AST and then immediately consumes it exactly once
   into `StencilNode` composition, without the intermediate value being inspected,
   shared, or reused elsewhere.
2. For each, state the coalgebra (`AST -> F(intermediate)`) and algebra
   (`F(StencilNode) -> StencilNode`) explicitly over a shared functor `F`, and verify the
   fusion law's precondition — the intermediate structure has no other consumer — holds
   for that specific call site (a shared/reused intermediate blocks fusion; this is a
   real constraint, not paperwork).
3. Rewrite the fused pair as one recursive function with no intermediate allocation,
   keeping the unfused two-pass version as a correctness oracle in tests until the fused
   version is verified to produce byte-identical `Stencil` output on the full corpus.

Acceptance: at least one concrete lowering stage identified in step 1 is rewritten as a
fused hylomorphism with a passing law statement (fused output structurally equal to the
composed ana-then-cata baseline on the full AST test corpus, not merely "looks right");
an allocation counter shows the intermediate structure no longer materializes at that
call site; alternating A/B on compile-time-sensitive workloads shows no regression and
documents any measured gain; a call site where the fusion precondition does *not* hold
(the intermediate is genuinely reused) is documented as correctly excluded, not silently
fused anyway.

Primary sources:
- Hylomorphism / Bird-Meertens formalism overview: <https://en.wikipedia.org/wiki/Hylomorphism_(computer_science)>
- Takano & Meijer, *Shortcut Deforestation in Calculational Form*: <https://www.kurims.kyoto-u.ac.jp/~kyodo/kokyuroku/contents/pdf/0918-21.pdf>
- Gibbons, *Hylomorphisms and Metamorphisms* (OPLSS lecture notes): <https://www.cs.uoregon.edu/research/summerschool/summer22/lectures/Gibbons4notes.pdf>
