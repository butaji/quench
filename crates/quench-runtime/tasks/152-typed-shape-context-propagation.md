# 152 — Typed-shape context propagation

Status: planned

Extend Task 144's context `Gamma` so object facts contain both shape identity and known
property-slot value representation. A shape-guard stencil refines
`Object(Unknown) -> Object(ShapeId)`; subsequent property loads from that object reuse
the proof and refine their result without another shape or tag test. Object mutation,
unknown calls, prototype effects, and aliasing explicitly kill the affected facts.

This is one region-level morphism, not per-property fusion. The quote contains facts and
effects; a fixed-point rewrite removes guards proven redundant; rustc/LLVM-cooked leaves
emit the surviving guards and fixed-offset loads. The canonical generic stencil remains
the slow coproduct arm.

Start with own data properties and exact shape identity. Typed property descriptors and
safe alias facts are derived extensions. All context/version limits are named constants,
and selection is first-use/bounded rather than hotness based.

Acceptance: repeated own-property reads in one block/loop execute one entry shape guard;
mutation and opaque-call tests kill the proof; differential tests agree with generic
semantics; shape/type-test counters fall on Richards and DeltaBlue; full V8v7 A/B passes.

Primary source: <https://arxiv.org/abs/1507.02437>.

The paper's measured mechanism is the required first slice, not merely motivation:
propagate an exact shape fact after one guard, use the shape's field representation at
the load, and preserve the fact until an explicit shape/prototype/unknown-effect kill.
The reported 48% reduction in type tests, 17% code-size reduction, and 25% execution-time
reduction are not predictions for this VM, but they make repeated-guard elimination a
higher-priority experiment than adding more isolated guarded property leaves.

## Round-forty-six bounded propagation policy

Start with a named maximum of two propagated shapes per `(block, value)` context and widen
to `UnknownShape` beyond it. This is not an arbitrary tuning number: the source study found
`maxshapes=2` retained the mean benefit while unlimited propagation caused pathological code
growth on Splay. Re-evaluate the constant only through a complete code-size plus V8v7 A/B
experiment. Tasks 410–412 reinforce the same mechanism locally: wider copied inherited PICs
reduced Richards misses but did not produce a robust aggregate win.

Overflow does not append more inline guards. It composes the shared Task 159 lookup kernel,
preserving bounded instance bytes and the same typed connector.
