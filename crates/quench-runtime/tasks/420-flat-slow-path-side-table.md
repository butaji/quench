# 420 — Flat AOT slow-path side table

Status: planned

Deegen's baseline JIT never analyzes a slow path at runtime: every slow path is compiled
ahead of time, and a parallel `SlowPathData` stream carries exactly what that path needs
(source operands, return address, branch targets) indexed alongside the bytecode/stencil it
falls back from. Hot code that hits an uncommon case jumps straight to a fully outlined cold
handler with the matching side-table entry, with no runtime bookkeeping to reconstruct
context.

Quench's native-fallback paths (Task 09 object memory model, effectful-region vocabulary in
Task 128, slot pools in Tasks 103/106/107/111/112) each carry their own ad hoc piece of this
idea, but there is no single flat format that every native-execute-tier stencil publishes its
fallback context through. Define one: a build-time-cooked, index-aligned side table keyed by
stencil site, holding the operand/register/branch-target facts a canonical VM handler needs
to resume correctly, so a guard miss or proof failure is a table lookup plus one jump rather
than a bespoke reconstruction per opcode family.

Acceptance: a single `SlowPathTable` (or equivalent) type shared by at least three existing
native-fallback sites without per-site special-casing; disassembly/measurement showing the
fallback jump is branchless relative to the current per-site reconstruction; full correctness
suite; no component-floor violation in the V8v7 A/B gate.

Primary source: sillycross, "Building a baseline JIT for Lua automatically"
<https://sillycross.github.io/2023/05/12/2023-05-12/> (SlowPathData section).
