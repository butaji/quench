# 427 — Precompiled register-allocation variant catalog for DFG-style nodes

Status: planned

Deegen's DFG tier does not run a general register allocator over generated native code at
codegen time. Instead, each IR node's native implementation is AOT-cooked into a finite
catalog of register-allocation *variants* (`deegen_dfg_reg_alloc_variants.cpp`), and a
lightweight selection step at link time (`deegen_dfg_select_variant_logic_creator.cpp`)
picks the precompiled variant whose fixed register assignment matches what the surrounding
allocation actually produced, patching in the concrete registers via the same copy-and-patch
mechanism used for constants. The register allocator's real job shrinks to choosing among a
bounded, build-time-enumerated set of physical layouts rather than solving general
allocation at runtime.

Quench already has register-allocation-quality and ceiling tasks (30, 158, 251, 292, 313) and
a resident CPS stencil planner (158), but none of them describe a bounded catalog-of-
precompiled-variants-plus-match architecture; they describe allocation quality within a
presumably more general/runtime allocator. This task asks whether Quench's stencil planner
should adopt the same shape: enumerate a small, build-time-fixed set of register-assignment
variants per multi-operation stencil family, and make the runtime "allocator" a lookup/match
rather than a solver, keeping all allocation-quality work at cook time where it can be
verified once instead of re-derived per compilation.

Acceptance: at least one existing multi-variant stencil family (candidate: the counted
numeric stencils from Tasks 267/268) re-expressed as a fixed variant catalog with a matching
step replacing whatever runtime allocation decision currently selects a physical layout;
disassembly proof the runtime path is a table lookup plus patch, not a solver invocation;
full correctness suite; V8v7 A/B gate with no component-floor violation.

Primary source: luajit-remake `deegen/deegen_dfg_reg_alloc_variants.cpp` and
`deegen_dfg_select_variant_logic_creator.cpp`
<https://github.com/luajit-remake/luajit-remake/tree/master/deegen>.
