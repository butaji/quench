# 34 — Coproduct-typed tag dispatch for interpreter-style megamorphic code

Status: planned

earley-boyer is a Scheme interpreter written in JS: its dominant cost is not arithmetic or property access after a guard succeeds, but the dispatch itself — `sc_isNumber`/`sc_typeof`/`sc_equal`/`sc_less`-style functions branch on `typeof`/tag checks per call, and interpreter node dispatch is effectively megamorphic (every AST-node-type check probes a tagged S-expression object). No existing task addresses this shape of cost: guard connectors ([[19-guard-typed-connectors]], [[25-generalized-speculative-guards]]) assume a fast path exists once a guard passes, but here every path is taken roughly equally often — there is no single dominant guarded case to specialize toward.

Model the repeated `typeof`/tag check itself as a categorified case-split: a closed coproduct of tag values with one compact dispatch stencil (a jump table keyed by tag) replacing a chain of sequential `if (typeof x === ...)` guard-and-bail branches. This is not speculative — no path is assumed hot — so there is no guard-failure/deopt machinery involved, only a cost improvement to how a known-closed set of cases is dispatched. The coproduct's case arms remain ordinary `Stencil<Ctx, Ctx>` morphisms; only the entry mechanism changes from linear chained branches to a single indexed jump.

Acceptance: a chain of N `typeof`-style sequential checks compiles to one indexed dispatch instead of up to N sequential branch-and-compare steps, verified by instruction-count comparison on an earley-boyer-shaped microbenchmark; adding a new tag case requires only extending the coproduct, not restructuring existing arms; semantics are identical to the sequential chain for every tag value including an unrecognized/default case.
