# 309 — Semantic micro-op algebra between bytecode and stencil tiling

Status: planned

Introduce one small, immutable operation vocabulary below `DynOp` and inside the quoted
`RegionPlan`. A complex bytecode lowers from the same semantic macro definition to a
sequence drawn from families such as:

- `Guard(fact, failure_state)`;
- `Load(place)` and `Store(place, value)`;
- `Convert(representation)` and `PureOp(kind, operands)`;
- `Call(recipe)` and `Effect(kind, inputs)`;
- `Branch(predicate, edges)`, `Return(value)`, and `Throw(value)`.

These are compiler data, never interpreted at runtime. Pure reducers propagate constants,
representations, shapes, ownership, and locations; eliminate redundant guards/loads/
stores; form bounded context versions; and normalize control. The costed tiler maps the
result to the existing finite rustc/LLVM-cooked `StencilTemplate` and shared `Kernel`
families. Final linking remains the sole byte-emission effect.

The vocabulary is an algebra over typed connector contexts. A bytecode lowering is a
homomorphism into the free sequence of micro-ops, and micro-op-to-stencil lowering must
preserve identity and composition. Effects and failure continuations are explicit, so a
rewrite may cross an operation only when its effect/ownership/exception facts license it.

Do not hand-implement JS semantics a second time. The build-time semantic definition from
Task 149 must derive both the generic kernel obligation and the micro-op quote. Every
unsupported or dynamic arm lowers to an explicit generic kernel call with a canonical
materialization state, not to an interpreter.

Initial experiment: lower numeric arithmetic/comparison, local loads/stores, static own
property loads, direct calls, and branches. Demonstrate one cross-bytecode optimization
that existing `DynOp` tiling cannot express—for example one dominating numeric/shape guard
feeding multiple operations—and verify the emitted image contains fewer guards or frame
round-trips.

Acceptance: differential semantics against generic stencil execution; composition and
effect-law property tests; named limits for context versions, reducer iterations, and
catalog growth; deterministic byte output; no runtime micro-op dispatch; complete V8v7
correctness; alternating A/B improves before the optimized lowering becomes default.

Primary sources: CPython PEP 744's optimized micro-op-to-copy-and-patch pipeline
<https://peps.python.org/pep-0744/>, SpiderMonkey CacheIR's guard/pure/result vocabulary
<https://firefox-source-docs.mozilla.org/js/how-we-optimize.html>, and Deegen's controlled
semantic derivation <https://arxiv.org/abs/2411.11469>.
