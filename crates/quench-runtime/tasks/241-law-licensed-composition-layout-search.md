# 241 — Law-licensed composition-order search for optimal machine-code layout

Status: planned

States the concrete payoff of [[01-stencil-category-core]]'s already-proven laws
(associativity, identity) and [[64-commutative-parallel-composition]]'s proven
commutativity that no existing task yet cashes in: an algebraic law is not only a
license to *delete* redundant structure ([[18]]'s identity erasure, [[231]]/[[232]]'s
guard erasure) — it is a license to **search**, safely and cheaply, because every
candidate the law generates is correct by construction, with no per-candidate
correctness re-verification required. This is the specific reason algebraic laws are
worth more here than ad hoc heuristics: a normal layout search (try reordering, check
each ordering is still correct, pick the fastest) pays a verification cost per
candidate; a *law-licensed* search (try every reassociation/reordering the proven law
already guarantees is semantically identical) pays that cost exactly once — at the point
the law itself was proven — and every subsequent search is pure cost comparison.

Concrete target: [[28-profile-driven-block-layout]] and [[209-static-branch-weight-aot-metadata]]
already want to choose a good instruction layout for composed stencil regions (fast arm
as fallthrough, cold arm outlined, per [[161]]'s cited LLVM machine-block-placement
model), but neither states where its *safe candidate set* comes from — today it is
implicitly "the one composition order the compiler happened to build." [[01]]'s proven
associativity means a chain of `n` composed stencils has up to `n!`-adjacent
re-orderings (bounded further by [[64]]'s commutativity proof, which additionally
licenses swapping any two sub-morphisms proven independent) that are *all* the same
program, by the law, not merely plausibly-the-same-pending-a-check. [[62-pluggable-cost-extraction]]
already separates cost-based extraction from rewrite-rule construction for the e-graph
specifically; this task is the same separation applied one level down, to physical
instruction layout within an already-selected composition, using the *already-proven*
laws as the candidate generator instead of e-graph rewrite rules.

Concrete steps:
1. State exactly which reorderings a composed stencil chain licenses today: full
   associativity re-bracketing (any parenthesization of a `Seq` composition, per [[01]]
   and [[17]]'s linear-flatten normal form) always licensed; adjacent-element swaps
   additionally licensed wherever [[64]]'s commutativity proof already applies (proven
   independent sub-morphisms); nothing else — a reordering the laws do not license stays
   out of the candidate set, full stop, regardless of how good its measured layout looks
   in isolation.
2. Reuse [[62]]'s cost-extraction infrastructure (dynamic operations removed, code
   bytes, per [[157]]'s stated cost measure) to rank the law-licensed candidate set by
   predicted/measured machine-code quality — branch-prediction-friendly fallthrough
   ordering, cache-line packing of hot arms, code size — rather than building a second,
   parallel cost model.
3. Wire the AOT linker ([[02]]/[[43]]/[[52]]) to select among the ranked candidates at
   final stencil-image emission time, verifying via disassembly that the chosen layout
   is one of the law-licensed set (a guard against a bug silently expanding the
   candidate set beyond what the laws actually cover).

Acceptance: at least one composed stencil chain with a nontrivial law-licensed candidate
set (recommend a chain already touched by [[209]]'s guard/loop/error branch-weight work,
since its fast/cold arm classification gives an immediate, checkable cost signal) has its
final machine-code layout chosen from among multiple law-verified-equivalent candidates,
with the selection documented as which law licensed each candidate (not "we tried
reordering it and it worked"); a candidate outside the law-licensed set is never
selectable, verified by a negative test that attempts to inject an unlicensed reordering
and confirms it is rejected structurally, not merely absent from this run's output;
alternating A/B on the affected region shows a measured layout-quality gain (fewer taken
branches on the hot path, smaller hot-path code footprint, or both) with zero
correctness regression, since correctness was never in question for a law-licensed
candidate to begin with — only which one is fastest.

Primary sources: this task cashes in [[01]]'s and [[64]]'s existing proofs and reuses
[[62]]'s and [[161]]'s existing citations (LLVM machine block placement,
`MachineBlockPlacement_8cpp.html`); no new external citation is needed beyond
confirming the well-known fact that a proven algebraic law makes exhaustive search over
its equivalence class free of per-candidate correctness cost — this is the standard
justification for equality-saturation-style search ([[67-egraph-as-primary-strategy]]'s
own citation, Willsey et al., *egg*, PLDI 2021) applied here to physical layout rather
than to term rewriting.

## Empirical caution: the ranking function itself must be measured, not assumed

The law licenses *which candidates are safe to compare*; it says nothing about *which
one is fastest* — that must come from measurement on the actual target
microarchitecture, not from a heuristic like "fewer branches is always better" or
"maximize fallthrough." V8 found that a supposedly superior branchless decompression
strategy was actually 7% *slower* than the branchful version on x64, specifically
because of microarchitectural effects a static branch-count heuristic does not model.
This project's own [[209-static-branch-weight-aot-metadata]] and this task's cost
ranking (step 2, reusing [[62]]'s cost-extraction infrastructure) must therefore be
validated against actual measured cycles on the target host for at least one
non-obvious case, not assumed correct because a candidate has fewer static branches or
better-looking fallthrough — the law-licensed candidate *set* is proven safe; the
*order* within it is an empirical question every time.

