# 220 — Closed-coproduct tag dispatch for earley-boyer's tagged datum hierarchy

Status: planned

This refines [[34-coproduct-tag-dispatch]] with concrete evidence from the actual V8v7
corpus at `/private/tmp/js-engine-benchmark/v8-v7/earley-boyer.js`, and states the
categorical design that makes the refinement more than a bigger pattern table.

**Evidence.** The single largest entry in the whole-suite residual profile
(`reports/task132-residual-generic-blocks.jsonl`, 9.7M total entries) is one shape:
`LoadLocal,LoadName,InstanceOf,Unary:Not,JumpIfFalse` at 745,752 entries — 7.7% of every
residual block execution in the entire suite, from one source file. earley-boyer
implements a Scheme datum representation as a JS class hierarchy — `sc_Pair`,
`sc_Vector`, `sc_Char`, `sc_Struct`, `sc_HashtableElement`, `sc_Trampoline`, `sc_Values`,
`sc_InputPort`, `sc_OutputPort` (confirmed by grep: every one of these appears only as
an `instanceof` guard target, never in a `class ... extends` chain — they are disjoint
tag classes, not a real inheritance hierarchy) — and tests membership by chained
`instanceof` (e.g. line 2907: `!(o instanceof sc_Pair || o instanceof sc_Vector)`,
line 3693-3694: nested `if (x instanceof sc_Pair) if (y instanceof sc_Pair)`, dozens
more). Each `instanceof` walks the prototype chain and re-derives a fact that, at the
point of construction, was already known and fixed for the object's entire lifetime.

**Categorical design.** This set of constructors is exactly a coproduct (sum type):
`SchemeDatum = sc_Pair + sc_Vector + sc_Char + sc_Struct + sc_HashtableElement + ...`.
The universal property of a coproduct is that case analysis over it — "do X if it's a
`sc_Pair`, Y if it's a `sc_Vector`, ..." — factors uniquely through the coproduct's
injections, which is precisely a single tag-load-and-jump-table, not a sequence of
independent membership tests each re-examining the whole object. The current
`instanceof`-chain code is doing case analysis on a coproduct through repeated,
redundant, non-constant-time membership predicates instead of the O(1) dispatch the
coproduct's own structure licenses.

Concrete steps:
1. Confirm at each `instanceof`-chain guard site in [[04-bytecode-coverage-map]]'s
   coverage that the tested classes form a closed set reachable from that guard (no
   `class`/prototype extension observed elsewhere in the source that would make the
   coproduct open) — this is the same closure proof [[31-bounded-polymorphic-guards]]
   already needs for its 2-4-shape bound, applied here to justify the coproduct being
   *exactly* those tags, not merely bounded.
2. Assign each closed-coproduct member a fixed small tag at construction (or reuse an
   existing shape-id from [[07]]/[[135]] if one already uniquely identifies each
   constructor), so `instanceof T` against a coproduct member lowers to one tag-equality
   compare, and a chain like `x instanceof A || x instanceof B` lowers to one
   tag-range/tag-set test, not two guards.
3. Replace the residual `LoadLocal,LoadName,InstanceOf,Unary:Not,JumpIfFalse` shape with
   a direct tag-dispatch stencil family, reusing [[84]]/[[160]]'s existing
   dead-result/nullish condition-stencil machinery as the template for a fused
   tag-compare-then-branch stencil.

Acceptance: every `instanceof` guard against this closed coproduct in earley-boyer
compiles to a single tag compare with no prototype-chain walk, verified by instruction
count on a representative guard; the specific residual shape's entry count in a rerun of
[[132]]'s profiler drops to near zero for this pattern; earley-boyer's suite score
improves in an alternating A/B with every other suite at or above the standing floor;
the closure proof from step 1 is documented per guard site, not assumed globally.

Source: `/private/tmp/js-engine-benchmark/v8-v7/earley-boyer.js` (local V8v7 corpus
checkout); residual evidence: `reports/task132-residual-generic-blocks.jsonl`,
`reports/task132-residual-generic-blocks-summary.txt`.
