# 348 — Two-pass stencil function linker

Status: planned

Replace recursive intermediate-vector materialization with one explicit two-pass final
interpretation of the normalized `StencilExpr`. Composition remains cold, immutable, and
inspectable; only this final linker performs effects.

Pass one flattens the free-monoid expression into selected atoms, assigns each atom to a
hot, cold, or support stream, computes checked stream sizes and alignments, interns labels,
and assigns every image-relative offset. Pass two allocates each final stream once, copies
each selected `Kernel` reference or `StencilTemplate` instance once as appropriate, and
patches internal, symbolic, external, operand, IC, and continuation holes from the pass-one
layout. No atom may observe a partially emitted neighbor or use `out.len()` as hidden
composition state.

The layout result is one immutable `FunctionLayoutPlan`, consumed by both size/cost
reporting and materialization. This preserves associativity: different parenthesization
of the same normalized sequence produces the same plan and bytes. Empty/identity
morphisms consume no storage. Shared immutable kernels are addressed, not copied;
identical closed instances may be interned only by their complete bindings.

All stream alignment, maximum image/atom/label counts, offset widths, and padding budgets
are named constants with checked arithmetic. Allocation, publication, and instruction
cache synchronization remain edge-confined and W^X.

Acceptance: property tests compare left/right-associated and flat expressions; pass-one
sizes equal pass-two writes exactly; every hole resolves once with the correct kind;
hot/cold/support streams allocate once each; disassembly and task statistics prove no
recursive temporary code vectors remain; release tests and full V8v7 A/B do not regress.

