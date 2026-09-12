# 174 — Sparse conditional value propagation

Status: planned

Run a bounded SCCP-style fixed point over Task 171's executable CFG edges and SSA values.
The value lattice includes unreachable, constant JS values, representation facts
(`I32`, `F64`, tagged, boolean), known shape/element facts, and overdefined. Transfer
functions come from the same semantic recipes that drive generic and specialized
stencils; constant evaluation must not reimplement JavaScript semantics.

Rewrite unreachable blocks to identity, constant branches to one edge, and operations
with proven representations to compatible cheaper morphisms. Feed the normalized result
to Task 157's costed tiler. Widening and iteration limits are named constants, and a
limit hit yields the conservative general value.

Categorically this is an abstract-interpretation functor from concrete region states to
a finite fact lattice. Rewrites are accepted only when lowering the abstract result
commutes with the canonical semantic path.

Acceptance: differential tests cover NaN, signed zero, coercion, overflow, nullish
values, shape transitions, loops, and exceptions; diagnostics count dead edges and
specialized operations; compile time stays bounded; full V8v7 A/B improves.

Source: <https://doi.org/10.1145/318593.318659>.

