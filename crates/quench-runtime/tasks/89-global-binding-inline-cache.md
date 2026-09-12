# 89 — Effect-refreshed captured/global binding snapshots

Status: complete

Close the common `LoadLocal; LoadName; NumericCompare; JumpIfFalse` block without a
per-iteration environment lookup. Globals and captured variables in this VM live in the
same parent-linked `Environment` slot model as lexical bindings; they are not properties
of a separate global object. The earlier task text claiming global-object property
lookup was therefore factually wrong and is superseded by this design.

Resolve each selected named operand at frame entry into an owned `Value` snapshot and
expose a read-only pointer through the fixed stencil-frame ABI. A macro-generated numeric
branch stencil reads the local and snapshot directly, with the canonical slow exit for
coercing/nonnumber cases. Every bytecode effect that can change name resolution or a
binding value—declaration, store, catch binding, call, or construct—refreshes the selected
snapshots after completing. Effects are rare relative to loop comparisons, and frames
with no selected name stencil have an empty-list fast return.

There is no assumption based on benchmark identity, hotness, spelling, or literal
value. Owned snapshots preserve `Rc` lifetimes for heap values and disappear with the
frame. Scripts and functions use the same effect boundary, so declarations and
undeclared global assignments remain correct even when their environment vector grows.

Categorically, frame entry supplies an environment projection to a closed branch
morphism. Explicit algebraic effects invalidate and re-evaluate that projection at their
boundary; pure stencil regions consume it as an immutable value.

Evidence motivating priority: `reports/semantic-block-profile.jsonl` records about 1.58
million `local; name; <=; branch` entries and another 249 thousand `<` entries.

Implementation result: `DynFrame` now owns a dense snapshot vector and exposes its
read-only base through the named `NAME_SNAPSHOTS_FRAME_WORD_OFFSET` ABI field. The
macro-generated rustc/LLVM handlers cover the six numeric comparison families. The
selector is structural and uses whole-register liveness, never benchmark identity,
source spelling, or a hotness threshold. Calls, construction, declarations, stores,
and catch binding are explicit invalidation effects which refresh the projection.

The first implementation checked for refresh work in the shared generic executor and
was rejected: the early targeted comparison in
`reports/effect-name-snapshot-targeted-ab/comparison.txt` showed a roughly 15% Crypto
regression. The accepted implementation monomorphizes the block kernel into snapshot
and no-snapshot variants, so frames without these stencils pay no per-op refresh test.

Acceptance evidence: 40 release tests pass, including mutation through a native call
between two selected comparisons. The complete eight-suite smoke is
`reports/effect-name-snapshot-split-smoke.jsonl`. The four-repetition alternating A/B
in `reports/effect-name-snapshot-split-full-ab/comparison.txt` moves the aggregate from
566.194 to 571.086 (+0.86%); Navier-Stokes improves 23.58%, and the largest suite
regression is Splay at -4.67%, inside the standing -5% gate.
