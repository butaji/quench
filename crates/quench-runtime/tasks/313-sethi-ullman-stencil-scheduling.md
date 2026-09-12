# 313 — Sethi-Ullman register-pressure-aware stencil scheduling

Status: planned

Once semantic micro-ops expose a pure expression forest and Task 292 exposes physical
connector registers as resources, label each pure subtree with its Sethi-Ullman register
need. Evaluate the higher-need child first when dependency/effect/exception laws permit;
for equal needs use the target cost table and stable source order as deterministic
tie-breakers. Then select a cooked stencil cover for that order.

This makes categorical tensor/commutativity useful without claiming every operation is
commutative. Only disjoint pure sub-morphisms may swap. JS left-to-right observable
evaluation, coercions, getters, allocation, exceptions, ownership destruction, and heap
effects pin order. The output is still one `StencilExpr`; scheduling creates no runtime
dispatcher and does not modify templates.

The algorithm is an inexpensive first policy for Task 241's law-licensed candidate set,
not a replacement for region register allocation. Its objective is fewer live
temporaries, edge moves, and spills; instruction latency and code bytes are secondary
named costs.

Acceptance: exhaustive small-tree tests compare the chosen schedule with the minimum
spill count; negative tests prove an effectful or throwing sibling cannot move; linked
AArch64 disassembly shows fewer guest-slot round trips on at least one general
expression family; full-suite A/B must improve before default enablement.

Primary source: Sethi and Ullman, “The Generation of Optimal Code for Arithmetic
Expressions,” <https://doi.org/10.1145/321607.321620>. Aho and Johnson's broader
linear-time dynamic-programming model is <https://doi.org/10.1145/800116.803770>.

Depends on Tasks 01, 157, 158, 171, 173, 241, 292, and 309.
