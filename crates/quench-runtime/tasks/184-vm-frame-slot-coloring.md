# 184 — Liveness-driven VM frame-slot coloring

Status: planned

Color compiler-generated virtual registers onto a smaller set of physical frame slots
when their CFG live ranges do not overlap. Parameters, captured/address-taken locals,
observable lexical bindings, handler state, and name snapshots remain pinned. Derive one
`VirtualRegister -> FrameSlot` map from liveness and use it as external patch data for
every leaf, region, and kernel connector.

This is distinct from Task 30: CPU register allocation acts inside machine stencils;
slot coloring shrinks the memory-resident VM frame shared by every abstraction level.
Ownership SSA must schedule destruction before a colored slot is reused, and exceptional
edges must participate in liveness.

Use named constants for the colorer iteration limit and any conservative spill budget.
Acceptance: law tests show alpha-renaming of virtual registers does not change the linked
image; exception/capture tests preserve values; representative large functions use fewer
frame slots and perform fewer initialization/destruction operations; full V8v7 A/B
improves.

Primary source: LLVM's stack-coloring implementation,
<https://github.com/llvm/llvm-project/blob/main/llvm/lib/CodeGen/StackColoring.cpp>.

